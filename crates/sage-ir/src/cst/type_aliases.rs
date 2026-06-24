use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::cst::attrs::AttrCst;
use crate::cst::generics::GenericParamCst;
use crate::cst::ty::TypeCst;
use crate::cst::where_clause::WhereClauseCst;
use crate::name::Name;
use crate::span::RelativeSpan;

pub type TypeAliasCst<'db> = Stashed<Ptr<TypeAliasCstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct TypeAliasCstData<'db> {
    pub attrs: Slice<AttrCst<'db>>,
    pub name: Name<'db>,
    pub generics: Slice<GenericParamCst<'db>>,
    pub ty: Option<Ptr<TypeCst<'db>>>,
    pub where_clauses: Slice<WhereClauseCst<'db>>,
    pub span: RelativeSpan,
}

// ---------------------------------------------------------------------------
// ToTokens
// ---------------------------------------------------------------------------

use crate::tokens::{
    Punct, ToTokens, TokenCtx, TokenSink, emit_attrs_filtered, emit_generics, emit_where_clauses,
};

impl<'db> TypeAliasCstData<'db> {
    pub fn to_tokens_skip_attrs(
        &self,
        ctx: &TokenCtx<'_, 'db>,
        sink: &mut dyn TokenSink,
        skip: &dyn Fn(usize) -> bool,
    ) {
        emit_attrs_filtered(ctx, sink, self.attrs, skip);
        sink.ident("type");
        sink.ident(self.name.text(ctx.db));
        emit_generics(ctx, sink, self.generics);
        emit_where_clauses(ctx, sink, self.where_clauses);
        if let Some(ty_ptr) = self.ty {
            sink.punct(Punct::Eq);
            ctx.stash[ty_ptr].to_tokens(ctx, sink);
        }
        sink.punct(Punct::Semi);
    }
}

impl<'db> ToTokens<'db> for TypeAliasCstData<'db> {
    fn to_tokens(&self, ctx: &TokenCtx<'_, 'db>, sink: &mut dyn TokenSink) {
        self.to_tokens_skip_attrs(ctx, sink, &|_| false);
    }
}
