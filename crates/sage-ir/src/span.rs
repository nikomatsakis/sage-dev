use crate::source::SourceFile;

/// Indices into a `SpanTable`'s byte_offsets vec. 8 bytes, `Copy`, no `'db`.
/// Stored densely in body nodes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct SpanIndices {
    pub start: u32,
    pub end: u32,
}

/// One span table per top-level item. Editing one item's body doesn't
/// affect other items' span tables. Semantic queries never read
/// `byte_offsets`, so span changes don't invalidate them.
#[salsa::tracked]
pub struct SpanTable<'db> {
    #[tracked]
    pub file: SourceFile,
    #[tracked]
    #[returns(ref)]
    pub byte_offsets: Vec<u32>,
}
