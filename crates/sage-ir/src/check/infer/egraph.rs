use rustc_hash::{FxHashMap, FxHashSet};
use sage_stash::{Ptr, Stash};

use crate::generic_param::GenericParam;
use crate::ty::{InferVarIndex, Ty};

use super::bound::Bound;
use super::version::{Universe, VarInfo, Version, VersionTree};

/// Per-version mutable inference state (a sparse diff from its ancestry).
#[derive(Debug, Default)]
struct VersionState<'db> {
    parents: FxHashMap<Ptr<Ty<'db>>, Ptr<Ty<'db>>>,
    bounds: FxHashMap<Ptr<Ty<'db>>, Bound<'db>>,
    universe_ceilings: FxHashMap<InferVarIndex, Universe>,
    dependents: FxHashMap<Ptr<Ty<'db>>, FxHashSet<Ptr<Ty<'db>>>>,
    worklist: Vec<Ptr<Ty<'db>>>,
    wakes: FxHashSet<InferVarIndex>,
    semantic_revision: u64,
}

/// Effects published by atomically collapsing a child into its parent.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CommitEffects {
    pub wakes: Vec<InferVarIndex>,
    pub semantic_revision: u64,
}

/// Versioned union-find and bounds over a stash of types.
///
/// Every operation names its version explicitly. A version with live children
/// is a frozen snapshot: reads are permitted, while all mutation is rejected.
pub struct VersionedEGraph<'db> {
    versions: VersionTree,
    states: Vec<VersionState<'db>>,
    placeholder_universes: FxHashMap<GenericParam<'db>, Universe>,
}

impl<'db> VersionedEGraph<'db> {
    pub fn new() -> Self {
        Self {
            versions: VersionTree::new(),
            states: vec![VersionState::default()],
            placeholder_universes: FxHashMap::default(),
        }
    }

    pub fn root_version(&self) -> Version {
        self.versions.root()
    }

    pub fn version_tree(&self) -> &VersionTree {
        &self.versions
    }

    pub fn semantic_revision(&self, version: Version) -> u64 {
        self.versions.assert_live(version);
        self.states[version.0 as usize].semantic_revision
    }

    // -----------------------------------------------------------------------
    // Variables and universes
    // -----------------------------------------------------------------------

    pub fn alloc_var(&mut self, version: Version, universe: Universe) -> InferVarIndex {
        let index = self.versions.alloc_var(version, universe);
        self.states[version.0 as usize]
            .universe_ceilings
            .insert(index, universe);
        index
    }

    pub fn get_var_info(&self, version: Version, index: InferVarIndex) -> &VarInfo {
        self.versions.get_variable(version, index)
    }

    pub fn current_universe(&self, version: Version, index: InferVarIndex) -> Universe {
        let info = self.versions.get_variable(version, index);
        for ancestor in self.versions.ancestors(version) {
            if let Some(universe) = self.states[ancestor.0 as usize]
                .universe_ceilings
                .get(&index)
            {
                return *universe;
            }
        }
        info.creation_universe
    }

    pub fn lower_universe(
        &mut self,
        version: Version,
        index: InferVarIndex,
        ceiling: Universe,
    ) -> bool {
        self.assert_mutable(version);
        self.versions.get_variable(version, index);
        let old = self.current_universe(version, index);
        if ceiling >= old {
            return false;
        }
        self.states[version.0 as usize]
            .universe_ceilings
            .insert(index, ceiling);
        self.mark_semantic_change(version, Some(index));
        true
    }

    pub fn register_placeholder(&mut self, param: GenericParam<'db>, universe: Universe) {
        match self.placeholder_universes.insert(param, universe) {
            Some(previous) => assert_eq!(
                previous, universe,
                "placeholder registered in two different universes"
            ),
            None => {}
        }
    }

    pub fn placeholder_universe(&self, param: GenericParam<'db>) -> Universe {
        self.placeholder_universes
            .get(&param)
            .copied()
            .unwrap_or(Universe::ROOT)
    }

    // -----------------------------------------------------------------------
    // Version lifecycle
    // -----------------------------------------------------------------------

    pub fn branch_from(&mut self, parent: Version) -> Version {
        let revision = self.semantic_revision(parent);
        let child = self.versions.branch_from(parent);
        assert_eq!(child.0 as usize, self.states.len());
        self.states.push(VersionState {
            semantic_revision: revision,
            ..VersionState::default()
        });
        child
    }

