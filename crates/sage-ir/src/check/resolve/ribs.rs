use crate::generic_param::GenericParam;
use crate::name::Name;
use crate::symbol::Symbol;
use crate::ty::Ty;
use crate::tytree::LocalId;

use super::Namespace;

/// The result of resolving a name or path.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Resolution<'db> {
    Local(LocalId),
    Param(GenericParam<'db>),
    Sym(Symbol<'db>),
    SelfTy(Ty<'db>),
    Error,
}

impl<'db> Resolution<'db> {
    pub fn sym(self) -> Option<Symbol<'db>> {
        match self {
            Resolution::Sym(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct Ribs<'db> {
    scopes: Vec<Vec<(Name<'db>, Namespace, Resolution<'db>)>>,
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

    pub fn add(&mut self, name: Name<'db>, ns: Namespace, entry: Resolution<'db>) {
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
                self.add(name, Namespace::Type, Resolution::Param(gp));
            }
        }
    }

    pub fn lookup(&self, name: Name<'db>, ns: Namespace) -> Option<Resolution<'db>> {
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
