use rustc_hash::FxHashSet;
use sage_stash::{Ptr, Stash};

use crate::diagnostic::ErrorReported;
use crate::ty::{Const, InferVarIndex, Lifetime, Ty};

use super::bound::Bound;
use super::egraph::{CommitEffects, VersionedEGraph};
use super::skeleton::decompose;
use super::version::{InferDomain, Universe, Version};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UnifyError<'db> {
    Mismatch {
        left: Ptr<Ty<'db>>,
        right: Ptr<Ty<'db>>,
    },
    OccursCheck {
        variable: InferVarIndex,
        in_ty: Ptr<Ty<'db>>,
    },
    UniverseLeak {
        variable: InferVarIndex,
        ceiling: Universe,
    },
    DomainConflict {
        left: Ptr<Ty<'db>>,
        left_domain: Option<InferDomain>,
        right: Ptr<Ty<'db>>,
        right_domain: Option<InferDomain>,
    },
    Reported(ErrorReported),
}

/// Apply one structural equality atomically against `parent`.
pub fn try_unify<'db>(
    egraph: &mut VersionedEGraph<'db>,
    stash: &mut Stash,
    parent: Version,
    left: Ptr<Ty<'db>>,
    right: Ptr<Ty<'db>>,
) -> Result<CommitEffects, UnifyError<'db>> {
    try_unify_batch(egraph, stash, parent, &[(left, right)])
}

/// Apply a batch of structural equalities in one transaction.
pub fn try_unify_batch<'db>(
    egraph: &mut VersionedEGraph<'db>,
    stash: &mut Stash,
    parent: Version,
    equalities: &[(Ptr<Ty<'db>>, Ptr<Ty<'db>>)],
) -> Result<CommitEffects, UnifyError<'db>> {
    try_probe(egraph, stash, parent, |egraph, stash, child| {
        for &(left, right) in equalities {
            unify_in_probe(egraph, stash, child, left, right)?;
        }
        Ok(())
    })
    .map(|(_, effects)| effects)
}

/// Set a bound atomically, including universe validation/lowering.
pub fn try_set_bound<'db>(
    egraph: &mut VersionedEGraph<'db>,
    stash: &mut Stash,
    parent: Version,
    variable_ty: Ptr<Ty<'db>>,
    bound: Bound<'db>,
) -> Result<CommitEffects, UnifyError<'db>> {
    try_probe(egraph, stash, parent, |egraph, stash, child| {
        let variable_ty = egraph.find(child, variable_ty);
        let variable = match stash[variable_ty] {
            Ty::InferVar(variable) => variable,
            Ty::Bool
            | Ty::Char
            | Ty::Int(_)
            | Ty::Uint(_)
            | Ty::Float(_)
            | Ty::Str
            | Ty::Adt(_, _)
            | Ty::Alias(_)
            | Ty::Ref(_, _, _)
            | Ty::Tuple(_)
            | Ty::Slice(_)
            | Ty::Array(_, _)
            | Ty::FnPtr(_, _)
            | Ty::Param(_)
            | Ty::Never
            | Ty::Error(_) => {
                return Err(UnifyError::Mismatch {
                    left: variable_ty,
                    right: bound.ty().unwrap_or(variable_ty),
                });
            }
        };
        match bound {
            Bound::None => {}
            Bound::AtLeast(bound_ty) => {
                let ceiling = egraph.current_universe(child, variable);
                make_accessible(
                    egraph,
                    stash,
                    child,
                    bound_ty,
                    variable,
                    ceiling,
                    &mut FxHashSet::default(),
                )?;
            }
            Bound::Exactly(bound_ty) => {
                unify_in_probe(egraph, stash, child, variable_ty, bound_ty)?;
            }
        }
        egraph.set_bound_in(child, stash, variable_ty, bound);
        Ok(())
    })
    .map(|(_, effects)| effects)
}

/// Own a short-lived child probe from creation through exactly one commit or
/// discard path. Solver candidates intentionally do not use this helper: they
/// extract and discard their long-lived alternative branches instead.
fn try_probe<'db, T>(
    egraph: &mut VersionedEGraph<'db>,
    stash: &mut Stash,
    parent: Version,
    operation: impl FnOnce(&mut VersionedEGraph<'db>, &mut Stash, Version) -> Result<T, UnifyError<'db>>,
) -> Result<(T, CommitEffects), UnifyError<'db>> {
    let child = egraph.branch_from(parent);
    match operation(egraph, stash, child) {
        Ok(value) => {
            egraph.rebuild(child, stash);
            let effects = egraph.collapse_into(child, parent);
            Ok((value, effects))
        }
        Err(error) => {
            egraph.discard(child);
            Err(error)
        }
    }
}

