use std::{
    alloc::Layout,
    borrow::Borrow,
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    num::{NonZeroU8, NonZeroU32},
    ops::{Deref, Range},
};

use aligned_vec::{AVec, ConstAlign};
use flate2::{
    Compression,
    read::{GzDecoder, ZlibDecoder},
    write::{GzEncoder, ZlibEncoder},
};
use lz4_java_wrc::{Lz4BlockInput, Lz4BlockOutput};
use simdnbt::owned::Nbt;
use static_assertions::const_assert;

mod private {
    pub trait Sealed {}
}

#[derive(Debug, thiserror::Error)]
pub enum CompressionError {
    #[error("Gzip error: {0}")]
    Gzip(std::io::Error),
    #[error("Zlib error: {0}")]
    Zlib(std::io::Error),
    #[error("Lz4 error: {0}")]
    Lz4(std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum RegionReadError {
    #[error("Missing MCRegion headers")]
    MissingHeaders,
    #[error("Unaligned data")]
    UnalignedData,
    #[error("Missing chunk data")]
    MissingChunkData,
    #[error("Invalid chunk coordinates ({0}, {1})")]
    InvalidChunkCoords(u8, u8),
    #[error("Invalid chunk length field")]
    InvalidChunkLengthField,
    #[error("Invalid chunk offset")]
    InvalidChunkOffset,
    #[error("Invalid compression {0}")]
    InvalidCompression(u8),
    #[error("Oversized chunk {0}")]
    OversizedChunk(usize),
    #[error("Invalid backing buffer size: {0} % {SECTOR_SIZE} != 0")]
    InvalidBackingBufferSize(usize),
    #[error("Compression error {0}")]
    CompressionError(CompressionError),
    #[error("NBT error {0}")]
    NbtError(simdnbt::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum RegionWriteError {
    #[error("Oversized chunk {0} > {MAX_CHUNK_BYTES}")]
    OversizedChunk(usize),
    // This shouldn't happen as it corresponds to a 64GiB region file...
    #[error("Not enough free sectors")]
    NotEnoguhFreeSectors,
    #[error("Reading error")]
    ReadingError(RegionReadError),
    #[error("Chunk length {0} isn't representable on 32bits")]
    UnrepresentableChunkLength(usize),
    #[error("Invalid chunk coordinates ({0}, {1})")]
    InvalidChunkCoords(u8, u8),
    #[error("Compression error {0}")]
    CompressionError(CompressionError),
}

pub const SECTOR_SIZE: usize = 4096;
pub const REGION_SIZE: u8 = 32;
pub const REGION_SIZE_SQ: usize = (REGION_SIZE as usize).pow(2);
pub const MAX_CHUNK_BYTES: usize = 255 * SECTOR_SIZE;

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    pub allow_oversized_chunks: bool,
}

pub struct MCRegionReader<'a> {
    options: Options,
    data: &'a [u8],
    headers: &'a MCRegionHeaders,
    timestamps: &'a MCRegionTimestamps,
}

pub const fn chunk_idx_fast(x: u8, z: u8) -> usize {
    ((x % REGION_SIZE) as usize) + 32 * ((z % REGION_SIZE) as usize)
}

pub trait RegionReaderImpl<'a>: Sized + private::Sealed + Deref<Target = &'a [u8]> {
    fn get_header_unchecked(&self, x: u8, z: u8) -> &MCRegionHeader;
    fn get_timestamp_unchecked(&self, x: u8, z: u8) -> u32;
    fn options(&self) -> &Options;

    fn actual_sector_count(
        &self,
        header: &MCRegionHeader,
    ) -> Result<Option<(usize, usize)>, RegionReadError> {
        let (offset, sectors) = header.unpack();
        if offset == 0 {
            return Ok(None);
        }
        if offset == 1 {
            return Err(RegionReadError::InvalidChunkOffset);
        }

        let actual_sectors = if sectors == 255 && self.options().allow_oversized_chunks {
            let byte_offset = offset as usize * SECTOR_SIZE;
            let length = match self.get(byte_offset..byte_offset + 4) {
                None => return Err(RegionReadError::MissingChunkData),
                Some(slice) => {
                    // Safety: The returned slice is valid, 4 bytes aligned, and of length 4
                    u32::from_be(unsafe { core::ptr::read(slice.as_ptr() as *const u32) }) as usize
                }
            };
            (length + 4).div_ceil(SECTOR_SIZE)
        } else {
            sectors as usize
        };

        Ok(Some((offset as usize, actual_sectors)))
    }

    fn chunk_offset_in_sectors_raw_unchecked(
        &self,
        x: u8,
        z: u8,
    ) -> Result<Option<NonZeroU32>, RegionReadError> {
        let header = self.get_header_unchecked(x, z);
        let offset = header.offset();
        if offset == 1 {
            Err(RegionReadError::InvalidChunkOffset)
        } else {
            Ok(NonZeroU32::new(header.offset()))
        }
    }

    fn chunk_offset_in_sectors_raw(
        &self,
        x: u8,
        z: u8,
    ) -> Result<Option<NonZeroU32>, RegionReadError> {
        if x <= REGION_SIZE && z <= REGION_SIZE {
            self.chunk_offset_in_sectors_raw_unchecked(x, z)
        } else {
            Err(RegionReadError::InvalidChunkCoords(x, z))
        }
    }

    fn chunk_sector_count_raw_unchecked(&self, x: u8, z: u8) -> Option<NonZeroU8> {
        let header = self.get_header_unchecked(x, z);
        NonZeroU8::new(header.sector_count())
    }

    fn chunk_sector_count_raw(&self, x: u8, z: u8) -> Result<Option<NonZeroU8>, RegionReadError> {
        if x <= REGION_SIZE && z <= REGION_SIZE {
            Ok(self.chunk_sector_count_raw_unchecked(x, z))
        } else {
            Err(RegionReadError::InvalidChunkCoords(x, z))
        }
    }

    fn chunk_header_unchecked(
        &self,
        x: u8,
        z: u8,
    ) -> Result<Option<(NonZeroU32, NonZeroU8)>, RegionReadError> {
        let header = self.get_header_unchecked(x, z);
        let (offset, sector_count) = header.unpack();
        if offset == 1 {
            Err(RegionReadError::InvalidChunkOffset)
        } else {
            match (NonZeroU32::new(offset), NonZeroU8::new(sector_count)) {
                (Some(offset), Some(sector_count)) => Ok(Some((offset, sector_count))),
                _ => Ok(None),
            }
        }
    }

    fn chunk_header(
        &self,
        x: u8,
        z: u8,
    ) -> Result<Option<(NonZeroU32, NonZeroU8)>, RegionReadError> {
        if x <= REGION_SIZE && z <= REGION_SIZE {
            self.chunk_header_unchecked(x, z)
        } else {
            Err(RegionReadError::InvalidChunkCoords(x, z))
        }
    }

    fn chunk_timestamp_unchecked(&self, x: u8, z: u8) -> Option<NonZeroU32> {
        let timestamp = self.get_timestamp_unchecked(x, z);
        NonZeroU32::new(timestamp)
    }

    fn chunk_timestamp(&self, x: u8, z: u8) -> Result<Option<NonZeroU32>, RegionReadError> {
        if x <= REGION_SIZE && z <= REGION_SIZE {
            Ok(self.chunk_timestamp_unchecked(x, z))
        } else {
            Err(RegionReadError::InvalidChunkCoords(x, z))
        }
    }

    fn chunk_data_raw_unchecked(&self, x: u8, z: u8) -> Result<RawChunkData<'a>, RegionReadError> {
        match self.chunk_header_unchecked(x, z)? {
            None => Ok(RawChunkData::None),
            Some((sector_offset, sector_count)) => {
                let byte_offset = sector_offset.get() as usize * SECTOR_SIZE;
                let byte_count = sector_count.get() as usize * SECTOR_SIZE;
                match self.get(byte_offset..byte_offset + byte_count) {
                    None => Err(RegionReadError::MissingChunkData),
                    Some(chunk_data) => {
                        // Safety: sector_count is at least 1, so byte_count is at least 4096, so we do have 5 valid bytes to read...
                        let length = unsafe {
                            let temp = *(chunk_data.as_ptr() as *const u32);
                            u32::from_be(temp)
                        };
                        let compression_byte = unsafe { *chunk_data.get_unchecked(4) };

                        let compression = match compression_byte & 0x7F {
                            1 => ChunkCompression::Gzip,
                            2 => ChunkCompression::Zlib,
                            3 => ChunkCompression::None,
                            4 => ChunkCompression::Lz4,
                            _ => return Err(RegionReadError::InvalidCompression(compression_byte)),
                        };

                        let timestamp = self.get_timestamp_unchecked(x, z);

                        if compression_byte & 0x80 != 0 {
                            return Ok(RawChunkData::External {
                                offset_sector: sector_offset.get(),
                                compression,
                                timestamp,
                            });
                        }

                        if sector_count.get() == 255 {
                            let recalculated_sector_count =
                                (length + 4).div_ceil(SECTOR_SIZE as u32);

                            if recalculated_sector_count > sector_count.get() as u32 {
                                // oversized chunk
                                return if self.options().allow_oversized_chunks {
                                    match self
                                        .get(byte_offset + 5..byte_offset + 4 + length as usize)
                                    {
                                        Some(bytes) => Ok(RawChunkData::Chunk {
                                            offset_sector: sector_offset.get(),
                                            bytes: CompressedChunk(bytes),
                                            compression,
                                            timestamp,
                                        }),
                                        None => Err(RegionReadError::MissingChunkData),
                                    }
                                } else {
                                    Err(RegionReadError::OversizedChunk(length as usize))
                                };
                            }
                        }

                        // length field includes the compression byte
                        match chunk_data.get(5..(4 + length as usize)) {
                            Some(bytes) => Ok(RawChunkData::Chunk {
                                offset_sector: sector_offset.get(),
                                bytes: CompressedChunk(bytes),
                                compression,
                                timestamp,
                            }),
                            None => Err(RegionReadError::InvalidChunkLengthField),
                        }
                    }
                }
            }
        }
    }

    fn chunk_data_raw(&self, x: u8, z: u8) -> Result<RawChunkData<'a>, RegionReadError> {
        if x <= REGION_SIZE && z <= REGION_SIZE {
            self.chunk_data_raw_unchecked(x, z)
        } else {
            Ok(RawChunkData::None)
        }
    }

