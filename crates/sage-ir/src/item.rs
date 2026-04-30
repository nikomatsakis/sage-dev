use crate::body::FunctionBody;
use crate::name::Name;
use crate::span::{SpanIndices, SpanTable};
use crate::types::{Attr, FieldDef, Param, Path, TypeRef, UseImport, VariantDef};

/// Thin enum over all item kinds. `Copy` because salsa tracked struct
/// handles are just IDs.
#[derive(Copy, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub enum Item<'db> {
    Function(FunctionItem<'db>),
    Struct(StructItem<'db>),
    Enum(EnumItem<'db>),
    Trait(TraitItem<'db>),
    Impl(ImplItem<'db>),
    TypeAlias(TypeAliasItem<'db>),
    Const(ConstItem<'db>),
    Static(StaticItem<'db>),
    Mod(ModItem<'db>),
    Use(UseGroup<'db>),
    /// Unrecognized or unsupported item node.
    Error(SpanIndices),
}

// -- Function --

#[salsa::tracked]
pub struct FunctionItem<'db> {
    pub name: Name<'db>,

    #[tracked]
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[tracked]
    #[returns(ref)]
    pub params: Vec<Param<'db>>,

    #[tracked]
    pub ret_type: Option<TypeRef<'db>>,

    #[tracked]
    pub is_async: bool,

    #[tracked]
    pub is_unsafe: bool,

    #[tracked]
    #[returns(ref)]
    pub body: FunctionBody<'db>,

    #[tracked]
    pub span_table: SpanTable<'db>,

    #[tracked]
    pub span: SpanIndices,
}

// -- Struct --

#[salsa::tracked]
pub struct StructItem<'db> {
    pub name: Name<'db>,

    #[tracked]
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[tracked]
    #[returns(ref)]
    pub fields: Vec<FieldDef<'db>>,

    #[tracked]
    pub span_table: SpanTable<'db>,

    #[tracked]
    pub span: SpanIndices,
}

// -- Enum --

#[salsa::tracked]
pub struct EnumItem<'db> {
    pub name: Name<'db>,

    #[tracked]
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[tracked]
    #[returns(ref)]
    pub variants: Vec<VariantDef<'db>>,

    #[tracked]
    pub span_table: SpanTable<'db>,

    #[tracked]
    pub span: SpanIndices,
}

// -- Trait --

#[salsa::tracked]
pub struct TraitItem<'db> {
    pub name: Name<'db>,

    #[tracked]
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[tracked]
    #[returns(ref)]
    pub items: Vec<Item<'db>>,

    #[tracked]
    pub span_table: SpanTable<'db>,

    #[tracked]
    pub span: SpanIndices,
}

// -- Impl --

#[salsa::tracked]
pub struct ImplItem<'db> {
    #[tracked]
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[tracked]
    pub self_ty: TypeRef<'db>,

    #[tracked]
    pub trait_path: Option<Path<'db>>,

    #[tracked]
    #[returns(ref)]
    pub items: Vec<Item<'db>>,

    #[tracked]
    pub span_table: SpanTable<'db>,

    #[tracked]
    pub span: SpanIndices,
}

// -- Type alias --

#[salsa::tracked]
pub struct TypeAliasItem<'db> {
    pub name: Name<'db>,

    #[tracked]
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[tracked]
    pub ty: Option<TypeRef<'db>>,

    #[tracked]
    pub span_table: SpanTable<'db>,

    #[tracked]
    pub span: SpanIndices,
}

// -- Const --

#[salsa::tracked]
pub struct ConstItem<'db> {
    pub name: Name<'db>,

    #[tracked]
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[tracked]
    pub ty: Option<TypeRef<'db>>,

    #[tracked]
    pub span_table: SpanTable<'db>,

    #[tracked]
    pub span: SpanIndices,
}

// -- Static --

#[salsa::tracked]
pub struct StaticItem<'db> {
    pub name: Name<'db>,

    #[tracked]
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[tracked]
    pub ty: Option<TypeRef<'db>>,

    #[tracked]
    pub is_mut: bool,

    #[tracked]
    pub span_table: SpanTable<'db>,

    #[tracked]
    pub span: SpanIndices,
}

// -- Mod --

#[salsa::tracked]
pub struct ModItem<'db> {
    pub name: Name<'db>,

    #[tracked]
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    /// `None` for `mod foo;` (file-based module).
    #[tracked]
    #[returns(ref)]
    pub items: Option<Vec<Item<'db>>>,

    #[tracked]
    pub span_table: SpanTable<'db>,

    #[tracked]
    pub span: SpanIndices,
}

// -- Use --

/// A use declaration, desugared into flat imports.
#[salsa::tracked]
pub struct UseGroup<'db> {
    #[tracked]
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[tracked]
    #[returns(ref)]
    pub imports: Vec<UseImport<'db>>,

    #[tracked]
    pub span_table: SpanTable<'db>,

    #[tracked]
    pub span: SpanIndices,
}
