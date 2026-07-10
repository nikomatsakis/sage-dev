use rustc_hash::FxHashSet;
use sage_stash::{AllocStashData, Ptr, Slice, Stash};

use crate::generic_param::{AlphaEquivParam, GenericParamKind};
use crate::ty::Ty;

use super::goal::{CanonicalVarInfo, CanonicalVarRole, Goal};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct SubstEntry<'db> {
    pub key: AlphaEquivParam<'db>,
    pub value: Ptr<Ty<'db>>,
}

pub type Subst<'db> = Slice<SubstEntry<'db>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct ResponseVarInfo<'db> {
    pub param: AlphaEquivParam<'db>,
    pub kind: GenericParamKind,
    pub relative_universe: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct QueryResult<'db> {
    pub bound_vars: Slice<ResponseVarInfo<'db>>,
    pub value: QueryResultData<'db>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum QueryResultData<'db> {
    Yes {
        subst: Subst<'db>,
        modulo: Goal<'db>,
    },
    Maybe {
        hints: Subst<'db>,
    },
    No,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum GoalResult<'db> {
    Yes { modulo: Goal<'db> },
    Maybe,
    No,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SubstitutionError<'db> {
    UnknownKey(AlphaEquivParam<'db>),
    RigidKey(AlphaEquivParam<'db>),
    NonTypeKey(AlphaEquivParam<'db>),
    DuplicateKey(AlphaEquivParam<'db>),
}

/// Validate the public substitution invariant before allocating it.
pub fn validate_and_alloc_subst<'db>(
    stash: &mut Stash,
    canonical_vars: &[CanonicalVarInfo<'db>],
    entries: impl IntoIterator<Item = (AlphaEquivParam<'db>, Ptr<Ty<'db>>)>,
) -> Result<Subst<'db>, SubstitutionError<'db>> {
    let mut seen = FxHashSet::default();
    let mut validated = Vec::new();
    for (key, value) in entries {
        let Some((position, info)) = canonical_vars
            .iter()
            .enumerate()
            .find(|(_, info)| info.param == key)
        else {
            return Err(SubstitutionError::UnknownKey(key));
        };
        if info.role != CanonicalVarRole::ExistentialInput {
            return Err(SubstitutionError::RigidKey(key));
        }
        if info.kind != GenericParamKind::Type {
            return Err(SubstitutionError::NonTypeKey(key));
        }
        if !seen.insert(key) {
            return Err(SubstitutionError::DuplicateKey(key));
        }
        validated.push((position, key, value));
    }
    validated.sort_by_key(|(position, _, _)| *position);
    let entries: Vec<_> = validated
        .into_iter()
        .map(|(_, key, value)| SubstEntry { key, value })
        .collect();
    Ok(stash.alloc_slice(&entries))
}
