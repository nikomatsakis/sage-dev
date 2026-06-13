use sage_stash::{Stash, Stashed};

use crate::resolve::Resolver;
use crate::ribs::Ribs;

pub struct CstLowerCtx<'a, 'db> {
    pub resolver: Resolver<'db>,
    pub src: &'a Stash,
    pub dst: Stash,
    pub ribs: Ribs<'db>,
}

impl<'a, 'db> CstLowerCtx<'a, 'db> {
    pub fn new(src: &'a Stash, resolver: Resolver<'db>) -> Self {
        let mut ribs = Ribs::new();
        ribs.push_scope();
        Self {
            resolver,
            src,
            dst: Stash::new(),
            ribs,
        }
    }

    pub fn finish<T>(self, root: T) -> Stashed<T> {
        Stashed::new(self.dst, root)
    }
}
