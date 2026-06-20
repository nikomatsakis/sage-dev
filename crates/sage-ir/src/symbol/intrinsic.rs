use sage_stash::StashDirect;

use crate::ty::{FloatTy, IntTy, UintTy};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum Intrinsic {
    Bool,
    Char,
    Str,
    Int(IntTy),
    Uint(UintTy),
    Float(FloatTy),
}

impl Intrinsic {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "bool" => Some(Self::Bool),
            "char" => Some(Self::Char),
            "str" => Some(Self::Str),
            "i8" => Some(Self::Int(IntTy::I8)),
            "i16" => Some(Self::Int(IntTy::I16)),
            "i32" => Some(Self::Int(IntTy::I32)),
            "i64" => Some(Self::Int(IntTy::I64)),
            "i128" => Some(Self::Int(IntTy::I128)),
            "isize" => Some(Self::Int(IntTy::Isize)),
            "u8" => Some(Self::Uint(UintTy::U8)),
            "u16" => Some(Self::Uint(UintTy::U16)),
            "u32" => Some(Self::Uint(UintTy::U32)),
            "u64" => Some(Self::Uint(UintTy::U64)),
            "u128" => Some(Self::Uint(UintTy::U128)),
            "usize" => Some(Self::Uint(UintTy::Usize)),
            "f32" => Some(Self::Float(FloatTy::F32)),
            "f64" => Some(Self::Float(FloatTy::F64)),
            _ => None,
        }
    }
}

impl StashDirect for Intrinsic {}
