use sage_stash::{Stash, StashHash, Stashed};

use crate::diagnostic::{Diagnostic, ErrorReported, Span};
use crate::local_syms::LocalModItemSym;
use crate::resolve::Resolver;
use crate::span::RelativeSpan;
use crate::ty::{CheckedParameterEnv, SolverEligibility, WherePredicate};

pub struct Check<'a, 'db> {
    pub db: &'db dyn crate::Db,
    pub resolver: Resolver<'db>,
    pub source_stash: &'a Stash,
    pub target_stash: Stash,
    pub diagnostics: Vec<Diagnostic<'db>>,
    pub current_sym: Option<LocalModItemSym<'db>>,
    type_use_predicates: Vec<WherePredicate<'db>>,
    type_use_eligibility: SolverEligibility,
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
            type_use_predicates: Vec::new(),
            type_use_eligibility: SolverEligibility::Eligible,
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

    pub fn record_type_use_parameter_env(&mut self, environment: CheckedParameterEnv<'db>) {
        self.type_use_predicates
            .extend_from_slice(&self.target_stash[environment.where_clauses]);
        self.type_use_eligibility = self
            .type_use_eligibility
            .and(environment.solver_eligibility);
    }

    pub fn complete_parameter_env(
        &mut self,
        where_clauses: sage_stash::Slice<WherePredicate<'db>>,
        solver_eligibility: SolverEligibility,
    ) -> CheckedParameterEnv<'db> {
        let mut predicates = self.target_stash[where_clauses].to_vec();
        predicates.extend_from_slice(&self.type_use_predicates);
        CheckedParameterEnv {
            where_clauses: self.target_stash.alloc_slice(&predicates),
            solver_eligibility: solver_eligibility.and(self.type_use_eligibility),
        }
    }

    pub fn finish<T: StashHash + Copy>(self, root: T) -> Stashed<T> {
        Stashed::new(self.target_stash, root)
    }
}
