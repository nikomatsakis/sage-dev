use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::cst::attrs::AttrCst;
use crate::cst::generics::GenericParamCst;
use crate::cst::paths::Path;
use crate::cst::traits::TraitItemCst;
use crate::cst::ty::TypeCst;
use crate::cst::where_clause::WhereClauseCst;
use crate::span::RelativeSpan;

pub type ImplCst<'db> = Stashed<Ptr<ImplCstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct ImplCstData<'db> {
    pub attrs: Slice<AttrCst<'db>>,
    pub generics: Slice<GenericParamCst<'db>>,
    pub self_ty: Ptr<TypeCst<'db>>,
    pub trait_path: Option<Ptr<Path<'db>>>,
    pub where_clauses: Slice<WhereClauseCst<'db>>,
    pub items: Slice<TraitItemCst<'db>>,
    pub span: RelativeSpan,
}

// ---------------------------------------------------------------------------
// ToTokens
// ---------------------------------------------------------------------------

use crate::tokens::{
    Delimiter, ToTokens, TokenCtx, TokenSink, emit_attrs_filtered, emit_generics,
    emit_where_clauses,
};

impl<'db> ImplCstData<'db> {
    pub fn to_tokens_skip_attrs(
        &self,
        ctx: &TokenCtx<'_, 'db>,
        sink: &mut dyn TokenSink,
        skip: &dyn Fn(usize) -> bool,
    ) {
        emit_attrs_filtered(ctx, sink, self.attrs, skip);
        sink.ident("impl");
        emit_generics(ctx, sink, self.generics);
        if let Some(trait_ptr) = self.trait_path {
            ctx.stash[trait_ptr].to_tokens(ctx, sink);
            sink.ident("for");
        }
        ctx.stash[self.self_ty].to_tokens(ctx, sink);
        emit_where_clauses(ctx, sink, self.where_clauses);
        sink.group(Delimiter::Brace, &mut |s| {
            for item in &ctx.stash[self.items] {
                item.to_tokens(ctx, s);
            }
        });
    }
}

impl<'db> ToTokens<'db> for ImplCstData<'db> {
    fn to_tokens(&self, ctx: &TokenCtx<'_, 'db>, sink: &mut dyn TokenSink) {
        self.to_tokens_skip_attrs(ctx, sink, &|_| false);
    }
}
