use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::cst::attrs::AttrCst;
use crate::cst::generics::GenericParamCst;
use crate::cst::ty::TypeCst;
use crate::cst::where_clause::WhereClauseCst;
use crate::name::Name;
use crate::span::RelativeSpan;

pub type StructCst<'db> = Stashed<Ptr<StructCstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct StructCstData<'db> {
    pub attrs: Slice<AttrCst<'db>>,
    pub name: Name<'db>,
    pub generics: Slice<GenericParamCst<'db>>,
    pub fields: Slice<FieldCst<'db>>,
    pub where_clauses: Slice<WhereClauseCst<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct FieldCst<'db> {
    pub name: Name<'db>,
    pub ty: Ptr<TypeCst<'db>>,
    pub span: RelativeSpan,
}

// ---------------------------------------------------------------------------
// ToTokens
// ---------------------------------------------------------------------------

use crate::tokens::{
    Delimiter, Punct, ToTokens, TokenCtx, TokenSink, emit_attrs_filtered, emit_generics,
    emit_where_clauses,
};

impl<'db> StructCstData<'db> {
    pub fn to_tokens_skip_attrs(
        &self,
        ctx: &TokenCtx<'_, 'db>,
        sink: &mut dyn TokenSink,
        skip: &dyn Fn(usize) -> bool,
    ) {
        emit_attrs_filtered(ctx, sink, self.attrs, skip);
        sink.ident("struct");
        sink.ident(self.name.text(ctx.db));
        emit_generics(ctx, sink, self.generics);
        emit_where_clauses(ctx, sink, self.where_clauses);
        let fields = &ctx.stash[self.fields];
        if fields.is_empty() {
            sink.punct(Punct::Semi);
        } else {
            sink.group(Delimiter::Brace, &mut |s| {
                for field in fields {
                    field.to_tokens(ctx, s);
                    s.punct(Punct::Comma);
                }
            });
        }
    }
}

impl<'db> ToTokens<'db> for StructCstData<'db> {
    fn to_tokens(&self, ctx: &TokenCtx<'_, 'db>, sink: &mut dyn TokenSink) {
        self.to_tokens_skip_attrs(ctx, sink, &|_| false);
    }
}

impl<'db> ToTokens<'db> for FieldCst<'db> {
    fn to_tokens(&self, ctx: &TokenCtx<'_, 'db>, sink: &mut dyn TokenSink) {
        sink.ident(self.name.text(ctx.db));
        sink.punct(Punct::Colon);
        ctx.stash[self.ty].to_tokens(ctx, sink);
    }
}
