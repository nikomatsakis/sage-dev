//! `Symbol`: a `Copy` wrapper-of-enum unifying workspace-local
//! (`ItemAst`) and external (`SymExt`) definitions.
//!
//! The wrapper isn't interned — identity flows from the inner data:
//! local symbols inherit the underlying `ItemAst`'s salsa id; external
//! symbols use the structural `(CrateNum, DefIndex)` pair.

use sage_stash::{AllocStashData, StashDirect};

use crate::item::{ItemAst, StructAst};
use crate::module::{CrateNum, DefIndex, ModSymbol};
use crate::ty::{FloatTy, IntTy, UintTy};

// ---------------------------------------------------------------------------
// Intrinsic — compiler-known built-in symbols
// ---------------------------------------------------------------------------

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

/// A resolved symbol — local item or external definition.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update, AllocStashData)]
pub struct Symbol<'db> {
    data: SymbolData<'db>,
}

/// The kind of an external symbol, mirroring rustc's `DefKind`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum SymExtKind {
    Fn,
    Struct,
    TupleStructCtor,
    Enum,
    Trait,
    Impl,
    Mod,
    TypeAlias,
    Const,
    Static,
    MacroDef,
    Use,
    Other,
}

/// External symbol — a thin handle into rustc's metadata. Plain
/// `Copy` struct, structural identity.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct SymExt {
    pub crate_num: CrateNum,
    pub def_index: DefIndex,
    pub kind: SymExtKind,
}

impl SymExt {
    pub const fn new(crate_num: CrateNum, def_index: DefIndex, kind: SymExtKind) -> Self {
        Self {
            crate_num,
            def_index,
            kind,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum SymbolData<'db> {
    Ast(ItemAst<'db>),
    TupleStructCtor(StructAst<'db>),
    Ext(SymExt),
    Intrinsic(Intrinsic),
}

impl StashDirect for SymbolData<'_> {}
impl StashDirect for SymExt {}

impl<'db> Symbol<'db> {
    pub fn ast(item: ItemAst<'db>) -> Self {
        Self {
            data: SymbolData::Ast(item),
        }
    }

    pub fn tuple_struct_ctor(s: StructAst<'db>) -> Self {
        Self {
            data: SymbolData::TupleStructCtor(s),
        }
    }

    pub fn ext(ext: SymExt) -> Self {
        Self {
            data: SymbolData::Ext(ext),
        }
    }

    pub fn intrinsic(i: Intrinsic) -> Self {
        Self {
            data: SymbolData::Intrinsic(i),
        }
    }

    pub fn external(crate_num: CrateNum, def_index: DefIndex) -> Self {
        Self::ext(SymExt::new(crate_num, def_index, SymExtKind::Other))
    }

    pub fn data(self) -> SymbolData<'db> {
        self.data
    }
}

impl<'db> From<ItemAst<'db>> for Symbol<'db> {
    fn from(item: ItemAst<'db>) -> Self {
        Self::ast(item)
    }
}

impl From<SymExt> for Symbol<'_> {
    fn from(ext: SymExt) -> Self {
        Self::ext(ext)
    }
}

impl<'db> From<ModSymbol<'db>> for Symbol<'db> {
    fn from(m: ModSymbol<'db>) -> Self {
        match m.data() {
            crate::module::ModSymbolData::Ast(ast) => Symbol::ast(ItemAst::Mod(ast)),
            crate::module::ModSymbolData::Ext(ext) => Symbol::ext(ext),
        }
    }
}

// ---------------------------------------------------------------------------
// Per-kind symbol wrappers
// ---------------------------------------------------------------------------

macro_rules! define_kind_symbol {
    (
        $(#[$meta:meta])*
        $vis:vis struct $Name:ident, $AstTy:ty, $DataName:ident;
    ) => {
        $(#[$meta])*
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
        $vis struct $Name<'db> {
            data: $DataName<'db>,
        }

        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
        enum $DataName<'db> {
            Ast($AstTy),
            Ext(SymExt),
        }

        impl<'db> $Name<'db> {
            pub fn ast(ast: $AstTy) -> Self {
                Self { data: $DataName::Ast(ast) }
            }

            pub fn ext(ext: SymExt) -> Self {
                Self { data: $DataName::Ext(ext) }
            }

            pub fn as_ast(self) -> Option<$AstTy> {
                match self.data {
                    $DataName::Ast(ast) => Some(ast),
                    $DataName::Ext(_) => None,
                }
            }

            pub fn as_ext(self) -> Option<SymExt> {
                match self.data {
                    $DataName::Ast(_) => None,
                    $DataName::Ext(ext) => Some(ext),
                }
            }
        }

        impl<'db> From<$AstTy> for $Name<'db> {
            fn from(ast: $AstTy) -> Self {
                Self::ast(ast)
            }
        }

        impl From<SymExt> for $Name<'_> {
            fn from(ext: SymExt) -> Self {
                Self::ext(ext)
            }
        }
    };
}

define_kind_symbol! {
    pub struct FnSymbol, crate::item::FnAst<'db>, FnSymbolData;
}

define_kind_symbol! {
    pub struct StructSymbol, crate::item::StructAst<'db>, StructSymbolData;
}

define_kind_symbol! {
    pub struct EnumSymbol, crate::item::EnumAst<'db>, EnumSymbolData;
}

define_kind_symbol! {
    pub struct TraitSymbol, crate::item::TraitAst<'db>, TraitSymbolData;
}

define_kind_symbol! {
    pub struct TypeAliasSymbol, crate::item::TypeAliasAst<'db>, TypeAliasSymbolData;
}

define_kind_symbol! {
    pub struct ConstSymbol, crate::item::ConstAst<'db>, ConstSymbolData;
}

define_kind_symbol! {
    pub struct StaticSymbol, crate::item::StaticAst<'db>, StaticSymbolData;
}