    fn is_generated(&self, x: u8, z: u8) -> bool {
        if x <= REGION_SIZE && z <= REGION_SIZE {
            !self.get_header_unchecked(x, z).is_none()
        } else {
            false
        }
    }
}

impl<'a> MCRegionReader<'a> {
    pub fn new(options: Options, data: &'a [u8]) -> Result<Self, RegionReadError> {
        if data.len() < 2 * SECTOR_SIZE {
            Err(RegionReadError::MissingHeaders)
        } else if !(data.as_ptr() as usize).is_multiple_of(SECTOR_SIZE) {
            Err(RegionReadError::UnalignedData)
        } else if !data.len().is_multiple_of(SECTOR_SIZE) {
            Err(RegionReadError::InvalidBackingBufferSize(data.len()))
        } else {
            const_assert!(Layout::new::<MCRegionHeaders>().size() == 4096);
            const_assert!(Layout::new::<MCRegionHeaders>().align() == 4096);

            const_assert!(Layout::new::<MCRegionTimestamps>().size() == 4096);
            const_assert!(Layout::new::<MCRegionTimestamps>().align() == 4096);

            // SAFETY:
            // self.data is a valid reference to at least 8192 bytes, so we can reinterpret those bytes as a an array of 1024 MCRegionHeader and u32
            // we have also checked that the pointer is correctly aligned
            let (headers, timestamps) = unsafe {
                (
                    &*(data.as_ptr() as *const MCRegionHeaders),
                    &*(data.as_ptr().byte_add(4096) as *const MCRegionTimestamps),
                )
            };

            Ok(MCRegionReader {
                options,
                data,
                headers,
                timestamps,
            })
        }
    }
}

