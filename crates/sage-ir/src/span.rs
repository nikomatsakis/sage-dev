use sage_stash::StashDirect;

use crate::local_syms::macro_invocations::LocalMacroInvocationSym;
use crate::source::SourceFile;
use crate::symbol::MacroDefSymbol;

/// The source of parseable text — either a real file or a macro expansion.
///
/// This is a plain enum (NOT a salsa tracked struct). Bang-macro output uses
/// `LocalMacroInvocationSym::parse_output` as its tracked identity boundary;
/// derive output carries an explicit `DeriveExpansion` identity.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum ParseSource<'db> {
    /// A real source file on disk.
    SourceFile(SourceFile),

    /// Output of a `foo!(...)` macro invocation, linked back to the macro definition.
    BangMacro(MacroDefSymbol<'db>, LocalMacroInvocationSym<'db>),

    /// Output synthesized by a `#[derive(...)]` invocation.
    Derive(crate::derive::DeriveExpansion<'db>),
}

impl<'db> sage_reflect::Reflect<'db> for ParseSource<'db> {
    fn reflect(
        &self,
        context: &mut sage_reflect::ReflectionContext<'_>,
        stash: Option<&sage_stash::Stash>,
    ) -> sage_reflect::ValueNode {
        use sage_reflect::{ReferenceKey, ValueField, ValueNode};
        use salsa::plumbing::AsId;

        context.reflect_node("ParseSource", |context| match self {
            ParseSource::SourceFile(source_file) => {
                let key = ReferenceKey {
                    family: "source-file",
                    id: source_file.as_id().as_bits(),
                };
                context
                    .reflected_value(&key)
                    .unwrap_or_else(|| ValueNode::scalar("SourceFile", format!("input:{}", key.id)))
            }
            ParseSource::BangMacro(definition, invocation) => {
                let invocation: crate::symbol::Symbol<'db> = (*invocation).into();
                ValueNode::variant(
                    "ParseSource",
                    "BangMacro",
                    vec![
                        ValueField::new("definition", definition.reflect(context, stash)),
                        ValueField::new("invocation", invocation.reflect(context, stash)),
                    ],
                )
            }
            ParseSource::Derive(expansion) => {
                let key = ReferenceKey {
                    family: "derive-expansion",
                    id: expansion.as_id().as_bits(),
                };
                context.reflected_value(&key).unwrap_or_else(|| {
                    ValueNode::scalar("DeriveExpansion", format!("tracked:{}", key.id))
                })
            }
        })
    }
}

/// Byte offset range within a source (file or macro expansion), together
/// with the source identity. Stored on items.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update, sage_reflect::Reflect)]
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
            ParseSource::BangMacro(..) | ParseSource::Derive(..) => None,
        }
    }
}

/// Byte offset range relative to the containing item's start.
/// Stored on body nodes (expressions, statements, patterns)
/// and signature types (paths, type refs, params, etc.).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update, sage_reflect::Reflect)]
pub struct RelativeSpan {
    pub start: u32,
    pub end: u32,
}

impl StashDirect for RelativeSpan {}
