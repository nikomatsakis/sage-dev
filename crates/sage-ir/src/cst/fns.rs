use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::cst::attrs::AttrCst;
use crate::cst::expr::ExprCst;
use crate::cst::generics::GenericParamCst;
use crate::cst::ty::TypeCst;
use crate::cst::where_clause::WhereClauseCst;
use crate::name::Name;
use crate::span::RelativeSpan;

pub type FnCst<'db> = Stashed<Ptr<FnCstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct FnCstData<'db> {
    pub attrs: Slice<AttrCst<'db>>,
    pub name: Name<'db>,
    pub generics: Slice<GenericParamCst<'db>>,
    pub params: Slice<ParamCst<'db>>,
    pub ret: Option<Ptr<TypeCst<'db>>>,
    pub body: Option<Ptr<ExprCst<'db>>>,
    pub where_clauses: Slice<WhereClauseCst<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct ParamCst<'db> {
    pub name: Option<Name<'db>>,
    pub ty: Ptr<TypeCst<'db>>,
    pub span: RelativeSpan,
}

// ---------------------------------------------------------------------------
// ToTokens
// ---------------------------------------------------------------------------

use crate::tokens::{
    Delimiter, Punct, ToTokens, TokenCtx, TokenSink, emit_attrs_filtered, emit_comma_sep,
    emit_generics, emit_span_raw, emit_where_clauses,
};

impl<'db> FnCstData<'db> {
    pub fn to_tokens_skip_attrs(
        &self,
        ctx: &TokenCtx<'_, 'db>,
        sink: &mut dyn TokenSink,
        skip: &dyn Fn(usize) -> bool,
    ) {
        emit_attrs_filtered(ctx, sink, self.attrs, skip);
        sink.ident("fn");
        sink.ident(self.name.text(ctx.db));
        emit_generics(ctx, sink, self.generics);
        sink.group(Delimiter::Paren, &mut |s| {
            emit_comma_sep(ctx, s, &ctx.stash[self.params]);
        });
        if let Some(ret_ptr) = self.ret {
            sink.punct(Punct::Arrow);
            ctx.stash[ret_ptr].to_tokens(ctx, sink);
        }
        emit_where_clauses(ctx, sink, self.where_clauses);
        if let Some(body_ptr) = self.body {
            let span = ctx.stash[body_ptr].span;
            emit_span_raw(ctx, sink, span.start, span.end);
        } else {
            sink.punct(Punct::Semi);
        }
    }
}

impl<'db> ToTokens<'db> for FnCstData<'db> {
    fn to_tokens(&self, ctx: &TokenCtx<'_, 'db>, sink: &mut dyn TokenSink) {
        self.to_tokens_skip_attrs(ctx, sink, &|_| false);
    }
}

impl<'db> ToTokens<'db> for ParamCst<'db> {
    fn to_tokens(&self, ctx: &TokenCtx<'_, 'db>, sink: &mut dyn TokenSink) {
        if let Some(name) = self.name {
            sink.ident(name.text(ctx.db));
            sink.punct(Punct::Colon);
        }
        ctx.stash[self.ty].to_tokens(ctx, sink);
    }
}
