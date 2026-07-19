use std::{
    fs::{File, OpenOptions},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use anyhow::anyhow;
use egui::Id;
use flate2::{
    Compression,
    read::{GzDecoder, ZlibDecoder},
    write::{GzEncoder, ZlibEncoder},
};
use memmap2::Mmap;
use rfd::AsyncFileDialog;
use simdnbt::owned::Nbt;

use crate::mcregion::{
    ChunkCompression, ChunkIteratorDirection, MCRegionReader, MCRegionWriter, Options, REGION_SIZE,
    RawChunkData, RegionReadError, RegionReaderImpl, RegionWriteError, SECTOR_SIZE, chunk_idx_fast,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NbtCompression {
    None,
    Gzip,
    Zlib,
}

pub enum DocumentData {
    Saving,
    Loading,
    ReadError(Arc<anyhow::Error>),
    Nbt(Nbt, NbtCompression),
    MCRegion(Box<MCRegionEditor>, ChunkIteratorDirection),
}

#[derive(Debug, thiserror::Error)]
pub enum NbtParseError {
    #[error("all NBT decoders failed")]
    AllFailed {
        gzip: anyhow::Error,
        zlib: anyhow::Error,
        raw: anyhow::Error,
        mcregion: anyhow::Error,
    },
    #[error("I/O Error")]
    Io(std::io::Error),
}

impl From<std::io::Error> for NbtParseError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl DocumentData {
    pub fn take_to_save(&mut self) -> DocumentData {
        match self {
            DocumentData::Loading => DocumentData::Loading,
            DocumentData::Saving => DocumentData::Saving,
            DocumentData::ReadError(e) => DocumentData::ReadError(Arc::clone(e)),
            DocumentData::Nbt(_, _) | DocumentData::MCRegion(_, _) => {
                let mut to_swap = DocumentData::Saving;
                std::mem::swap(&mut to_swap, self);
                to_swap
            }
        }
    }

    #[inline(always)]
    fn try_from_gzip(bytes: &[u8]) -> anyhow::Result<Self> {
        let mut decoder = GzDecoder::new(bytes);
        let mut out = Vec::new();
        decoder.read_to_end(&mut out)?;
        let nbt = simdnbt::owned::read(&mut Cursor::new(&out))?;
        Ok(Self::Nbt(nbt, NbtCompression::Gzip))
    }

    #[inline(always)]
    fn try_from_zlib(bytes: &[u8]) -> anyhow::Result<Self> {
        let mut decoder = ZlibDecoder::new(bytes);
        let mut out = Vec::new();
        decoder.read_to_end(&mut out)?;
        let nbt = simdnbt::owned::read(&mut Cursor::new(&out))?;
        Ok(Self::Nbt(nbt, NbtCompression::Zlib))
    }

    #[inline(always)]
    fn try_from_raw(bytes: &[u8]) -> anyhow::Result<Self> {
        let nbt = simdnbt::owned::read(&mut Cursor::new(bytes))?;
        Ok(Self::Nbt(nbt, NbtCompression::None))
    }

    #[inline(always)]
    fn try_from_mcregion(bytes: &[u8]) -> anyhow::Result<Self> {
        let reader = MCRegionReader::new(
            Options {
                allow_oversized_chunks: true,
            },
            bytes,
        )?;
        let editor = Box::new(MCRegionEditor::new(&reader)?);
        Ok(Self::MCRegion(editor, ChunkIteratorDirection::Natural))
    }

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, NbtParseError> {
        let path = path.as_ref();
        let file = File::open(path)?;

        let mmap = unsafe { Mmap::map(&file)? };

        let gzip = match Self::try_from_gzip(&mmap) {
            Ok(s) => return Ok(s),
            Err(e) => e,
        };

        let zlib = match Self::try_from_zlib(&mmap) {
            Ok(s) => return Ok(s),
            Err(e) => e,
        };

        let mcregion = match Self::try_from_mcregion(&mmap) {
            Ok(s) => return Ok(s),
            Err(e) => e,
        };

        let raw = match Self::try_from_raw(&mmap) {
            Ok(s) => return Ok(s),
            Err(e) => e,
        };

        Err(NbtParseError::AllFailed {
            raw,
            gzip,
            zlib,
            mcregion,
        })
    }
}

pub struct NbtDocumentTab {
    pub title_short: String,
    pub title_long: String,
    pub tab_id: usize,
    pub root_id: Id,
    pub modified: bool,
    pub saved_location: Option<PathBuf>,
    pub data: DocumentData,
}

impl NbtDocumentTab {
    pub fn new_titled(title: String) -> Self {
        Self {
            title_short: title.clone(),
            title_long: title,
            modified: true,
            saved_location: None,
            data: DocumentData::Nbt(Nbt::None, NbtCompression::Gzip),
            tab_id: 0,
            root_id: Id::new("tab").with(SystemTime::now()),
        }
    }

    pub fn new(path: PathBuf) -> Self {
        Self {
            title_short: path
                .file_name()
                .map(|osstr| osstr.to_string_lossy().to_string())
                .unwrap_or_else(|| String::from("Untitled")),
            title_long: path.to_string_lossy().to_string(),
            modified: false,
            saved_location: Some(path),
            data: DocumentData::Loading,
            root_id: Id::new("tab").with(SystemTime::now()),
            tab_id: 0,
        }
    }

    pub fn update_id(&mut self, tab_id: usize) {
        self.tab_id = tab_id;
        self.root_id = Id::new("tab").with(SystemTime::now()).with(tab_id);
    }

    pub fn action_save_as(
        &mut self,
    ) -> impl Future<Output = (Option<PathBuf>, DocumentData, anyhow::Result<()>)> + 'static + use<>
    {
        let last_save_loc = self.saved_location.take();
        let task = self.action_save();
        self.saved_location = last_save_loc;
        task
    }

    pub fn action_save(
        &mut self,
    ) -> impl Future<Output = (Option<PathBuf>, DocumentData, anyhow::Result<()>)> + 'static + use<>
    {
        let save_data = self.data.take_to_save();
        let save_loc = self.saved_location.clone();
        async move {
            if matches!(
                save_data,
                DocumentData::Loading | DocumentData::Saving | DocumentData::ReadError(..)
            ) {
                return (save_loc, save_data, Ok(()));
            }

            let save_loc = match save_loc {
                Some(l) => l,
                None => {
                    if let Some(l) = AsyncFileDialog::new()
                        .set_can_create_directories(true)
                        .save_file()
                        .await
                    {
                        l.path().into()
                    } else {
                        return (save_loc, save_data, Ok(()));
                    }
                }
            };

            match save_data {
                DocumentData::Loading | DocumentData::Saving | DocumentData::ReadError(..) => {
                    (Some(save_loc), save_data, Ok(()))
                }
                DocumentData::Nbt(nbt, compression) => tokio::task::spawn_blocking(move || {
                    let mut buffer = Vec::new();
                    nbt.write(&mut buffer);

                    let r: anyhow::Result<()> = OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(true)
                        .open(&save_loc)
                        .and_then(|mut f| {
                            let data = match compression {
                                NbtCompression::None => buffer,
                                NbtCompression::Gzip => {
                                    let mut encoder =
                                        GzEncoder::new(Vec::new(), Compression::default());
                                    encoder.write_all(&buffer)?;
                                    encoder.finish()?
                                }
                                NbtCompression::Zlib => {
                                    let mut encoder =
                                        ZlibEncoder::new(Vec::new(), Compression::default());
                                    encoder.write_all(&buffer)?;
                                    encoder.finish()?
                                }
                            };
                            f.write_all(&data)
                        })
                        .map_err(|e| e.into());

                    (Some(save_loc), DocumentData::Nbt(nbt, compression), r)
                })
                .await
                .unwrap(),
                DocumentData::MCRegion(mut reg, dir) => tokio::task::spawn_blocking(move || {
                    let r: anyhow::Result<()> = OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(true)
                        .open(&save_loc)
                        .map_err(|e| e.into())
                        .and_then(|mut f| {
                            let writer = reg.create_writer(Options {
                                allow_oversized_chunks: true,
                            })?;
                            f.write_all(&writer.reader())?;
                            Ok(())
                        });

                    (Some(save_loc), DocumentData::MCRegion(reg, dir), r)
                })
                .await
                .unwrap(),
            }
        }
    }

    pub fn action_load(
        &mut self,
    ) -> impl Future<Output = (usize, anyhow::Result<DocumentData>)> + 'static {
        let skip = !matches!(self.data, DocumentData::Loading);
        let path = self.saved_location.clone();
        let tab_id = self.tab_id;

        async move {
            if !skip && let Some(path) = path {
                (
                    tab_id,
                    tokio::task::spawn_blocking(move || DocumentData::load_from_file(path))
                        .await
                        .map_err(Into::into)
                        .and_then(|inner| inner.map_err(Into::into)),
                )
            } else {
                (tab_id, Err(anyhow!("Invalid state")))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum ChunkData {
    NotGenerated,
    External {
        compression: ChunkCompression,
        timestamp: u32,
    },
    Chunk {
        compression: ChunkCompression,
        uncompressed_size_on_read: usize,
        compressed_size_on_read: usize,
        was_oversized: bool,
        timestamp: u32,
        nbt: Nbt,
    },
}

pub struct MCRegionEditor {
    chunk_cache: [ChunkData; 1024],
}

impl MCRegionEditor {
    pub fn new<'a, T: RegionReaderImpl<'a>>(reader: &T) -> Result<Self, RegionReadError> {
        Ok(Self {
            chunk_cache: ChunkIteratorDirection::Natural
                .into_iter()
                .map(|(x, z)| {
                    reader
                        .chunk_data_raw_unchecked(x, z)
                        .and_then(|raw| match raw {
                            RawChunkData::None => Ok(ChunkData::NotGenerated),
                            RawChunkData::External { compression, timestamp, .. } => {
                                Ok(ChunkData::External{compression,timestamp})
                            }
                            RawChunkData::Chunk {
                                compression, bytes, timestamp, ..
                            } => {
                                let (uncompressed_size, nbt) = bytes.decompress_to_nbt(compression)?;
                                Ok(ChunkData::Chunk{compression, nbt, timestamp, uncompressed_size_on_read: uncompressed_size, compressed_size_on_read: bytes.len(), was_oversized: bytes.len() > (255*SECTOR_SIZE - 5)})
                            }
                        })
                })
                .collect::<Result<Vec<_>, _>>()?
                .try_into()
                .expect("MCRegionEditor::new: ChunkIteratorDirection::Natural is expected to return exactly 1024 elements"),
        })
    }

    pub fn get(&mut self, x: u8, z: u8) -> Option<&mut ChunkData> {
        (x <= REGION_SIZE && z <= REGION_SIZE)
            .then_some(chunk_idx_fast(x, z))
            .and_then(|idx| self.chunk_cache.get_mut(idx))
    }

    pub fn create_writer(&mut self, options: Options) -> Result<MCRegionWriter, RegionWriteError> {
        let mut writer = MCRegionWriter::new(options);

        for ((x, z), chunk_data) in ChunkIteratorDirection::Natural
            .into_iter()
            .zip(&mut self.chunk_cache)
        {
            match chunk_data {
                ChunkData::NotGenerated => {}
                ChunkData::External {
                    compression,
                    timestamp,
                } => {
                    writer.write_chunk_external_unchecked(x, z, *compression, Some(*timestamp))?;
                }
                ChunkData::Chunk {
                    uncompressed_size_on_read,
                    compressed_size_on_read,
                    was_oversized,
                    compression,
                    timestamp,
                    nbt,
                } => {
                    let mut uncompressed_data = Vec::with_capacity(SECTOR_SIZE);
                    nbt.write(&mut uncompressed_data);

                    *uncompressed_size_on_read = uncompressed_data.len();

                    let compressed_size = writer.write_chunk_data_uncompressed_unchecked(
                        x,
                        z,
                        &uncompressed_data,
                        *compression,
                        Compression::default(),
                        Some(*timestamp),
                    )?;

                    *compressed_size_on_read = compressed_size;
                    *was_oversized = compressed_size > 255 * SECTOR_SIZE - 5;
                }
            }
        }

        Ok(writer)
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut ChunkData> {
        self.chunk_cache.iter_mut()
    }
}
