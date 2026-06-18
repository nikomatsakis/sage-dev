use sage_stash::{Stash, StashHash, Stashed};

use crate::resolve::Resolver;

pub struct Check<'a, 'db> {
    pub db: &'db dyn crate::Db,
    pub resolver: Resolver<'db>,
    pub src: &'a Stash,
    pub target_stash: Stash,
}

impl<'a, 'db> Check<'a, 'db> {
    pub fn new(db: &'db dyn crate::Db, src: &'a Stash, resolver: Resolver<'db>) -> Self {
        Self {
            db,
            resolver,
            src,
            target_stash: Stash::new(),
        }
    }

    pub fn finish<T: StashHash + Copy>(self, root: T) -> Stashed<T> {
        Stashed::new(self.target_stash, root)
    }
}
