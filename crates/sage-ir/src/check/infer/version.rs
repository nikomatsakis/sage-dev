use sage_stash::AllocStashData;

use crate::ty::{InferVarIndex, Ty};

// ANCHOR: inference_domain_lattice
/// Concrete type family permitted for an inference equality class.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, AllocStashData)]
pub enum InferDomain {
    #[default]
    Any,
    Integer,
    Float,
}

impl InferDomain {
    // {anchor}`crates.sage_ir.inference.numeric_domains`
    pub fn intersect(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Any, domain) | (domain, Self::Any) => Some(domain),
            (Self::Integer, Self::Integer) => Some(Self::Integer),
            (Self::Float, Self::Float) => Some(Self::Float),
            (Self::Integer, Self::Float) | (Self::Float, Self::Integer) => None,
        }
    }

    pub fn admits(self, ty: Ty<'_>) -> bool {
        match self {
            Self::Any => true,
            Self::Integer => match ty {
                Ty::Int(_) | Ty::Uint(_) | Ty::Error(_) => true,
                Ty::Bool
                | Ty::Char
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
                | Ty::Never => false,
            },
            Self::Float => match ty {
                Ty::Float(_) | Ty::Error(_) => true,
                Ty::Bool
                | Ty::Char
                | Ty::Int(_)
                | Ty::Uint(_)
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
                | Ty::Never => false,
            },
        }
    }
}
// ANCHOR_END: inference_domain_lattice

/// Stable version-tree node identifier.
///
/// Version identities are append-only for the lifetime of an egraph. A removed
/// version is never reused, which makes stale branch handles reliably invalid.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Version(pub u32);

impl Version {
    pub const ROOT: Version = Version(0);
}

/// Scope depth. A variable at universe `U` cannot name a rigid placeholder
/// from a universe greater than `U`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Universe(pub u32);

impl Universe {
    pub const ROOT: Universe = Universe(0);
}

/// Immutable metadata for one globally unique inference variable.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct VarInfo {
    pub owner: Version,
    pub creation_universe: Universe,
    pub initial_domain: InferDomain,
}

/// A node in the version tree.
#[derive(Debug)]
pub struct VersionNode {
    parent: Option<Version>,
    children: Vec<Version>,
    owned_variables: Vec<InferVarIndex>,
    removed: bool,
}

impl VersionNode {
    fn root() -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            owned_variables: Vec::new(),
            removed: false,
        }
    }

    fn child(parent: Version) -> Self {
        Self {
            parent: Some(parent),
            children: Vec::new(),
            owned_variables: Vec::new(),
            removed: false,
        }
    }
}

/// The append-only version tree and global inference-variable identity table.
#[derive(Debug)]
pub struct VersionTree {
    nodes: Vec<VersionNode>,
    variables: Vec<VarInfo>,
}

impl VersionTree {
    pub fn new() -> Self {
        Self {
            nodes: vec![VersionNode::root()],
            variables: Vec::new(),
        }
    }

    pub fn root(&self) -> Version {
        Version::ROOT
    }

    pub fn is_live(&self, version: Version) -> bool {
        self.nodes
            .get(version.0 as usize)
            .is_some_and(|node| !node.removed)
    }

    pub fn assert_live(&self, version: Version) {
        assert!(
            self.is_live(version),
            "stale or unknown version {version:?}"
        );
    }

    // {anchor}`crates.sage_ir.inference.body_local_quiescence`
    pub fn is_leaf(&self, version: Version) -> bool {
        self.assert_live(version);
        self.nodes[version.0 as usize].children.is_empty()
    }

    pub fn parent(&self, version: Version) -> Option<Version> {
        self.assert_live(version);
        self.nodes[version.0 as usize].parent
    }

    pub fn children(&self, version: Version) -> &[Version] {
        self.assert_live(version);
        &self.nodes[version.0 as usize].children
    }

    pub fn is_ancestor(&self, ancestor: Version, mut version: Version) -> bool {
        self.assert_live(ancestor);
        self.assert_live(version);
        loop {
            if ancestor == version {
                return true;
            }
            let Some(parent) = self.nodes[version.0 as usize].parent else {
                return false;
            };
            version = parent;
        }
    }

