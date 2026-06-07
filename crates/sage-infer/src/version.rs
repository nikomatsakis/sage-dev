use sage_ir::ty::InferVarIndex;

/// Version tree node identifier.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Version(pub u32);

impl Version {
    pub const ROOT: Version = Version(0);
}

/// Scope depth. A variable at universe U cannot escape to bounds at universe < U.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Universe(pub u32);

/// Metadata for one inference variable.
#[derive(Copy, Clone, Debug)]
pub struct VarInfo {
    pub universe: Universe,
}

/// A node in the version tree.
#[derive(Debug)]
pub struct VersionNode {
    pub parent: Version,
    pub children: Vec<Version>,
    /// First `InferVarIndex` belonging to this version.
    pub variable_start: InferVarIndex,
    /// Inference variables created at this version.
    pub variables: Vec<VarInfo>,
    /// Whether this version has been removed.
    pub removed: bool,
}

impl VersionNode {
    pub fn new(parent: Version, variable_start: InferVarIndex) -> Self {
        Self {
            parent,
            children: Vec::new(),
            variable_start,
            variables: Vec::new(),
            removed: false,
        }
    }
}

/// The version tree — manages branching for speculative trait resolution.
#[derive(Debug)]
pub struct VersionTree {
    nodes: Vec<VersionNode>,
}

impl VersionTree {
    pub fn new() -> Self {
        let root = VersionNode::new(Version::ROOT, InferVarIndex(0));
        Self { nodes: vec![root] }
    }

    pub fn root(&self) -> Version {
        Version::ROOT
    }

    pub fn get(&self, v: Version) -> &VersionNode {
        &self.nodes[v.0 as usize]
    }

    pub fn get_mut(&mut self, v: Version) -> &mut VersionNode {
        &mut self.nodes[v.0 as usize]
    }

    /// Create a child version branching from `parent`.
    pub fn branch(&mut self, parent: Version) -> Version {
        let start = self.variable_count_at(parent);
        let child = Version(self.nodes.len() as u32);
        let node = VersionNode::new(parent, start);
        self.nodes.push(node);
        self.nodes[parent.0 as usize].children.push(child);
        child
    }

    /// Remove a version and its subtree.
    /// Remove a version and its subtree. Returns all removed version IDs.
    pub fn remove(&mut self, v: Version) -> Vec<Version> {
        assert_ne!(v, Version::ROOT, "cannot remove root version");
        let mut removed = Vec::new();
        self.remove_subtree(v, &mut removed);
        let parent = self.nodes[v.0 as usize].parent;
        self.nodes[parent.0 as usize].children.retain(|c| *c != v);
        removed
    }

    fn remove_subtree(&mut self, v: Version, out: &mut Vec<Version>) {
        self.nodes[v.0 as usize].removed = true;
        out.push(v);
        let children: Vec<_> = self.nodes[v.0 as usize].children.clone();
        for child in children {
            self.remove_subtree(child, out);
        }
    }

    /// Total number of inference variables visible at this version
    /// (including ancestors).
    pub fn variable_count_at(&self, v: Version) -> InferVarIndex {
        let node = &self.nodes[v.0 as usize];
        InferVarIndex(node.variable_start.0 + node.variables.len() as u32)
    }

    /// Allocate a new inference variable at the given version.
    pub fn alloc_var(&mut self, v: Version, info: VarInfo) -> InferVarIndex {
        let node = &mut self.nodes[v.0 as usize];
        let idx = InferVarIndex(node.variable_start.0 + node.variables.len() as u32);
        node.variables.push(info);
        idx
    }

    /// Look up variable metadata by walking up the version tree.
    pub fn get_variable(&self, v: Version, idx: InferVarIndex) -> &VarInfo {
        let node = &self.nodes[v.0 as usize];
        if idx.0 < node.variable_start.0 {
            assert_ne!(v, Version::ROOT, "InferVarIndex({}) not found", idx.0);
            self.get_variable(node.parent, idx)
        } else {
            let offset = (idx.0 - node.variable_start.0) as usize;
            assert!(
                offset < node.variables.len(),
                "InferVarIndex({}) out of bounds at version {:?}",
                idx.0,
                v
            );
            &node.variables[offset]
        }
    }

    /// Walk from `v` up to root, yielding each version along the path.
    pub fn ancestors(&self, v: Version) -> AncestorIter<'_> {
        AncestorIter {
            tree: self,
            current: Some(v),
        }
    }
}

pub struct AncestorIter<'a> {
    tree: &'a VersionTree,
    current: Option<Version>,
}

impl Iterator for AncestorIter<'_> {
    type Item = Version;

    fn next(&mut self) -> Option<Self::Item> {
        let v = self.current?;
        if v == Version::ROOT {
            self.current = None;
        } else {
            self.current = Some(self.tree.nodes[v.0 as usize].parent);
        }
        Some(v)
    }
}
