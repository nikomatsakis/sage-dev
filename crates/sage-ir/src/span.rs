use sage_stash::StashDirect;

use crate::local_syms::macro_invocations::LocalMacroInvocationSym;
use crate::source::SourceFile;
use crate::symbol::MacroDefSymbol;

/// The source of parseable text — either a real file or a macro expansion.
///
/// This is a plain enum (NOT a salsa tracked struct). The tracked identity
/// for macro expansions lives on `LocalMacroInvocationSym::parse_output`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum ParseSource<'db> {
    /// A real source file on disk.
    SourceFile(SourceFile),

    /// Output of a `foo!(...)` macro invocation, linked back to the macro definition.
    BangMacro(MacroDefSymbol<'db>, LocalMacroInvocationSym<'db>),
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
            ParseSource::BangMacro(..) => None,
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
