use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::cst::attrs::AttrCst;
use crate::cst::generics::GenericParamCst;
use crate::cst::structs::FieldCst;
use crate::cst::ty::TypeCst;
use crate::cst::where_clause::WhereClauseCst;
use crate::name::Name;
use crate::span::RelativeSpan;

pub type EnumCst<'db> = Stashed<Ptr<EnumCstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct EnumCstData<'db> {
    pub attrs: Slice<AttrCst<'db>>,
    pub name: Name<'db>,
    pub generics: Slice<GenericParamCst<'db>>,
    pub variants: Slice<VariantCst<'db>>,
    pub where_clauses: Slice<WhereClauseCst<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct VariantCst<'db> {
    pub name: Name<'db>,
    pub fields: Slice<FieldCst<'db>>,
    pub discriminant: Option<Ptr<TypeCst<'db>>>,
    pub span: RelativeSpan,
}

// ---------------------------------------------------------------------------
// ToTokens
// ---------------------------------------------------------------------------

use crate::tokens::{
    Delimiter, Punct, ToTokens, TokenCtx, TokenSink, emit_attrs_filtered, emit_comma_sep,
    emit_generics, emit_where_clauses,
};

impl<'db> EnumCstData<'db> {
    pub fn to_tokens_skip_attrs(
        &self,
        ctx: &TokenCtx<'_, 'db>,
        sink: &mut dyn TokenSink,
        skip: &dyn Fn(usize) -> bool,
    ) {
        emit_attrs_filtered(ctx, sink, self.attrs, skip);
        sink.ident("enum");
        sink.ident(self.name.text(ctx.db));
        emit_generics(ctx, sink, self.generics);
        emit_where_clauses(ctx, sink, self.where_clauses);
        sink.group(Delimiter::Brace, &mut |s| {
            let variants = &ctx.stash[self.variants];
            for (i, variant) in variants.iter().enumerate() {
                if i > 0 {
                    s.punct(Punct::Comma);
                }
                variant.to_tokens(ctx, s);
            }
        });
    }
}

impl<'db> ToTokens<'db> for EnumCstData<'db> {
    fn to_tokens(&self, ctx: &TokenCtx<'_, 'db>, sink: &mut dyn TokenSink) {
        self.to_tokens_skip_attrs(ctx, sink, &|_| false);
    }
}

impl<'db> ToTokens<'db> for VariantCst<'db> {
    fn to_tokens(&self, ctx: &TokenCtx<'_, 'db>, sink: &mut dyn TokenSink) {
        sink.ident(self.name.text(ctx.db));
        let fields = &ctx.stash[self.fields];
        if !fields.is_empty() {
            sink.group(Delimiter::Brace, &mut |s| {
                emit_comma_sep(ctx, s, fields);
            });
        }
        if let Some(disc_ptr) = self.discriminant {
            sink.punct(Punct::Eq);
            ctx.stash[disc_ptr].to_tokens(ctx, sink);
        }
    }
}
