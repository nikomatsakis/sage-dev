use crate::generic_param::GenericParam;
use crate::name::Name;
use crate::resolved::LocalId;
use crate::symbol::Symbol;
use crate::ty::Ty;

use crate::resolve::Namespace;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RibEntry<'db> {
    Local(LocalId),
    Param(GenericParam<'db>),
    Sym(Symbol<'db>),
    SelfTy(Ty<'db>),
}

#[derive(Default)]
pub struct Ribs<'db> {
    scopes: Vec<Vec<(Name<'db>, Namespace, RibEntry<'db>)>>,
}

impl<'db> Ribs<'db> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub fn add(&mut self, name: Name<'db>, ns: Namespace, entry: RibEntry<'db>) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.push((name, ns, entry));
        }
    }

    /// Adds generic parameters to the current scope.
    pub fn add_generic_params(
        &mut self,
        db: &'db dyn crate::Db,
        params: impl Iterator<Item = GenericParam<'db>>,
    ) {
        self.push_scope();
        for gp in params {
            if let Some(name) = gp.name(db) {
                self.add(name, Namespace::Type, RibEntry::Param(gp));
            }
        }
    }

    pub fn lookup(&self, name: Name<'db>, ns: Namespace) -> Option<RibEntry<'db>> {
        for scope in self.scopes.iter().rev() {
            for (n, entry_ns, entry) in scope.iter().rev() {
                if *n == name && *entry_ns == ns {
                    return Some(*entry);
                }
            }
        }
        None
    }
}
