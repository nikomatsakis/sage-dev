use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::cst::attrs::AttrCst;
use crate::cst::expr::ExprCst;
use crate::cst::ty::TypeCst;
use crate::name::Name;
use crate::span::RelativeSpan;

pub type StaticCst<'db> = Stashed<Ptr<StaticCstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct StaticCstData<'db> {
    pub attrs: Slice<AttrCst<'db>>,
    pub name: Name<'db>,
    pub is_mut: bool,
    pub ty: Option<Ptr<TypeCst<'db>>>,
    pub value: Option<Ptr<ExprCst<'db>>>,
    pub span: RelativeSpan,
}

// ---------------------------------------------------------------------------
// ToTokens
// ---------------------------------------------------------------------------

use crate::tokens::{Punct, ToTokens, TokenCtx, TokenSink, emit_attrs_filtered, emit_span_raw};

impl<'db> StaticCstData<'db> {
    pub fn to_tokens_skip_attrs(
        &self,
        ctx: &TokenCtx<'_, 'db>,
        sink: &mut dyn TokenSink,
        skip: &dyn Fn(usize) -> bool,
    ) {
        emit_attrs_filtered(ctx, sink, self.attrs, skip);
        sink.ident("static");
        if self.is_mut {
            sink.ident("mut");
        }
        sink.ident(self.name.text(ctx.db));
        if let Some(ty_ptr) = self.ty {
            sink.punct(Punct::Colon);
            ctx.stash[ty_ptr].to_tokens(ctx, sink);
        }
        if let Some(val_ptr) = self.value {
            sink.punct(Punct::Eq);
            let span = ctx.stash[val_ptr].span;
            emit_span_raw(ctx, sink, span.start, span.end);
        }
        sink.punct(Punct::Semi);
    }
}

impl<'db> ToTokens<'db> for StaticCstData<'db> {
    fn to_tokens(&self, ctx: &TokenCtx<'_, 'db>, sink: &mut dyn TokenSink) {
        self.to_tokens_skip_attrs(ctx, sink, &|_| false);
    }
}