/// Partially mutating unification used only inside an owning transaction.
pub(crate) fn unify_in_probe<'db>(
    egraph: &mut VersionedEGraph<'db>,
    stash: &mut Stash,
    version: Version,
    left: Ptr<Ty<'db>>,
    right: Ptr<Ty<'db>>,
) -> Result<(), UnifyError<'db>> {
    let left = egraph.find_mut(version, left);
    let right = egraph.find_mut(version, right);
    if left == right {
        return Ok(());
    }

    match (stash[left], stash[right]) {
        (Ty::Error(error), _) | (_, Ty::Error(error)) => {
            return Err(UnifyError::Reported(error));
        }
        (Ty::InferVar(left_var), Ty::InferVar(right_var)) => {
            let domain = compatible_domain(egraph, stash, version, left, right)?
                .expect("two inference classes must have a common domain");
            let common_ceiling = egraph
                .current_universe(version, left_var)
                .min(egraph.current_universe(version, right_var));
            egraph.lower_universe(version, left_var, common_ceiling);
            egraph.lower_universe(version, right_var, common_ceiling);
            let root = egraph.union(version, stash, left, right);
            egraph.set_domain_in(version, stash, root, domain);
            return Ok(());
        }
        (Ty::InferVar(variable), _) => {
            compatible_domain(egraph, stash, version, left, right)?;
            bind_variable(egraph, stash, version, left, variable, right)?;
            return Ok(());
        }
        (_, Ty::InferVar(variable)) => {
            compatible_domain(egraph, stash, version, left, right)?;
            bind_variable(egraph, stash, version, right, variable, left)?;
            return Ok(());
        }
        (
            Ty::Bool
            | Ty::Char
            | Ty::Int(_)
            | Ty::Uint(_)
            | Ty::Float(_)
            | Ty::Str
            | Ty::Adt(_, _)
            | Ty::Alias(_)
            | Ty::Ref(_, _, _)
            | Ty::Tuple(_)
            | Ty::Slice(_)
            | Ty::Array(_, _)
            | Ty::FnPtr(_, _)
            | Ty::Param(_)
            | Ty::Never,
            _,
        ) => {}
    }

    let left_decomposed = decompose(stash, left);
    let right_decomposed = decompose(stash, right);
    if left_decomposed.skeleton != right_decomposed.skeleton
        || left_decomposed.children.len() != right_decomposed.children.len()
    {
        return Err(UnifyError::Mismatch { left, right });
    }

    for child in &left_decomposed.children {
        egraph.add_dependent(version, *child, left);
    }
    for child in &right_decomposed.children {
        egraph.add_dependent(version, *child, right);
    }

    for (left_child, right_child) in left_decomposed
        .children
        .into_iter()
        .zip(right_decomposed.children)
    {
        unify_in_probe(egraph, stash, version, left_child, right_child)?;
    }

    egraph.union(version, stash, left, right);
    Ok(())
}

/// Return the intersection when both sides are unresolved inference classes.
/// A successful variable-to-rigid relation has already passed membership and
/// therefore returns `None`.
fn compatible_domain<'db>(
    egraph: &VersionedEGraph<'db>,
    stash: &Stash,
    version: Version,
    left: Ptr<Ty<'db>>,
    right: Ptr<Ty<'db>>,
) -> Result<Option<InferDomain>, UnifyError<'db>> {
    let left = egraph.find(version, left);
    let right = egraph.find(version, right);
    let left_domain = egraph.effective_domain(version, stash, left);
    let right_domain = egraph.effective_domain(version, stash, right);

    match (left_domain, right_domain) {
        (Some(left_domain), Some(right_domain)) => left_domain
            .intersect(right_domain)
            .map(Some)
            .ok_or_else(|| domain_conflict(egraph, stash, version, left, right)),
        (Some(domain), None) => {
            if domain.admits(stash[right]) {
                Ok(None)
            } else {
                Err(domain_conflict(egraph, stash, version, left, right))
            }
        }
        (None, Some(domain)) => {
            if domain.admits(stash[left]) {
                Ok(None)
            } else {
                Err(domain_conflict(egraph, stash, version, left, right))
            }
        }
        (None, None) => Ok(None),
    }
}

fn domain_conflict<'db>(
    egraph: &VersionedEGraph<'db>,
    stash: &Stash,
    version: Version,
    left: Ptr<Ty<'db>>,
    right: Ptr<Ty<'db>>,
) -> UnifyError<'db> {
    UnifyError::DomainConflict {
        left,
        left_domain: semantic_domain(egraph, stash, version, left),
        right,
        right_domain: semantic_domain(egraph, stash, version, right),
    }
}

fn semantic_domain<'db>(
    egraph: &VersionedEGraph<'db>,
    stash: &Stash,
    version: Version,
    ty: Ptr<Ty<'db>>,
) -> Option<InferDomain> {
    let root = egraph.find(version, ty);
    if let Some(domain) = egraph.effective_domain(version, stash, root) {
        return Some(domain);
    }
    match stash[root] {
        Ty::Int(_) | Ty::Uint(_) => Some(InferDomain::Integer),
        Ty::Float(_) => Some(InferDomain::Float),
        Ty::Bool
        | Ty::Char
        | Ty::Str
        | Ty::Adt(_, _)
        | Ty::Alias(_)
        | Ty::Ref(_, _, _)
        | Ty::Tuple(_)
        | Ty::Slice(_)
        | Ty::Array(_, _)
        | Ty::FnPtr(_, _)
        | Ty::Param(_)
        | Ty::InferVar(_)
        | Ty::Never
        | Ty::Error(_) => None,
    }
}

