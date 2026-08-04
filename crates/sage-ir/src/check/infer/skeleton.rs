use crate::cst::Mutability;
use crate::diagnostic::ErrorReported;
use crate::generic_param::GenericParam;
use crate::symbol::{Symbol, TraitSymbol, TypeAliasSymbol};
use crate::ty::{
    AliasTy, Const, FloatTy, InferVarIndex, IntTy, Lifetime, NamedAliasTy, OpaqueAliasTy,
    ProjectionTy, TraitRef, Ty, UintTy,
};
use sage_stash::{Ptr, Stash};
use smallvec::SmallVec;

pub type Children<'db> = SmallVec<[Ptr<Ty<'db>>; 5]>;

/// The rigid part of a type — everything except the sub-type children.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Skeleton<'db> {
    // Primitives (0 children)
    Bool,
    Char,
    Int(IntTy),
    Uint(UintTy),
    Float(FloatTy),
    Str,
    Never,
    Error(ErrorReported),
    Param(GenericParam<'db>),
    InferVar(InferVarIndex),

    // Compound (1+ children)
    Adt(Symbol<'db>, u32),
    NamedAlias(TypeAliasSymbol<'db>, u32),
    AssociatedAlias {
        associated_ty: TypeAliasSymbol<'db>,
        trait_sym: TraitSymbol<'db>,
        trait_arg_count: u32,
        item_arg_count: u32,
    },
    OpaqueAlias(TypeAliasSymbol<'db>, u32),
    Ref(Mutability, Lifetime),
    Tuple(u32),
    Slice,
    Array(Const<'db>),
    /// Children are [params..., ret].
    FnPtr(u32),
}

/// A decomposed type: skeleton + children.
pub struct Decomposed<'db> {
    pub skeleton: Skeleton<'db>,
    pub children: Children<'db>,
}

/// Decompose a type into its skeleton and sub-type children.
pub fn decompose<'db>(stash: &Stash, ty: Ptr<Ty<'db>>) -> Decomposed<'db> {
    match stash[ty] {
        Ty::Bool => leaf(Skeleton::Bool),
        Ty::Char => leaf(Skeleton::Char),
        Ty::Int(i) => leaf(Skeleton::Int(i)),
        Ty::Uint(u) => leaf(Skeleton::Uint(u)),
        Ty::Float(f) => leaf(Skeleton::Float(f)),
        Ty::Str => leaf(Skeleton::Str),
        Ty::Never => leaf(Skeleton::Never),
        Ty::Error(e) => leaf(Skeleton::Error(e)),
        Ty::Param(p) => leaf(Skeleton::Param(p)),
        Ty::InferVar(idx) => leaf(Skeleton::InferVar(idx)),

        Ty::Adt(sym, args) => {
            let children: Children<'db> = stash[args].iter().copied().collect();
            let arity = children.len() as u32;
            Decomposed {
                skeleton: Skeleton::Adt(sym, arity),
                children,
            }
        }

        Ty::Alias(AliasTy::Named(alias)) => {
            let children: Children<'db> = stash[alias.args].iter().copied().collect();
            let arity = children.len() as u32;
            Decomposed {
                skeleton: Skeleton::NamedAlias(alias.def, arity),
                children,
            }
        }

        Ty::Alias(AliasTy::Associated(projection)) => {
            let mut children = Children::new();
            children.push(projection.self_ty);
            children.extend(stash[projection.trait_ref.args].iter().copied());
            children.extend(stash[projection.args].iter().copied());
            Decomposed {
                skeleton: Skeleton::AssociatedAlias {
                    associated_ty: projection.associated_ty,
                    trait_sym: projection.trait_ref.trait_sym,
                    trait_arg_count: stash[projection.trait_ref.args].len() as u32,
                    item_arg_count: stash[projection.args].len() as u32,
                },
                children,
            }
        }

        Ty::Alias(AliasTy::Opaque(alias)) => {
            let children: Children<'db> = stash[alias.args].iter().copied().collect();
            let arity = children.len() as u32;
            Decomposed {
                skeleton: Skeleton::OpaqueAlias(alias.def, arity),
                children,
            }
        }

        Ty::Ref(inner, m, lt) => Decomposed {
            skeleton: Skeleton::Ref(m, lt),
            children: smallvec::smallvec![inner],
        },

        Ty::Tuple(elems) => {
            let children: Children<'db> = stash[elems].iter().copied().collect();
            let arity = children.len() as u32;
            Decomposed {
                skeleton: Skeleton::Tuple(arity),
                children,
            }
        }

        Ty::Slice(inner) => Decomposed {
            skeleton: Skeleton::Slice,
            children: smallvec::smallvec![inner],
        },

        Ty::Array(inner, c) => Decomposed {
            skeleton: Skeleton::Array(c),
            children: smallvec::smallvec![inner],
        },

        Ty::FnPtr(params, ret) => {
            let mut children: Children<'db> = stash[params].iter().copied().collect();
            let param_count = children.len() as u32;
            children.push(ret);
            Decomposed {
                skeleton: Skeleton::FnPtr(param_count),
                children,
            }
        }
    }
}

/// Recompose a skeleton and children back into a type.
pub fn recompose<'db>(
    stash: &mut Stash,
    skeleton: Skeleton<'db>,
    children: &[Ptr<Ty<'db>>],
) -> Ptr<Ty<'db>> {
    let ty: Ty<'db> = match skeleton {
        Skeleton::Bool => Ty::Bool,
        Skeleton::Char => Ty::Char,
        Skeleton::Int(i) => Ty::Int(i),
        Skeleton::Uint(u) => Ty::Uint(u),
        Skeleton::Float(f) => Ty::Float(f),
        Skeleton::Str => Ty::Str,
        Skeleton::Never => Ty::Never,
        Skeleton::Error(e) => Ty::Error(e),
        Skeleton::Param(p) => Ty::Param(p),
        Skeleton::InferVar(idx) => Ty::InferVar(idx),

        Skeleton::Adt(sym, _arity) => {
            let args = stash.alloc_slice(children);
            Ty::Adt(sym, args)
        }

        Skeleton::NamedAlias(def, _arity) => Ty::Alias(AliasTy::Named(NamedAliasTy {
            def,
            args: stash.alloc_slice(children),
        })),

        Skeleton::AssociatedAlias {
            associated_ty,
            trait_sym,
            trait_arg_count,
            item_arg_count,
        } => {
            let trait_end = 1 + trait_arg_count as usize;
            let item_end = trait_end + item_arg_count as usize;
            Ty::Alias(AliasTy::Associated(ProjectionTy {
                associated_ty,
                self_ty: children[0],
                trait_ref: TraitRef {
                    trait_sym,
                    args: stash.alloc_slice(&children[1..trait_end]),
                },
                args: stash.alloc_slice(&children[trait_end..item_end]),
            }))
        }

        Skeleton::OpaqueAlias(def, _arity) => Ty::Alias(AliasTy::Opaque(OpaqueAliasTy {
            def,
            args: stash.alloc_slice(children),
        })),

        Skeleton::Ref(m, lt) => Ty::Ref(children[0], m, lt),

        Skeleton::Tuple(_arity) => {
            let elems = stash.alloc_slice(children);
            Ty::Tuple(elems)
        }

        Skeleton::Slice => Ty::Slice(children[0]),

        Skeleton::Array(c) => Ty::Array(children[0], c),

        Skeleton::FnPtr(param_count) => {
            let params_slice = stash.alloc_slice(&children[..param_count as usize]);
            let ret = children[param_count as usize];
            Ty::FnPtr(params_slice, ret)
        }
    };

    stash.alloc(ty)
}

