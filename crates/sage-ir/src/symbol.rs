//! `Symbol`: a `Copy` wrapper-of-enum unifying workspace-local
//! (`ItemAst`) and external (`SymExt`) definitions.
//!
//! The wrapper isn't interned — identity flows from the inner data:
//! local symbols inherit the underlying `ItemAst`'s salsa id; external
//! symbols use the structural `(CrateNum, DefIndex)` pair.

use sage_stash::StashDirect;

use crate::{
    Db,
    cst::uses::UseKind,
    local_syms,
    memmap::local_expanded_module_items,
    name::Name,
    resolve::Namespace,
    ty::{FloatTy, IntTy, UintTy},
};

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

#[salsa::tracked]
impl<'db> SymExt<'db> {
    #[salsa::tracked]
    pub fn name(self, db: &'db dyn Db) -> Option<(Name<'db>, Namespace)> {
        let namespace = match self.kind(db) {
            SymExtKind::Fn => Namespace::Value,
            SymExtKind::Struct => Namespace::Type,
            SymExtKind::TupleStructCtor => Namespace::Value,
            SymExtKind::Enum => Namespace::Type,
            SymExtKind::Trait => Namespace::Type,
            SymExtKind::Impl => return None,
            SymExtKind::Mod => Namespace::Type,
            SymExtKind::TypeAlias => Namespace::Type,
            SymExtKind::Const => Namespace::Value,
            SymExtKind::Static => Namespace::Value,
            SymExtKind::MacroDef => Namespace::Macro,
            SymExtKind::Use => Namespace::Type,
            SymExtKind::Other => return None,
        };
        let n = db.tcx().item_name(self.crate_num(db), self.def_index(db))?;
        Some((Name::new(db, n), namespace))
    }

    #[salsa::tracked(returns(ref))]
    pub fn expanded_module_items(self, db: &'db dyn Db) -> Vec<Symbol<'db>> {
        assert_eq!(self.kind(db), SymExtKind::Mod);
        db.tcx()
            .module_children(self.crate_num(db), self.def_index(db))
            .into_iter()
            .map(|raw_child| {
                SymExt::new(db, raw_child.crate_num, raw_child.def_index, raw_child.kind).into()
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Per-kind symbol wrappers
// ---------------------------------------------------------------------------

macro_rules! define_kind_symbols {
    (
        $SymVis:vis struct $SymName:ident<$SymLt:lifetime> { data: $SymPrivateData:ident<$SymPrivateDataLt:lifetime>}
        $SymDataVis:vis enum $SymData:ident<$SymDataLt:lifetime> { .. }

        $(
            $(#[$meta:meta])*
            $vis:vis enum $Name:ident<$lt:lifetime> { Local($LocalTy:ty), Ext($ExtKind:path) }
        )*
    ) => {
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
        $SymVis struct $SymName<$SymLt> {
            data: $SymPrivateData<$SymLt>,
        }

        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
        enum $SymPrivateData<$SymLt> {
            $(
                $Name($LocalTy),
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
                        $SymPrivateData::$Name(Local) => ast.into(),
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
            impl<$SymLt> From<$LocalTy> for $SymName<$SymLt> {
                fn from(ast: $LocalTy) -> Self {
                    Self {
                        data: $SymPrivateData::$Name(ast)
                    }
                }
            }

            impl<$SymLt> From<$Name<$SymLt>> for $SymName<$SymLt> {
                fn from(sym: $Name<$SymLt>) -> Self {
                    match sym {
                        $Name::Local(ast) => ast.into(),
                        $Name::Ext(ext) => ext.into(),
                    }
                }
            }

            impl<$SymLt> From<$LocalTy> for $SymData<$SymLt> {
                fn from(ast: $LocalTy) -> Self {
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
                Local($LocalTy),
                Ext(SymExt<$lt>),
            }

            impl<$lt> From<SymExt<$lt>> for $Name<$lt> {
                fn from(ext: SymExt<$lt>) -> Self {
                    Self::Ext(ext)
                }
            }

            impl<$lt> From<$LocalTy> for $Name<$lt> {
                fn from(ast: $LocalTy) -> Self {
                    Self::Local(ast)
                }
            }

            impl<$lt> StashDirect for $Name<$lt> {}
        )*
    };
}

define_kind_symbols! {
    pub struct Symbol<'db> { data: SymbolDataPriv<'db> }
    pub enum SymbolData<'db> { .. }

    pub enum FnSymbol<'db> { Local(crate::local_syms::fns::LocalFnSym<'db>), Ext(SymExtKind::Fn) }
    pub enum StructSymbol<'db> { Local(crate::local_syms::structs::LocalStructSym<'db>), Ext(SymExtKind::Struct) }
    pub enum EnumSymbol<'db> { Local(crate::local_syms::enums::LocalEnumSym<'db>), Ext(SymExtKind::Enum) }
    pub enum TraitSymbol<'db> { Local(crate::local_syms::traits::LocalTraitSym<'db>), Ext(SymExtKind::Trait) }
    pub enum TypeAliasSymbol<'db> { Local(crate::local_syms::type_aliases::LocalTypeAliasSym<'db>), Ext(SymExtKind::TypeAlias) }
    pub enum ConstSymbol<'db> { Local(crate::local_syms::consts::LocalConstSym<'db>), Ext(SymExtKind::Const) }
    pub enum StaticSymbol<'db> { Local(crate::local_syms::statics::LocalStaticSym<'db>), Ext(SymExtKind::Static) }
    pub enum ImplSymbol<'db> { Local(crate::local_syms::impls::LocalImplSym<'db>), Ext(SymExtKind::Impl) }
    pub enum ModSymbol<'db> { Local(crate::local_syms::mods::LocalModSym<'db>), Ext(SymExtKind::Mod) }
    pub enum MacroDefSymbol<'db> { Local(crate::local_syms::macro_defs::LocalMacroDefSym<'db>), Ext(SymExtKind::MacroDef) }
    pub enum UseSymbol<'db> { Local(crate::local_syms::uses::LocalUseSym<'db>), Ext(SymExtKind::Mod) }
}

