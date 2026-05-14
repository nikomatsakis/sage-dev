use crate::source::SourceFile;

/// Byte offset range within a file, together with the file identity.
/// Stored on items.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct AbsoluteSpan {
    pub file: SourceFile,
    pub start: u32,
    pub end: u32,
}

/// Byte offset range relative to the containing item's start.
/// Stored on body nodes (expressions, statements, patterns)
/// and signature types (paths, type refs, params, etc.).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct RelativeSpan {
    pub start: u32,
    pub end: u32,
}

impl AbsoluteSpan {
    pub fn resolve(&self, relative: RelativeSpan) -> AbsoluteSpan {
        AbsoluteSpan {
            file: self.file,
            start: self.start + relative.start,
            end: self.start + relative.end,
        }
    }
}