    /// Atomically merge an exclusive leaf child into its direct parent.
    pub fn collapse_into(&mut self, child: Version, parent: Version) -> CommitEffects {
        self.versions.assert_live(child);
        self.versions.assert_live(parent);
        assert_eq!(self.versions.parent(child), Some(parent));
        assert!(
            self.versions.is_leaf(child),
            "cannot collapse a non-leaf child"
        );
        assert_eq!(
            self.versions.children(parent),
            &[child],
            "a child may collapse only when it is its parent's sole live child"
        );

        let child_state = std::mem::take(&mut self.states[child.0 as usize]);
        let revision = child_state.semantic_revision;
        let mut wakes: Vec<_> = child_state.wakes.iter().copied().collect();
        wakes.sort_unstable();

        let parent_state = &mut self.states[parent.0 as usize];
        parent_state.parents.extend(child_state.parents);
        parent_state.bounds.extend(child_state.bounds);
        parent_state
            .universe_ceilings
            .extend(child_state.universe_ceilings);
        for (ty, dependents) in child_state.dependents {
            parent_state
                .dependents
                .entry(ty)
                .or_default()
                .extend(dependents);
        }
        parent_state.worklist.extend(child_state.worklist);
        parent_state.wakes.extend(child_state.wakes);
        parent_state.semantic_revision = revision;

        self.versions.reown_variables(child, parent);
        let removed = self.versions.remove(child);
        debug_assert_eq!(removed, vec![child]);

        CommitEffects {
            wakes,
            semantic_revision: revision,
        }
    }

    pub fn discard(&mut self, version: Version) {
        let removed = self.versions.remove(version);
        for removed_version in removed {
            self.states[removed_version.0 as usize] = VersionState::default();
        }
    }

    fn assert_mutable(&self, version: Version) {
        self.versions.assert_mutable(version);
    }

    // -----------------------------------------------------------------------
    // Union-find
    // -----------------------------------------------------------------------

    pub fn find(&self, version: Version, mut ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        self.versions.assert_live(version);
        loop {
            let parent = self.get_parent(version, ty);
            if parent == ty {
                return ty;
            }
            ty = parent;
        }
    }

    pub fn find_mut(&mut self, version: Version, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        self.assert_mutable(version);
        let root = self.find(version, ty);
        self.compress_path(version, ty, root);
        root
    }

    fn compress_path(&mut self, version: Version, mut ty: Ptr<Ty<'db>>, root: Ptr<Ty<'db>>) {
        loop {
            let parent = self.get_parent(version, ty);
            if parent == root || parent == ty {
                break;
            }
            self.set_parent(version, ty, root);
            ty = parent;
        }
    }

    fn get_parent(&self, version: Version, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        for ancestor in self.versions.ancestors(version) {
            if let Some(parent) = self.states[ancestor.0 as usize].parents.get(&ty) {
                return *parent;
            }
        }
        ty
    }

    fn set_parent(&mut self, version: Version, ty: Ptr<Ty<'db>>, parent: Ptr<Ty<'db>>) {
        self.assert_mutable(version);
        self.states[version.0 as usize].parents.insert(ty, parent);
    }

    /// Union two classes. Concrete/structured representatives are preferred.
    pub fn union(
        &mut self,
        version: Version,
        stash: &Stash,
        left: Ptr<Ty<'db>>,
        right: Ptr<Ty<'db>>,
    ) -> Ptr<Ty<'db>> {
        self.assert_mutable(version);
        let left_root = self.find(version, left);
        let right_root = self.find(version, right);
        if left_root == right_root {
            return left_root;
        }

        let (child, parent) = match (stash[left_root], stash[right_root]) {
            (Ty::InferVar(_), Ty::InferVar(_)) | (Ty::InferVar(_), _) => (left_root, right_root),
            (
                Ty::Bool
                | Ty::Char
                | Ty::Int(_)
                | Ty::Uint(_)
                | Ty::Float(_)
                | Ty::Str
                | Ty::Adt(_, _)
                | Ty::Ref(_, _, _)
                | Ty::Tuple(_)
                | Ty::Slice(_)
                | Ty::Array(_, _)
                | Ty::FnPtr(_, _)
                | Ty::Param(_)
                | Ty::Never
                | Ty::Error(_),
                _,
            ) => (right_root, left_root),
        };

        self.set_parent(version, child, parent);
        self.states[version.0 as usize].worklist.push(child);