    /// Create a child version branching from the specified parent.
    // {anchor}`crates.sage_ir.inference.atomic_checking`
    // {anchor}`crates.sage_ir.inference.transaction_isolation`
    pub fn branch_from(&mut self, parent: Version) -> Version {
        self.assert_live(parent);
        let child = Version(self.nodes.len() as u32);
        self.nodes.push(VersionNode::child(parent));
        self.nodes[parent.0 as usize].children.push(child);
        child
    }

    /// Allocate a globally unique variable owned by a leaf version.
    pub fn alloc_var(&mut self, version: Version, creation_universe: Universe) -> InferVarIndex {
        self.alloc_var_in_domain(version, creation_universe, InferDomain::Any)
    }

    /// Allocate a globally unique variable with an explicit initial domain.
    pub fn alloc_var_in_domain(
        &mut self,
        version: Version,
        creation_universe: Universe,
        initial_domain: InferDomain,
    ) -> InferVarIndex {
        self.assert_mutable(version);
        let index = InferVarIndex(self.variables.len() as u32);
        self.variables.push(VarInfo {
            owner: version,
            creation_universe,
            initial_domain,
        });
        self.nodes[version.0 as usize].owned_variables.push(index);
        index
    }

    /// Return metadata after checking that `version` may see the variable.
    pub fn get_variable(&self, version: Version, index: InferVarIndex) -> &VarInfo {
        self.assert_live(version);
        let info = self
            .variables
            .get(index.0 as usize)
            .unwrap_or_else(|| panic!("unknown inference variable {index:?}"));
        assert!(
            self.is_ancestor(info.owner, version),
            "version {version:?} cannot access variable {index:?} owned by {:?}",
            info.owner
        );
        info
    }

    pub fn visible_variables(&self, version: Version) -> impl Iterator<Item = InferVarIndex> + '_ {
        self.assert_live(version);
        self.variables
            .iter()
            .enumerate()
            .filter_map(move |(i, info)| {
                (self.is_live(info.owner) && self.is_ancestor(info.owner, version))
                    .then_some(InferVarIndex(i as u32))
            })
    }

    pub fn ancestors(&self, version: Version) -> AncestorIter<'_> {
        self.assert_live(version);
        AncestorIter {
            tree: self,
            current: Some(version),
        }
    }

    pub fn assert_mutable(&self, version: Version) {
        self.assert_live(version);
        assert!(
            self.nodes[version.0 as usize].children.is_empty(),
            "cannot mutate frozen non-leaf version {version:?}"
        );
    }

    /// Re-own variables allocated in `child` immediately before a commit.
    pub fn reown_variables(&mut self, child: Version, parent: Version) {
        self.assert_live(child);
        self.assert_live(parent);
        assert_eq!(self.parent(child), Some(parent));

        let variables = std::mem::take(&mut self.nodes[child.0 as usize].owned_variables);
        for index in &variables {
            self.variables[index.0 as usize].owner = parent;
        }
        self.nodes[parent.0 as usize]
            .owned_variables
            .extend(variables);
    }

    /// Remove a version and all descendants. Identities remain tombstoned.
    pub fn remove(&mut self, version: Version) -> Vec<Version> {
        assert_ne!(version, Version::ROOT, "cannot remove the root version");
        self.assert_live(version);
        let parent = self.parent(version).expect("non-root version has a parent");
        let mut removed = Vec::new();
        self.remove_subtree(version, &mut removed);
        self.nodes[parent.0 as usize]
            .children
            .retain(|child| *child != version);
        removed
    }

    fn remove_subtree(&mut self, version: Version, removed: &mut Vec<Version>) {
        let children = self.nodes[version.0 as usize].children.clone();
        for child in children {
            self.remove_subtree(child, removed);
        }
        let node = &mut self.nodes[version.0 as usize];
        node.children.clear();
        node.removed = true;
        removed.push(version);
    }
}

impl Default for VersionTree {
    fn default() -> Self {
        Self::new()
    }
}

pub struct AncestorIter<'tree> {
    tree: &'tree VersionTree,
    current: Option<Version>,
}

