use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::cst::attrs::AttrCst;
use crate::cst::consts::ConstCstData;
use crate::cst::fns::FnCstData;
use crate::cst::generics::GenericParamCst;
use crate::cst::generics::TypeBoundCst;
use crate::cst::type_aliases::TypeAliasCstData;
use crate::cst::where_clause::WhereClauseCst;
use crate::name::Name;
use crate::span::RelativeSpan;

pub type TraitCst<'db> = Stashed<Ptr<TraitCstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub struct TraitCstData<'db> {
    pub attrs: Slice<AttrCst<'db>>,
    pub name: Name<'db>,
    pub generics: Slice<GenericParamCst<'db>>,
    pub supertraits: Slice<TypeBoundCst<'db>>,
    pub is_unsafe: bool,
    pub is_auto: bool,
    pub where_clauses: Slice<WhereClauseCst<'db>>,
    pub items: Slice<TraitItemCst<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, sage_reflect::Reflect)]
pub enum TraitItemCst<'db> {
    Fn {
        cst: Ptr<FnCstData<'db>>,
        placement: RelativeSpan,
    },
    Type {
        cst: Ptr<TypeAliasCstData<'db>>,
        placement: RelativeSpan,
    },
    Const {
        cst: Ptr<ConstCstData<'db>>,
        placement: RelativeSpan,
    },
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
        if self.is_unsafe {
            sink.ident("unsafe");
        }
        if self.is_auto {
            sink.ident("auto");
        }
        sink.ident("trait");
        sink.ident(self.name.text(ctx.db));
        emit_generics(ctx, sink, self.generics);
        let supertraits = &ctx.stash[self.supertraits];
        if !supertraits.is_empty() {
            sink.punct(crate::tokens::Punct::Colon);
            for (index, bound) in supertraits.iter().enumerate() {
                if index > 0 {
                    sink.punct(crate::tokens::Punct::Plus);
                }
                bound.to_tokens(ctx, sink);
            }
        }
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
            TraitItemCst::Fn { cst, .. } => ctx.stash[cst].to_tokens(ctx, sink),
            TraitItemCst::Type { cst, .. } => ctx.stash[cst].to_tokens(ctx, sink),
            TraitItemCst::Const { cst, .. } => ctx.stash[cst].to_tokens(ctx, sink),
        }
    }
}