        let changed_var = match stash[child] {
            Ty::InferVar(index) => Some(index),
            Ty::Bool
            | Ty::Char
            | Ty::Int(_)
            | Ty::Uint(_)
            | Ty::Float(_)
            | Ty::Str
            | Ty::Adt(_, _)
            | Ty::Ref(_, _, _)
            | Ty::Tuple(_)
            | Ty::Slice(_)
            | Ty::Array(_, _)
            | Ty::FnPtr(_, _)
            | Ty::Param(_)
            | Ty::Never
            | Ty::Error(_) => None,
        };
        self.mark_semantic_change(version, changed_var);
        parent
    }

    // -----------------------------------------------------------------------
    // Bounds
    // -----------------------------------------------------------------------

    pub fn get_bound(&self, version: Version, ty: Ptr<Ty<'db>>) -> Bound<'db> {
        self.versions.assert_live(version);
        for ancestor in self.versions.ancestors(version) {
            if let Some(bound) = self.states[ancestor.0 as usize].bounds.get(&ty) {
                return *bound;
            }
        }
        Bound::None
    }

    /// Set a bound inside an already-transactional leaf version.
    pub(crate) fn set_bound_in(
        &mut self,
        version: Version,
        stash: &Stash,
        ty: Ptr<Ty<'db>>,
        bound: Bound<'db>,
    ) -> bool {
        self.assert_mutable(version);
        let index = match stash[ty] {
            Ty::InferVar(index) => index,
            other => panic!("set_bound_in called on non-inference type {other:?}"),
        };
        self.versions.get_variable(version, index);
        if self.get_bound(version, ty) == bound {
            return false;
        }
        self.states[version.0 as usize].bounds.insert(ty, bound);
        self.mark_semantic_change(version, Some(index));
        true
    }

    // -----------------------------------------------------------------------
    // Congruence closure
    // -----------------------------------------------------------------------

    pub fn add_dependent(
        &mut self,
        version: Version,
        arg_ty: Ptr<Ty<'db>>,
        parent_ty: Ptr<Ty<'db>>,
    ) {
        self.assert_mutable(version);
        self.states[version.0 as usize]
            .dependents
            .entry(arg_ty)
            .or_default()
            .insert(parent_ty);
    }

    pub fn rebuild(&mut self, version: Version, stash: &mut Stash) -> Vec<Ptr<Ty<'db>>> {
        self.assert_mutable(version);
        let mut changed = Vec::new();
        loop {
            let worklist = std::mem::take(&mut self.states[version.0 as usize].worklist);
            if worklist.is_empty() {
                break;
            }
            for merged in worklist {
                for dependent in self.collect_dependents(version, merged) {
                    let old_canonical = self.find(version, dependent);
                    if let Some(new_ty) = self.recanonicalize(version, stash, dependent) {
                        let new_canonical = self.find(version, new_ty);
                        if new_canonical != old_canonical {
                            self.set_parent(version, dependent, new_canonical);
                            self.states[version.0 as usize].worklist.push(dependent);
                            changed.push(dependent);
                        }
                    }
                }
            }
        }
        changed
    }

    fn collect_dependents(&self, version: Version, ty: Ptr<Ty<'db>>) -> Vec<Ptr<Ty<'db>>> {
        let mut dependents = FxHashSet::default();
        for ancestor in self.versions.ancestors(version) {
            if let Some(found) = self.states[ancestor.0 as usize].dependents.get(&ty) {
                dependents.extend(found.iter().copied());
            }
        }
        dependents.into_iter().collect()
    }

    fn recanonicalize(
        &self,
        version: Version,
        stash: &mut Stash,
        ty: Ptr<Ty<'db>>,
    ) -> Option<Ptr<Ty<'db>>> {
        use super::skeleton::{Children, decompose, recompose};

        let decomposed = decompose(stash, ty);
        if decomposed.children.is_empty() {
            return None;
        }

        let new_children: Children<'db> = decomposed
            .children
            .iter()
            .map(|child| self.find(version, *child))
            .collect();
        if new_children == decomposed.children {
            return None;
        }

        Some(recompose(stash, decomposed.skeleton, &new_children))
    }

    fn mark_semantic_change(&mut self, version: Version, wake: Option<InferVarIndex>) {
        let state = &mut self.states[version.0 as usize];
        state.semantic_revision = state.semantic_revision.wrapping_add(1);
        if let Some(index) = wake {
            state.wakes.insert(index);
        }
    }
}

impl Default for VersionedEGraph<'_> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_is_frozen_while_children_are_live() {
        let mut egraph = VersionedEGraph::new();
        let child = egraph.branch_from(Version::ROOT);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            egraph.alloc_var(Version::ROOT, Universe::ROOT)
        }));
        assert!(result.is_err());

        egraph.discard(child);
        egraph.alloc_var(Version::ROOT, Universe::ROOT);
    }

    #[test]
    fn collapse_requires_an_exclusive_child() {
        let mut egraph = VersionedEGraph::new();
        let left = egraph.branch_from(Version::ROOT);
        let right = egraph.branch_from(Version::ROOT);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            egraph.collapse_into(left, Version::ROOT)
        }));
        assert!(result.is_err());

        egraph.discard(right);
        egraph.collapse_into(left, Version::ROOT);
    }

    #[test]
    fn committed_child_variable_is_reowned_by_parent() {
        let mut egraph = VersionedEGraph::new();
        let child = egraph.branch_from(Version::ROOT);
        let variable = egraph.alloc_var(child, Universe(2));
        egraph.collapse_into(child, Version::ROOT);

        let info = egraph.get_var_info(Version::ROOT, variable);
        assert_eq!(info.owner, Version::ROOT);
        assert_eq!(info.creation_universe, Universe(2));
    }
}
