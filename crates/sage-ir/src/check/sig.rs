use sage_stash::{Stash, StashHash, Stashed};

use crate::resolve::Resolver;

pub struct CstLowerCtx<'a, 'db> {
    pub resolver: Resolver<'db>,
    pub src: &'a Stash,
    pub dst: Stash,
}

impl<'a, 'db> CstLowerCtx<'a, 'db> {
    pub fn new(src: &'a Stash, resolver: Resolver<'db>) -> Self {
        Self {
            resolver,
            src,
            dst: Stash::new(),
        }
    }

    pub fn finish<T: StashHash + Copy>(self, root: T) -> Stashed<T> {
        Stashed::new(self.dst, root)
    }
}
