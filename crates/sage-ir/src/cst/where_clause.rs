use sage_stash::{AllocStashData, Ptr, Slice};

use crate::cst::generics::TypeBoundCst;
use crate::cst::ty::TypeCst;
use crate::span::RelativeSpan;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct WhereClauseCst<'db> {
    pub subject: Ptr<TypeCst<'db>>,
    pub bounds: Slice<TypeBoundCst<'db>>,
    pub span: RelativeSpan,
}

// ---------------------------------------------------------------------------
// ToTokens
// ---------------------------------------------------------------------------

use crate::tokens::{Punct, ToTokens, TokenCtx, TokenSink};

impl<'db> ToTokens<'db> for WhereClauseCst<'db> {
    fn to_tokens(&self, ctx: &TokenCtx<'_, 'db>, sink: &mut dyn TokenSink) {
        ctx.stash[self.subject].to_tokens(ctx, sink);
        sink.punct(Punct::Colon);
        let bounds = &ctx.stash[self.bounds];
        for (i, bound) in bounds.iter().enumerate() {
            if i > 0 {
                sink.punct(Punct::Plus);
            }
            bound.to_tokens(ctx, sink);
        }
    }
}