fn bind_variable<'db>(
    egraph: &mut VersionedEGraph<'db>,
    stash: &mut Stash,
    version: Version,
    variable_ty: Ptr<Ty<'db>>,
    variable: InferVarIndex,
    value: Ptr<Ty<'db>>,
) -> Result<(), UnifyError<'db>> {
    if occurs_in(
        egraph,
        stash,
        version,
        variable,
        value,
        &mut FxHashSet::default(),
    ) {
        return Err(UnifyError::OccursCheck {
            variable,
            in_ty: value,
        });
    }

    let ceiling = egraph.current_universe(version, variable);
    make_accessible(
        egraph,
        stash,
        version,
        value,
        variable,
        ceiling,
        &mut FxHashSet::default(),
    )?;
    egraph.set_bound_in(version, stash, variable_ty, Bound::Exactly(value));
    egraph.union(version, stash, variable_ty, value);
    Ok(())
}

/// Install a previously reported error as the exact recovery type.
pub(crate) fn install_error_recovery<'db>(
    egraph: &mut VersionedEGraph<'db>,
    stash: &mut Stash,
    parent: Version,
    variable_ty: Ptr<Ty<'db>>,
    error_ty: Ptr<Ty<'db>>,
) -> Result<CommitEffects, UnifyError<'db>> {
    try_probe(egraph, stash, parent, |egraph, stash, child| {
        let variable_ty = egraph.find(child, variable_ty);
        let error_ty = egraph.find(child, error_ty);
        match stash[variable_ty] {
            Ty::InferVar(_) => {}
            Ty::Bool
            | Ty::Char
            | Ty::Int(_)
            | Ty::Uint(_)
            | Ty::Float(_)
            | Ty::Str
            | Ty::Adt(_, _)
            | Ty::Alias(_)
            | Ty::Ref(_, _, _)
            | Ty::Tuple(_)
            | Ty::Slice(_)
            | Ty::Array(_, _)
            | Ty::FnPtr(_, _)
            | Ty::Param(_)
            | Ty::Never
            | Ty::Error(_) => {
                return Err(UnifyError::Mismatch {
                    left: variable_ty,
                    right: error_ty,
                });
            }
        }
        match stash[error_ty] {
            Ty::Error(_) => {}
            Ty::Bool
            | Ty::Char
            | Ty::Int(_)
            | Ty::Uint(_)
            | Ty::Float(_)
            | Ty::Str
            | Ty::Adt(_, _)
            | Ty::Alias(_)
            | Ty::Ref(_, _, _)
            | Ty::Tuple(_)
            | Ty::Slice(_)
            | Ty::Array(_, _)
            | Ty::FnPtr(_, _)
            | Ty::Param(_)
            | Ty::InferVar(_)
            | Ty::Never => {
                return Err(UnifyError::Mismatch {
                    left: variable_ty,
                    right: error_ty,
                });
            }
        }
        debug_assert!(
            egraph
                .effective_domain(child, stash, variable_ty)
                .is_some_and(|domain| domain.admits(stash[error_ty]))
        );
        egraph.set_bound_in(child, stash, variable_ty, Bound::Exactly(error_ty));
        egraph.union(child, stash, variable_ty, error_ty);
        Ok(())
    })
    .map(|(_, effects)| effects)
}

fn occurs_in<'db>(
    egraph: &VersionedEGraph<'db>,
    stash: &Stash,
    version: Version,
    needle: InferVarIndex,
    ty: Ptr<Ty<'db>>,
    visited: &mut FxHashSet<Ptr<Ty<'db>>>,
) -> bool {
    let ty = egraph.find(version, ty);
    if !visited.insert(ty) {
        return false;
    }
    match stash[ty] {
        Ty::InferVar(index) => {
            if index == needle {
                return true;
            }
            egraph
                .get_bound(version, ty)
                .ty()
                .is_some_and(|bound| occurs_in(egraph, stash, version, needle, bound, visited))
        }
        Ty::Bool
        | Ty::Char
        | Ty::Int(_)
        | Ty::Uint(_)
        | Ty::Float(_)
        | Ty::Str
        | Ty::Adt(_, _)
        | Ty::Alias(_)
        | Ty::Ref(_, _, _)
        | Ty::Tuple(_)
        | Ty::Slice(_)
        | Ty::Array(_, _)
        | Ty::FnPtr(_, _)
        | Ty::Param(_)
        | Ty::Never
        | Ty::Error(_) => decompose(stash, ty)
            .children
            .into_iter()
            .any(|child| occurs_in(egraph, stash, version, needle, child, visited)),
    }
}

