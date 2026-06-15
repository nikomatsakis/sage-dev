//! `Symbol`: a `Copy` wrapper-of-enum unifying workspace-local
//! (`ItemAst`) and external (`SymExt`) definitions.
//!
//! The wrapper isn't interned — identity flows from the inner data:
//! local symbols inherit the underlying `ItemAst`'s salsa id; external
//! symbols use the structural `(CrateNum, DefIndex)` pair.

use sage_stash::StashDirect;

use crate::ty::{FloatTy, IntTy, UintTy};

/// Opaque crate number (matches rustc's CrateNum).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct CrateNum(pub u32);

impl StashDirect for CrateNum {}

/// Opaque definition index within a crate.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct DefIndex(pub u32);

impl StashDirect for DefIndex {}

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
#[salsa::interned(debug)]
pub struct SymExt<'db> {
    pub crate_num: CrateNum,
    pub def_index: DefIndex,
    pub kind: SymExtKind,
}

impl<'db> StashDirect for SymExt<'db> {}
impl StashDirect for SymExtKind {}

// ---------------------------------------------------------------------------
// Per-kind symbol wrappers
// ---------------------------------------------------------------------------

macro_rules! define_kind_symbols {
    (
        $SymVis:vis struct $SymName:ident<$SymLt:lifetime> { data: $SymPrivateData:ident<$SymPrivateDataLt:lifetime>}
        $SymDataVis:vis enum $SymData:ident<$SymDataLt:lifetime> { .. }

        $(
            $(#[$meta:meta])*
            $vis:vis enum $Name:ident<$lt:lifetime> { Ast($AstTy:ty), Ext($ExtKind:path) }
        )*
    ) => {
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
        $SymVis struct $SymName<$SymLt> {
            data: $SymPrivateData<$SymLt>,
        }

        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
        enum $SymPrivateData<$SymLt> {
            $(
                $Name($AstTy),
            )*
            Ext(SymExt<$SymLt>),
        }

        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
        $SymDataVis enum $SymData<$SymLt> {
            $(
                $Name($Name<$SymLt>),
            )*
        }

        impl<$SymLt> StashDirect for $SymName<$SymLt> {}
        impl<$SymLt> StashDirect for $SymData<$SymLt> {}

        impl<$SymLt> From<SymExt<$SymLt>> for $SymName<$SymLt> {
            fn from(ext: SymExt<$SymLt>) -> Self {
                Self {
                    data: $SymPrivateData::Ext(ext)
                }
            }
        }

        impl<$SymLt> $SymName<$SymLt> {
            $SymVis fn data(self, db: &$SymLt dyn crate::Db) -> $SymData<$SymLt> {
                match self.data {
                    $(
                        $SymPrivateData::$Name(ast) => ast.into(),
                    )*
                    $SymPrivateData::Ext(ext) => match ext.kind(db) {
                        $(
                            $ExtKind => $Name::Ext(ext).into(),
                        )*
                        _ => todo!("NDM: add all variants here")
                    }
                }
            }
        }

        $(
            impl<$SymLt> From<$AstTy> for $SymName<$SymLt> {
                fn from(ast: $AstTy) -> Self {
                    Self {
                        data: $SymPrivateData::$Name(ast)
                    }
                }
            }

            impl<$SymLt> From<$AstTy> for $SymData<$SymLt> {
                fn from(ast: $AstTy) -> Self {
                    $SymData::$Name(ast.into())
                }
            }

            impl<$SymLt> From<$Name<$SymLt>> for $SymData<$SymLt> {
                fn from(sym: $Name<$SymLt>) -> Self {
                    $SymData::$Name(sym)
                }
            }
        )*


        $(
            $(#[$meta])*
            #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
            $vis enum $Name<$lt> {
                Ast($AstTy),
                Ext(SymExt<$lt>),
            }

            impl<$lt> From<SymExt<$lt>> for $Name<$lt> {
                fn from(ext: SymExt<$lt>) -> Self {
                    Self::Ext(ext)
                }
            }

            impl<$lt> From<$AstTy> for $Name<$lt> {
                fn from(ast: $AstTy) -> Self {
                    Self::Ast(ast)
                }
            }

            impl<$lt> StashDirect for $Name<$lt> {}
        )*
    };
}

define_kind_symbols! {
    pub struct Symbol<'db> { data: SymbolDataPriv<'db> }
    pub enum SymbolData<'db> { .. }

    pub enum FnSymbol<'db> { Ast(crate::local_syms::fns::LocalFnSym<'db>), Ext(SymExtKind::Fn) }
    pub enum StructSymbol<'db> { Ast(crate::local_syms::structs::LocalStructSym<'db>), Ext(SymExtKind::Struct) }
    pub enum EnumSymbol<'db> { Ast(crate::local_syms::enums::LocalEnumSym<'db>), Ext(SymExtKind::Enum) }
    pub enum TraitSymbol<'db> { Ast(crate::local_syms::traits::LocalTraitSym<'db>), Ext(SymExtKind::Trait) }
    pub enum TypeAliasSymbol<'db> { Ast(crate::local_syms::type_aliases::LocalTypeAliasSym<'db>), Ext(SymExtKind::TypeAlias) }
    pub enum ConstSymbol<'db> { Ast(crate::local_syms::consts::LocalConstSym<'db>), Ext(SymExtKind::Const) }
    pub enum StaticSymbol<'db> { Ast(crate::local_syms::statics::LocalStaticSym<'db>), Ext(SymExtKind::Static) }
    pub enum ImplSymbol<'db> { Ast(crate::local_syms::impls::LocalImplSym<'db>), Ext(SymExtKind::Impl) }
    pub enum ModSymbol<'db> { Ast(crate::local_syms::mods::LocalModSym<'db>), Ext(SymExtKind::Mod) }
}