impl<'a> private::Sealed for MCRegionReader<'a> {}

impl<'a> Deref for MCRegionReader<'a> {
    type Target = &'a [u8];

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<'a> RegionReaderImpl<'a> for MCRegionReader<'a> {
    fn get_header_unchecked(&self, x: u8, z: u8) -> &MCRegionHeader {
        let idx = chunk_idx_fast(x, z);
        // Safety: idx < 1024
        unsafe { self.headers.0.get_unchecked(idx) }
    }

    fn get_timestamp_unchecked(&self, x: u8, z: u8) -> u32 {
        let idx = chunk_idx_fast(x, z);
        // Safety: idx < 1024
        unsafe { *self.timestamps.0.get_unchecked(idx) }
    }

    fn options(&self) -> &Options {
        &self.options
    }
}

#[repr(C, align(4))]
#[derive(Debug, Clone, Copy, Default)]
pub struct MCRegionHeader(pub [u8; 4]);

impl MCRegionHeader {
    pub const fn offset(&self) -> u32 {
        u32::from_be_bytes(self.0) >> 8
    }

    pub const fn sector_count(&self) -> u8 {
        self.0[3]
    }

    pub const fn new(offset: usize, sector_count: u8) -> Option<Self> {
        if offset <= 0xFFFFFF {
            Some(Self(
                ((offset as u32) << 8 | (sector_count as u32)).to_be_bytes(),
            ))
        } else {
            None
        }
    }

