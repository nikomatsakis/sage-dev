use crate::body::FunctionBody;
use crate::name::Name;
use crate::span::{SpanIndices, SpanTable};
use crate::types::{Attr, FieldDef, Param, Path, TypeRef, UseImport, VariantDef};

/// Thin enum over all item kinds. `Copy` because salsa tracked struct
/// handles are just IDs.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum ItemAst<'db> {
    Function(FnAst<'db>),
    Struct(StructAst<'db>),
    Enum(EnumAst<'db>),
    Trait(TraitAst<'db>),
    Impl(ImplAst<'db>),
    TypeAlias(TypeAliasAst<'db>),
    Const(ConstAst<'db>),
    Static(StaticAst<'db>),
    Mod(ModAst<'db>),
    Use(UseGroupAst<'db>),
    MacroDef(MacroDefAst<'db>),
    MacroInvocation(MacroInvocationAst<'db>),
    /// Unrecognized or unsupported item node.
    Error(SpanIndices),
}

// -- Function --

#[salsa::tracked(debug)]
pub struct FnAst<'db> {
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

#[salsa::tracked(debug)]
pub struct StructAst<'db> {
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

#[salsa::tracked(debug)]
pub struct EnumAst<'db> {
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

#[salsa::tracked(debug)]
pub struct TraitAst<'db> {
    pub name: Name<'db>,

    #[tracked]
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[tracked]
    #[returns(ref)]
    pub items: Vec<ItemAst<'db>>,

    #[tracked]
    pub span_table: SpanTable<'db>,

    #[tracked]
    pub span: SpanIndices,
}

// -- Impl --

#[salsa::tracked(debug)]
pub struct ImplAst<'db> {
    #[tracked]
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[tracked]
    pub self_ty: TypeRef<'db>,

    #[tracked]
    pub trait_path: Option<Path<'db>>,

    #[tracked]
    #[returns(ref)]
    pub items: Vec<ItemAst<'db>>,

    #[tracked]
    pub span_table: SpanTable<'db>,

    #[tracked]
    pub span: SpanIndices,
}

// -- Type alias --

#[salsa::tracked(debug)]
pub struct TypeAliasAst<'db> {
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

#[salsa::tracked(debug)]
pub struct ConstAst<'db> {
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

#[salsa::tracked(debug)]
pub struct StaticAst<'db> {
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

/// A module written in (or synthesized for) the local workspace.
///
/// `ModAst` carries both the syntactic data (name, attrs, items if
/// inline) and the resolution context (`parent`, `file`). At lowering
/// time, declaration-site ModAsts are created with `parent = None,
/// file = None`; resolution mints a separate ModAst with those filled
/// in. Two ModAsts at the same source location with different parent/
/// file context are distinct salsa tracked structs.
#[salsa::tracked(debug)]
pub struct ModAst<'db> {
    pub name: Name<'db>,

    /// The enclosing module, if any. `None` only for the crate root
    /// and for "raw" declaration ModAsts emitted by lowering before
    /// resolution has run.
    pub parent: Option<crate::module::ModSymbol<'db>>,

    /// The source file backing this module's contents, if any.
    /// `Some` for the crate root and for `mod foo;` (file-based)
    /// modules. `None` for inline `mod foo { ... }` (the contents
    /// live in `items` and the enclosing file is reached via
    /// `parent`) and for unresolved declaration-site ModAsts.
    pub file: Option<crate::source::SourceFile>,

    #[tracked]
    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    /// Inline module body, before macro expansion. `Some(items)` for
    /// `mod foo { ... }`, `None` for `mod foo;` (file-based) and for
    /// the crate root (whose contents come from `file_item_tree(file)`).
    ///
    /// Most callers want [`ModAst::unexpanded_items`], which unifies
    /// the inline and file-backed cases.
    #[tracked]
    #[returns(ref)]
    pub inline_unexpanded_items: Option<Vec<ItemAst<'db>>>,

    #[tracked]
    pub span_table: SpanTable<'db>,

    #[tracked]
    pub span: SpanIndices,
}

/// Build a synthetic crate-root ModAst for a given file. The
/// crate root has no declaring `mod` site, no parent, and inherits
/// its contents from `file_item_tree(file)`.
///
/// Wrapped in a `#[salsa::tracked]` function so callers can use it
/// from outside another tracked context (e.g. test setup).
#[salsa::tracked]
pub fn crate_root_mod<'db>(db: &'db dyn crate::Db, file: crate::source::SourceFile) -> ModAst<'db> {
    ModAst::new(
        db,
        crate::name::Name::new(db, "crate".to_owned()),
        None,
        Some(file),
        Vec::new(),
        None,
        crate::span::SpanTable::new(db, file, vec![0, 0]),
        SpanIndices { start: 0, end: 0 },
    )
}

/// Build a synthetic file-backed child ModAst — useful for tests
/// that wire up a multi-file workspace without going through
/// `resolve_mod`.
#[salsa::tracked]
pub fn synthetic_child_mod<'db>(
    db: &'db dyn crate::Db,
    name: Name<'db>,
    parent: crate::module::ModSymbol<'db>,
    file: crate::source::SourceFile,
) -> ModAst<'db> {
    ModAst::new(
        db,
        name,
        Some(parent),
        Some(file),
        Vec::new(),
        None,
        crate::span::SpanTable::new(db, file, vec![0, 0]),
        SpanIndices { start: 0, end: 0 },
    )
}

impl<'db> ModAst<'db> {
    /// The module's pre-expansion item list — the items as written
    /// at item position, with macro invocations and `use`
    /// declarations not yet processed.
    ///
    /// For inline modules (`mod foo { ... }`), reads
    /// `inline_unexpanded_items`. For file-backed modules (crate
    /// root, `mod foo;`), parses the file via `file_item_tree`. For
    /// raw declaration-site ModAsts (no parent, no file, no inline
    /// items), returns an empty list.
    ///
    /// Macro expansion and use-redirect handling happen later in
    /// `expanded_module`; downstream code should usually go through
    /// that query rather than this helper.
    pub fn unexpanded_items(self, db: &'db dyn crate::Db) -> Vec<ItemAst<'db>> {
        if let Some(items) = self.inline_unexpanded_items(db) {
            items.clone()
        } else if let Some(file) = self.file(db) {
            crate::lower::file_item_tree(db, file).clone()
        } else {
            Vec::new()
        }
    }

    /// Convenience wrapper around `crate_root_mod`.
    pub fn crate_root(db: &'db dyn crate::Db, file: crate::source::SourceFile) -> ModAst<'db> {
        crate_root_mod(db, file)
    }

    /// Convenience wrapper around `synthetic_child_mod`.
    pub fn synthetic_child(
        db: &'db dyn crate::Db,
        name: &str,
        parent: crate::module::ModSymbol<'db>,
        file: crate::source::SourceFile,
    ) -> ModAst<'db> {
        synthetic_child_mod(db, Name::new(db, name.to_owned()), parent, file)
    }
}

// -- Use --

/// A use declaration, desugared into flat imports.
#[salsa::tracked(debug)]
pub struct UseGroupAst<'db> {
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

// -- MacroDef --

/// A `macro_rules!` definition at item level.
#[salsa::tracked(debug)]
pub struct MacroDefAst<'db> {
    pub name: Name<'db>,

    #[tracked]
    #[returns(ref)]
    pub body_tokens: String,

    #[tracked]
    pub span: SpanIndices,
}

// -- MacroInvocation --

/// An item-level macro invocation (e.g. `m!()` or `foo::bar::m!()`).
#[salsa::tracked(debug)]
pub struct MacroInvocationAst<'db> {
    pub path: Path<'db>,

    /// The token stream passed to the macro at the invocation site — i.e.
    /// the contents of `m!(...)`, with the outer delimiter pair stripped.
    /// Empty for zero-argument invocations like `m!()`.
    #[tracked]
    #[returns(ref)]
    pub input_tokens: String,

    #[tracked]
    pub span: SpanIndices,
}
