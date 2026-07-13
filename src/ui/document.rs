use std::{
    f32, f64,
    fs::{self, OpenOptions},
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
use rfd::AsyncFileDialog;
use simdnbt::owned::{BaseNbt, Nbt, NbtCompound, NbtList, NbtTag};

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
}

#[derive(Debug, thiserror::Error)]
pub enum NbtParseError {
    #[error("all NBT decoders failed")]
    AllFailed {
        gzip: anyhow::Error,
        zlib: anyhow::Error,
        raw: anyhow::Error,
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
            DocumentData::Nbt(_, _) => {
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

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, NbtParseError> {
        let path = path.as_ref();
        let bytes = fs::read(path)?;

        let raw = match Self::try_from_raw(&bytes) {
            Ok(s) => return Ok(s),
            Err(e) => e,
        };

        let gzip = match Self::try_from_gzip(&bytes) {
            Ok(s) => return Ok(s),
            Err(e) => e,
        };

        let zlib = match Self::try_from_zlib(&bytes) {
            Ok(s) => return Ok(s),
            Err(e) => e,
        };

        Err(NbtParseError::AllFailed { raw, gzip, zlib })
    }

    pub fn example_nbt() -> Self {
        Self::Nbt(
            Nbt::Some(BaseNbt::new(
                "",
                NbtCompound::from_values(vec![
                    ("byte_value".into(), NbtTag::Byte(8)),
                    ("short_value".into(), NbtTag::Short(16)),
                    ("int_value".into(), NbtTag::Int(32)),
                    ("long_value".into(), NbtTag::Long(64)),
                    ("float_value".into(), NbtTag::Float(f32::consts::E)),
                    ("double_value".into(), NbtTag::Double(f64::consts::PI)),
                    (
                        "string_value".into(),
                        NbtTag::String("Hello, World !".into()),
                    ),
                    (
                        "byte_array_value".into(),
                        NbtTag::ByteArray((0u8..=255u8).collect()),
                    ),
                    (
                        "int_array_value".into(),
                        NbtTag::IntArray(
                            (0i32..20i32)
                                .map(|n| n.wrapping_mul(314_159_265).wrapping_add(271_828_182))
                                .collect(),
                        ),
                    ),
                    (
                        "long_array_value".into(),
                        NbtTag::LongArray(
                            (0i64..20i64)
                                .map(|n| {
                                    n.wrapping_mul(2_718_281_828_459_045_235)
                                        .wrapping_add(3_141_592_653_589_793_238)
                                })
                                .collect(),
                        ),
                    ),
                    (
                        "compound_value".into(),
                        NbtTag::Compound(NbtCompound::from_values(vec![
                            ("byte_value".into(), NbtTag::Byte(8)),
                            ("short_value".into(), NbtTag::Short(16)),
                            ("int_value".into(), NbtTag::Int(32)),
                            ("long_value".into(), NbtTag::Long(64)),
                            ("float_value".into(), NbtTag::Float(f32::consts::E)),
                            ("double_value".into(), NbtTag::Double(f64::consts::PI)),
                            (
                                "empty_compound".into(),
                                NbtTag::Compound(NbtCompound::new()),
                            ),
                            ("empty_list".into(), NbtTag::List(NbtList::Empty)),
                            (
                                "empty_list_of_bytes".into(),
                                NbtTag::List(NbtList::Byte(vec![])),
                            ),
                            (
                                "empty_list_of_lists".into(),
                                NbtTag::List(NbtList::List(vec![])),
                            ),
                            ("empty_byte_array".into(), NbtTag::ByteArray(vec![])),
                            ("empty_int_array".into(), NbtTag::IntArray(vec![])),
                            ("empty_long_array".into(), NbtTag::LongArray(vec![])),
                        ])),
                    ),
                    (
                        "lists".into(),
                        NbtTag::Compound(NbtCompound::from_values(vec![
                            (
                                "of_bytes".into(),
                                NbtTag::List(NbtList::Byte((-128i8..=127i8).collect())),
                            ),
                            (
                                "of_shorts".into(),
                                NbtTag::List(NbtList::Short(
                                    (0i16..20i16)
                                        .map(|v| v.wrapping_mul(31_415).wrapping_add(27_182))
                                        .collect(),
                                )),
                            ),
                            (
                                "of_ints".into(),
                                NbtTag::List(NbtList::Int(
                                    (0i32..20i32)
                                        .rev()
                                        .map(|n| {
                                            n.wrapping_mul(314_159_265).wrapping_add(271_828_182)
                                        })
                                        .collect(),
                                )),
                            ),
                            (
                                "of_longs".into(),
                                NbtTag::List(NbtList::Long(
                                    (0i64..20i64)
                                        .rev()
                                        .map(|n| {
                                            n.wrapping_mul(2_718_281_828_459_045_235)
                                                .wrapping_add(3_141_592_653_589_793_238)
                                        })
                                        .collect(),
                                )),
                            ),
                            (
                                "of_floats".into(),
                                NbtTag::List(NbtList::Float(vec![
                                    f32::consts::PI,
                                    f32::consts::TAU,
                                    f32::consts::GOLDEN_RATIO,
                                    f32::consts::EULER_GAMMA,
                                    f32::consts::E,
                                    f32::consts::LN_2,
                                ])),
                            ),
                            (
                                "of_doubles".into(),
                                NbtTag::List(NbtList::Double(vec![
                                    f64::consts::PI,
                                    f64::consts::TAU,
                                    f64::consts::GOLDEN_RATIO,
                                    f64::consts::EULER_GAMMA,
                                    f64::consts::E,
                                    f64::consts::LN_2,
                                ])),
                            ),
                            (
                                "of_strings".into(),
                                NbtTag::List(NbtList::String(vec![
                                    "Hello".into(),
                                    ", World".into(),
                                    "!".into(),
                                    "Next one is an empty string".into(),
                                    "".into(),
                                ])),
                            ),
                            (
                                "of_compounds".into(),
                                NbtTag::List(NbtList::Compound(vec![
                                    NbtCompound::from_values(vec![
                                        (
                                            "Just a list of".into(),
                                            NbtTag::String("NBT compounds".into()),
                                        ),
                                        (
                                            "Next two compounds in this list".into(),
                                            NbtTag::String("are empty".into()),
                                        ),
                                    ]),
                                    NbtCompound::from_values(vec![]),
                                    NbtCompound::from_values(vec![]),
                                    NbtCompound::from_values(vec![
                                        (
                                            "Just a list of".into(),
                                            NbtTag::String("NBT compounds".into()),
                                        ),
                                        ("This one".into(), NbtTag::String("is not empty".into())),
                                    ]),
                                ])),
                            ),
                            (
                                "of_lists".into(),
                                NbtTag::List(NbtList::List(vec![
                                    NbtList::Empty,
                                    NbtList::String(vec![
                                        "A".into(),
                                        "list".into(),
                                        "of".into(),
                                        "lists".into(),
                                        "??".into(),
                                    ]),
                                    NbtList::Int((0..10).collect()),
                                    NbtList::List(vec![
                                        NbtList::String(vec![
                                            "A".into(),
                                            "list".into(),
                                            "of".into(),
                                            "lists".into(),
                                            "??".into(),
                                            "of".into(),
                                            "lists".into(),
                                            "????".into(),
                                        ]),
                                        NbtList::List(vec![NbtList::String(vec![
                                            "A".into(),
                                            "list".into(),
                                            "of".into(),
                                            "lists".into(),
                                            "??".into(),
                                            "of".into(),
                                            "lists".into(),
                                            "????".into(),
                                            "of".into(),
                                            "lists".into(),
                                            "??????".into(),
                                            "WHAT ???".into(),
                                        ])]),
                                    ]),
                                ])),
                            ),
                            (
                                "of_byte_arrays".into(),
                                NbtTag::List(NbtList::ByteArray(
                                    (0u8..16u8)
                                        .map(|hi| (0u8..16u8).map(|lo| (hi << 4) | lo).collect())
                                        .collect(),
                                )),
                            ),
                            (
                                "of_int_arrays".into(),
                                NbtTag::List(NbtList::IntArray(
                                    (0..10)
                                        .map(|pow| {
                                            (0..10)
                                                .map(|mul| 10i32.pow(pow).wrapping_mul(mul))
                                                .collect()
                                        })
                                        .collect(),
                                )),
                            ),
                            (
                                "of_long_arrays".into(),
                                NbtTag::List(NbtList::LongArray(
                                    (0..20)
                                        .map(|pow| {
                                            (0..10)
                                                .map(|mul| {
                                                    10i64.wrapping_pow(pow).wrapping_mul(mul)
                                                })
                                                .collect()
                                        })
                                        .collect(),
                                )),
                            ),
                        ])),
                    ),
                    ("are we done yet ?".into(), NbtTag::String("".into())),
                ]),
            )),
            NbtCompression::None,
        )
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
