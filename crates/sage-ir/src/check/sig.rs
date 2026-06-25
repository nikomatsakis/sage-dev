use sage_stash::{Stash, StashHash, Stashed};

use crate::diagnostic::{Diagnostic, ErrorReported, Span};
use crate::local_syms::LocalModItemSym;
use crate::resolve::Resolver;
use crate::span::RelativeSpan;

pub struct Check<'a, 'db> {
    pub db: &'db dyn crate::Db,
    pub resolver: Resolver<'db>,
    pub source_stash: &'a Stash,
    pub target_stash: Stash,
    pub diagnostics: Vec<Diagnostic<'db>>,
    pub current_sym: Option<LocalModItemSym<'db>>,
}

impl<'a, 'db> Check<'a, 'db> {
    pub fn new(db: &'db dyn crate::Db, source_stash: &'a Stash, resolver: Resolver<'db>) -> Self {
        Self {
            db,
            resolver,
            source_stash,
            target_stash: Stash::new(),
            diagnostics: Vec::new(),
            current_sym: None,
        }
    }

    pub fn span(&self, relative: RelativeSpan) -> Span<'db> {
        Span::Relative(
            self.current_sym.expect("span() called without current_sym"),
            relative,
        )
    }

    pub fn report(&mut self, diag: Diagnostic<'db>) -> ErrorReported {
        crate::diagnostic::report(&mut self.diagnostics, diag)
    }

    pub fn finish<T: StashHash + Copy>(self, root: T) -> Stashed<T> {
        Stashed::new(self.target_stash, root)
    }
}