/// Lower nested flexible variables to `ceiling` and reject inaccessible rigid
/// parameters. This is called only inside a child transaction, so all lowering
/// is discarded if a later check fails.
fn make_accessible<'db>(
    egraph: &mut VersionedEGraph<'db>,
    stash: &Stash,
    version: Version,
    ty: Ptr<Ty<'db>>,
    outer_variable: InferVarIndex,
    ceiling: Universe,
    visited: &mut FxHashSet<Ptr<Ty<'db>>>,
) -> Result<(), UnifyError<'db>> {
    let ty = egraph.find(version, ty);
    if !visited.insert(ty) {
        return Ok(());
    }

    match stash[ty] {
        Ty::Param(param) => {
            if egraph.placeholder_universe(param) > ceiling {
                return Err(UnifyError::UniverseLeak {
                    variable: outer_variable,
                    ceiling,
                });
            }
        }
        Ty::InferVar(variable) => {
            egraph.lower_universe(version, variable, ceiling);
            if let Some(bound) = egraph.get_bound(version, ty).ty() {
                make_accessible(
                    egraph,
                    stash,
                    version,
                    bound,
                    outer_variable,
                    ceiling,
                    visited,
                )?;
            }
        }
        Ty::Array(_, Const::Param(param)) => {
            if egraph.placeholder_universe(param) > ceiling {
                return Err(UnifyError::UniverseLeak {
                    variable: outer_variable,
                    ceiling,
                });
            }
        }
        Ty::Bool
        | Ty::Char
        | Ty::Int(_)
        | Ty::Uint(_)
        | Ty::Float(_)
        | Ty::Str
        | Ty::Adt(_, _)
        | Ty::Alias(_)
        | Ty::Ref(_, _, Lifetime::Dummy)
        | Ty::Tuple(_)
        | Ty::Slice(_)
        | Ty::Array(_, Const::Literal(_) | Const::Other(_))
        | Ty::FnPtr(_, _)
        | Ty::Never
        | Ty::Error(_) => {}
    }

    for child in decompose(stash, ty).children {
        make_accessible(
            egraph,
            stash,
            version,
            child,
            outer_variable,
            ceiling,
            visited,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::diagnostic::{Diagnostic, Span, report};
    use crate::generic_param::{AlphaEquivParam, GenericParam, GenericParamKind};
    use crate::source::SourceFile;
    use crate::span::{AbsoluteSpan, ParseSource};
    use crate::ty::{FloatTy, IntTy, UintTy};

    fn variable<'db>(
        egraph: &mut VersionedEGraph<'db>,
        stash: &mut Stash,
        version: Version,
        universe: Universe,
    ) -> Ptr<Ty<'db>> {
        let index = egraph.alloc_var(version, universe);
        stash.alloc(Ty::InferVar(index))
    }

    fn variable_in_domain<'db>(
        egraph: &mut VersionedEGraph<'db>,
        stash: &mut Stash,
        version: Version,
        universe: Universe,
        domain: InferDomain,
    ) -> Ptr<Ty<'db>> {
        let index = egraph.alloc_var_in_domain(version, universe, domain);
        stash.alloc(Ty::InferVar(index))
    }

    fn variable_index(stash: &Stash, variable: Ptr<Ty<'_>>) -> InferVarIndex {
        match stash[variable] {
            Ty::InferVar(index) => index,
            Ty::Bool
            | Ty::Char
            | Ty::Int(_)
            | Ty::Uint(_)
            | Ty::Float(_)
            | Ty::Str
            | Ty::Adt(_, _)
            | Ty::Alias(_)
            | Ty::Ref(_, _, _)
            | Ty::Tuple(_)
            | Ty::Slice(_)
            | Ty::Array(_, _)
            | Ty::FnPtr(_, _)
            | Ty::Param(_)
            | Ty::Never
            | Ty::Error(_) => panic!("expected inference variable"),
        }
    }

    #[test]
    fn variable_union_intersects_domains_in_both_orders() {
        for reverse in [false, true] {
            let mut egraph = VersionedEGraph::new();
            let mut stash = Stash::new();
            let any = variable(&mut egraph, &mut stash, Version::ROOT, Universe::ROOT);
            let integer = variable_in_domain(
                &mut egraph,
                &mut stash,
                Version::ROOT,
                Universe::ROOT,
                InferDomain::Integer,
            );
            let any_index = variable_index(&stash, any);
            let (left, right) = if reverse {
                (integer, any)
            } else {
                (any, integer)
            };

            let effects = try_unify(&mut egraph, &mut stash, Version::ROOT, left, right).unwrap();

            assert_eq!(
                egraph.effective_domain(Version::ROOT, &stash, any),
                Some(InferDomain::Integer)
            );
            assert_eq!(
                egraph.effective_domain(Version::ROOT, &stash, integer),
                Some(InferDomain::Integer)
            );
            assert!(effects.wakes.contains(&any_index));
        }
    }

    #[test]
    fn incompatible_transitive_domain_union_rolls_back() {
        let mut egraph = VersionedEGraph::new();
        let mut stash = Stash::new();
        let any = variable(&mut egraph, &mut stash, Version::ROOT, Universe::ROOT);
        let integer = variable_in_domain(
            &mut egraph,
            &mut stash,
            Version::ROOT,
            Universe::ROOT,
            InferDomain::Integer,
        );
        let float = variable_in_domain(
            &mut egraph,
            &mut stash,
            Version::ROOT,
            Universe::ROOT,
            InferDomain::Float,
        );
        try_unify(&mut egraph, &mut stash, Version::ROOT, any, integer).unwrap();
        let integer_root = egraph.find(Version::ROOT, any);
        let float_root = egraph.find(Version::ROOT, float);
        let revision = egraph.semantic_revision(Version::ROOT);

        assert!(matches!(
            try_unify(&mut egraph, &mut stash, Version::ROOT, any, float),
            Err(UnifyError::DomainConflict {
                left_domain: Some(InferDomain::Integer),
                right_domain: Some(InferDomain::Float),
                ..
            })
        ));
        assert_eq!(egraph.find(Version::ROOT, any), integer_root);
        assert_eq!(egraph.find(Version::ROOT, float), float_root);
        assert_eq!(
            egraph.effective_domain(Version::ROOT, &stash, any),
            Some(InferDomain::Integer)
        );
        assert_eq!(egraph.semantic_revision(Version::ROOT), revision);
    }

    #[test]
    fn numeric_domains_accept_members_and_reject_other_families() {
        for integer in [Ty::Int(IntTy::I128), Ty::Uint(UintTy::Usize)] {
            let mut egraph = VersionedEGraph::new();
            let mut stash = Stash::new();
            let variable = variable_in_domain(
                &mut egraph,
                &mut stash,
                Version::ROOT,
                Universe::ROOT,
                InferDomain::Integer,
            );
            let value = stash.alloc(integer);
            try_unify(&mut egraph, &mut stash, Version::ROOT, variable, value).unwrap();
            assert_eq!(egraph.find(Version::ROOT, variable), value);
        }

        for float in [FloatTy::F32, FloatTy::F64] {
            let mut egraph = VersionedEGraph::new();
            let mut stash = Stash::new();
            let variable = variable_in_domain(
                &mut egraph,
                &mut stash,
                Version::ROOT,
                Universe::ROOT,
                InferDomain::Float,
            );
            let value = stash.alloc(Ty::Float(float));
            try_unify(&mut egraph, &mut stash, Version::ROOT, variable, value).unwrap();
            assert_eq!(egraph.find(Version::ROOT, variable), value);
        }

        let mut egraph = VersionedEGraph::new();
        let mut stash = Stash::new();
        let integer = variable_in_domain(
            &mut egraph,
            &mut stash,
            Version::ROOT,
            Universe::ROOT,
            InferDomain::Integer,
        );
        let float = stash.alloc(Ty::Float(FloatTy::F32));
        let revision = egraph.semantic_revision(Version::ROOT);
        assert!(matches!(
            try_unify(&mut egraph, &mut stash, Version::ROOT, integer, float),
            Err(UnifyError::DomainConflict { .. })
        ));
        assert_eq!(egraph.find(Version::ROOT, integer), integer);
        assert_eq!(egraph.semantic_revision(Version::ROOT), revision);

        let mut egraph = VersionedEGraph::new();
        let mut stash = Stash::new();
        let float = variable_in_domain(
            &mut egraph,
            &mut stash,
            Version::ROOT,
            Universe::ROOT,
            InferDomain::Float,
        );
        let integer = stash.alloc(Ty::Uint(UintTy::U32));
        let revision = egraph.semantic_revision(Version::ROOT);
        assert!(matches!(
            try_unify(&mut egraph, &mut stash, Version::ROOT, float, integer,),
            Err(UnifyError::DomainConflict { .. })
        ));
        assert_eq!(egraph.find(Version::ROOT, float), float);
        assert_eq!(egraph.semantic_revision(Version::ROOT), revision);
    }

    #[test]
    fn exact_bounds_enforce_domains_transactionally() {
        let mut egraph = VersionedEGraph::new();
        let mut stash = Stash::new();
        let integer = variable_in_domain(
            &mut egraph,
            &mut stash,
            Version::ROOT,
            Universe::ROOT,
            InferDomain::Integer,
        );
        let float = stash.alloc(Ty::Float(FloatTy::F64));
        let revision = egraph.semantic_revision(Version::ROOT);

        assert!(matches!(
            try_set_bound(
                &mut egraph,
                &mut stash,
                Version::ROOT,
                integer,
                Bound::Exactly(float)
            ),
            Err(UnifyError::DomainConflict { .. })
        ));
        assert_eq!(egraph.get_bound(Version::ROOT, integer), Bound::None);
        assert_eq!(
            egraph.effective_domain(Version::ROOT, &stash, integer),
            Some(InferDomain::Integer)
        );
        assert_eq!(egraph.semantic_revision(Version::ROOT), revision);
    }

    #[test]
    fn exact_bounds_join_the_validated_equality_class() {
        let mut egraph = VersionedEGraph::new();
        let mut stash = Stash::new();
        let any = variable(&mut egraph, &mut stash, Version::ROOT, Universe::ROOT);
        let integer = variable_in_domain(
            &mut egraph,
            &mut stash,
            Version::ROOT,
            Universe::ROOT,
            InferDomain::Integer,
        );

        try_set_bound(
            &mut egraph,
            &mut stash,
            Version::ROOT,
            any,
            Bound::Exactly(integer),
        )
        .unwrap();
        assert_eq!(egraph.find(Version::ROOT, any), integer);
        assert_eq!(
            egraph.effective_domain(Version::ROOT, &stash, any),
            Some(InferDomain::Integer)
        );

        let float = stash.alloc(Ty::Float(FloatTy::F32));
        let revision = egraph.semantic_revision(Version::ROOT);
        assert!(matches!(
            try_set_bound(
                &mut egraph,
                &mut stash,
                Version::ROOT,
                any,
                Bound::Exactly(float),
            ),
            Err(UnifyError::DomainConflict { .. })
        ));
        assert_eq!(egraph.find(Version::ROOT, any), integer);
        assert_eq!(egraph.semantic_revision(Version::ROOT), revision);

        let mut egraph = VersionedEGraph::new();
        let mut stash = Stash::new();
        let any = variable(&mut egraph, &mut stash, Version::ROOT, Universe::ROOT);
        let float = stash.alloc(Ty::Float(FloatTy::F32));
        try_set_bound(
            &mut egraph,
            &mut stash,
            Version::ROOT,
            any,
            Bound::Exactly(float),
        )
        .unwrap();
        let integer = variable_in_domain(
            &mut egraph,
            &mut stash,
            Version::ROOT,
            Universe::ROOT,
            InferDomain::Integer,
        );
        assert!(matches!(
            try_unify(&mut egraph, &mut stash, Version::ROOT, integer, any,),
            Err(UnifyError::DomainConflict { .. })
        ));
    }

    #[test]
    fn reported_error_recovery_is_admitted_by_numeric_domains() {
        let db = Database::default();
        let source = SourceFile::new(&db, "domain-recovery.rs".to_owned(), String::new());
        let mut diagnostics = Vec::new();
        let reported = report(
            &mut diagnostics,
            Diagnostic::error(
                Span::Absolute(AbsoluteSpan {
                    source: ParseSource::SourceFile(source),
                    start: 0,
                    end: 0,
                }),
                "test recovery",
            ),
        );
        assert_eq!(diagnostics.len(), 1);

        let mut egraph = VersionedEGraph::new();
        let mut stash = Stash::new();
        let error_ty = stash.alloc(Ty::Error(reported));
        let error_alias = variable(&mut egraph, &mut stash, Version::ROOT, Universe::ROOT);
        install_error_recovery(
            &mut egraph,
            &mut stash,
            Version::ROOT,
            error_alias,
            error_ty,
        )
        .unwrap();

        for domain in [InferDomain::Integer, InferDomain::Float] {
            let restricted = variable_in_domain(
                &mut egraph,
                &mut stash,
                Version::ROOT,
                Universe::ROOT,
                domain,
            );
            install_error_recovery(
                &mut egraph,
                &mut stash,
                Version::ROOT,
                restricted,
                error_alias,
            )
            .unwrap();
            assert_eq!(egraph.find(Version::ROOT, restricted), error_ty);
        }
    }

    #[test]
    fn domain_narrowing_follows_child_commit_and_discard() {
        let mut egraph = VersionedEGraph::new();
        let mut stash = Stash::new();
        let any = variable(&mut egraph, &mut stash, Version::ROOT, Universe::ROOT);
        let integer = variable_in_domain(
            &mut egraph,
            &mut stash,
            Version::ROOT,
            Universe::ROOT,
            InferDomain::Integer,
        );
        let any_index = variable_index(&stash, any);

        let discarded = egraph.branch_from(Version::ROOT);
        unify_in_probe(&mut egraph, &mut stash, discarded, any, integer).unwrap();
        assert_eq!(
            egraph.effective_domain(discarded, &stash, any),
            Some(InferDomain::Integer)
        );
        assert_eq!(
            egraph.effective_domain(Version::ROOT, &stash, any),
            Some(InferDomain::Any)
        );
        egraph.discard(discarded);
        assert_eq!(
            egraph.effective_domain(Version::ROOT, &stash, any),
            Some(InferDomain::Any)
        );

        let committed = egraph.branch_from(Version::ROOT);
        unify_in_probe(&mut egraph, &mut stash, committed, integer, any).unwrap();
        let effects = egraph.collapse_into(committed, Version::ROOT);
        assert_eq!(
            egraph.effective_domain(Version::ROOT, &stash, any),
            Some(InferDomain::Integer)
        );
        assert!(effects.wakes.contains(&any_index));
    }

    #[test]
    fn concrete_and_nested_unification() {
        let mut egraph = VersionedEGraph::new();
        let mut stash = Stash::new();
        let variable = variable(&mut egraph, &mut stash, Version::ROOT, Universe::ROOT);
        let int = stash.alloc(Ty::Int(IntTy::I32));
        let left_elements = stash.alloc_slice(&[variable]);
        let left = stash.alloc(Ty::Tuple(left_elements));
        let right_elements = stash.alloc_slice(&[int]);
        let right = stash.alloc(Ty::Tuple(right_elements));

        try_unify(&mut egraph, &mut stash, Version::ROOT, left, right).unwrap();
        assert_eq!(egraph.find(Version::ROOT, variable), int);
        assert_eq!(
            egraph.find(Version::ROOT, left),
            egraph.find(Version::ROOT, right)
        );
    }

    #[test]
    fn late_mismatch_rolls_back_earlier_equality() {
        let mut egraph = VersionedEGraph::new();
        let mut stash = Stash::new();
        let variable = variable(&mut egraph, &mut stash, Version::ROOT, Universe::ROOT);
        let int = stash.alloc(Ty::Int(IntTy::I32));
        let boolean = stash.alloc(Ty::Bool);
        let left_elements = stash.alloc_slice(&[variable, boolean]);
        let left = stash.alloc(Ty::Tuple(left_elements));
        let right_elements = stash.alloc_slice(&[int, int]);
        let right = stash.alloc(Ty::Tuple(right_elements));
        let revision = egraph.semantic_revision(Version::ROOT);

        assert!(matches!(
            try_unify(&mut egraph, &mut stash, Version::ROOT, left, right),
            Err(UnifyError::Mismatch { .. })
        ));
        assert_eq!(egraph.find(Version::ROOT, variable), variable);
        assert_eq!(egraph.semantic_revision(Version::ROOT), revision);
    }

    #[test]
    fn batch_failure_is_atomic() {
        let mut egraph = VersionedEGraph::new();
        let mut stash = Stash::new();
        let variable = variable(&mut egraph, &mut stash, Version::ROOT, Universe::ROOT);
        let int = stash.alloc(Ty::Int(IntTy::I32));
        let boolean = stash.alloc(Ty::Bool);

        assert!(
            try_unify_batch(
                &mut egraph,
                &mut stash,
                Version::ROOT,
                &[(variable, int), (int, boolean)]
            )
            .is_err()
        );
        assert_eq!(egraph.find(Version::ROOT, variable), variable);
    }

    #[test]
    fn direct_occurs_check_is_atomic() {
        let mut egraph = VersionedEGraph::new();
        let mut stash = Stash::new();
        let variable = variable(&mut egraph, &mut stash, Version::ROOT, Universe::ROOT);
        let elements = stash.alloc_slice(&[variable]);
        let tuple = stash.alloc(Ty::Tuple(elements));

        assert!(matches!(
            try_unify(&mut egraph, &mut stash, Version::ROOT, variable, tuple),
            Err(UnifyError::OccursCheck { .. })
        ));
        assert_eq!(egraph.find(Version::ROOT, variable), variable);
    }

    #[test]
    fn nested_variable_ceiling_is_lowered_on_commit() {
        let mut egraph = VersionedEGraph::new();
        let mut stash = Stash::new();
        let outer = variable(&mut egraph, &mut stash, Version::ROOT, Universe::ROOT);
        let inner = variable(&mut egraph, &mut stash, Version::ROOT, Universe(1));
        let inner_index = match stash[inner] {
            Ty::InferVar(index) => index,
            _ => unreachable!(),
        };
        let elements = stash.alloc_slice(&[inner]);
        let tuple = stash.alloc(Ty::Tuple(elements));

        let effects = try_unify(&mut egraph, &mut stash, Version::ROOT, outer, tuple).unwrap();
        assert_eq!(
            egraph.current_universe(Version::ROOT, inner_index),
            Universe::ROOT
        );
        assert!(effects.wakes.contains(&inner_index));
    }

    #[test]
    fn unequal_concrete_leaves_do_not_change_revision() {
        let mut egraph = VersionedEGraph::new();
        let mut stash = Stash::new();
        let int = stash.alloc(Ty::Int(IntTy::I32));
        let boolean = stash.alloc(Ty::Bool);
        let revision = egraph.semantic_revision(Version::ROOT);

        assert!(matches!(
            try_unify(&mut egraph, &mut stash, Version::ROOT, int, boolean),
            Err(UnifyError::Mismatch { .. })
        ));
        assert_eq!(egraph.semantic_revision(Version::ROOT), revision);
    }

    #[test]
    fn inference_variables_coalesce() {
        let mut egraph = VersionedEGraph::new();
        let mut stash = Stash::new();
        let left = variable(&mut egraph, &mut stash, Version::ROOT, Universe(2));
        let right = variable(&mut egraph, &mut stash, Version::ROOT, Universe(1));

        try_unify(&mut egraph, &mut stash, Version::ROOT, left, right).unwrap();
        assert_eq!(
            egraph.find(Version::ROOT, left),
            egraph.find(Version::ROOT, right)
        );
    }

    #[test]
    fn indirect_occurs_check_rolls_back() {
        let mut egraph = VersionedEGraph::new();
        let mut stash = Stash::new();
        let left = variable(&mut egraph, &mut stash, Version::ROOT, Universe::ROOT);
        let right = variable(&mut egraph, &mut stash, Version::ROOT, Universe::ROOT);
        try_unify(&mut egraph, &mut stash, Version::ROOT, left, right).unwrap();
        let root_before = egraph.find(Version::ROOT, left);
        let elements = stash.alloc_slice(&[left]);
        let tuple = stash.alloc(Ty::Tuple(elements));

        assert!(matches!(
            try_unify(&mut egraph, &mut stash, Version::ROOT, right, tuple),
            Err(UnifyError::OccursCheck { .. })
        ));
        assert_eq!(egraph.find(Version::ROOT, left), root_before);
    }

    #[test]
    fn inaccessible_placeholder_and_bound_are_rejected_atomically() {
        let db = Database::default();
        let alpha = AlphaEquivParam::new(&db, GenericParamKind::Type, 0);
        let param = GenericParam::AlphaEquiv(alpha);
        let mut egraph = VersionedEGraph::new();
        egraph.register_placeholder(param, Universe(1));
        let mut stash = Stash::new();
        let variable = variable(&mut egraph, &mut stash, Version::ROOT, Universe::ROOT);
        let placeholder = stash.alloc(Ty::Param(param));
        let revision = egraph.semantic_revision(Version::ROOT);

        assert!(matches!(
            try_unify(
                &mut egraph,
                &mut stash,
                Version::ROOT,
                variable,
                placeholder
            ),
            Err(UnifyError::UniverseLeak { .. })
        ));
        assert_eq!(egraph.find(Version::ROOT, variable), variable);
        assert_eq!(egraph.semantic_revision(Version::ROOT), revision);

        assert!(matches!(
            try_set_bound(
                &mut egraph,
                &mut stash,
                Version::ROOT,
                variable,
                Bound::AtLeast(placeholder)
            ),
            Err(UnifyError::UniverseLeak { .. })
        ));
        assert_eq!(egraph.get_bound(Version::ROOT, variable), Bound::None);
    }

    #[test]
    fn discarded_ceiling_lowering_has_no_effects() {
        let mut egraph = VersionedEGraph::new();
        let mut stash = Stash::new();
        let outer = variable(&mut egraph, &mut stash, Version::ROOT, Universe::ROOT);
        let inner = variable(&mut egraph, &mut stash, Version::ROOT, Universe(1));
        let inner_index = match stash[inner] {
            Ty::InferVar(index) => index,
            _ => unreachable!(),
        };
        let boolean = stash.alloc(Ty::Bool);
        let int = stash.alloc(Ty::Int(IntTy::I32));
        let elements = stash.alloc_slice(&[inner, boolean]);
        let tuple = stash.alloc(Ty::Tuple(elements));
        let other_elements = stash.alloc_slice(&[outer, int]);
        let other = stash.alloc(Ty::Tuple(other_elements));
        let revision = egraph.semantic_revision(Version::ROOT);

        assert!(try_unify(&mut egraph, &mut stash, Version::ROOT, tuple, other).is_err());
        assert_eq!(
            egraph.current_universe(Version::ROOT, inner_index),
            Universe(1)
        );
        assert_eq!(egraph.semantic_revision(Version::ROOT), revision);
    }

    #[test]
    fn dummy_lifetimes_unify_and_const_leaves_compare_structurally() {
        let mut egraph = VersionedEGraph::new();
        let mut stash = Stash::new();
        let int = stash.alloc(Ty::Int(IntTy::I32));
        let left = stash.alloc(Ty::Ref(
            int,
            crate::cst::Mutability::Shared,
            Lifetime::Dummy,
        ));
        let right = stash.alloc(Ty::Ref(
            int,
            crate::cst::Mutability::Shared,
            Lifetime::Dummy,
        ));

        assert!(try_unify(&mut egraph, &mut stash, Version::ROOT, left, right).is_ok());

        let array_left = stash.alloc(Ty::Array(int, Const::Literal(1)));
        let array_right = stash.alloc(Ty::Array(int, Const::Literal(2)));
        assert!(matches!(
            try_unify(
                &mut egraph,
                &mut stash,
                Version::ROOT,
                array_left,
                array_right
            ),
            Err(UnifyError::Mismatch { .. })
        ));
    }
}
