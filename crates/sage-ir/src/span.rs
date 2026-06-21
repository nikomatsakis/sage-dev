use sage_stash::StashDirect;

use crate::Db;
use crate::local_syms::LocalModItemSym;
use crate::local_syms::macro_invocations::LocalMacroInvocationSym;
use crate::name::Name;
use crate::source::SourceFile;
use crate::symbol::MacroDefSymbol;

/// What triggered a macro expansion.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum ExpansionOrigin<'db> {
    /// A bang-macro invocation (`foo!(...)` at item level).
    Invocation(LocalMacroInvocationSym<'db>),
    /// A `#[derive(Foo)]` attribute on an item.
    Derive(AbsoluteSpan<'db>, Name<'db>),
    /// An attribute proc-macro (`#[attr]` on an item).
    Attribute(AbsoluteSpan<'db>, Name<'db>),
}

impl StashDirect for ExpansionOrigin<'_> {}

/// Output of a macro expansion, linked back to the invocation site.
///
/// Created by `expand_macro`. Its salsa identity enables memoized parsing
/// of the same expansion result.
#[salsa::tracked(debug)]
pub struct MacroExpansion<'db> {
    pub macro_def: MacroDefSymbol<'db>,
    pub origin: ExpansionOrigin<'db>,
}

#[salsa::tracked]
impl<'db> MacroExpansion<'db> {
    /// Parse a macro expansion's output text into module items.
    #[salsa::tracked]
    pub fn parse(self, db: &'db dyn Db) -> Vec<LocalModItemSym<'db>> {
        match self.origin(db) {
            ExpansionOrigin::Invocation(sym) => {
                let scope = sym.scope(db);
                let (stash, cst) = sym.cst(db).open_deref();
                let tokens = &stash[cst.input_tokens];
                let token_str = str::from_utf8(tokens).unwrap();
                crate::parse::parse_str_to_cst(
                    db,
                    ParseSource::MacroExpansion(self),
                    token_str,
                    scope,
                )
            }
            ExpansionOrigin::Derive(..) | ExpansionOrigin::Attribute(..) => {
                panic!("need to think about how to represent these!")
            }
        }
    }
}

/// The source of parseable text — either a real file or a macro expansion.
///
/// This is a plain enum (NOT a salsa tracked struct). The tracked identity
/// lives on `MacroExpansion` where salsa memoization matters.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum ParseSource<'db> {
    /// A real source file on disk.
    SourceFile(SourceFile),

    /// Output of a macro expansion, linked back to the invocation site.
    MacroExpansion(MacroExpansion<'db>),
}

impl<'db> ParseSource<'db> {
    /// Get the text content of this parse source.
    pub fn text(&self, db: &'db dyn crate::Db) -> &'db str {
        match self {
            ParseSource::SourceFile(f) => f.text(db),
            ParseSource::MacroExpansion(exp) => exp.text(db),
        }
    }
}

/// Byte offset range within a source (file or macro expansion), together
/// with the source identity. Stored on items.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct AbsoluteSpan<'db> {
    pub source: ParseSource<'db>,
    pub start: u32,
    pub end: u32,
}

impl StashDirect for AbsoluteSpan<'_> {}

impl<'db> AbsoluteSpan<'db> {
    pub fn resolve(&self, relative: RelativeSpan) -> AbsoluteSpan<'db> {
        AbsoluteSpan {
            source: self.source,
            start: self.start + relative.start,
            end: self.start + relative.end,
        }
    }

    /// Convenience: get the source file if this span is from a real file.
    pub fn file(&self) -> Option<SourceFile> {
        match self.source {
            ParseSource::SourceFile(f) => Some(f),
            ParseSource::MacroExpansion(_) => None,
        }
    }
}

/// Byte offset range relative to the containing item's start.
/// Stored on body nodes (expressions, statements, patterns)
/// and signature types (paths, type refs, params, etc.).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct RelativeSpan {
    pub start: u32,
    pub end: u32,
}

impl StashDirect for RelativeSpan {}