impl Iterator for AncestorIter<'_> {
    type Item = Version;

    fn next(&mut self) -> Option<Self::Item> {
        let version = self.current?;
        self.current = self.tree.nodes[version.0 as usize].parent;
        Some(version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty::{FloatTy, IntTy, UintTy};

    #[test]
    fn inference_domain_intersection_is_commutative() {
        assert_eq!(
            InferDomain::Any.intersect(InferDomain::Integer),
            Some(InferDomain::Integer)
        );
        assert_eq!(
            InferDomain::Integer.intersect(InferDomain::Any),
            Some(InferDomain::Integer)
        );
        assert_eq!(
            InferDomain::Float.intersect(InferDomain::Float),
            Some(InferDomain::Float)
        );
        assert_eq!(InferDomain::Integer.intersect(InferDomain::Float), None);
        assert_eq!(InferDomain::Float.intersect(InferDomain::Integer), None);
    }

    #[test]
    fn numeric_domains_admit_only_their_primitive_families() {
        for integer in [
            IntTy::I8,
            IntTy::I16,
            IntTy::I32,
            IntTy::I64,
            IntTy::I128,
            IntTy::Isize,
        ] {
            assert!(InferDomain::Integer.admits(Ty::Int(integer)));
            assert!(!InferDomain::Float.admits(Ty::Int(integer)));
        }
        for integer in [
            UintTy::U8,
            UintTy::U16,
            UintTy::U32,
            UintTy::U64,
            UintTy::U128,
            UintTy::Usize,
        ] {
            assert!(InferDomain::Integer.admits(Ty::Uint(integer)));
            assert!(!InferDomain::Float.admits(Ty::Uint(integer)));
        }
        for float in [FloatTy::F32, FloatTy::F64] {
            assert!(InferDomain::Float.admits(Ty::Float(float)));
            assert!(!InferDomain::Integer.admits(Ty::Float(float)));
        }
        assert!(InferDomain::Any.admits(Ty::Bool));
        assert!(!InferDomain::Integer.admits(Ty::Bool));
        assert!(!InferDomain::Float.admits(Ty::Bool));
    }

    #[test]
    fn sibling_variables_are_globally_unique_and_invisible() {
        let mut tree = VersionTree::new();
        let left = tree.branch_from(Version::ROOT);
        let right = tree.branch_from(Version::ROOT);
        let left_var = tree.alloc_var(left, Universe::ROOT);
        let right_var = tree.alloc_var(right, Universe::ROOT);

        assert_ne!(left_var, right_var);
        assert_eq!(tree.get_variable(left, left_var).owner, left);
        assert_eq!(tree.get_variable(right, right_var).owner, right);

        let sibling_access = std::panic::catch_unwind(|| tree.get_variable(right, left_var));
        assert!(sibling_access.is_err());
    }

    #[test]
    fn variable_metadata_preserves_its_initial_domain() {
        let mut tree = VersionTree::new();
        let any = tree.alloc_var(Version::ROOT, Universe::ROOT);
        let integer = tree.alloc_var_in_domain(Version::ROOT, Universe::ROOT, InferDomain::Integer);

        assert_eq!(
            tree.get_variable(Version::ROOT, any).initial_domain,
            InferDomain::Any
        );
        assert_eq!(
            tree.get_variable(Version::ROOT, integer).initial_domain,
            InferDomain::Integer
        );
    }

    #[test]
    fn version_ids_are_not_reused() {
        let mut tree = VersionTree::new();
        let first = tree.branch_from(Version::ROOT);
        tree.remove(first);
        let second = tree.branch_from(Version::ROOT);

        assert_ne!(first, second);
        assert!(!tree.is_live(first));
        assert!(tree.is_live(second));
    }

    #[test]
    fn discarded_variables_are_not_visible_from_root() {
        let mut tree = VersionTree::new();
        let child = tree.branch_from(Version::ROOT);
        let discarded = tree.alloc_var(child, Universe::ROOT);
        tree.remove(child);

        let visible: Vec<_> = tree.visible_variables(Version::ROOT).collect();
        assert!(!visible.contains(&discarded));
    }
}
