use sage_stash::{AllocStashData, Ptr, Slice};

use crate::cst::paths::Path;
use crate::span::RelativeSpan;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub struct AttrCst<'db> {
    pub kind: AttrCstKind,
    pub path: Ptr<Path<'db>>,
    /// Raw token tree bytes (including delimiters) for normal attrs,
    /// or comment text bytes for doc comments. Empty if no arguments.
    pub args: Slice<u8>,
    pub is_inner: bool,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub enum AttrCstKind {
    Normal,
    DocComment,
}

/// Attributes whose presence does not transform an item before type checking.
pub(crate) fn is_known_inert_item_attribute(name: &str) -> bool {
    matches!(
        name,
        "inline" | "repr" | "allow" | "deny" | "warn" | "forbid"
    )
}

/// Inner attributes that affect linting only and can therefore be ignored by
/// the current module and trait-candidate pipelines.
pub(crate) fn is_known_inert_inner_attribute(name: &str) -> bool {
    matches!(name, "allow" | "deny" | "warn" | "forbid")
}

// ---------------------------------------------------------------------------
// ToTokens
// ---------------------------------------------------------------------------

use crate::tokens::{Delimiter, Punct, ToTokens, TokenCtx, TokenSink};

impl<'db> ToTokens<'db> for AttrCst<'db> {
    fn to_tokens(&self, ctx: &TokenCtx<'_, 'db>, sink: &mut dyn TokenSink) {
        match self.kind {
            AttrCstKind::Normal => {
                if self.is_inner {
                    sink.punct(Punct::HashBang);
                } else {
                    sink.punct(Punct::Hash);
                }
                sink.group(Delimiter::Bracket, &mut |s| {
                    let path = ctx.stash[self.path];
                    path.to_tokens(ctx, s);
                    let args = &ctx.stash[self.args];
                    if !args.is_empty() {
                        let args_str = std::str::from_utf8(args).unwrap_or("");
                        s.raw(args_str);
                    }
                });
            }
            AttrCstKind::DocComment => {
                let text = &ctx.stash[self.args];
                let text_str = std::str::from_utf8(text).unwrap_or("");
                sink.raw(text_str);
            }
        }
    }
}
