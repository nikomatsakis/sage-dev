use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::cst::attrs::AttrCst;
use crate::cst::consts::ConstCstData;
use crate::cst::fns::FnCstData;
use crate::cst::generics::GenericParamCst;
use crate::cst::type_aliases::TypeAliasCstData;
use crate::cst::where_clause::WhereClauseCst;
use crate::name::Name;
use crate::span::RelativeSpan;

pub type TraitCst<'db> = Stashed<Ptr<TraitCstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct TraitCstData<'db> {
    pub attrs: Slice<AttrCst<'db>>,
    pub name: Name<'db>,
    pub generics: Slice<GenericParamCst<'db>>,
    pub where_clauses: Slice<WhereClauseCst<'db>>,
    pub items: Slice<TraitItemCst<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum TraitItemCst<'db> {
    Fn(Ptr<FnCstData<'db>>),
    Type(Ptr<TypeAliasCstData<'db>>),
    Const(Ptr<ConstCstData<'db>>),
}

// ---------------------------------------------------------------------------
// ToTokens
// ---------------------------------------------------------------------------

use crate::tokens::{
    Delimiter, ToTokens, TokenCtx, TokenSink, emit_attrs_filtered, emit_generics,
    emit_where_clauses,
};

impl<'db> TraitCstData<'db> {
    pub fn to_tokens_skip_attrs(
        &self,
        ctx: &TokenCtx<'_, 'db>,
        sink: &mut dyn TokenSink,
        skip: &dyn Fn(usize) -> bool,
    ) {
        emit_attrs_filtered(ctx, sink, self.attrs, skip);
        sink.ident("trait");
        sink.ident(self.name.text(ctx.db));
        emit_generics(ctx, sink, self.generics);
        emit_where_clauses(ctx, sink, self.where_clauses);
        sink.group(Delimiter::Brace, &mut |s| {
            for item in &ctx.stash[self.items] {
                item.to_tokens(ctx, s);
            }
        });
    }
}

impl<'db> ToTokens<'db> for TraitCstData<'db> {
    fn to_tokens(&self, ctx: &TokenCtx<'_, 'db>, sink: &mut dyn TokenSink) {
        self.to_tokens_skip_attrs(ctx, sink, &|_| false);
    }
}

impl<'db> ToTokens<'db> for TraitItemCst<'db> {
    fn to_tokens(&self, ctx: &TokenCtx<'_, 'db>, sink: &mut dyn TokenSink) {
        match *self {
            TraitItemCst::Fn(ptr) => ctx.stash[ptr].to_tokens(ctx, sink),
            TraitItemCst::Type(ptr) => ctx.stash[ptr].to_tokens(ctx, sink),
            TraitItemCst::Const(ptr) => ctx.stash[ptr].to_tokens(ctx, sink),
        }
    }
}
