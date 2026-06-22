use sage_stash::{AllocStashData, Ptr, Slice};

use crate::check::Check;
use crate::cst::paths::Path;
use crate::cst::ty::TypeCst;
use crate::generic_param::{AstGenericParam, GenericParam, GenericParamKind};
use crate::name::Name;
use crate::resolve::{Namespace, Resolution};
use crate::span::RelativeSpan;
use crate::symbol::Symbol;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum GenericParamCst<'db> {
    Type {
        name: Name<'db>,
        bounds: Slice<TypeBoundCst<'db>>,
        span: RelativeSpan,
    },
    Lifetime {
        name: Name<'db>,
        span: RelativeSpan,
    },
    Const {
        name: Name<'db>,
        ty: Ptr<TypeCst<'db>>,
        span: RelativeSpan,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum TypeBoundCst<'db> {
    Trait(Ptr<Path<'db>>),
    Lifetime(Name<'db>),
}

// ---------------------------------------------------------------------------
// ToTokens
// ---------------------------------------------------------------------------

use crate::tokens::{Punct, ToTokens, TokenCtx, TokenSink};

impl<'db> ToTokens<'db> for GenericParamCst<'db> {
    fn to_tokens(&self, ctx: &TokenCtx<'_, 'db>, sink: &mut dyn TokenSink) {
        match *self {
            GenericParamCst::Type { name, bounds, .. } => {
                sink.ident(name.text(ctx.db));
                let bounds_slice = &ctx.stash[bounds];
                if !bounds_slice.is_empty() {
                    sink.punct(Punct::Colon);
                    for (i, bound) in bounds_slice.iter().enumerate() {
                        if i > 0 {
                            sink.punct(Punct::Plus);
                        }
                        bound.to_tokens(ctx, sink);
                    }
                }
            }
            GenericParamCst::Lifetime { name, .. } => {
                sink.ident(name.text(ctx.db));
            }
            GenericParamCst::Const { name, ty, .. } => {
                sink.ident("const");
                sink.ident(name.text(ctx.db));
                sink.punct(Punct::Colon);
                ctx.stash[ty].to_tokens(ctx, sink);
            }
        }
    }
}

impl<'db> ToTokens<'db> for TypeBoundCst<'db> {
    fn to_tokens(&self, ctx: &TokenCtx<'_, 'db>, sink: &mut dyn TokenSink) {
        match *self {
            TypeBoundCst::Trait(path_ptr) => {
                ctx.stash[path_ptr].to_tokens(ctx, sink);
            }
            TypeBoundCst::Lifetime(name) => {
                sink.ident(name.text(ctx.db));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Checking generic params: mint GenericParam symbols and bind in ribs
// ---------------------------------------------------------------------------

pub trait CheckGenerics<'db> {
    fn check(
        self,
        db: &'db dyn crate::Db,
        cx: &mut Check<'_, 'db>,
        parent: Symbol<'db>,
    ) -> Slice<GenericParam<'db>>;
}

impl<'db> CheckGenerics<'db> for Slice<GenericParamCst<'db>> {
    fn check(
        self,
        db: &'db dyn crate::Db,
        cx: &mut Check<'_, 'db>,
        parent: Symbol<'db>,
    ) -> Slice<GenericParam<'db>> {
        let params = &cx.source_stash[self];
        let mut generic_params = Vec::new();
        for (i, param) in params.iter().enumerate() {
            let (name, span, kind) = match *param {
                GenericParamCst::Type { name, span, .. } => (name, span, GenericParamKind::Type),
                GenericParamCst::Lifetime { name, span, .. } => {
                    (name, span, GenericParamKind::Lifetime)
                }
                GenericParamCst::Const { name, span, .. } => (name, span, GenericParamKind::Const),
            };
            let ast_param = AstGenericParam::new(db, kind, Some(name), span, parent, i as u32);
            let gp = GenericParam::Ast(ast_param);
            cx.resolver
                .ribs
                .add(name, Namespace::Type, Resolution::Param(gp));
            generic_params.push(gp);
        }
        cx.target_stash.alloc_slice(&generic_params)
    }
}
