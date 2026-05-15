use crate::memmap::MacroInput;
use crate::source::SourceFile;

/// Output of a macro expansion, linked back to the invocation site.
///
/// Created by `expand_macro`. Its salsa identity enables memoized parsing
/// of the same expansion result.
#[salsa::tracked(debug)]
pub struct MacroExpansion<'db> {
    pub input: MacroInput<'db>,
    #[returns(ref)]
    pub text: String,
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

    /// Parse this source into items via the appropriate tracked wrapper.
    pub fn parse(&self, db: &'db dyn crate::Db) -> &'db [crate::item::ItemAst<'db>] {
        match self {
            ParseSource::SourceFile(f) => crate::lower::parse_source_file(db, *f),
            ParseSource::MacroExpansion(exp) => crate::lower::parse_macro_expansion(db, *exp),
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
