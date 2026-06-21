use sage_stash::{AllocStashData, Ptr, Slice};

use crate::cst::Mutability;
use crate::cst::paths::Path;
use crate::name::Name;
use crate::span::RelativeSpan;
use crate::ty::{Const, Lifetime, Ty};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct TypeCst<'db> {
    pub kind: TypeCstKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum TypeCstKind<'db> {
    Path(Ptr<Path<'db>>),
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

use crate::check::Check;
use crate::cst::paths::Resolution;
use crate::resolve::Namespace;
use crate::symbol::intrinsic::Intrinsic;
use crate::symbol::{Symbol, SymbolData};

impl<'db> TypeCst<'db> {
    pub(crate) fn check(self, cx: &mut Check<'_, 'db>) -> Ty<'db> {
        let src = cx.src;
        match self.kind {
            TypeCstKind::Path(path_ptr) => {
                let path = src[path_ptr];
                let type_args = path.final_segment(cx).check_type_args(cx);
                match path.resolve(cx, Namespace::Type) {
                    Resolution::Param(param) => Ty::Param(param),
                    Resolution::Sym(sym) => resolution_to_ty(cx.db, sym, type_args),
                    Resolution::SelfTy(ty) => ty,
                    Resolution::Local(_) | Resolution::Error => Ty::Error,
                }
            }
            TypeCstKind::Reference(inner, m) => {
                let inner_ty = src[inner].check(cx);
                let inner = cx.target_stash.alloc(inner_ty);
                Ty::Ref(inner, m, Lifetime::Erased)
            }
            TypeCstKind::Tuple(elems) => {
                let tys: Vec<_> = src[elems].iter().map(|e| e.check(cx)).collect();
                let ptrs: Vec<_> = tys.into_iter().map(|t| cx.target_stash.alloc(t)).collect();
                let elems = cx.target_stash.alloc_slice(&ptrs);
                Ty::Tuple(elems)
            }
            TypeCstKind::Slice(inner) => {
                let inner_ty = src[inner].check(cx);
                let inner = cx.target_stash.alloc(inner_ty);
                Ty::Slice(inner)
            }
            TypeCstKind::Array(inner) => {
                let inner_ty = src[inner].check(cx);
                let inner = cx.target_stash.alloc(inner_ty);
                Ty::Array(inner, Const::Literal(0))
            }
            TypeCstKind::Fn(params, ret) => {
                let param_tys: Vec<_> = src[params].iter().map(|p| p.check(cx)).collect();
                let param_ptrs: Vec<_> = param_tys
                    .into_iter()
                    .map(|t| cx.target_stash.alloc(t))
                    .collect();
                let param_slice = cx.target_stash.alloc_slice(&param_ptrs);
                let ret_ty = match ret {
                    Some(r) => src[r].check(cx),
                    None => {
                        let unit = cx.target_stash.alloc_slice(&[]);
                        Ty::Tuple(unit)
                    }
                };
                let ret_ptr = cx.target_stash.alloc(ret_ty);
                Ty::FnPtr(param_slice, ret_ptr)
            }
            TypeCstKind::Never => Ty::Never,
            TypeCstKind::Infer | TypeCstKind::Error => Ty::Error,
        }
    }
}

fn resolution_to_ty<'db>(
    db: &'db dyn crate::Db,
    sym: Symbol<'db>,
    type_args: Slice<Ptr<Ty<'db>>>,
) -> Ty<'db> {
    match sym.data(db) {
        SymbolData::IntrinsicTypeSymbol(s) => intrinsic_to_ty(s.0.intrinsic(db)),
        _ => Ty::Adt(sym, type_args),
    }
}

fn intrinsic_to_ty(intrinsic: Intrinsic) -> Ty<'static> {
    match intrinsic {
        Intrinsic::Bool => Ty::Bool,
        Intrinsic::Char => Ty::Char,
        Intrinsic::Str => Ty::Str,
        Intrinsic::Int(i) => Ty::Int(i),
        Intrinsic::Uint(u) => Ty::Uint(u),
        Intrinsic::Float(f) => Ty::Float(f),
    }
}
