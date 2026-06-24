use crate::name::Name;
use crate::symbol::intrinsic::Intrinsic;
use crate::ty::{FloatTy, IntTy, UintTy};

#[salsa::interned(debug)]
pub struct IntrinsicTypeSym<'db> {
    pub intrinsic: Intrinsic,
}

impl<'db> IntrinsicTypeSym<'db> {
    pub fn name(self, db: &'db dyn crate::Db) -> Name<'db> {
        let s = match self.intrinsic(db) {
            Intrinsic::Bool => "bool",
            Intrinsic::Char => "char",
            Intrinsic::Str => "str",
            Intrinsic::Int(IntTy::I8) => "i8",
            Intrinsic::Int(IntTy::I16) => "i16",
            Intrinsic::Int(IntTy::I32) => "i32",
            Intrinsic::Int(IntTy::I64) => "i64",
            Intrinsic::Int(IntTy::I128) => "i128",
            Intrinsic::Int(IntTy::Isize) => "isize",
            Intrinsic::Uint(UintTy::U8) => "u8",
            Intrinsic::Uint(UintTy::U16) => "u16",
            Intrinsic::Uint(UintTy::U32) => "u32",
            Intrinsic::Uint(UintTy::U64) => "u64",
            Intrinsic::Uint(UintTy::U128) => "u128",
            Intrinsic::Uint(UintTy::Usize) => "usize",
            Intrinsic::Float(FloatTy::F32) => "f32",
            Intrinsic::Float(FloatTy::F64) => "f64",
        };
        Name::new(db, s.to_owned())
    }
}