    pub const fn unpack(&self) -> (u32, u8) {
        (self.offset(), self.sector_count())
    }

    pub fn is_none(&self) -> bool {
        self.0 == [0, 0, 0, 0]
    }
}

#[repr(C, align(4096))]
#[derive(Debug, Clone)]
pub struct MCRegionHeaders(pub [MCRegionHeader; 1024]);

#[repr(C, align(4096))]
#[derive(Debug, Clone)]
pub struct MCRegionTimestamps(pub [u32; 1024]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkCompression {
    Gzip,
    Zlib,
    None,
    Lz4,
}

#[derive(Debug, Clone, Copy)]
pub struct CompressedChunk<'a>(&'a [u8]);

impl<'a> Borrow<[u8]> for CompressedChunk<'a> {
    fn borrow(&self) -> &[u8] {
        self.0
    }
}

impl<'a> AsRef<[u8]> for CompressedChunk<'a> {
    fn as_ref(&self) -> &[u8] {
        self.0
    }
}

impl<'a> Deref for CompressedChunk<'a> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.0
    }
}

impl<'a> CompressedChunk<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self(data)
    }

    pub fn decompress_to(
        &self,
        buf: &mut Vec<u8>,
        compression: ChunkCompression,
    ) -> Result<usize, CompressionError> {
        match compression {
            ChunkCompression::None => {
                buf.extend_from_slice(self);
                Ok(self.len())
            }
            ChunkCompression::Gzip => {
                let mut decoder = GzDecoder::new(self.as_ref());
                decoder.read_to_end(buf).map_err(CompressionError::Gzip)
            }
            ChunkCompression::Zlib => {
                let mut decoder = ZlibDecoder::new(self.as_ref());
                decoder.read_to_end(buf).map_err(CompressionError::Zlib)
            }
            ChunkCompression::Lz4 => {
                let mut decoder = Lz4BlockInput::new(self.as_ref());
                decoder.read_to_end(buf).map_err(CompressionError::Lz4)
            }
        }
    }

    pub fn decompress_to_nbt(
        &self,
        compression: ChunkCompression,
    ) -> Result<(usize, Nbt), RegionReadError> {
        match compression {
            ChunkCompression::None => simdnbt::owned::read(&mut Cursor::new(self))
                .map(|nbt| (self.len(), nbt))
                .map_err(RegionReadError::NbtError),
            _ => {
                let mut buf = Vec::with_capacity(8 * SECTOR_SIZE);
                self.decompress_to(&mut buf, compression)
                    .map_err(RegionReadError::CompressionError)?;
                simdnbt::owned::read(&mut Cursor::new(&buf))
                    .map(|nbt| (buf.len(), nbt))
                    .map_err(RegionReadError::NbtError)
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum RawChunkData<'a> {
    None,
    Chunk {
        bytes: CompressedChunk<'a>,
        offset_sector: u32,
        timestamp: u32,
        compression: ChunkCompression,
    },
    External {
        offset_sector: u32,
        timestamp: u32,
        compression: ChunkCompression,
    },
}

pub struct DecompressedChunk<'a> {
    bytes: &'a [u8],
    compression: ChunkCompression,
}

impl<'a> DecompressedChunk<'a> {
    pub fn new(bytes: &'a [u8], compression: ChunkCompression) -> Self {
        DecompressedChunk { bytes, compression }
    }

    pub fn get_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub fn get_compression(&self) -> ChunkCompression {
        self.compression
    }
}

pub struct BTreeFreeSectorAllocator {
    // start -> len
    tree: BTreeMap<usize, usize>,
}

impl BTreeFreeSectorAllocator {
    pub fn new(sector_count: usize) -> Option<Self> {
        if sector_count > isize::MAX as usize {
            None
        } else {
            Some(Self {
                tree: BTreeMap::from_iter([(0, sector_count)]),
            })
        }
    }

    pub fn mark_allocated(&mut self, begin_sector: usize, count: usize) -> bool {
        if count == 0 {
            return false;
        }

        let Some((&free_begin, &free_len)) = self.tree.range(..=begin_sector).next_back() else {
            return false;
        };

        let free_end = free_begin + free_len;
        let alloc_end = begin_sector + count;

        if alloc_end > free_end {
            return false;
        }

        self.tree.remove(&free_begin);

        // Left remainder.
        if begin_sector > free_begin {
            self.tree.insert(free_begin, begin_sector - free_begin);
        }

        // Right remainder.
        if alloc_end < free_end {
            self.tree.insert(alloc_end, free_end - alloc_end);
        }

        true
    }

    pub fn allocate(&mut self, count: usize) -> Option<usize> {
        if count == 0 {
            return None;
        }

        let (&start, &len) = self.tree.iter().find(|(_, len)| **len >= count)?;

        self.tree.remove(&start);

        if len > count {
            self.tree.insert(start + count, len - count);
        }

        Some(start)
    }

    pub fn free(&mut self, offset: usize, count: usize) {
        if count == 0 {
            return;
        }

        let mut start = offset;
        let mut len = count;

        // Merge with the free range immediately before
        if let Some((&prev_start, &prev_len)) = self.tree.range(..offset).next_back()
            && prev_start + prev_len == offset
        {
            start = prev_start;
            len += prev_len;
            self.tree.remove(&prev_start);
        }

        // Merge with the free range immediately after
        if let Some((&next_start, &next_len)) = self.tree.range(offset..).next()
            && offset + count == next_start
        {
            len += next_len;
            self.tree.remove(&next_start);
        }

        self.tree.insert(start, len);
    }
}

pub struct MCRegionWriter {
    options: Options,
    data: AVec<u8, ConstAlign<SECTOR_SIZE>>,
    allocator: BTreeFreeSectorAllocator,
}

impl MCRegionWriter {
    pub fn new(options: Options) -> Self {
        let mut data = AVec::with_capacity(SECTOR_SIZE, SECTOR_SIZE * 2);
        data.resize(SECTOR_SIZE * 2, 0);

        let mut allocator = BTreeFreeSectorAllocator::new(2).unwrap();
        allocator.mark_allocated(0, 2);

        Self {
            options,
            data,
            allocator,
        }
    }

    fn build_allocator(
        options: Options,
        data: &AVec<u8, ConstAlign<SECTOR_SIZE>>,
    ) -> Result<BTreeFreeSectorAllocator, RegionReadError> {
        let mut allocator = BTreeFreeSectorAllocator::new(data.len() / SECTOR_SIZE).unwrap();
        allocator.mark_allocated(0, 2);

        let reader = MCRegionReader::new(options, data)?;

        for header in reader.headers.0.iter() {
            if let Some((offset, sectors)) = reader.actual_sector_count(header)? {
                allocator.mark_allocated(offset, sectors);
            }
        }

        Ok(allocator)
    }

    /// # Safety
    /// ptr must be 4096 bytes aligned, len is the length of the region data, and capacity is the size of the allocation pointed to by ptr
    pub unsafe fn from_raw_parts(
        options: Options,
        ptr: *mut u8,
        len: usize,
        capacity: usize,
    ) -> Result<Self, RegionReadError> {
        // Safety: The pointer must be correctly aligned, len is the length of the data, and capacity is the size of the allocation
        let data = unsafe { AVec::from_raw_parts(ptr, SECTOR_SIZE, len, capacity) };
        Self::from_aligned_vec(options, data)
    }

    pub fn from_aligned_vec(
        options: Options,
        data: AVec<u8, ConstAlign<SECTOR_SIZE>>,
    ) -> Result<Self, RegionReadError> {
        if data.len() < 2 * SECTOR_SIZE {
            return Err(RegionReadError::MissingHeaders);
        }

        if !data.len().is_multiple_of(SECTOR_SIZE) {
            return Err(RegionReadError::InvalidBackingBufferSize(data.len()));
        }

        let allocator = Self::build_allocator(options, &data)?;

        Ok(Self {
            options,
            data,
            allocator,
        })
    }

    pub fn new_from_owned_bytes(
        options: Options,
        bytes: Box<[u8]>,
    ) -> Result<Self, RegionReadError> {
        if bytes.len() < 2 * SECTOR_SIZE {
            return Err(RegionReadError::MissingHeaders);
        }

        if !bytes.len().is_multiple_of(SECTOR_SIZE) {
            return Err(RegionReadError::InvalidBackingBufferSize(bytes.len()));
        }

        if (bytes.as_ptr() as usize).is_multiple_of(SECTOR_SIZE) {
            let ptr: *mut [u8] = Box::into_raw(bytes);

            // Safety: The pointer is correctly aligned, and the allocation is of exactly the size of the slice
            let data =
                unsafe { AVec::from_raw_parts(ptr as *mut u8, SECTOR_SIZE, ptr.len(), ptr.len()) };

            let allocator = Self::build_allocator(options, &data)?;

            Ok(Self {
                options,
                data,
                allocator,
            })
        } else {
            Self::new_from_slice(options, &bytes)
        }
    }

    pub fn new_from_slice(options: Options, bytes: &[u8]) -> Result<Self, RegionReadError> {
        let data = AVec::from_slice(SECTOR_SIZE, bytes);
        let allocator = Self::build_allocator(options, &data)?;

        Ok(Self {
            options,
            data,
            allocator,
        })
    }

    pub fn reader<'a>(&'a self) -> WrappedMCRegionReader<'a> {
        WrappedMCRegionReader(&self.data, &self.options)
    }

    pub fn get_header_unchecked_mut(&mut self, x: u8, z: u8) -> &mut MCRegionHeader {
        let idx = chunk_idx_fast(x, z);
        // Safe: idx < 1024
        unsafe {
            &mut *(self
                .data
                .get_unchecked_mut(4 * idx..4 * idx + 4)
                .as_mut_ptr() as *mut MCRegionHeader)
        }
    }

    pub fn set_timestamp_unchecked(&self, x: u8, z: u8, timestamp: u32) {
        let idx = chunk_idx_fast(x, z);
        // Safe: idx < 1024
        unsafe {
            *(self
                .data
                .get_unchecked(4 * idx + 4100..4 * idx + 4100)
                .as_ptr() as *mut u32) = timestamp;
        }
    }

    fn helper_free_old(&mut self, header: MCRegionHeader) -> Result<(), RegionWriteError> {
        let reader = self.reader();
        if let Some((offset, count)) = reader
            .actual_sector_count(&header)
            .map_err(RegionWriteError::ReadingError)?
        {
            self.allocator.free(offset, count);
        }
        Ok(())
    }

    fn get_or_grow(&mut self, range: Range<usize>) -> &mut [u8] {
        let need_bytes = range.end.div_ceil(SECTOR_SIZE) * SECTOR_SIZE;

        if need_bytes > self.data.len() {
            self.data.resize(need_bytes, 0);
        }

        &mut self.data[range]
    }

    pub fn write_chunk_data_compressed_unchecked(
        &mut self,
        x: u8,
        z: u8,
        compressed_data: &[u8],
        compression: ChunkCompression,
    ) -> Result<usize, RegionWriteError> {
        let old = *self.get_header_unchecked_mut(x, z);
        self.helper_free_old(old)?;

        let field_len = 1 + compressed_data.len();

        if field_len > u32::MAX as usize {
            return Err(RegionWriteError::UnrepresentableChunkLength(field_len));
        }

        let sector_count = (field_len + 5).div_ceil(SECTOR_SIZE);

        if sector_count > 255 && !self.options.allow_oversized_chunks {
            return Err(RegionWriteError::OversizedChunk(sector_count));
        }

        let offset = self
            .allocator
            .allocate(sector_count)
            .unwrap_or(self.data.len().div_ceil(SECTOR_SIZE));

        let Some(new_header) = MCRegionHeader::new(offset, sector_count.min(255) as u8) else {
            // User messed up
            return Err(RegionWriteError::NotEnoguhFreeSectors);
        };

        let compression_byte = match compression {
            ChunkCompression::Gzip => 1,
            ChunkCompression::Zlib => 2,
            ChunkCompression::None => 3,
            ChunkCompression::Lz4 => 4,
        };

        let bytes =
            self.get_or_grow((offset * SECTOR_SIZE)..(offset * SECTOR_SIZE + 4 + field_len));

        // Safety: The slice is a valid reference of the right length and it has the correct alignment
        unsafe {
            core::ptr::write(bytes.as_mut_ptr() as *mut u32, u32::to_be(field_len as u32));
            *bytes.get_unchecked_mut(4) = compression_byte;
            bytes
                .get_unchecked_mut(5..)
                .copy_from_slice(compressed_data);
        }

        *self.get_header_unchecked_mut(x, z) = new_header;
        Ok(compressed_data.len())
    }

    pub fn write_chunk_data_compressed(
        &mut self,
        x: u8,
        z: u8,
        compressed_data: &[u8],
        compression: ChunkCompression,
    ) -> Result<usize, RegionWriteError> {
        if x <= REGION_SIZE && z <= REGION_SIZE {
            self.write_chunk_data_compressed_unchecked(x, z, compressed_data, compression)
        } else {
            Err(RegionWriteError::InvalidChunkCoords(x, z))
        }
    }

    pub fn write_chunk_data_uncompressed_unchecked(
        &mut self,
        x: u8,
        z: u8,
        uncompressed_data: &[u8],
        compression: ChunkCompression,
        compression_level: Compression,
        timestamp: Option<u32>,
    ) -> Result<usize, RegionWriteError> {
        let mut compressed_data = Vec::with_capacity(8 * SECTOR_SIZE);

        match compression {
            ChunkCompression::None => {}
            ChunkCompression::Gzip => {
                let mut encoder = GzEncoder::new(&mut compressed_data, compression_level);
                encoder
                    .write_all(uncompressed_data)
                    .map_err(CompressionError::Gzip)
                    .map_err(RegionWriteError::CompressionError)?;
            }
            ChunkCompression::Zlib => {
                let mut encoder = ZlibEncoder::new(&mut compressed_data, compression_level);
                encoder
                    .write_all(uncompressed_data)
                    .map_err(CompressionError::Zlib)
                    .map_err(RegionWriteError::CompressionError)?;
            }
            ChunkCompression::Lz4 => {
                let mut encoder = Lz4BlockOutput::new(&mut compressed_data);
                encoder
                    .write_all(uncompressed_data)
                    .map_err(CompressionError::Lz4)
                    .map_err(RegionWriteError::CompressionError)?;
            }
        };

        if let Some(timestamp) = timestamp {
            self.set_timestamp_unchecked(x, z, timestamp);
        }

        self.write_chunk_data_compressed_unchecked(
            x,
            z,
            match compression {
                ChunkCompression::None => uncompressed_data,
                ChunkCompression::Gzip | ChunkCompression::Zlib | ChunkCompression::Lz4 => {
                    &compressed_data
                }
            },
            compression,
        )
    }

    pub fn write_chunk_data_uncompressed(
        &mut self,
        x: u8,
        z: u8,
        uncompressed_data: &[u8],
        compression: ChunkCompression,
        compression_level: Compression,
        timestamp: Option<u32>,
    ) -> Result<usize, RegionWriteError> {
        if x <= REGION_SIZE && z <= REGION_SIZE {
            self.write_chunk_data_uncompressed_unchecked(
                x,
                z,
                uncompressed_data,
                compression,
                compression_level,
                timestamp,
            )
        } else {
            Err(RegionWriteError::InvalidChunkCoords(x, z))
        }
    }

    pub fn write_chunk_external_unchecked(
        &mut self,
        x: u8,
        z: u8,
        compression: ChunkCompression,
        timestamp: Option<u32>,
    ) -> Result<(), RegionWriteError> {
        let old = *self.get_header_unchecked_mut(x, z);
        self.helper_free_old(old)?;

        let compression_byte = match compression {
            ChunkCompression::Gzip => 1,
            ChunkCompression::Zlib => 2,
            ChunkCompression::None => 3,
            ChunkCompression::Lz4 => 4,
        };

        let offset = self
            .allocator
            .allocate(1)
            .unwrap_or(self.data.len().div_ceil(SECTOR_SIZE));

        let Some(new_header) = MCRegionHeader::new(offset, 1) else {
            // User messed up
            return Err(RegionWriteError::NotEnoguhFreeSectors);
        };

        let bytes = self.get_or_grow((offset * SECTOR_SIZE)..(offset * SECTOR_SIZE + 5));

        // Safety: The slice is a valid reference of the right length and it has the correct alignment
        unsafe {
            core::ptr::write(bytes.as_mut_ptr() as *mut u32, u32::to_be(1));
            *bytes.get_unchecked_mut(4) = compression_byte;
        }

        if let Some(timestamp) = timestamp {
            self.set_timestamp_unchecked(x, z, timestamp);
        }

        *self.get_header_unchecked_mut(x, z) = new_header;

        Ok(())
    }

    pub fn write_chunk_external(
        &mut self,
        x: u8,
        z: u8,
        compression: ChunkCompression,
        timestamp: Option<u32>,
    ) -> Result<(), RegionWriteError> {
        if x <= REGION_SIZE && z <= REGION_SIZE {
            self.write_chunk_external_unchecked(x, z, compression, timestamp)
        } else {
            Err(RegionWriteError::InvalidChunkCoords(x, z))
        }
    }

    pub fn write_chunk_free_unchecked(&mut self, x: u8, z: u8) -> Result<(), RegionWriteError> {
        let old = *self.get_header_unchecked_mut(x, z);
        self.helper_free_old(old)?;
        self.set_timestamp_unchecked(x, z, 0);
        Ok(())
    }

    pub fn write_chunk_free(&mut self, x: u8, z: u8) -> Result<(), RegionWriteError> {
        if x <= REGION_SIZE && z <= REGION_SIZE {
            self.write_chunk_free_unchecked(x, z)
        } else {
            Err(RegionWriteError::InvalidChunkCoords(x, z))
        }
    }
}

