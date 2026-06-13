use sage_stash::{AllocStashData, Ptr, Slice};

use crate::cst::paths::PathCst;
use crate::name::Name;
use crate::span::RelativeSpan;
use crate::types::Mutability;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct TypeCst<'db> {
    pub kind: TypeCstKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum TypeCstKind<'db> {
    Path(Ptr<PathCst<'db>>),
    Reference(Ptr<TypeCst<'db>>, Mutability),
    Slice(Ptr<TypeCst<'db>>),
    Array(Ptr<TypeCst<'db>>),
    Tuple(Slice<TypeCst<'db>>),
    Fn(Slice<TypeCst<'db>>, Option<Ptr<TypeCst<'db>>>),
    Never,
    Infer,
    Error,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum LifetimeCst<'db> {
    Named(Name<'db>),
    Anonymous,
}

// ---------------------------------------------------------------------------
// Type checking: TypeCst → Ty
// ---------------------------------------------------------------------------

use crate::cst::paths::Resolution;
use crate::resolve::Namespace;
use crate::sig_lower::CstLowerCtx;
use crate::symbol::{Intrinsic, SymbolData};

impl<'db> TypeCst<'db> {
    pub(crate) fn check(self, cx: &mut CstLowerCtx<'_, 'db>) -> Ty<'db> {
        let src = cx.src;
        match self.kind {
            TypeCstKind::Path(path_ptr) => {
                let path = src[path_ptr];
                let segments = &src[path.segments];
                let type_args = segments
                    .last()
                    .map(|s| s.check_type_args(cx))
                    .unwrap_or_else(|| cx.dst.alloc_slice(&[]));
                match path.resolve(cx, Namespace::Type) {
                    Resolution::Param(param) => Ty {
                        data: TyData::Param(param),
                    },
                    Resolution::Sym(sym) => resolution_to_ty(sym, type_args),
                    Resolution::SelfTy(ty) => ty,
                    Resolution::Local(_) | Resolution::Error => Ty {
                        data: TyData::Error,
                    },
                }
            }
            TypeCstKind::Reference(inner, m) => {
                let inner_ty = src[inner].check(cx);
                let inner = cx.dst.alloc(inner_ty);
                Ty {
                    data: TyData::Ref(inner, m, Lifetime::Erased),
                }
            }
            TypeCstKind::Tuple(elems) => {
                let tys: Vec<_> = src[elems].iter().map(|e| e.check(cx)).collect();
                let ptrs: Vec<_> = tys.into_iter().map(|t| cx.dst.alloc(t)).collect();
                let elems = cx.dst.alloc_slice(&ptrs);
                Ty {
                    data: TyData::Tuple(elems),
                }
            }
            TypeCstKind::Slice(inner) => {
                let inner_ty = src[inner].check(cx);
                let inner = cx.dst.alloc(inner_ty);
                Ty {
                    data: TyData::Slice(inner),
                }
            }
            TypeCstKind::Array(inner) => {
                let inner_ty = src[inner].check(cx);
                let inner = cx.dst.alloc(inner_ty);
                Ty {
                    data: TyData::Array(inner, Const::Literal(0)),
                }
            }
            TypeCstKind::Fn(params, ret) => {
                let param_tys: Vec<_> = src[params].iter().map(|p| p.check(cx)).collect();
                let param_ptrs: Vec<_> = param_tys.into_iter().map(|t| cx.dst.alloc(t)).collect();
                let param_slice = cx.dst.alloc_slice(&param_ptrs);
                let ret_ty = match ret {
                    Some(r) => src[r].check(cx),
                    None => {
                        let unit = cx.dst.alloc_slice(&[]);
                        Ty {
                            data: TyData::Tuple(unit),
                        }
                    }
                };
                let ret_ptr = cx.dst.alloc(ret_ty);
                Ty {
                    data: TyData::FnPtr(param_slice, ret_ptr),
                }
            }
            TypeCstKind::Never => Ty {
                data: TyData::Never,
            },
            TypeCstKind::Infer | TypeCstKind::Error => Ty {
                data: TyData::Error,
            },
        }
    }
}

fn resolution_to_ty<'db>(sym: Symbol<'db>, type_args: Slice<Ptr<Ty<'db>>>) -> Ty<'db> {
    match sym {
        SymbolData::Intrinsic(intrinsic) => Ty {
            data: intrinsic_to_ty_data(intrinsic),
        },
        _ => Ty {
            data: TyData::Adt(sym, type_args),
        },
    }
}

fn intrinsic_to_ty_data(intrinsic: Intrinsic) -> TyData<'static> {
    match intrinsic {
        Intrinsic::Bool => TyData::Bool,
        Intrinsic::Char => TyData::Char,
        Intrinsic::Str => TyData::Str,
        Intrinsic::Int(i) => TyData::Int(i),
        Intrinsic::Uint(u) => TyData::Uint(u),
        Intrinsic::Float(f) => TyData::Float(f),
    }
}