impl<'db> Symbol<'db> {
    /// Returns the name of the item defined by this symbol, if any.
    ///
    /// None is returned for:
    ///
    /// - Anonymous items, like impls.
    /// - `use` symbols, which may define multiple items (e.g., globs etc).
    pub fn name(&self, db: &'db dyn Db) -> Option<(Name<'db>, Namespace)> {
        match self.data {
            SymbolDataPriv::FnSymbol(sym) => Some((sym.name(db), Namespace::Value)),
            SymbolDataPriv::StructSymbol(sym) => Some((sym.name(db), Namespace::Type)),
            SymbolDataPriv::EnumSymbol(sym) => Some((sym.name(db), Namespace::Type)),
            SymbolDataPriv::TraitSymbol(sym) => Some((sym.name(db), Namespace::Type)),
            SymbolDataPriv::TypeAliasSymbol(sym) => Some((sym.name(db), Namespace::Type)),
            SymbolDataPriv::ConstSymbol(sym) => Some((sym.name(db), Namespace::Value)),
            SymbolDataPriv::StaticSymbol(sym) => Some((sym.name(db), Namespace::Value)),
            SymbolDataPriv::ImplSymbol(_) => None,
            SymbolDataPriv::ModSymbol(sym) => Some((sym.name(db), Namespace::Type)),
            SymbolDataPriv::MacroDefSymbol(sym) => Some((sym.name(db), Namespace::Macro)),
            SymbolDataPriv::Ext(sym_ext) => sym_ext.name(db),
            SymbolDataPriv::UseSymbol(_) => None,
        }
    }

    pub fn module(&self, db: &'db dyn Db) -> Option<ModSymbol<'db>> {
        match self.data(db) {
            SymbolData::ModSymbol(sym) => Some(sym),
            _ => None,
        }
    }
}

#[salsa::tracked]
impl<'db> ModSymbol<'db> {
    pub fn expanded_module_items(self, db: &'db dyn Db) -> &'db [Symbol<'db>] {
        match self {
            ModSymbol::Local(sym) => local_expanded_module_items(db, sym),
            ModSymbol::Ext(sym_ext) => sym_ext.expanded_module_items(db),
        }
    }
}
