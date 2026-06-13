use sage_stash::StashDirect;

use crate::item::LocalModItemSym;
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::source::SourceFile;
use crate::span::AbsoluteSpan;
use crate::types::Attr;

/// A module written in (or synthesized for) the local workspace.
///
/// `ModAst` carries both the syntactic data (name, attrs, items if
/// inline) and the resolution context (`parent`, `file`). At lowering
/// time, declaration-site ModAsts are created with `parent = None,
/// file = None`; resolution mints a separate ModAst with those filled
/// in. Two ModAsts at the same source location with different parent/
/// file context are distinct salsa tracked structs.
#[salsa::tracked(debug)]
pub struct LocalModSym<'db> {
    pub name: Name<'db>,

    /// The enclosing module, if any. `None` only for the crate root
    /// and for "raw" declaration ModAsts emitted by lowering before
    /// resolution has run.
    pub parent: Option<ScopeSymbol<'db>>,

    #[returns(ref)]
    pub source: ModAstSource<'db>,

    #[returns(ref)]
    pub attrs: Vec<Attr<'db>>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

#[derive(Clone, Debug, Hash, salsa::Update)]
pub enum ModAstSource<'db> {
    /// The source file backing this module's contents, if any.
    /// `Some` for the crate root and for `mod foo;` (file-based)
    /// modules. `None` for inline `mod foo { ... }` (the contents
    /// live in `items` and the enclosing file is reached via
    /// `parent`) and for unresolved declaration-site ModAsts.
    File(SourceFile),

    /// Inline module body, before macro expansion. `Some(items)` for
    /// `mod foo { ... }`, `None` for `mod foo;` (file-based) and for
    /// the crate root (whose contents come from `parse_source_file(file)`).
    ///
    /// Most callers want [`ModAst::unexpanded_items`], which unifies
    /// the inline and file-backed cases.
    Inline(Vec<LocalModItemSym<'db>>),
}

impl StashDirect for LocalModSym<'_> {}
