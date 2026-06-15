use rustc_hash::{FxHashMap, FxHashSet};

use crate::ty::{InferVarIndex, Ty};
use sage_stash::{Ptr, Stash};

use super::bound::Bound;
use super::version::{VarInfo, Version, VersionTree};

/// Per-version mutable inference state (sparse diff from ancestry).
#[derive(Debug, Default)]
struct VersionState<'db> {
    parents: FxHashMap<Ptr<Ty<'db>>, Ptr<Ty<'db>>>,
    bounds: FxHashMap<Ptr<Ty<'db>>, Bound<'db>>,
    dependents: FxHashMap<Ptr<Ty<'db>>, FxHashSet<Ptr<Ty<'db>>>>,
    worklist: Vec<Ptr<Ty<'db>>>,
}

/// The versioned egraph: union-find + bounds over a stash of types.
pub struct VersionedEGraph<'db> {
    versions: VersionTree,
    states: Vec<VersionState<'db>>,
    current: Version,
}

impl<'db> VersionedEGraph<'db> {
    pub fn new() -> Self {
        let versions = VersionTree::new();
        let states = vec![VersionState::default()];
        Self {
            versions,
            states,
            current: Version::ROOT,
        }
    }

    pub fn current_version(&self) -> Version {
        self.current
    }

    pub fn set_current_version(&mut self, v: Version) {
        self.current = v;
    }

    pub fn version_tree(&self) -> &VersionTree {
        &self.versions
    }

    // -----------------------------------------------------------------------
    // Variable allocation
    // -----------------------------------------------------------------------

    pub fn alloc_var(&mut self, info: VarInfo) -> InferVarIndex {
        self.versions.alloc_var(self.current, info)
    }

    pub fn get_var_info(&self, idx: InferVarIndex) -> &VarInfo {
        self.versions.get_variable(self.current, idx)
    }

    // -----------------------------------------------------------------------
    // Version management
    // -----------------------------------------------------------------------

    pub fn branch(&mut self) -> Version {
        let child = self.versions.branch(self.current);
        while self.states.len() <= child.0 as usize {
            self.states.push(VersionState::default());
        }
        child
    }

    pub fn discard(&mut self, v: Version) {
        assert_ne!(v, self.current, "cannot discard the active version");
        let removed = self.versions.remove(v);
        for r in removed {
            self.states[r.0 as usize] = VersionState::default();
        }
    }

    // -----------------------------------------------------------------------
    // Union-find: find
    // -----------------------------------------------------------------------

    pub fn find(&self, mut ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        loop {
            let parent = self.get_parent(ty);
            if parent == ty {
                return ty;
            }
            ty = parent;
        }
    }

    pub fn find_mut(&mut self, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        let root = self.find(ty);
        self.compress_path(ty, root);
        root
    }

    fn compress_path(&mut self, mut ty: Ptr<Ty<'db>>, root: Ptr<Ty<'db>>) {
        let mut skip = false;
        loop {
            let parent = self.get_parent(ty);
            if parent == root {
                break;
            }
            if !skip {
                self.set_parent(ty, root);
            }
            skip = !skip;
            ty = parent;
        }
    }

    fn get_parent(&self, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        for v in self.versions.ancestors(self.current) {
            if let Some(&parent) = self.states[v.0 as usize].parents.get(&ty) {
                return parent;
            }
        }
        ty
    }

    fn set_parent(&mut self, ty: Ptr<Ty<'db>>, parent: Ptr<Ty<'db>>) {
        self.states[self.current.0 as usize]
            .parents
            .insert(ty, parent);
    }

    // -----------------------------------------------------------------------
    // Union-find: union
    // -----------------------------------------------------------------------

    /// Union two types. Prefers non-InferVar as the representative.
    pub fn union(&mut self, stash: &Stash, a: Ptr<Ty<'db>>, b: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        let a_root = self.find(a);
        let b_root = self.find(b);
        if a_root == b_root {
            return a_root;
        }

        let a_data = stash[a_root];
        let b_data = stash[b_root];
        let (child, parent) = match (a_data, b_data) {
            (Ty::InferVar(_), _) => (a_root, b_root),
            _ => (b_root, a_root),
        };

        self.set_parent(child, parent);
        self.states[self.current.0 as usize].worklist.push(child);

        parent
    }

    // -----------------------------------------------------------------------
    // Bounds
    // -----------------------------------------------------------------------

    pub fn get_bound(&self, ty: Ptr<Ty<'db>>) -> Bound<'db> {
        for v in self.versions.ancestors(self.current) {
            if let Some(bound) = self.states[v.0 as usize].bounds.get(&ty) {
                return *bound;
            }
        }
        Bound::None
    }

    pub fn set_bound(&mut self, stash: &Stash, ty: Ptr<Ty<'db>>, bound: Bound<'db>) {
        debug_assert!(
            matches!(stash[ty], Ty::InferVar(_)),
            "set_bound called on non-InferVar type: {:?}",
            stash[ty]
        );
        self.states[self.current.0 as usize]
            .bounds
            .insert(ty, bound);
    }

    // -----------------------------------------------------------------------
    // Congruence closure
    // -----------------------------------------------------------------------

    pub fn add_dependent(&mut self, arg_ty: Ptr<Ty<'db>>, parent_ty: Ptr<Ty<'db>>) {
        self.states[self.current.0 as usize]
            .dependents
            .entry(arg_ty)
            .or_default()
            .insert(parent_ty);
    }

    pub fn rebuild(&mut self, stash: &mut Stash) -> Vec<Ptr<Ty<'db>>> {
        let mut changed = Vec::new();
        loop {
            let worklist = std::mem::take(&mut self.states[self.current.0 as usize].worklist);
            if worklist.is_empty() {
                break;
            }
            for merged in worklist {
                let deps = self.collect_dependents(merged);
                for dep in deps {
                    let old_canon = self.find(dep);
                    if let Some(new_ty) = self.recanon(stash, dep) {
                        if new_ty != old_canon {
                            self.set_parent(dep, new_ty);
                            self.states[self.current.0 as usize].worklist.push(dep);
                            changed.push(dep);
                        }
                    }
                }
            }
        }
        changed
    }

    fn collect_dependents(&self, ty: Ptr<Ty<'db>>) -> Vec<Ptr<Ty<'db>>> {
        let mut deps = Vec::new();
        for v in self.versions.ancestors(self.current) {
            if let Some(set) = self.states[v.0 as usize].dependents.get(&ty) {
                deps.extend(set.iter().copied());
            }
        }
        deps
    }

    fn recanon(&mut self, stash: &mut Stash, ty: Ptr<Ty<'db>>) -> Option<Ptr<Ty<'db>>> {
        use super::skeleton::{Children, decompose, recompose};

        let d = decompose(&stash, ty);
        if d.children.is_empty() {
            return None;
        }

        let new_children: Children<'db> = d.children.iter().map(|c| self.find(*c)).collect();
        if new_children == d.children {
            return None;
        }

        Some(recompose(stash, d.skeleton, &new_children))
    }
}
