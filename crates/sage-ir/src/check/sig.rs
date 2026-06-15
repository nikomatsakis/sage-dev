use sage_stash::{Stash, StashHash, Stashed};

use crate::resolve::Resolver;

pub struct Check<'a, 'db> {
    pub resolver: Resolver<'db>,
    pub src: &'a Stash,
    pub target_stash: Stash,
}

impl<'a, 'db> Check<'a, 'db> {
    pub fn new(src: &'a Stash, resolver: Resolver<'db>) -> Self {
        Self {
            resolver,
            src,
            target_stash: Stash::new(),
        }
    }

    pub fn finish<T: StashHash + Copy>(self, root: T) -> Stashed<T> {
        Stashed::new(self.target_stash, root)
    }
}
