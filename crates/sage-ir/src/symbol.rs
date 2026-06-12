//! `Symbol`: a `Copy` wrapper-of-enum unifying workspace-local
//! (`ItemAst`) and external (`SymExt`) definitions.
//!
//! The wrapper isn't interned — identity flows from the inner data:
//! local symbols inherit the underlying `ItemAst`'s salsa id; external
//! symbols use the structural `(CrateNum, DefIndex)` pair.

use sage_stash::{AllocStashData, StashDirect};

use crate::generic_param::AstGenericParam;
use crate::item::{ItemAst, MacroDefAst, MacroInvocationAst, StructAst, UseGroupAst};
use crate::module::{CrateNum, DefIndex, ModSymbol};
use crate::name::Name;
use crate::span::AbsoluteSpan;
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
    Fn(FnSymbol<'db>),
    Struct(StructSymbol<'db>),
    TupleStructCtor(StructSymbol<'db>),
    Enum(EnumSymbol<'db>),
    Trait(TraitSymbol<'db>),
    Impl(ImplSymbol<'db>),
    Mod(ModSymbol<'db>),
    TypeAlias(TypeAliasSymbol<'db>),
    Const(ConstSymbol<'db>),
    Static(StaticSymbol<'db>),
    MacroDef(MacroDefAst<'db>),
    Use(UseGroupAst<'db>),
    MacroInvocation(MacroInvocationAst<'db>),
    GenericParam(AstGenericParam<'db>),
    Intrinsic(Intrinsic),
    Error(AbsoluteSpan<'db>),
    Unknown(SymExt),
}

impl StashDirect for SymbolData<'_> {}
impl StashDirect for SymExt {}
impl StashDirect for SymExtKind {}

impl<'db> Symbol<'db> {
    pub fn ast(item: ItemAst<'db>) -> Self {
        let data = match item {
            ItemAst::Function(f) => SymbolData::Fn(FnSymbol::ast(f)),
            ItemAst::Struct(s) => SymbolData::Struct(StructSymbol::ast(s)),
            ItemAst::Enum(e) => SymbolData::Enum(EnumSymbol::ast(e)),
            ItemAst::Trait(t) => SymbolData::Trait(TraitSymbol::ast(t)),
            ItemAst::Impl(i) => SymbolData::Impl(ImplSymbol::ast(i)),
            ItemAst::TypeAlias(t) => SymbolData::TypeAlias(TypeAliasSymbol::ast(t)),
            ItemAst::Const(c) => SymbolData::Const(ConstSymbol::ast(c)),
            ItemAst::Static(s) => SymbolData::Static(StaticSymbol::ast(s)),
            ItemAst::Mod(m) => SymbolData::Mod(ModSymbol::ast(m)),
            ItemAst::Use(u) => SymbolData::Use(u),
            ItemAst::MacroDef(d) => SymbolData::MacroDef(d),
            ItemAst::MacroInvocation(m) => SymbolData::MacroInvocation(m),
            ItemAst::Error(span) => SymbolData::Error(span),
        };
        Self { data }
    }

    pub fn tuple_struct_ctor(s: StructAst<'db>) -> Self {
        Self {
            data: SymbolData::TupleStructCtor(StructSymbol::ast(s)),
        }
    }

    pub fn ext(ext: SymExt) -> Self {
        let data = match ext.kind {
            SymExtKind::Fn => SymbolData::Fn(FnSymbol::ext(ext)),
            SymExtKind::Struct => SymbolData::Struct(StructSymbol::ext(ext)),
            SymExtKind::TupleStructCtor => SymbolData::TupleStructCtor(StructSymbol::ext(ext)),
            SymExtKind::Enum => SymbolData::Enum(EnumSymbol::ext(ext)),
            SymExtKind::Trait => SymbolData::Trait(TraitSymbol::ext(ext)),
            SymExtKind::Impl => SymbolData::Impl(ImplSymbol::ext(ext)),
            SymExtKind::Mod => SymbolData::Mod(ModSymbol::ext(ext)),
            SymExtKind::TypeAlias => SymbolData::TypeAlias(TypeAliasSymbol::ext(ext)),
            SymExtKind::Const => SymbolData::Const(ConstSymbol::ext(ext)),
            SymExtKind::Static => SymbolData::Static(StaticSymbol::ext(ext)),
            SymExtKind::MacroDef | SymExtKind::Use | SymExtKind::Other => SymbolData::Unknown(ext),
        };
        Self { data }
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

    /// Extract the `SymExt` handle if this symbol is external.
    pub fn as_ext(self) -> Option<SymExt> {
        match self.data {
            SymbolData::Fn(s) => s.as_ext(),
            SymbolData::Struct(s) => s.as_ext(),
            SymbolData::TupleStructCtor(s) => s.as_ext(),
            SymbolData::Enum(s) => s.as_ext(),
            SymbolData::Trait(s) => s.as_ext(),
            SymbolData::Impl(s) => s.as_ext(),
            SymbolData::Mod(m) => match m.data() {
                crate::module::ModSymbolData::Ext(ext) => Some(ext),
                crate::module::ModSymbolData::Ast(_) => None,
            },
            SymbolData::TypeAlias(s) => s.as_ext(),
            SymbolData::Const(s) => s.as_ext(),
            SymbolData::Static(s) => s.as_ext(),
            SymbolData::Unknown(ext) => Some(ext),
            SymbolData::MacroDef(_)
            | SymbolData::Use(_)
            | SymbolData::MacroInvocation(_)
            | SymbolData::GenericParam(_)
            | SymbolData::Intrinsic(_)
            | SymbolData::Error(_) => None,
        }
    }

    /// True if this symbol is workspace-local (backed by AST).
    pub fn is_local(self) -> bool {
        self.as_ext().is_none()
    }

    /// True if this symbol is external (backed by `SymExt`).
    pub fn is_external(self) -> bool {
        self.as_ext().is_some()
    }

    /// The name of this symbol, if it has one.
    pub fn name(self, db: &'db dyn crate::Db) -> Option<Name<'db>> {
        match self.data {
            SymbolData::Fn(s) => s.as_ast().map(|f| f.name(db)),
            SymbolData::Struct(s) => s.as_ast().map(|s| s.name(db)),
            SymbolData::TupleStructCtor(s) => s.as_ast().map(|s| s.name(db)),
            SymbolData::Enum(s) => s.as_ast().map(|e| e.name(db)),
            SymbolData::Trait(s) => s.as_ast().map(|t| t.name(db)),
            SymbolData::TypeAlias(s) => s.as_ast().map(|t| t.name(db)),
            SymbolData::Const(s) => s.as_ast().map(|c| c.name(db)),
            SymbolData::Static(s) => s.as_ast().map(|s| s.name(db)),
            SymbolData::Mod(m) => match m.data() {
                crate::module::ModSymbolData::Ast(a) => Some(a.name(db)),
                crate::module::ModSymbolData::Ext(_) => None,
            },
            SymbolData::MacroDef(d) => Some(d.name(db)),
            SymbolData::GenericParam(p) => p.name(db),
            SymbolData::Impl(_)
            | SymbolData::Use(_)
            | SymbolData::MacroInvocation(_)
            | SymbolData::Intrinsic(_)
            | SymbolData::Error(_)
            | SymbolData::Unknown(_) => None,
        }
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
        Self {
            data: SymbolData::Mod(m),
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
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
        $vis struct $Name<'db> {
            data: $DataName<'db>,
        }

        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
        enum $DataName<'db> {
            Ast($AstTy, Option<crate::scope::ScopeSymbol<'db>>),
            Ext(SymExt),
        }

        impl<'db> $Name<'db> {
            pub fn local(ast: $AstTy, scope: crate::scope::ScopeSymbol<'db>) -> Self {
                Self { data: $DataName::Ast(ast, Some(scope)) }
            }

            pub fn ast(ast: $AstTy) -> Self {
                Self { data: $DataName::Ast(ast, None) }
            }

            pub fn ext(ext: SymExt) -> Self {
                Self { data: $DataName::Ext(ext) }
            }

            pub fn as_ast(self) -> Option<$AstTy> {
                match self.data {
                    $DataName::Ast(ast, _) => Some(ast),
                    $DataName::Ext(_) => None,
                }
            }

            pub fn scope(self) -> Option<crate::scope::ScopeSymbol<'db>> {
                match self.data {
                    $DataName::Ast(_, scope) => scope,
                    $DataName::Ext(_) => None,
                }
            }

            pub fn as_ext(self) -> Option<SymExt> {
                match self.data {
                    $DataName::Ast(..) => None,
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

        impl StashDirect for $Name<'_> {}
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

define_kind_symbol! {
    pub struct ImplSymbol, crate::item::ImplAst<'db>, ImplSymbolData;
}