pub struct WrappedMCRegionReader<'a>(&'a [u8], &'a Options);

impl<'a> private::Sealed for WrappedMCRegionReader<'a> {}

impl<'a> Deref for WrappedMCRegionReader<'a> {
    type Target = &'a [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> RegionReaderImpl<'a> for WrappedMCRegionReader<'a> {
    fn get_header_unchecked(&self, x: u8, z: u8) -> &MCRegionHeader {
        let idx = chunk_idx_fast(x, z);
        // Safe: idx < 1024
        unsafe { &*(self.0.get_unchecked(4 * idx..4 * idx + 4).as_ptr() as *const MCRegionHeader) }
    }

    fn get_timestamp_unchecked(&self, x: u8, z: u8) -> u32 {
        let idx = chunk_idx_fast(x, z);
        // Safe: idx < 1024
        unsafe {
            *(self
                .0
                .get_unchecked(4 * idx + 4100..4 * idx + 4100)
                .as_ptr() as *const u32)
        }
    }

    fn options(&self) -> &Options {
        self.1
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ChunkIteratorDirection {
    /// In order of the region file headers. When enumerating: idx == x + 32*z
    Natural,
    // When enumerating: idx == 32*x + z
    Transpose,
}

impl IntoIterator for ChunkIteratorDirection {
    type Item = (u8, u8);
    type IntoIter = ChunkCoordsIterator;

    fn into_iter(self) -> Self::IntoIter {
        ChunkCoordsIterator::new(self)
    }
}

impl IntoIterator for &ChunkIteratorDirection {
    type Item = (u8, u8);
    type IntoIter = ChunkCoordsIterator;

    fn into_iter(self) -> Self::IntoIter {
        ChunkCoordsIterator::new(*self)
    }
}

impl IntoIterator for &mut ChunkIteratorDirection {
    type Item = (u8, u8);
    type IntoIter = ChunkCoordsIterator;

    fn into_iter(self) -> Self::IntoIter {
        ChunkCoordsIterator::new(*self)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ChunkCoordsIterator {
    direction: ChunkIteratorDirection,
    x: u8,
    z: u8,
}

impl ChunkCoordsIterator {
    pub fn new(direction: ChunkIteratorDirection) -> Self {
        Self {
            direction,
            x: 0,
            z: 0,
        }
    }
}

impl Iterator for ChunkCoordsIterator {
    type Item = (u8, u8);

    fn next(&mut self) -> Option<Self::Item> {
        let Self { x, z, direction } = *self;
        match direction {
            ChunkIteratorDirection::Natural => {
                if z >= REGION_SIZE {
                    return None;
                }
                self.x += 1;
                if self.x.is_multiple_of(REGION_SIZE) {
                    self.x = 0;
                    self.z += 1;
                }
            }
            ChunkIteratorDirection::Transpose => {
                if x >= REGION_SIZE {
                    return None;
                }
                self.z += 1;
                if self.z.is_multiple_of(REGION_SIZE) {
                    self.z = 0;
                    self.x += 1;
                }
            }
        }
        Some((x, z))
    }

    fn count(self) -> usize {
        let idx = self.x as usize + REGION_SIZE as usize * self.z as usize;
        REGION_SIZE_SQ.saturating_sub(idx)
    }
}