fn leaf<'db>(skeleton: Skeleton<'db>) -> Decomposed<'db> {
    Decomposed {
        skeleton,
        children: Children::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty::{IntTy, Ty};

    #[test]
    fn round_trip_leaf() {
        let mut stash = Stash::new();
        let ty = stash.alloc(Ty::Int(IntTy::I32));
        let d = decompose(&stash, ty);
        assert_eq!(d.skeleton, Skeleton::Int(IntTy::I32));
        assert!(d.children.is_empty());

        let recomposed = recompose(&mut stash, d.skeleton, &d.children);
        assert_eq!(recomposed, ty);
    }

    #[test]
    fn round_trip_tuple() {
        let mut stash = Stash::new();
        let i32_ptr = stash.alloc(Ty::Int(IntTy::I32));
        let bool_ptr = stash.alloc(Ty::Bool);
        let tup_elems = stash.alloc_slice(&[i32_ptr, bool_ptr]);
        let tup = stash.alloc(Ty::Tuple(tup_elems));

        let d = decompose(&stash, tup);
        assert_eq!(d.skeleton, Skeleton::Tuple(2));
        assert_eq!(d.children.len(), 2);

        let recomposed = recompose(&mut stash, d.skeleton, &d.children);
        assert_eq!(recomposed, tup);
    }

    #[test]
    fn round_trip_fn_ptr() {
        let mut stash = Stash::new();
        let i32_ptr = stash.alloc(Ty::Int(IntTy::I32));
        let bool_ptr = stash.alloc(Ty::Bool);
        let params = stash.alloc_slice(&[i32_ptr]);
        let fn_ty = stash.alloc(Ty::FnPtr(params, bool_ptr));

        let d = decompose(&stash, fn_ty);
        assert_eq!(d.skeleton, Skeleton::FnPtr(1));
        assert_eq!(d.children.len(), 2);

        let recomposed = recompose(&mut stash, d.skeleton, &d.children);
        assert_eq!(recomposed, fn_ty);
    }
}
