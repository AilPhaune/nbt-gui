use std::{f32, f64};

use simdnbt::owned::{BaseNbt, Nbt, NbtCompound, NbtList, NbtTag};

use crate::document::{DocumentData, NbtCompression};

pub fn example_nbt() -> DocumentData {
    DocumentData::Nbt(
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
                                    .map(|n| n.wrapping_mul(314_159_265).wrapping_add(271_828_182))
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
                                            .map(|mul| 10i64.wrapping_pow(pow).wrapping_mul(mul))
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

pub fn example_nbt_huge() -> DocumentData {
    DocumentData::Nbt(
        Nbt::Some(BaseNbt::new(
            "",
            NbtCompound::from_values(vec![
                (
                    "huge_lists".into(),
                    NbtTag::Compound(NbtCompound::from_values(vec![
                        (
                            "huge_1k_b".into(),
                            NbtTag::List(NbtList::Byte(
                                (0i32..1_000i32).map(|i| i as i8).collect(),
                            )),
                        ),
                        (
                            "huge_10k_b".into(),
                            NbtTag::List(NbtList::Byte(
                                (0i32..10_000i32).map(|i| i as i8).collect(),
                            )),
                        ),
                        (
                            "huge_100k_b".into(),
                            NbtTag::List(NbtList::Byte(
                                (0i32..100_000i32).map(|i| i as i8).collect(),
                            )),
                        ),
                        (
                            "huge_1M_b".into(),
                            NbtTag::List(NbtList::Byte(
                                (0i32..1_000_000i32).map(|i| i as i8).collect(),
                            )),
                        ),
                        (
                            "huge_10M_b".into(),
                            NbtTag::List(NbtList::Byte(
                                (0i32..10_000_000i32).map(|i| i as i8).collect(),
                            )),
                        ),
                    ])),
                ),
                (
                    "huge_compounds".into(),
                    NbtTag::Compound(NbtCompound::from_values(vec![
                        (
                            "huge_1k_b".into(),
                            NbtTag::Compound(NbtCompound::from_values(
                                (0i32..1_000i32)
                                    .map(|i| (i.to_string().into(), NbtTag::Int(i)))
                                    .collect(),
                            )),
                        ),
                        (
                            "huge_10k_b".into(),
                            NbtTag::Compound(NbtCompound::from_values(
                                (0i32..10_000i32)
                                    .map(|i| (i.to_string().into(), NbtTag::Int(i)))
                                    .collect(),
                            )),
                        ),
                        (
                            "huge_100k_b".into(),
                            NbtTag::Compound(NbtCompound::from_values(
                                (0i32..100_000i32)
                                    .map(|i| (i.to_string().into(), NbtTag::Int(i)))
                                    .collect(),
                            )),
                        ),
                        (
                            "huge_1M_b".into(),
                            NbtTag::Compound(NbtCompound::from_values(
                                (0i32..1_000_000i32)
                                    .map(|i| (i.to_string().into(), NbtTag::Int(i)))
                                    .collect(),
                            )),
                        ),
                        (
                            "huge_10M_b".into(),
                            NbtTag::Compound(NbtCompound::from_values(
                                (0i32..10_000_000i32)
                                    .map(|i| (i.to_string().into(), NbtTag::Int(i)))
                                    .collect(),
                            )),
                        ),
                    ])),
                ),
                (
                    "huge_arrays".into(),
                    NbtTag::Compound(NbtCompound::from_values(vec![
                        (
                            "bytes_10M".into(),
                            NbtTag::ByteArray((0i32..10_000_000i32).map(|i| i as u8).collect()),
                        ),
                        (
                            "ints_10M".into(),
                            NbtTag::IntArray((0i32..10_000_000i32).collect()),
                        ),
                        (
                            "longs_10M_b".into(),
                            NbtTag::LongArray((0i64..10_000_000i64).collect()),
                        ),
                    ])),
                ),
            ]),
        )),
        NbtCompression::None,
    )
}
