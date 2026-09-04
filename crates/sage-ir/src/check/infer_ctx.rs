use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Wake;

use rustc_hash::{FxHashMap, FxHashSet};
use sage_stash::{Ptr, Stash, StashCopy, Stashed};

use crate::diagnostic::{Diagnostic, ErrorReported, Span};
use crate::display::TyDisplay;
use crate::local_syms::LocalModItemSym;
use crate::name::Name;
use crate::resolve::{Namespace, Resolution, Resolver};
use crate::scope::LocalCrateSymbol;
use crate::span::RelativeSpan;
use crate::ty::{
    AliasTy, Binder, CheckedParameterEnv, FnSig, InferVarIndex, Lifetime, MethodReceiver,
    SolverEligibility, Ty,
};
use crate::tytree::*;

use super::infer::bound::Bound;
use super::infer::egraph::VersionedEGraph;
use super::infer::obligations::{
    Obligation, ObligationGoal, ObligationManager, ObligationProvenance, ObligationReason,
    ObligationRecovery, ObligationState, StagedObligationBatch,
};
use super::infer::runtime::Runtime;
use super::infer::unify::{
    RequiredUnifyFailure, UnifyError, install_error_recovery, try_set_bound, try_unify,
    try_unify_required,
};
use super::infer::version::{InferDomain, Universe, Version};

struct MainTaskWake(AtomicBool);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum TraitGoalCertainty {
    Yes,
    Maybe,
    No,
}

enum NormalizationProgress<'db> {
    Yes,
    Maybe,
    No,
    Residual(super::solve::Goal<'db>),
}

struct CallSignatureNormalizer<'a, 'db> {
    source: &'a Stash,
    target: &'a mut Stash,
    egraph: &'a mut VersionedEGraph<'db>,
    relations: Vec<(AliasTy<'db>, Ptr<Ty<'db>>)>,
}

impl<'db> crate::ty_fold::TyFolder<'db> for CallSignatureNormalizer<'_, 'db> {
    fn target(&mut self) -> &mut Stash {
        self.target
    }

    fn source(&self) -> &Stash {
        self.source
    }

    fn fold_ty(&mut self, ty: Ty<'db>) -> Ty<'db> {
        match ty {
            Ty::Alias(AliasTy::Associated(alias)) => {
                let alias = crate::ty_fold::fold_alias_ty(self, AliasTy::Associated(alias));
                let index = self.egraph.alloc_var(Version::ROOT, Universe(1));
                let expected = self.target.alloc(Ty::InferVar(index));
                self.relations.push((alias, expected));
                Ty::InferVar(index)
            }
            Ty::Alias(AliasTy::Named(_) | AliasTy::Opaque(_))
            | Ty::Bool
            | Ty::Char
            | Ty::Int(_)
            | Ty::Uint(_)
            | Ty::Float(_)
            | Ty::Str
            | Ty::Adt(..)
            | Ty::Ref(..)
            | Ty::Tuple(_)
            | Ty::Slice(_)
            | Ty::Array(..)
            | Ty::FnPtr(..)
            | Ty::Param(_)
            | Ty::InferVar(_)
            | Ty::Never
            | Ty::Error(_) => crate::ty_fold::default_fold_ty(self, ty),
        }
    }
}

impl Wake for MainTaskWake {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::Release);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.store(true, Ordering::Release);
    }
}

// ---------------------------------------------------------------------------
// TypeError — carried over from body.rs
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum TypeError<'db> {
    Fresh {
        kind: TypeErrorKind<'db>,
        span: RelativeSpan,
        context: Vec<ErrorContext>,
    },
    Reported(ErrorReported),
}

#[derive(Clone, Debug)]
pub enum TypeErrorKind<'db> {
    Mismatch {
        expected: Ptr<Ty<'db>>,
        actual: Ptr<Ty<'db>>,
    },
    DomainConflict {
        expected: Ptr<Ty<'db>>,
        expected_domain: Option<InferDomain>,
        actual: Ptr<Ty<'db>>,
        actual_domain: Option<InferDomain>,
        affected: Vec<InferVarIndex>,
        relation_expected: Ptr<Ty<'db>>,
        relation_actual: Ptr<Ty<'db>>,
    },
    UnresolvedInferVar {
        var: InferVarIndex,
        domain: InferDomain,
    },
    AmbiguousName {
        count: usize,
    },
}

#[derive(Clone, Debug)]
pub enum ErrorContext {
    ReturnType {
        ret_span: RelativeSpan,
    },
    Argument {
        index: usize,
        call_span: RelativeSpan,
    },
    FieldInit {
        field_span: RelativeSpan,
    },
}

enum RequiredRelationRecovery<'db> {
    Done,
    Atom(Ptr<Ty<'db>>, Ptr<Ty<'db>>),
    Children(Vec<(Ptr<Ty<'db>>, Ptr<Ty<'db>>)>),
}

impl<'db> TypeError<'db> {
    pub fn with_context(mut self, context: ErrorContext) -> Self {
        match &mut self {
            TypeError::Fresh { context: ctx, .. } => ctx.push(context),
            TypeError::Reported(_) => {}
        }
        self
    }
}

fn domain_side_description<'db>(
    db: &'db dyn crate::Db,
    stash: &Stash,
    ty: Ptr<Ty<'db>>,
    domain: Option<InferDomain>,
) -> String {
    match stash[ty] {
        Ty::InferVar(_) => match domain {
            Some(InferDomain::Integer) => "an integer".to_owned(),
            Some(InferDomain::Float) => "a floating-point type".to_owned(),
            Some(InferDomain::Any) | None => "some type".to_owned(),
        },
        Ty::Bool
        | Ty::Char
        | Ty::Int(_)
        | Ty::Uint(_)
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
        | Ty::Never
        | Ty::Error(_) => {
            let display = TyDisplay::new(db, stash, ty).to_string();
            format!("`{}`", hide_inference_indices(&display))
        }
    }
}

fn hide_inference_indices(display: &str) -> String {
    // `TyDisplay`'s only source-internal fragment is an inference variable
    // written as `?<digits>`. Preserve its structural formatting while
    // replacing those fragments with source-level placeholders.
    let mut result = String::with_capacity(display.len());
    let mut characters = display.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '?' && characters.peek().is_some_and(char::is_ascii_digit) {
            result.push('_');
            while characters.peek().is_some_and(char::is_ascii_digit) {
                characters.next();
            }
        } else {
            result.push(character);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// CheckError — the fatal error type for async expression checking
// ---------------------------------------------------------------------------

/// A fatal check error: the node cannot be constructed.
/// Propagates via `?` until caught at a scope boundary, which records the
/// diagnostic and substitutes `TyExprData::Error(ErrorReported)`.
#[derive(Clone, Debug)]
pub struct CheckError<'db>(pub TypeError<'db>);

impl<'db> From<TypeError<'db>> for CheckError<'db> {
    fn from(err: TypeError<'db>) -> Self {
        CheckError(err)
    }
}

/// Extension trait for `Result<(), TypeError>` — report non-fatal errors
/// without propagating.
pub trait RecordErr<'db> {
    fn record_err(self, cx: &InferCtx<'_, 'db>);
}

impl<'db> RecordErr<'db> for Result<(), TypeError<'db>> {
    fn record_err(self, cx: &InferCtx<'_, 'db>) {
        if let Err(e) = self {
            cx.catch(e);
        }
    }
}

// ---------------------------------------------------------------------------
// InferCtx — shared inference state (one per function body)
// ---------------------------------------------------------------------------

/// Shared inference state — one per function body, eventually shared by all
/// concurrent tasks. Protected by `RefCell` since the executor is
/// single-threaded and cooperative.
// ANCHOR: architecture_infer_context
pub struct InferCtx<'check, 'db> {
    pub db: &'db dyn crate::Db,
    pub(crate) source_stash: &'check Stash,
    current_sym: Option<LocalModItemSym<'db>>,
    local_crate: Option<LocalCrateSymbol<'db>>,

    // The declared return type of the function being checked, and its span.
    ret_ty: Option<(Ptr<Ty<'db>>, Option<RelativeSpan>)>,

    // Shared mutable state
    egraph: RefCell<VersionedEGraph<'db>>,
    runtime: RefCell<Runtime>,
    target_stash: RefCell<Stash>,
    local_vars: RefCell<Vec<LocalVar<'db>>>,
    expr_slots: RefCell<Vec<Option<Ptr<TyExpr<'db>>>>>,

    // Trait facts available to this body and its retained proof requests.
    solver_assumptions: RefCell<sage_stash::Slice<super::solve::Assumption<'db>>>,
    assumptions_complete: RefCell<bool>,
    obligations: RefCell<ObligationManager<'db>>,

    // Wake queue: variables whose bounds changed during a poll.
    // Processed by block_on between polls (avoids double-borrow of runtime).
    pending_wakes: RefCell<Vec<InferVarIndex>>,

    // Inference targets reached only from `!` coercion sites. Other
    // constraints get first choice; any still-unbound target falls back to
    // `!` before unresolved-variable diagnostics are emitted.
    deferred_never_targets: RefCell<Vec<Ptr<Ty<'db>>>>,

    // Diagnostic accumulator
    diagnostics: RefCell<Vec<Diagnostic<'db>>>,
}
// ANCHOR_END: architecture_infer_context

impl<'check, 'db> InferCtx<'check, 'db> {
    pub fn new(
        db: &'db dyn crate::Db,
        source_stash: &'check Stash,
        current_sym: Option<LocalModItemSym<'db>>,
    ) -> Self {
        let local_crate = match current_sym {
            Some(LocalModItemSym::Function(function)) => Some(function.scope(db).local_crate(db)),
            Some(
                LocalModItemSym::Struct(_)
                | LocalModItemSym::Enum(_)
                | LocalModItemSym::Trait(_)
                | LocalModItemSym::Impl(_)
                | LocalModItemSym::TypeAlias(_)
                | LocalModItemSym::Const(_)
                | LocalModItemSym::Static(_)
                | LocalModItemSym::Mod(_)
                | LocalModItemSym::Use(_)
                | LocalModItemSym::MacroDef(_)
                | LocalModItemSym::MacroInvocation(_)
                | LocalModItemSym::Error(_),
            )
            | None => None,
        };
        let mut target_stash = Stash::new();
        let solver_assumptions = target_stash.alloc_slice(&[]);
        Self {
            db,
            source_stash,
            current_sym,
            local_crate,
            ret_ty: None,
            egraph: RefCell::new(VersionedEGraph::new()),
            runtime: RefCell::new(Runtime::new()),
            target_stash: RefCell::new(target_stash),
            local_vars: RefCell::new(Vec::new()),
            expr_slots: RefCell::new(Vec::new()),
            solver_assumptions: RefCell::new(solver_assumptions),
            assumptions_complete: RefCell::new(true),
            obligations: RefCell::new(ObligationManager::default()),
            pending_wakes: RefCell::new(Vec::new()),
            deferred_never_targets: RefCell::new(Vec::new()),
            diagnostics: RefCell::new(Vec::new()),
        }
    }

    pub fn set_ret_ty(&mut self, ty: Ptr<Ty<'db>>, span: Option<RelativeSpan>) {
        self.ret_ty = Some((ty, span));
    }

    pub fn ret_ty(&self) -> Option<(Ptr<Ty<'db>>, Option<RelativeSpan>)> {
        self.ret_ty
    }

    // ------------------------------------------------------------------
    // Span helpers
    // ------------------------------------------------------------------

    pub fn span(&self, relative: RelativeSpan) -> Span<'db> {
        Span::Relative(
            self.current_sym.expect("span() called without current_sym"),
            relative,
        )
    }

    // ------------------------------------------------------------------
    // Diagnostics
    // ------------------------------------------------------------------

    pub fn record(&self, diag: Diagnostic<'db>) -> ErrorReported {
        crate::diagnostic::report(&mut self.diagnostics.borrow_mut(), diag)
    }

    pub fn catch(&self, err: TypeError<'db>) -> ErrorReported {
        match &err {
            TypeError::Reported(e) => *e,
            TypeError::Fresh { kind, .. } => {
                let diag = self.type_error_to_diagnostic(&err).unwrap();
                let reported = self.record(diag);
                match kind {
                    TypeErrorKind::DomainConflict {
                        expected: _,
                        expected_domain: _,
                        actual: _,
                        actual_domain: _,
                        affected,
                        relation_expected,
                        relation_actual,
                    } => {
                        self.recover_domain_conflict(affected, reported);
                        self.recover_required_relation(
                            *relation_actual,
                            *relation_expected,
                            reported,
                        );
                    }
                    TypeErrorKind::Mismatch { .. }
                    | TypeErrorKind::UnresolvedInferVar { .. }
                    | TypeErrorKind::AmbiguousName { .. } => {}
                }
                reported
            }
        }
    }

    /// Relate every still-unresolved affected class to one reported error.
    fn recover_domain_conflict(&self, affected: &[InferVarIndex], reported: ErrorReported) {
        let roots = {
            let mut stash = self.target_stash.borrow_mut();
            let egraph = self.egraph.borrow();
            let mut roots = Vec::new();
            for variable in affected {
                let ty = stash.alloc(Ty::InferVar(*variable));
                let root = egraph.find(Version::ROOT, ty);
                if matches!(stash[root], Ty::InferVar(_)) && !roots.contains(&root) {
                    roots.push(root);
                }
            }
            roots
        };
        if roots.is_empty() {
            return;
        }

        let error_ty = self.target_stash.borrow_mut().alloc(Ty::Error(reported));
        for root in roots {
            let effects = {
                let mut egraph = self.egraph.borrow_mut();
                let mut stash = self.target_stash.borrow_mut();
                let root = egraph.find_mut(Version::ROOT, root);
                if !matches!(stash[root], Ty::InferVar(_)) {
                    None
                } else {
                    Some(
                        install_error_recovery(
                            &mut egraph,
                            &mut stash,
                            Version::ROOT,
                            root,
                            error_ty,
                        )
                        .expect("domain-error recovery must accept a reported error type"),
                    )
                }
            };
            if let Some(effects) = effects {
                self.publish_commit_effects(effects);
            }
        }
    }

    /// Replay the non-conflicting children of an equality after its primary
    /// domain error has been recovered. This restores compatible constraints
    /// that the atomic unifier rolled back without poisoning unrelated types.
    fn recover_required_relation(
        &self,
        actual: Ptr<Ty<'db>>,
        expected: Ptr<Ty<'db>>,
        reported: ErrorReported,
    ) {
        self.recover_required_relation_inner(actual, expected, reported, &mut FxHashSet::default());
    }

    fn recover_required_relation_inner(
        &self,
        actual: Ptr<Ty<'db>>,
        expected: Ptr<Ty<'db>>,
        reported: ErrorReported,
        visited: &mut FxHashSet<(Ptr<Ty<'db>>, Ptr<Ty<'db>>)>,
    ) {
        let step = {
            let stash = self.target_stash.borrow();
            let egraph = self.egraph.borrow();
            let actual = egraph.find(Version::ROOT, actual);
            let expected = egraph.find(Version::ROOT, expected);
            if actual == expected || !visited.insert((actual, expected)) {
                RequiredRelationRecovery::Done
            } else if matches!(stash[actual], Ty::Error(_))
                || matches!(stash[expected], Ty::Error(_))
            {
                RequiredRelationRecovery::Done
            } else if matches!(stash[actual], Ty::InferVar(_))
                || matches!(stash[expected], Ty::InferVar(_))
            {
                RequiredRelationRecovery::Atom(actual, expected)
            } else {
                let actual = super::infer::skeleton::decompose(&stash, actual);
                let expected = super::infer::skeleton::decompose(&stash, expected);
                if actual.skeleton != expected.skeleton
                    || actual.children.len() != expected.children.len()
                {
                    RequiredRelationRecovery::Done
                } else {
                    RequiredRelationRecovery::Children(
                        actual.children.into_iter().zip(expected.children).collect(),
                    )
                }
            }
        };

        match step {
            RequiredRelationRecovery::Done => {}
            RequiredRelationRecovery::Atom(actual, expected) => {
                let result = {
                    let mut egraph = self.egraph.borrow_mut();
                    let mut stash = self.target_stash.borrow_mut();
                    try_unify_required(&mut egraph, &mut stash, Version::ROOT, actual, expected)
                };
                match result {
                    Ok(effects) => self.publish_commit_effects(effects),
                    Err(RequiredUnifyFailure { error, affected }) => match error {
                        UnifyError::DomainConflict { .. } => {
                            self.recover_domain_conflict(&affected, reported);
                        }
                        UnifyError::Mismatch { .. }
                        | UnifyError::OccursCheck { .. }
                        | UnifyError::UniverseLeak { .. }
                        | UnifyError::Reported(_) => {}
                    },
                }
            }
            RequiredRelationRecovery::Children(children) => {
                for (actual, expected) in children {
                    self.recover_required_relation_inner(actual, expected, reported, visited);
                }
            }
        }
    }

    pub fn diagnostics_snapshot(&self) -> Vec<Diagnostic<'db>> {
        self.diagnostics.borrow().clone()
    }

    pub fn has_errors(&self) -> bool {
        !self.diagnostics.borrow().is_empty()
    }

    /// Find an error embedded anywhere in a type so later checking does not
    /// emit diagnostics derived from an already-reported failure.
    pub(crate) fn error_in_ty(&self, ty: Ptr<Ty<'db>>) -> Option<ErrorReported> {
        fn visit<'db>(
            stash: &Stash,
            egraph: &VersionedEGraph<'db>,
            ty: Ptr<Ty<'db>>,
            visited: &mut FxHashSet<Ptr<Ty<'db>>>,
        ) -> Option<ErrorReported> {
            let ty = egraph.find(Version::ROOT, ty);
            if !visited.insert(ty) {
                return None;
            }
            match stash[ty] {
                Ty::Error(error) => Some(error),
                Ty::Adt(_, arguments) | Ty::Tuple(arguments) => stash[arguments]
                    .iter()
                    .find_map(|argument| visit(stash, egraph, *argument, visited)),
                Ty::Alias(_) => crate::check::infer::skeleton::decompose(stash, ty)
                    .children
                    .into_iter()
                    .find_map(|child| visit(stash, egraph, child, visited)),
                Ty::Ref(inner, _, _) | Ty::Slice(inner) | Ty::Array(inner, _) => {
                    visit(stash, egraph, inner, visited)
                }
                Ty::FnPtr(parameters, return_ty) => stash[parameters]
                    .iter()
                    .find_map(|parameter| visit(stash, egraph, *parameter, visited))
                    .or_else(|| visit(stash, egraph, return_ty, visited)),
                Ty::Bool
                | Ty::Char
                | Ty::Int(_)
                | Ty::Uint(_)
                | Ty::Float(_)
                | Ty::Str
                | Ty::Param(_)
                | Ty::InferVar(_)
                | Ty::Never => None,
            }
        }

        let stash = self.target_stash.borrow();
        let egraph = self.egraph.borrow();
        visit(&stash, &egraph, ty, &mut FxHashSet::default())
    }

    fn mismatch_diagnostic(
        &self,
        span: Span<'db>,
        context: &[ErrorContext],
        expected: &str,
        actual: &str,
    ) -> Diagnostic<'db> {
        let message = format!("type mismatch: expected {expected}, found {actual}");
        let mut diagnostic =
            Diagnostic::error(span.clone(), message).label(span, format!("found {actual}"));
        for context in context {
            match context {
                ErrorContext::ReturnType { ret_span } => {
                    diagnostic = diagnostic.secondary(
                        self.span(*ret_span),
                        format!("expected {expected} because of return type"),
                    );
                }
                ErrorContext::Argument { index, call_span } => {
                    diagnostic = diagnostic.secondary(
                        self.span(*call_span),
                        format!("expected {expected} for argument {}", index + 1),
                    );
                }
                ErrorContext::FieldInit { field_span } => {
                    diagnostic = diagnostic.secondary(
                        self.span(*field_span),
                        format!("expected {expected} for this field"),
                    );
                }
            }
        }
        diagnostic
    }

    fn type_error_to_diagnostic(&self, err: &TypeError<'db>) -> Option<Diagnostic<'db>> {
        let TypeError::Fresh {
            kind,
            span,
            context,
        } = err
        else {
            return None;
        };
        let span_resolved = self.span(*span);
        let stash = self.target_stash.borrow();
        Some(match kind {
            TypeErrorKind::Mismatch { expected, actual } => {
                let expected = format!("`{}`", TyDisplay::new(self.db, &*stash, *expected));
                let actual = format!("`{}`", TyDisplay::new(self.db, &*stash, *actual));
                self.mismatch_diagnostic(span_resolved, context, &expected, &actual)
            }
            TypeErrorKind::DomainConflict {
                expected,
                expected_domain,
                actual,
                actual_domain,
                affected: _,
                relation_expected: _,
                relation_actual: _,
            } => {
                let expected =
                    domain_side_description(self.db, &stash, *expected, *expected_domain);
                let actual = domain_side_description(self.db, &stash, *actual, *actual_domain);
                self.mismatch_diagnostic(span_resolved, context, &expected, &actual)
            }
            TypeErrorKind::UnresolvedInferVar { var, domain } => {
                let message = match domain {
                    InferDomain::Any => format!("could not infer type for ?{}", var.0),
                    InferDomain::Integer => "could not infer an integer type".to_owned(),
                    InferDomain::Float => "could not infer a floating-point type".to_owned(),
                };
                Diagnostic::error(span_resolved, message)
            }
            TypeErrorKind::AmbiguousName { count } => {
                Diagnostic::error(span_resolved, format!("ambiguous name: {count} candidates"))
            }
        })
    }

    // ------------------------------------------------------------------
    // Stash access
    // ------------------------------------------------------------------

    pub fn stash(&self) -> std::cell::Ref<'_, Stash> {
        self.target_stash.borrow()
    }

    pub fn stash_mut(&self) -> std::cell::RefMut<'_, Stash> {
        self.target_stash.borrow_mut()
    }

    // ------------------------------------------------------------------
    // Type allocation
    // ------------------------------------------------------------------

    pub fn alloc_ty(&self, ty: Ty<'db>) -> Ptr<Ty<'db>> {
        self.target_stash.borrow_mut().alloc(ty)
    }

    pub fn unit_ty(&self) -> Ptr<Ty<'db>> {
        let elems = self.target_stash.borrow_mut().alloc_slice(&[]);
        self.target_stash.borrow_mut().alloc(Ty::Tuple(elems))
    }

    // ------------------------------------------------------------------
    // Variable allocation
    // ------------------------------------------------------------------

    pub fn fresh_ty_var(&self) -> Ptr<Ty<'db>> {
        self.fresh_ty_var_with_domain(InferDomain::Any)
    }

    pub(crate) fn fresh_ty_var_with_domain(&self, domain: InferDomain) -> Ptr<Ty<'db>> {
        self.fresh_ty_var_in_domain(Version::ROOT, Universe(1), domain)
    }

    pub(crate) fn fresh_ty_var_in(&self, version: Version, universe: Universe) -> Ptr<Ty<'db>> {
        self.fresh_ty_var_in_domain(version, universe, InferDomain::Any)
    }

    pub(crate) fn fresh_ty_var_in_domain(
        &self,
        version: Version,
        universe: Universe,
        domain: InferDomain,
    ) -> Ptr<Ty<'db>> {
        let idx = self
            .egraph
            .borrow_mut()
            .alloc_var_in_domain(version, universe, domain);
        let ty = Ty::InferVar(idx);
        self.target_stash.borrow_mut().alloc(ty)
    }

    // ------------------------------------------------------------------
    // Egraph operations
    // ------------------------------------------------------------------

    pub fn find(&self, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        self.find_in(Version::ROOT, ty)
    }

    pub fn find_mut(&self, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        self.egraph.borrow_mut().find_mut(Version::ROOT, ty)
    }

    pub fn get_bound(&self, ty: Ptr<Ty<'db>>) -> Bound<'db> {
        self.egraph.borrow().get_bound(Version::ROOT, ty)
    }

    pub fn set_bound(&self, ty: Ptr<Ty<'db>>, bound: Bound<'db>) {
        let effects = try_set_bound(
            &mut self.egraph.borrow_mut(),
            &mut self.target_stash.borrow_mut(),
            Version::ROOT,
            ty,
            bound,
        )
        .expect("invalid inference bound");
        self.publish_commit_effects(effects);
    }

    pub(crate) fn find_in(&self, version: Version, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        self.egraph.borrow().find(version, ty)
    }

    // ------------------------------------------------------------------
    // Core constraint operations
    // ------------------------------------------------------------------

    pub fn assume_eq(&self, a: Ptr<Ty<'db>>, b: Ptr<Ty<'db>>) {
        match try_unify(
            &mut self.egraph.borrow_mut(),
            &mut self.target_stash.borrow_mut(),
            Version::ROOT,
            a,
            b,
        ) {
            Ok(effects) => self.publish_commit_effects(effects),
            Err(UnifyError::Reported(_)) => {}
            Err(error) => panic!("invalid assumed equality: {error:?}"),
        }
    }

    pub fn require_eq(
        &self,
        a: Ptr<Ty<'db>>,
        b: Ptr<Ty<'db>>,
        span: RelativeSpan,
    ) -> Result<(), TypeError<'db>> {
        let result = {
            let mut egraph = self.egraph.borrow_mut();
            let mut stash = self.target_stash.borrow_mut();
            try_unify_required(&mut egraph, &mut stash, Version::ROOT, a, b)
        };
        match result {
            Ok(effects) => {
                self.publish_commit_effects(effects);
                Ok(())
            }
            Err(RequiredUnifyFailure { error, affected }) => match error {
                UnifyError::Reported(error) => Err(TypeError::Reported(error)),
                UnifyError::Mismatch { left, right } => Err(TypeError::Fresh {
                    kind: TypeErrorKind::Mismatch {
                        expected: right,
                        actual: left,
                    },
                    span,
                    context: Vec::new(),
                }),
                UnifyError::DomainConflict {
                    left,
                    left_domain,
                    right,
                    right_domain,
                } => Err(TypeError::Fresh {
                    kind: TypeErrorKind::DomainConflict {
                        expected: right,
                        expected_domain: right_domain,
                        actual: left,
                        actual_domain: left_domain,
                        affected,
                        relation_expected: b,
                        relation_actual: a,
                    },
                    span,
                    context: Vec::new(),
                }),
                UnifyError::OccursCheck { .. } | UnifyError::UniverseLeak { .. } => {
                    Err(TypeError::Fresh {
                        kind: TypeErrorKind::Mismatch {
                            expected: self.find(b),
                            actual: self.find(a),
                        },
                        span,
                        context: Vec::new(),
                    })
                }
            },
        }
    }

    pub fn require_sub(
        &self,
        a: Ptr<Ty<'db>>,
        b: Ptr<Ty<'db>>,
        span: RelativeSpan,
    ) -> Result<(), TypeError<'db>> {
        let a_canon = self.egraph.borrow_mut().find_mut(Version::ROOT, a);
        let b_canon = self.egraph.borrow_mut().find_mut(Version::ROOT, b);
        if a_canon == b_canon {
            return Ok(());
        }

        let stash = self.target_stash.borrow();
        let a_data = stash[a_canon];
        let b_data = stash[b_canon];
        drop(stash);

        match (a_data, b_data) {
            (
                Ty::Ref(inner_a, crate::cst::Mutability::Shared, _),
                Ty::Ref(inner_b, crate::cst::Mutability::Shared, _),
            ) => self.require_sub(inner_a, inner_b, span),
            (
                Ty::Ref(inner_a, crate::cst::Mutability::Mut, _),
                Ty::Ref(inner_b, crate::cst::Mutability::Mut, _),
            ) => self.require_eq(inner_a, inner_b, span),
            (
                Ty::Bool
                | Ty::Char
                | Ty::Int(_)
                | Ty::Uint(_)
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
                | Ty::Never
                | Ty::Error(_),
                _,
            ) => self.require_eq(a_canon, b_canon, span),
        }
    }

    pub fn require_coerce(
        &self,
        a: Ptr<Ty<'db>>,
        b: Ptr<Ty<'db>>,
        span: RelativeSpan,
    ) -> Result<(), TypeError<'db>> {
        if self.is_never_to_any_coercion(a, b) {
            self.defer_never_target_if_infer(b);
            Ok(())
        } else {
            self.require_sub(a, b, span)
        }
    }

    /// Relate an expression at a coercion site and materialize any selected
    /// conversion in the typed tree.
    pub fn coerce_expr(
        &self,
        expr: Ptr<TyExpr<'db>>,
        target: Ptr<Ty<'db>>,
        span: RelativeSpan,
    ) -> Result<Ptr<TyExpr<'db>>, TypeError<'db>> {
        let source = self.target_stash.borrow()[expr].ty;
        let never_to_any = self.is_never_to_any_coercion(source, target);
        if !never_to_any {
            self.require_sub(source, target, span)?;
        } else {
            self.defer_never_target_if_infer(target);
        }
        if never_to_any {
            let data = self.target_stash.borrow()[expr].data;
            match data {
                TyExprData::Block(statements, Some(tail)) => {
                    let tail_span = self.target_stash.borrow()[tail].span;
                    let tail = self.coerce_expr(tail, target, tail_span)?;
                    Ok(self.alloc_expr(TyExprData::Block(statements, Some(tail)), target, span))
                }
                TyExprData::Literal(_)
                | TyExprData::Path(_)
                | TyExprData::Block(_, None)
                | TyExprData::Call(_, _)
                | TyExprData::ResolvedCall(_, _)
                | TyExprData::MethodCall(_, _, _)
                | TyExprData::Field(_, _)
                | TyExprData::Binary(_, _, _)
                | TyExprData::Unary(_, _)
                | TyExprData::Deref(_)
                | TyExprData::Ref(_, _)
                | TyExprData::NeverToAny(_)
                | TyExprData::If(_, _, _)
                | TyExprData::IfLet(_, _, _, _)
                | TyExprData::Match(_, _)
                | TyExprData::Loop(_)
                | TyExprData::While(_, _)
                | TyExprData::WhileLet(_, _, _)
                | TyExprData::For(_, _, _)
                | TyExprData::Break(_)
                | TyExprData::Continue
                | TyExprData::Return(_)
                | TyExprData::Assign(_, _)
                | TyExprData::Await(_)
                | TyExprData::Try(_)
                | TyExprData::Closure(_, _)
                | TyExprData::Tuple(_)
                | TyExprData::Array(_)
                | TyExprData::Index(_, _)
                | TyExprData::Cast(_, _)
                | TyExprData::StructLit(_, _)
                | TyExprData::Range(_, _)
                | TyExprData::MacroCall(_, _)
                | TyExprData::Error(_)
                | TyExprData::Unresolved(_)
                | TyExprData::Missing => {
                    Ok(self.alloc_expr(TyExprData::NeverToAny(expr), target, span))
                }
            }
        } else {
            Ok(expr)
        }
    }

    pub fn coerce_expr_or_original(
        &self,
        expr: Ptr<TyExpr<'db>>,
        target: Ptr<Ty<'db>>,
        span: RelativeSpan,
    ) -> Ptr<TyExpr<'db>> {
        self.coerce_expr(expr, target, span)
            .unwrap_or_else(|error| {
                self.catch(error);
                expr
            })
    }

    pub fn coerce_expr_or_original_with_context(
        &self,
        expr: Ptr<TyExpr<'db>>,
        target: Ptr<Ty<'db>>,
        span: RelativeSpan,
        error_context: ErrorContext,
    ) -> Ptr<TyExpr<'db>> {
        self.coerce_expr(expr, target, span)
            .unwrap_or_else(|error| {
                self.catch(error.with_context(error_context));
                expr
            })
    }

    fn is_never_to_any_coercion(&self, source: Ptr<Ty<'db>>, target: Ptr<Ty<'db>>) -> bool {
        let (source, target) = {
            let mut egraph = self.egraph.borrow_mut();
            (
                egraph.find_mut(Version::ROOT, source),
                egraph.find_mut(Version::ROOT, target),
            )
        };
        let stash = self.target_stash.borrow();
        matches!(stash[source], Ty::Never) && !matches!(stash[target], Ty::Never)
    }

    fn defer_never_target_if_infer(&self, target: Ptr<Ty<'db>>) {
        let target = self.egraph.borrow_mut().find_mut(Version::ROOT, target);
        if matches!(self.target_stash.borrow()[target], Ty::InferVar(_)) {
            let mut deferred = self.deferred_never_targets.borrow_mut();
            if !deferred.contains(&target) {
                deferred.push(target);
            }
        }
    }

    // ------------------------------------------------------------------
    // Versioning
    // ------------------------------------------------------------------

    pub(crate) fn branch_from(&self, parent: Version) -> Version {
        self.egraph.borrow_mut().branch_from(parent)
    }

    pub(crate) fn discard_branch(&self, version: Version) {
        self.egraph.borrow_mut().discard(version);
    }

    #[cfg(test)]
    pub(crate) fn root_semantic_revision(&self) -> u64 {
        self.egraph.borrow().semantic_revision(Version::ROOT)
    }

    pub(crate) fn try_eq_in(
        &self,
        version: Version,
        left: Ptr<Ty<'db>>,
        right: Ptr<Ty<'db>>,
    ) -> bool {
        try_unify(
            &mut self.egraph.borrow_mut(),
            &mut self.target_stash.borrow_mut(),
            version,
            left,
            right,
        )
        .is_ok()
    }

    pub(crate) fn try_eq_or_never_in(
        &self,
        version: Version,
        source: Ptr<Ty<'db>>,
        target: Ptr<Ty<'db>>,
    ) -> bool {
        let source = self.egraph.borrow_mut().find_mut(version, source);
        if matches!(self.target_stash.borrow()[source], Ty::Never) {
            true
        } else {
            self.try_eq_in(version, source, target)
        }
    }

    pub(crate) fn commit_branch(&self, version: Version) {
        let effects = {
            let mut stash = self.target_stash.borrow_mut();
            let mut egraph = self.egraph.borrow_mut();
            egraph.rebuild(version, &mut stash);
            egraph.collapse_into(version, Version::ROOT)
        };
        self.publish_commit_effects(effects);
    }

    // ------------------------------------------------------------------
    // Expression slots
    // ------------------------------------------------------------------

    pub fn alloc_expr_slot(&self) -> (ExprSlot, Ptr<Ty<'db>>) {
        let ty = self.fresh_ty_var();
        let slot = ExprSlot(self.expr_slots.borrow().len() as u32);
        self.expr_slots.borrow_mut().push(None);
        (slot, ty)
    }

    pub fn fill_expr_slot(&self, slot: ExprSlot, expr: Ptr<TyExpr<'db>>) {
        self.expr_slots.borrow_mut()[slot.0 as usize] = Some(expr);
    }

    // ------------------------------------------------------------------
    // Execution
    // ------------------------------------------------------------------

    fn publish_commit_effects(&self, effects: super::infer::egraph::CommitEffects) {
        self.obligations.borrow_mut().wake(&effects.wakes);
        self.pending_wakes.borrow_mut().extend(effects.wakes);
    }

    /// Catch a CheckError: record the diagnostic and substitute an error node.
    pub fn error_expr(&self, err: CheckError<'db>, span: RelativeSpan) -> Ptr<TyExpr<'db>> {
        let e = self.catch(err.0);
        let ty = self.alloc_ty(Ty::Error(e));
        self.alloc_expr(TyExprData::Error(e), ty, span)
    }

    /// Flush pending wakes into the runtime, then drain ready tasks.
    /// Loops until no new wakes are produced (background tasks may push
    /// to pending_wakes during drain).
    fn flush_and_drain(&self) {
        loop {
            let wakes: Vec<_> = self.pending_wakes.borrow_mut().drain(..).collect();
            if wakes.is_empty() {
                break;
            }
            let mut rt = self.runtime.borrow_mut();
            for var in wakes {
                rt.wake_variable(var);
            }
            rt.drain();
        }
    }

    /// Run a future to completion. The future may call InferCtx methods
    /// (which push to pending_wakes); between each poll we flush wakes
    /// into the runtime and drain ready background tasks.
    pub fn block_on<F: std::future::Future>(&self, future: F) -> F::Output {
        self.block_on_inner(future, false)
    }

    /// Run the root body scope while allowing stable quiescence to drive
    /// inference fallback and mandatory obligation progress. A source task may
    /// therefore suspend on information which finalization itself supplies.
    pub(crate) fn block_on_body<F: std::future::Future>(&self, future: F) -> F::Output {
        self.block_on_inner(future, true)
    }

    fn block_on_inner<F: std::future::Future>(
        &self,
        future: F,
        recover_at_quiescence: bool,
    ) -> F::Output {
        use super::infer::runtime::CURRENT_TASK;
        use std::pin::pin;
        use std::task::{Context, Poll, Waker};

        // Allocate a task ID for the main future so await_concrete can register wakers.
        let main_task_id = {
            let mut rt = self.runtime.borrow_mut();
            rt.alloc_task_id()
        };
        let notified = Arc::new(MainTaskWake(AtomicBool::new(true)));
        let waker = Waker::from(notified.clone());

        let mut future = pin!(future);
        loop {
            if !notified.0.swap(false, Ordering::AcqRel) {
                self.flush_and_drain();
                if !notified.0.load(Ordering::Acquire) {
                    panic!("deadlock: main task pending with no wake source");
                }
                continue;
            }
            CURRENT_TASK.with(|t| *t.borrow_mut() = Some(main_task_id));
            let mut cx = Context::from_waker(&waker);
            let result = future.as_mut().poll(&mut cx);
            CURRENT_TASK.with(|t| *t.borrow_mut() = None);

            match result {
                Poll::Ready(result) => {
                    self.flush_and_drain();
                    return result;
                }
                Poll::Pending => {
                    // The main task is suspended. Flush wakes and drain
                    // background tasks — they may resolve the variable the
                    // main task is waiting on. If nothing moved, deadlock.
                    self.flush_and_drain();
                    let quiescent = self.runtime.borrow().is_quiescent()
                        && self.pending_wakes.borrow().is_empty();
                    if quiescent && !notified.0.load(Ordering::Acquire) {
                        if recover_at_quiescence {
                            let blocking_variables =
                                self.runtime.borrow().variables_waited_on_by(main_task_id);
                            self.default_deferred_never_targets(Some(&blocking_variables));
                            self.flush_and_drain();
                            if notified.0.load(Ordering::Acquire) {
                                continue;
                            }
                        }
                        panic!("deadlock: main task pending with no runnable tasks");
                    }
                }
            }
        }
    }

    /// Suspend until a type is concrete (not an unresolved infer var).
    /// After finalization, unresolved vars become Ty::Error, so this always terminates.
    pub async fn await_concrete(&self, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        use super::infer::runtime::CURRENT_TASK;
        use std::future::poll_fn;
        use std::task::Poll;

        loop {
            let canon = self.find(ty);
            let data = self.target_stash.borrow()[canon];
            match data {
                Ty::InferVar(idx) => {
                    // Suspend: register current task to wake on this variable's next bound change.
                    poll_fn(|poll_context| {
                        // Re-check in case it resolved between iterations
                        let canon = self.find(ty);
                        let data = self.target_stash.borrow()[canon];
                        if !matches!(data, Ty::InferVar(_)) {
                            return Poll::Ready(());
                        }
                        // Register waker
                        let task_id = CURRENT_TASK.with(|t| *t.borrow());
                        if let Some(task_id) = task_id {
                            self.runtime
                                .borrow_mut()
                                .wait_on(idx, task_id, poll_context.waker());
                        }
                        Poll::Pending
                    })
                    .await;
                }
                Ty::Bool
                | Ty::Char
                | Ty::Int(_)
                | Ty::Uint(_)
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
                | Ty::Never
                | Ty::Error(_) => return canon,
            }
        }
    }

    // ------------------------------------------------------------------
    // TyExpr allocation
    // ------------------------------------------------------------------

    pub fn alloc_expr(
        &self,
        data: TyExprData<'db>,
        ty: Ptr<Ty<'db>>,
        span: RelativeSpan,
    ) -> Ptr<TyExpr<'db>> {
        self.target_stash
            .borrow_mut()
            .alloc(TyExpr { data, ty, span })
    }

    pub(crate) fn adjust_method_receiver(
        &self,
        expression: Ptr<TyExpr<'db>>,
        receiver_ty: Ptr<Ty<'db>>,
        receiver: MethodReceiver,
        span: RelativeSpan,
    ) -> Ptr<TyExpr<'db>> {
        match receiver {
            MethodReceiver::Value { .. } => expression,
            MethodReceiver::Ref { mutability } => {
                let reference_ty = self.alloc_ty(Ty::Ref(receiver_ty, mutability, Lifetime::Dummy));
                self.alloc_expr(TyExprData::Ref(expression, mutability), reference_ty, span)
            }
        }
    }

    // ------------------------------------------------------------------
    // Locals
    // ------------------------------------------------------------------

    pub fn add_local_var(&self, local_var: LocalVar<'db>) {
        self.local_vars.borrow_mut().push(local_var);
    }

    // ------------------------------------------------------------------
    // Signature import
    // ------------------------------------------------------------------

    pub fn import_fn_sig(&self, sig: &Stashed<Binder<'db, FnSig<'db>>>) -> FnSig<'db> {
        let sig_stash = sig.stash();
        let binder = sig.root();
        let fn_sig = binder.value;

        let owner_self_ty = fn_sig.owner_self_ty.map(|owner_self_ty| {
            owner_self_ty.stash_copy(sig_stash, &mut *self.target_stash.borrow_mut())
        });
        let receiver = fn_sig.receiver.map(|receiver| crate::ty::CheckedReceiver {
            owner_self_ty: owner_self_ty.expect("a receiver must have an associated owner type"),
            form: receiver.form,
        });

        let params: smallvec::SmallVec<[Ptr<Ty<'db>>; 16]> = sig_stash[fn_sig.params]
            .iter()
            .map(|p| p.stash_copy(sig_stash, &mut *self.target_stash.borrow_mut()))
            .collect();
        let params = self.target_stash.borrow_mut().alloc_slice(&params);

        let ret = fn_sig
            .ret
            .stash_copy(sig_stash, &mut *self.target_stash.borrow_mut());

        let parameter_env = fn_sig
            .parameter_env
            .stash_copy(sig_stash, &mut *self.target_stash.borrow_mut());

        FnSig {
            owner_generic_count: fn_sig.owner_generic_count,
            owner_self_ty,
            receiver,
            params,
            ret,
            parameter_env,
            method_candidate_eligibility: fn_sig.method_candidate_eligibility,
            const_call_complete: fn_sig.const_call_complete,
        }
    }

    /// Replace associated projections in an instantiated call signature with
    /// caller inference variables and retain the input-only normalization
    /// relations which determine those variables. The expected variables are
    /// deliberately not part of the solver operation input.
    pub(crate) fn normalize_call_signature(
        &self,
        signature: FnSig<'db>,
        span: RelativeSpan,
    ) -> FnSig<'db> {
        let mut source = Stash::new();
        let signature = signature.stash_copy(&self.target_stash.borrow(), &mut source);
        let (signature, relations) = {
            let mut target = self.target_stash.borrow_mut();
            let mut egraph = self.egraph.borrow_mut();
            let mut folder = CallSignatureNormalizer {
                source: &source,
                target: &mut target,
                egraph: &mut egraph,
                relations: Vec::new(),
            };
            let signature = crate::ty_fold::fold_fn_sig(&mut folder, signature);
            (signature, folder.relations)
        };
        for (alias, expected) in relations {
            self.submit_alias_relation(
                alias,
                expected,
                ObligationProvenance {
                    span,
                    reason: ObligationReason::FunctionCall,
                },
            );
        }
        signature
    }

    // ------------------------------------------------------------------
    // Trait environments and obligations
    // ------------------------------------------------------------------

    /// Install the opened function parameter environment as body assumptions.
    pub fn set_solver_environment(&self, environment: CheckedParameterEnv<'db>) {
        use super::solve::Assumption;

        let (predicates, assumptions_complete) = self.elaborate_solver_predicates(environment);
        let assumptions: Vec<_> = predicates
            .into_iter()
            .map(|predicate| Assumption::TraitImpl {
                self_ty: predicate.self_ty,
                trait_ref: predicate.trait_ref,
            })
            .collect();
        let assumptions = Assumption::flatten(&mut self.target_stash.borrow_mut(), assumptions);
        *self.solver_assumptions.borrow_mut() = assumptions;
        *self.assumptions_complete.borrow_mut() = assumptions_complete;
    }

    /// Whether the current body may have trait-method providers introduced by
    /// its parameter environment that the first method-lookup slice does not
    /// enumerate by name.
    pub(crate) fn has_unhandled_method_bound_providers(&self, receiver_ty: Ptr<Ty<'db>>) -> bool {
        use super::solve::Assumption;

        if !*self.assumptions_complete.borrow() {
            return true;
        }
        let assumptions = self.target_stash.borrow()[*self.solver_assumptions.borrow()].to_vec();
        assumptions.into_iter().any(|assumption| match assumption {
            Assumption::TraitImpl { self_ty, .. } => self.types_may_unify(self_ty, receiver_ty),
            Assumption::NormalizesTo { .. } => false,
            Assumption::Implies(..) | Assumption::All(_) => true,
        })
    }

    fn types_may_unify(&self, left: Ptr<Ty<'db>>, right: Ptr<Ty<'db>>) -> bool {
        let mut egraph = self.egraph.borrow_mut();
        let mut stash = self.target_stash.borrow_mut();
        let transaction = egraph.branch_from(Version::ROOT);
        let result = try_unify(&mut egraph, &mut stash, transaction, left, right);
        egraph.discard(transaction);
        match result {
            Ok(_)
            | Err(UnifyError::OccursCheck { .. })
            | Err(UnifyError::UniverseLeak { .. })
            | Err(UnifyError::Reported(_)) => true,
            Err(UnifyError::Mismatch { .. } | UnifyError::DomainConflict { .. }) => false,
        }
    }

    fn elaborate_solver_predicates(
        &self,
        environment: CheckedParameterEnv<'db>,
    ) -> (Vec<crate::ty::WherePredicate<'db>>, bool) {
        use super::solve::IrCopier;
        use crate::ty::BinderExt;

        let mut complete = environment.solver_eligibility.is_eligible();
        let mut queue = self.target_stash.borrow()[environment.where_clauses].to_vec();
        let mut output = Vec::new();
        let mut seen = FxHashSet::default();

        while let Some(predicate) = queue.pop() {
            let key = {
                let source = self.target_stash.borrow();
                let mut target = Stash::new();
                let copied = predicate.stash_copy(&source, &mut target);
                Stashed::new(target, copied)
            };
            if !seen.insert(key) {
                continue;
            }
            output.push(predicate);

            let trait_sym = predicate.trait_ref.trait_sym;
            let Some(signature) = trait_sym.sig(self.db) else {
                complete = false;
                continue;
            };
            let (source, binder) = signature.open();
            if !binder.value.solver_eligibility.is_eligible() {
                complete = false;
                continue;
            }

            let mut mapping = FxHashMap::default();
            mapping.insert(binder.value.self_param, predicate.self_ty);
            for (generic, argument) in signature
                .iter_symbols()
                .filter(|generic| {
                    *generic != binder.value.self_param
                        && generic.kind(self.db) == crate::generic_param::GenericParamKind::Type
                })
                .zip(self.target_stash.borrow()[predicate.trait_ref.args].to_vec())
            {
                mapping.insert(generic, argument);
            }
            let defining = source[binder.value.where_clauses].to_vec();
            let mut target = self.target_stash.borrow_mut();
            let mut copier = IrCopier::new(source, &mut target, mapping, None);
            queue.extend(
                defining
                    .into_iter()
                    .map(|predicate| copier.copy_predicate(predicate)),
            );
        }

        output.reverse();
        (output, complete)
    }

    // ANCHOR: example_publish_parameter_env
    pub fn submit_parameter_env(
        &self,
        environment: CheckedParameterEnv<'db>,
        span: RelativeSpan,
        reason: ObligationReason,
    ) {
        let mut batch = StagedObligationBatch::new();
        batch.push_parameter_env(environment, span, reason);
        self.publish_obligation_batch(batch);
    }

    /// Publish obligations staged beside an inference transaction. A caller
    /// that rolls its inference child back simply drops the batch.
    pub fn publish_obligation_batch(&self, batch: StagedObligationBatch<'db>) {
        use super::solve::{Atom, Goal};

        for (environment, provenance) in batch.environments {
            let predicates = self.target_stash.borrow()[environment.where_clauses].to_vec();
            for predicate in predicates {
                self.submit_obligation_goal(
                    Goal::Atom(Atom::TraitImpl {
                        self_ty: predicate.self_ty,
                        trait_ref: predicate.trait_ref,
                    }),
                    provenance,
                    None,
                );
            }
            if environment.solver_eligibility == SolverEligibility::Unsupported {
                self.submit_obligation_goal(Goal::Maybe, provenance, None);
            }
        }
    }
    // ANCHOR_END: example_publish_parameter_env

    pub fn submit_trait_obligation(
        &self,
        self_ty: Ptr<Ty<'db>>,
        trait_ref: crate::ty::TraitRef<'db>,
        span: RelativeSpan,
        reason: ObligationReason,
    ) {
        self.submit_obligation_goal(
            super::solve::Goal::Atom(super::solve::Atom::TraitImpl { self_ty, trait_ref }),
            ObligationProvenance { span, reason },
            None,
        );
    }

    pub(crate) fn submit_operator_trait_obligation(
        &self,
        self_ty: Ptr<Ty<'db>>,
        trait_ref: crate::ty::TraitRef<'db>,
        span: RelativeSpan,
        slot: ExprSlot,
        watched_types: Vec<Ptr<Ty<'db>>>,
    ) {
        self.submit_operator_obligation(
            ObligationGoal::Prove(super::solve::Goal::Atom(super::solve::Atom::TraitImpl {
                self_ty,
                trait_ref,
            })),
            span,
            slot,
            watched_types,
        );
    }

    pub(crate) fn submit_operator_alias_relation(
        &self,
        alias: AliasTy<'db>,
        expected: Ptr<Ty<'db>>,
        span: RelativeSpan,
        slot: ExprSlot,
        watched_types: Vec<Ptr<Ty<'db>>>,
    ) {
        self.submit_operator_obligation(
            ObligationGoal::Normalize { alias, expected },
            span,
            slot,
            watched_types,
        );
    }

    fn submit_operator_obligation(
        &self,
        goal: ObligationGoal<'db>,
        span: RelativeSpan,
        slot: ExprSlot,
        watched_types: Vec<Ptr<Ty<'db>>>,
    ) {
        self.submit_obligation(
            goal,
            ObligationProvenance {
                span,
                reason: ObligationReason::BinaryOperator,
            },
            Some(ObligationRecovery {
                slot,
                span,
                watched_types,
            }),
        );
    }

    // ANCHOR: example_classify_fixed_trait_goal
    /// Evaluate a fixed-trait goal without asking the solver to discover a
    /// trait identity. This read-only classification is used while comparing
    /// method candidates; the first vertical slice accepts only answers with
    /// no caller-inference substitution and a trivial residual.
    pub(crate) fn classify_trait_goal(
        &self,
        self_ty: Ptr<Ty<'db>>,
        trait_ref: crate::ty::TraitRef<'db>,
    ) -> TraitGoalCertainty {
        use super::solve::{Atom, Goal, GoalQuery, QueryResultData};

        let local_crate = self
            .local_crate
            .expect("trait goals require a local crate context");
        let assumptions = *self.solver_assumptions.borrow();
        let assumptions_complete = *self.assumptions_complete.borrow();
        let (canonical, _) = self.canonicalize_obligation_goal(
            local_crate,
            assumptions_complete,
            assumptions,
            ObligationGoal::Prove(Goal::Atom(Atom::TraitImpl { self_ty, trait_ref })),
        );
        if canonical
            .mapping
            .inputs
            .iter()
            .any(|input| matches!(input, super::solve::CallerCanonicalVar::Existential(_)))
        {
            return TraitGoalCertainty::Maybe;
        }

        let response = GoalQuery::new(self.db, canonical.data).prove(self.db);
        match response.root().value {
            QueryResultData::Yes { modulo, .. } if modulo.is_trivially_true(response.stash()) => {
                TraitGoalCertainty::Yes
            }
            QueryResultData::Yes { .. } | QueryResultData::Maybe { .. } => {
                TraitGoalCertainty::Maybe
            }
            QueryResultData::No => TraitGoalCertainty::No,
        }
    }
    // ANCHOR_END: example_classify_fixed_trait_goal

    fn canonicalize_obligation_goal(
        &self,
        local_crate: crate::scope::LocalCrateSymbol<'db>,
        assumptions_complete: bool,
        assumptions: sage_stash::Slice<super::solve::Assumption<'db>>,
        goal: ObligationGoal<'db>,
    ) -> (
        super::solve::CanonicalizedGoal<'db>,
        Vec<crate::ty::InferVarIndex>,
    ) {
        let solver_goal = match goal {
            ObligationGoal::Prove(goal) => super::solve::SolverGoal::Prove(goal),
            ObligationGoal::Normalize { alias, .. } => super::solve::SolverGoal::Normalize(alias),
        };
        let canonical = super::solve::canonicalize_solver_goal(
            self.db,
            &self.target_stash.borrow(),
            &self.egraph.borrow(),
            Version::ROOT,
            local_crate,
            Universe(1),
            assumptions_complete,
            assumptions,
            solver_goal,
        );
        let stalled_on = canonical
            .mapping
            .inputs
            .iter()
            .filter_map(|input| match input {
                super::solve::CallerCanonicalVar::Existential(variable) => Some(*variable),
                super::solve::CallerCanonicalVar::Rigid(_) => None,
            })
            .collect();
        (canonical, stalled_on)
    }

    fn submit_obligation_goal(
        &self,
        goal: super::solve::Goal<'db>,
        provenance: ObligationProvenance,
        recovery: Option<ObligationRecovery<'db>>,
    ) {
        self.submit_obligation(ObligationGoal::Prove(goal), provenance, recovery);
    }

    fn submit_alias_relation(
        &self,
        alias: AliasTy<'db>,
        expected: Ptr<Ty<'db>>,
        provenance: ObligationProvenance,
    ) {
        self.submit_obligation(
            ObligationGoal::Normalize { alias, expected },
            provenance,
            None,
        );
    }

    fn submit_obligation(
        &self,
        goal: ObligationGoal<'db>,
        provenance: ObligationProvenance,
        recovery: Option<ObligationRecovery<'db>>,
    ) {
        let local_crate = self
            .local_crate
            .expect("solver obligations require a local crate context");
        let assumptions = *self.solver_assumptions.borrow();
        let assumptions_complete = *self.assumptions_complete.borrow();
        let (canonical, stalled_on) =
            self.canonicalize_obligation_goal(local_crate, assumptions_complete, assumptions, goal);

        let candidate = Obligation {
            goal,
            assumptions,
            assumptions_complete,
            local_crate,
            canonical_goal: canonical.data,
            mapping: canonical.mapping,
            provenance: vec![provenance],
            recoveries: recovery.into_iter().collect(),
            state: ObligationState::Ready,
            stalled_on,
            last_attempted_revision: None,
        };
        let mut manager = self.obligations.borrow_mut();
        if let Some(existing) = manager.obligations.iter_mut().find(|existing| {
            existing.state != ObligationState::Terminal && existing.same_dedup_key(&candidate)
        }) {
            if !existing.provenance.contains(&provenance) {
                existing.provenance.push(provenance);
            }
            return;
        }
        manager.obligations.push(candidate);
    }

    fn recanonicalize_obligation(&self, index: usize) -> u64 {
        let (goal, assumptions, assumptions_complete, local_crate) = {
            let manager = self.obligations.borrow();
            let obligation = &manager.obligations[index];
            (
                obligation.goal,
                obligation.assumptions,
                obligation.assumptions_complete,
                obligation.local_crate,
            )
        };
        let revision = self.egraph.borrow().semantic_revision(Version::ROOT);
        let (canonical, stalled_on) =
            self.canonicalize_obligation_goal(local_crate, assumptions_complete, assumptions, goal);
        let mut manager = self.obligations.borrow_mut();
        let obligation = &mut manager.obligations[index];
        obligation.canonical_goal = canonical.data;
        obligation.mapping = canonical.mapping;
        obligation.stalled_on = stalled_on;
        revision
    }

    fn process_ready_obligations(&self) {
        loop {
            self.deduplicate_pending_obligations();
            let ready = self.obligations.borrow().ready_indices();
            if ready.is_empty() {
                return;
            }
            for index in ready {
                self.attempt_obligation(index, false);
            }
        }
    }

    fn deduplicate_pending_obligations(&self) {
        let pending = self.obligations.borrow().pending_indices();
        for &index in &pending {
            self.recanonicalize_obligation(index);
        }
        let mut manager = self.obligations.borrow_mut();
        for (position, &index) in pending.iter().enumerate() {
            if manager.obligations[index].state == ObligationState::Terminal {
                continue;
            }
            for &duplicate in &pending[position + 1..] {
                if manager.obligations[duplicate].state == ObligationState::Terminal {
                    continue;
                }
                let equivalent = {
                    let left = &manager.obligations[index];
                    let right = &manager.obligations[duplicate];
                    left.same_dedup_key(right)
                };
                if equivalent {
                    let additional = manager.obligations[duplicate].provenance.clone();
                    for provenance in additional {
                        if !manager.obligations[index].provenance.contains(&provenance) {
                            manager.obligations[index].provenance.push(provenance);
                        }
                    }
                    manager.obligations[duplicate].state = ObligationState::Terminal;
                }
            }
        }
    }

    fn attempt_obligation(&self, index: usize, terminal: bool) {
        let revision = self.recanonicalize_obligation(index);
        if let Some(error) = self.obligation_recovery_error(index) {
            self.terminalize_obligation_with_error(index, revision, error);
            return;
        }
        let (goal, canonical_goal, mapping, unchanged) = {
            let manager = self.obligations.borrow();
            let obligation = &manager.obligations[index];
            (
                obligation.goal,
                obligation.canonical_goal.clone(),
                obligation.mapping.clone(),
                obligation.last_attempted_revision == Some(revision),
            )
        };
        match goal {
            ObligationGoal::Prove(_) => {}
            ObligationGoal::Normalize { alias, expected } => {
                let alias = self.alloc_ty(Ty::Alias(alias));
                if let Some(error) = self
                    .error_in_ty(alias)
                    .or_else(|| self.error_in_ty(expected))
                {
                    self.terminalize_obligation_with_error(index, revision, error);
                    return;
                }
            }
        }
        if unchanged && !terminal {
            self.obligations.borrow_mut().obligations[index].state = ObligationState::Stalled;
            return;
        }

        match goal {
            ObligationGoal::Prove(_) => {
                self.attempt_proof_obligation(index, terminal, canonical_goal, mapping)
            }
            ObligationGoal::Normalize { expected, .. } => self.attempt_normalization_obligation(
                index,
                terminal,
                canonical_goal,
                mapping,
                expected,
            ),
        }
    }

    fn attempt_proof_obligation(
        &self,
        index: usize,
        terminal: bool,
        canonical_goal: Stashed<super::solve::GoalQueryData<'db>>,
        mapping: super::solve::CanonicalMapping<'db>,
    ) {
        use super::solve::{AppliedCertainty, GoalQuery, apply_query_response};

        // ANCHOR: example_run_trait_query
        let response = GoalQuery::new(self.db, canonical_goal.clone()).prove(self.db);
        let applied = {
            let mut stash = self.target_stash.borrow_mut();
            let mut egraph = self.egraph.borrow_mut();
            apply_query_response(
                self.db,
                &mut stash,
                &mut egraph,
                Version::ROOT,
                &canonical_goal,
                &mapping,
                &response,
            )
        };
        // ANCHOR_END: example_run_trait_query
        let Ok(applied) = applied else {
            self.fail_obligation(
                index,
                "trait obligation was disproved by incompatible inference",
            );
            return;
        };
        let post_revision = applied.effects.semantic_revision;
        let wakes = applied.effects.wakes.clone();
        self.publish_commit_effects(applied.effects);

        match applied.certainty {
            AppliedCertainty::No => {
                self.fail_obligation(index, "trait obligation is not satisfied");
            }
            AppliedCertainty::Maybe => {
                if terminal {
                    self.fail_obligation(index, "trait obligation could not be resolved");
                    return;
                }
                self.recanonicalize_obligation(index);
                let mut manager = self.obligations.borrow_mut();
                let obligation = &mut manager.obligations[index];
                obligation.last_attempted_revision = Some(post_revision);
                obligation.state = if obligation
                    .stalled_on
                    .iter()
                    .any(|variable| wakes.contains(variable))
                {
                    ObligationState::Ready
                } else {
                    ObligationState::Stalled
                };
            }
            AppliedCertainty::Yes { modulo, .. } => {
                if modulo.is_trivially_true(&self.target_stash.borrow()) {
                    let mut manager = self.obligations.borrow_mut();
                    manager.obligations[index].state = ObligationState::Terminal;
                    manager.obligations[index].last_attempted_revision = Some(post_revision);
                } else if terminal {
                    self.fail_obligation(index, "trait obligation remains conditional");
                } else {
                    {
                        let mut manager = self.obligations.borrow_mut();
                        let obligation = &mut manager.obligations[index];
                        obligation.goal = ObligationGoal::Prove(modulo);
                        obligation.last_attempted_revision = Some(post_revision);
                        obligation.state = ObligationState::Stalled;
                    }
                    self.recanonicalize_obligation(index);
                }
            }
        }
    }

    fn attempt_normalization_obligation(
        &self,
        index: usize,
        terminal: bool,
        canonical_goal: Stashed<super::solve::GoalQueryData<'db>>,
        mapping: super::solve::CanonicalMapping<'db>,
        expected: Ptr<Ty<'db>>,
    ) {
        use super::solve::GoalQuery;

        let response = GoalQuery::new(self.db, canonical_goal.clone()).solve(self.db);
        let progress =
            self.apply_normalization_relation(&canonical_goal, &mapping, &response, expected);
        let revision = self.egraph.borrow().semantic_revision(Version::ROOT);
        match progress {
            NormalizationProgress::No => {
                self.fail_obligation(index, "associated type relation is not satisfied");
            }
            NormalizationProgress::Maybe => {
                if terminal {
                    self.fail_obligation(index, "associated type could not be normalized");
                    return;
                }
                self.recanonicalize_obligation(index);
                let mut manager = self.obligations.borrow_mut();
                let obligation = &mut manager.obligations[index];
                obligation.last_attempted_revision = Some(revision);
                obligation.state = ObligationState::Stalled;
            }
            NormalizationProgress::Yes => {
                let mut manager = self.obligations.borrow_mut();
                let obligation = &mut manager.obligations[index];
                obligation.state = ObligationState::Terminal;
                obligation.last_attempted_revision = Some(revision);
            }
            NormalizationProgress::Residual(modulo) => {
                if terminal {
                    self.fail_obligation(
                        index,
                        "associated type normalization remains conditional",
                    );
                    return;
                }
                {
                    let mut manager = self.obligations.borrow_mut();
                    let obligation = &mut manager.obligations[index];
                    obligation.goal = ObligationGoal::Prove(modulo);
                    obligation.last_attempted_revision = Some(revision);
                    obligation.state = ObligationState::Stalled;
                }
                self.recanonicalize_obligation(index);
            }
        }
    }

    fn apply_normalization_relation(
        &self,
        canonical_goal: &Stashed<super::solve::GoalQueryData<'db>>,
        mapping: &super::solve::CanonicalMapping<'db>,
        response: &Stashed<super::solve::QueryResult<'db>>,
        expected: Ptr<Ty<'db>>,
    ) -> NormalizationProgress<'db> {
        use super::solve::{AppliedCertainty, GoalOutput, apply_query_response};

        let mut stash = self.target_stash.borrow_mut();
        let mut egraph = self.egraph.borrow_mut();
        let transaction = egraph.branch_from(Version::ROOT);
        let Ok(applied) = apply_query_response(
            self.db,
            &mut stash,
            &mut egraph,
            transaction,
            canonical_goal,
            mapping,
            response,
        ) else {
            egraph.discard(transaction);
            return NormalizationProgress::No;
        };
        let (output, modulo) = match applied.certainty {
            AppliedCertainty::Yes {
                output: GoalOutput::Type(output),
                modulo,
            } => (output, modulo),
            AppliedCertainty::Yes {
                output: GoalOutput::Proven,
                ..
            } => unreachable!("normalization returned a proof output"),
            AppliedCertainty::Maybe => {
                egraph.discard(transaction);
                return NormalizationProgress::Maybe;
            }
            AppliedCertainty::No => {
                egraph.discard(transaction);
                return NormalizationProgress::No;
            }
        };
        match try_unify(&mut egraph, &mut stash, transaction, output, expected) {
            Ok(_) => {}
            Err(
                UnifyError::Mismatch { .. }
                | UnifyError::OccursCheck { .. }
                | UnifyError::UniverseLeak { .. }
                | UnifyError::DomainConflict { .. }
                | UnifyError::Reported(_),
            ) => {
                egraph.discard(transaction);
                return NormalizationProgress::No;
            }
        }
        let complete = modulo.is_trivially_true(&stash);
        let effects = egraph.collapse_into(transaction, Version::ROOT);
        drop(egraph);
        drop(stash);
        self.publish_commit_effects(effects);
        if complete {
            NormalizationProgress::Yes
        } else {
            NormalizationProgress::Residual(modulo)
        }
    }

    fn fail_obligation(&self, index: usize, message: &str) {
        let (provenances, recoveries) = {
            let mut manager = self.obligations.borrow_mut();
            let obligation = &mut manager.obligations[index];
            if obligation.state == ObligationState::Terminal {
                return;
            }
            obligation.state = ObligationState::Terminal;
            (obligation.provenance.clone(), obligation.recoveries.clone())
        };
        let Some(first) = provenances.first().copied() else {
            return;
        };
        let mut diagnostic = Diagnostic::error(self.span(first.span), message)
            .label(self.span(first.span), first.reason.description());
        for provenance in provenances.into_iter().skip(1) {
            diagnostic =
                diagnostic.secondary(self.span(provenance.span), provenance.reason.description());
        }
        let error = self.record(diagnostic);
        self.recover_expr_slots(&recoveries, error);
    }

    fn obligation_recovery_error(&self, index: usize) -> Option<ErrorReported> {
        let recoveries = self.obligations.borrow().obligations[index]
            .recoveries
            .clone();
        for recovery in &recoveries {
            let expression = self.expr_slots.borrow()[recovery.slot.0 as usize]
                .expect("obligation expression slot must be filled before solving");
            match self.target_stash.borrow()[expression].data {
                TyExprData::Error(error) => return Some(error),
                TyExprData::Literal(_)
                | TyExprData::Path(_)
                | TyExprData::Block(_, _)
                | TyExprData::Call(_, _)
                | TyExprData::ResolvedCall(_, _)
                | TyExprData::MethodCall(_, _, _)
                | TyExprData::Field(_, _)
                | TyExprData::Binary(_, _, _)
                | TyExprData::Unary(_, _)
                | TyExprData::Deref(_)
                | TyExprData::Ref(_, _)
                | TyExprData::NeverToAny(_)
                | TyExprData::If(_, _, _)
                | TyExprData::IfLet(_, _, _, _)
                | TyExprData::Match(_, _)
                | TyExprData::Loop(_)
                | TyExprData::While(_, _)
                | TyExprData::WhileLet(_, _, _)
                | TyExprData::For(_, _, _)
                | TyExprData::Break(_)
                | TyExprData::Continue
                | TyExprData::Return(_)
                | TyExprData::Assign(_, _)
                | TyExprData::Await(_)
                | TyExprData::Try(_)
                | TyExprData::Closure(_, _)
                | TyExprData::Tuple(_)
                | TyExprData::Array(_)
                | TyExprData::Index(_, _)
                | TyExprData::Cast(_, _)
                | TyExprData::StructLit(_, _)
                | TyExprData::Range(_, _)
                | TyExprData::MacroCall(_, _)
                | TyExprData::Unresolved(_)
                | TyExprData::Missing => {}
            }
            if let Some(error) = recovery
                .watched_types
                .iter()
                .copied()
                .find_map(|ty| self.error_in_ty(ty))
            {
                return Some(error);
            }
        }
        None
    }

    fn terminalize_obligation_with_error(&self, index: usize, revision: u64, error: ErrorReported) {
        let recoveries = {
            let mut manager = self.obligations.borrow_mut();
            let obligation = &mut manager.obligations[index];
            obligation.state = ObligationState::Terminal;
            obligation.last_attempted_revision = Some(revision);
            obligation.recoveries.clone()
        };
        self.recover_expr_slots(&recoveries, error);
    }

    fn recover_expr_slots(&self, recoveries: &[ObligationRecovery<'db>], error: ErrorReported) {
        let error_ty = self.alloc_ty(Ty::Error(error));
        for recovery in recoveries {
            let original = self.expr_slots.borrow()[recovery.slot.0 as usize]
                .expect("obligation expression slot must be filled before solving");
            let original_ty = self.target_stash.borrow()[original].ty;
            let root = self.find(original_ty);
            if matches!(self.target_stash.borrow()[root], Ty::InferVar(_)) {
                let effects = install_error_recovery(
                    &mut self.egraph.borrow_mut(),
                    &mut self.target_stash.borrow_mut(),
                    Version::ROOT,
                    root,
                    error_ty,
                )
                .expect("obligation recovery must accept a reported error type");
                self.publish_commit_effects(effects);
            }
            let expression = self.alloc_expr(TyExprData::Error(error), error_ty, recovery.span);
            self.fill_expr_slot(recovery.slot, expression);
        }
    }

    fn terminally_discharge_obligations(&self) {
        self.deduplicate_pending_obligations();
        let pending = self.obligations.borrow().pending_indices();
        for index in pending {
            self.attempt_obligation(index, true);
        }
        assert!(
            !self.obligations.borrow().has_pending(),
            "body checking finished with a pending trait obligation"
        );
    }

    // ------------------------------------------------------------------
    // Finalization
    // ------------------------------------------------------------------

    fn default_deferred_never_targets(&self, only: Option<&[InferVarIndex]>) {
        let deferred_never_targets = self.deferred_never_targets.borrow().clone();
        if deferred_never_targets.is_empty() {
            return;
        }

        let never = self.target_stash.borrow_mut().alloc(Ty::Never);
        for target in deferred_never_targets {
            let target = self.egraph.borrow_mut().find_mut(Version::ROOT, target);
            let Ty::InferVar(variable) = self.target_stash.borrow()[target] else {
                continue;
            };
            if only.is_some_and(|variables| !variables.contains(&variable)) {
                continue;
            }
            if !matches!(
                self.egraph.borrow().get_bound(Version::ROOT, target),
                Bound::None
            ) {
                continue;
            }

            let effects = try_unify(
                &mut self.egraph.borrow_mut(),
                &mut self.target_stash.borrow_mut(),
                Version::ROOT,
                target,
                never,
            )
            .expect("deferred never fallback must bind an unconstrained variable");
            self.publish_commit_effects(effects);
        }
    }

    pub fn finalize(&self) {
        // First consume every proof that current inference already enables.
        self.process_ready_obligations();

        self.default_deferred_never_targets(None);

        let mut unresolved_vars = Vec::new();

        let variables: Vec<_> = self
            .egraph
            .borrow()
            .version_tree()
            .visible_variables(Version::ROOT)
            .collect();
        for idx in variables {
            let ty = self.target_stash.borrow_mut().alloc(Ty::InferVar(idx));
            let canon = self.egraph.borrow_mut().find_mut(Version::ROOT, ty);

            if canon != ty {
                continue;
            }

            let bound = self.egraph.borrow().get_bound(Version::ROOT, ty);
            match bound {
                Bound::None => {
                    unresolved_vars.push((ty, idx));
                }
                Bound::AtLeast(bound_ty) => {
                    let effects = try_unify(
                        &mut self.egraph.borrow_mut(),
                        &mut self.target_stash.borrow_mut(),
                        Version::ROOT,
                        ty,
                        bound_ty,
                    )
                    .expect("final bound must be structurally valid");
                    self.publish_commit_effects(effects);
                }
                Bound::Exactly(_) => {}
            }
        }

        for (ty, idx) in unresolved_vars {
            let root = self.egraph.borrow_mut().find_mut(Version::ROOT, ty);
            if root != ty || !matches!(self.target_stash.borrow()[root], Ty::InferVar(_)) {
                continue;
            }
            let span = RelativeSpan { start: 0, end: 0 };
            let domain = self
                .egraph
                .borrow()
                .effective_domain(Version::ROOT, &self.target_stash.borrow(), ty)
                .expect("unresolved inference class must retain a domain");
            let err = TypeError::Fresh {
                kind: TypeErrorKind::UnresolvedInferVar { var: idx, domain },
                span,
                context: Vec::new(),
            };
            let e = self.catch(err);

            let error_ty = self.target_stash.borrow_mut().alloc(Ty::Error(e));
            let effects = install_error_recovery(
                &mut self.egraph.borrow_mut(),
                &mut self.target_stash.borrow_mut(),
                Version::ROOT,
                ty,
                error_ty,
            )
            .expect("error recovery must accept a reported error type");
            self.publish_commit_effects(effects);

            // A diagnostic for one unresolved input may invalidate a retained
            // expression and bind its synthetic result to the same error. Do
            // that before diagnosing the next variable so the dependent
            // result does not produce a second, cascading inference error.
            self.process_ready_obligations();
        }

        // Fallback and final bounds may make retained goals concrete. Give
        // those goals one ordinary retry before the mandatory terminal pass.
        self.process_ready_obligations();
        self.terminally_discharge_obligations();

        self.flush_and_drain();
        self.runtime.borrow_mut().wake_all();
        self.runtime.borrow_mut().drain();
    }

    // ------------------------------------------------------------------
    // Finish — consumes self, produces CheckedBody
    // ------------------------------------------------------------------

    pub fn finish(self, root: Ptr<TyExpr<'db>>, span: RelativeSpan) -> CheckedBody<'db> {
        assert!(
            !self.obligations.borrow().has_pending(),
            "body finished with a pending trait obligation"
        );
        assert!(
            self.egraph.borrow().version_tree().is_leaf(Version::ROOT),
            "body finished with a live inference branch"
        );
        assert!(
            self.runtime.borrow().is_quiescent(),
            "body finished with a live inference task"
        );
        assert!(
            self.pending_wakes.borrow().is_empty(),
            "body finished with unpublished inference wakes"
        );
        let local_vars = self.local_vars.into_inner();
        let expr_slots: Vec<_> = self
            .expr_slots
            .into_inner()
            .into_iter()
            .enumerate()
            .map(|(index, expression)| {
                expression.unwrap_or_else(|| panic!("expression slot {index} was never filled"))
            })
            .collect();
        let source = self.target_stash.into_inner();
        let egraph = self.egraph.into_inner();
        CheckedBody {
            body: super::finalize::finalize_typed_body(
                &source,
                &egraph,
                root,
                &expr_slots,
                &local_vars,
                span,
            ),
            diagnostics: self.diagnostics.into_inner(),
        }
    }
}

// ---------------------------------------------------------------------------
// Scope — task-local context (resolver ribs + visible locals)
// ---------------------------------------------------------------------------

/// Task-local scope — passed by `&Scope` (shared reference) to expression
/// checkers. Block-level code that introduces bindings clones the scope locally.
#[derive(Clone)]
pub struct Scope<'db> {
    pub resolver: Resolver<'db>,
    pub locals: Vec<Ptr<Ty<'db>>>,
}

impl<'db> Scope<'db> {
    pub fn new(resolver: Resolver<'db>) -> Self {
        Self {
            resolver,
            locals: Vec::new(),
        }
    }

    pub fn local_type(&self, id: u32) -> Ptr<Ty<'db>> {
        self.locals[id as usize]
    }

    pub fn add_binding(
        &mut self,
        cx: &InferCtx<'_, 'db>,
        name: Name<'db>,
        span: RelativeSpan,
    ) -> LocalId {
        let local_var_count = cx.local_vars.borrow().len();
        let id = LocalId(local_var_count as u32);
        cx.add_local_var(LocalVar { name, span });
        let var = cx.fresh_ty_var();
        self.locals.push(var);
        self.resolver
            .ribs
            .add(name, Namespace::Value, Resolution::Local(id));
        id
    }

    pub fn push_local(&mut self, ty: Ptr<Ty<'db>>) -> u32 {
        let id = self.locals.len() as u32;
        self.locals.push(ty);
        id
    }

    pub fn bind_params(
        &mut self,
        cx: &InferCtx<'_, 'db>,
        param_tys: sage_stash::Slice<Ptr<Ty<'db>>>,
        params_cst: sage_stash::Slice<crate::cst::fns::ParamCst<'db>>,
    ) {
        let stash = cx.stash();
        let params_list = cx.source_stash[params_cst].to_vec();
        let param_ty_list = stash[param_tys].to_vec();
        drop(stash);

        for (param_cst, param_ty) in params_list.iter().zip(param_ty_list.iter()) {
            if let Some(name) = param_cst.name {
                let local_var_count = cx.local_vars.borrow().len();
                let id = LocalId(local_var_count as u32);
                cx.add_local_var(LocalVar {
                    name,
                    span: param_cst.span,
                });
                self.locals.push(*param_ty);
                self.resolver
                    .ribs
                    .add(name, Namespace::Value, Resolution::Local(id));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Path resolution helper
// ---------------------------------------------------------------------------

pub fn resolve_path<'db>(
    cx: &InferCtx<'_, 'db>,
    scope: &Scope<'db>,
    path: crate::cst::paths::Path<'db>,
    ns: Namespace,
    span: RelativeSpan,
) -> Res<'db> {
    let mut resolver = scope.resolver.clone();
    let results = resolver.resolve_path(cx.source_stash, path, ns);
    if results.len() > 1 {
        let err = TypeError::Fresh {
            kind: TypeErrorKind::AmbiguousName {
                count: results.len(),
            },
            span,
            context: Vec::new(),
        };
        let e = cx.catch(err);
        return Res::Error(e);
    }
    match results.into_iter().next() {
        Some(Resolution::Sym(sym)) => Res::Def(sym),
        Some(Resolution::Local(id)) => Res::Local(id),
        Some(Resolution::Param(_) | Resolution::SelfTy(_)) | None => {
            let e = cx.record(Diagnostic::error(cx.span(span), "unresolved name"));
            Res::Error(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::local_syms::mods::{LocalModSym, ModBodySource};
    use crate::parse::parse_str_to_cst;
    use crate::scope::{ScopeSymbol, local_crate};
    use crate::source::SourceFile;
    use crate::span::{AbsoluteSpan, ParseSource};
    use crate::symbol::{ModSymbol, SymbolData, TraitSymbol};
    use crate::ty::{ProjectionTy, TraitItemDef, TraitRef};
    use sage_stash::{Stash, Stashed};
    use salsa::Database as _;

    fn make_cx(stash: &Stash) -> InferCtx<'_, 'static> {
        InferCtx::new(leak_db(), stash, None)
    }

    fn apply_alias_relation<'db>(
        cx: &InferCtx<'_, 'db>,
        alias: AliasTy<'db>,
        expected: Ptr<Ty<'db>>,
    ) -> NormalizationProgress<'db> {
        use crate::check::solve::{GoalQuery, SolverGoal, canonicalize_solver_goal};

        let canonical = canonicalize_solver_goal(
            cx.db,
            &cx.target_stash.borrow(),
            &cx.egraph.borrow(),
            Version::ROOT,
            cx.local_crate.unwrap(),
            Universe(1),
            *cx.assumptions_complete.borrow(),
            *cx.solver_assumptions.borrow(),
            SolverGoal::Normalize(alias),
        );
        let response = GoalQuery::new(cx.db, canonical.data.clone()).solve(cx.db);
        cx.apply_normalization_relation(&canonical.data, &canonical.mapping, &response, expected)
    }

    fn leak_db() -> &'static dyn crate::Db {
        let db = Box::new(Database::default());
        Box::leak(db)
    }

    #[salsa::tracked]
    fn setup_root_module<'db>(
        db: &'db dyn crate::Db,
        source_file: SourceFile,
    ) -> (LocalCrateSymbol<'db>, ModSymbol<'db>) {
        let mut empty_stash = Stash::new();
        let empty_attrs = empty_stash.alloc_slice::<crate::cst::attrs::AttrCst>(&[]);
        let empty_attrs = Stashed::new(empty_stash, empty_attrs);
        let source = ParseSource::SourceFile(source_file);
        let root = LocalModSym::new(
            db,
            Name::new(db, String::new()),
            crate::scope::Edition::Rust2021,
            None,
            ModBodySource::File(source_file),
            empty_attrs,
            AbsoluteSpan {
                source,
                start: 0,
                end: source_file.text(db).len() as u32,
            },
        );
        let krate = local_crate(db, root);
        let items = parse_str_to_cst(db, source, source_file.text(db), ScopeSymbol::Crate(krate));
        crate::local_syms::mods::unexpanded_items::specify(db, root, items);
        (krate, ModSymbol::Local(root))
    }

    #[test]
    fn await_concrete_already_resolved() {
        let stash = Stash::new();
        let cx = make_cx(&stash);
        let ty = cx.alloc_ty(Ty::Bool);

        let result = cx.block_on(async { cx.await_concrete(ty).await });
        assert_eq!(cx.stash()[result], Ty::Bool);
    }

    #[test]
    fn await_concrete_resolved_by_background_task() {
        let stash = Stash::new();
        let cx = make_cx(&stash);

        // Create an inference variable
        let var = cx.fresh_ty_var();
        let bool_ty = cx.alloc_ty(Ty::Bool);

        // For this test, resolve the variable directly after creating the
        // block_on future to demonstrate the await_concrete → wake → resolve path.
        // We simulate: a background obligation resolves var to bool.
        // The mechanism: require_eq pushes to pending_wakes, flush_and_drain
        // processes them, main task re-polls and finds the type concrete.

        // First, set up the test: main future awaits var, which is initially InferVar.
        // Before the first poll we resolve it so the re-poll finds it concrete.
        let result = cx.block_on(async {
            // Resolve the var BEFORE await_concrete — proves immediate resolution path
            cx.require_eq(var, bool_ty, RelativeSpan { start: 0, end: 0 })
                .unwrap();
            cx.await_concrete(var).await
        });

        let resolved = cx.stash()[result];
        assert_eq!(resolved, Ty::Bool);
    }

    #[test]
    fn block_on_with_pending_wakes() {
        let stash = Stash::new();
        let cx = make_cx(&stash);

        let var = cx.fresh_ty_var();
        let u32_ty = cx.alloc_ty(Ty::Int(crate::ty::IntTy::I32));

        // Demonstrate: require_eq pushes to pending_wakes, which get flushed
        cx.require_eq(var, u32_ty, RelativeSpan { start: 0, end: 0 })
            .unwrap();

        // pending_wakes should have an entry
        assert!(!cx.pending_wakes.borrow().is_empty());

        // block_on flushes them
        cx.block_on(async {});

        // After block_on, pending_wakes are flushed
        assert!(cx.pending_wakes.borrow().is_empty());
    }

    #[test]
    fn relate_alias_commits_success_and_rolls_back_partial_mismatch() {
        let mut database = Database::default();
        let source_file = database.add_source_file(
            "lib.rs".to_owned(),
            "trait Iterable { type Item; }\n\
             impl Iterable for bool { type Item = (bool, bool); }"
                .to_owned(),
        );
        database.attach(|db| {
            let (krate, root) = setup_root_module(db, source_file);
            let iterable = root
                .expanded_module_items(db)
                .iter()
                .find_map(|symbol| match symbol.data(db) {
                    SymbolData::TraitSymbol(TraitSymbol::Local(trait_sym)) => Some(trait_sym),
                    SymbolData::TraitSymbol(TraitSymbol::Ext(_))
                    | SymbolData::StructSymbol(_)
                    | SymbolData::FnSymbol(_)
                    | SymbolData::EnumSymbol(_)
                    | SymbolData::VariantSymbol(_)
                    | SymbolData::VariantCtorSymbol(_)
                    | SymbolData::TypeAliasSymbol(_)
                    | SymbolData::ConstSymbol(_)
                    | SymbolData::StaticSymbol(_)
                    | SymbolData::ImplSymbol(_)
                    | SymbolData::ModSymbol(_)
                    | SymbolData::MacroDefSymbol(_)
                    | SymbolData::UseSymbol(_)
                    | SymbolData::IntrinsicTypeSymbol(_)
                    | SymbolData::MacroInvocationSymbol(_) => None,
                })
                .expect("test trait should exist");
            let items = iterable.items(db);
            let (item_stash, items) = items.open();
            let associated_ty = item_stash[items.value]
                .iter()
                .find_map(|item| match item {
                    TraitItemDef::Type(associated_ty) => Some(*associated_ty),
                    TraitItemDef::Function(_) | TraitItemDef::Const(_) => None,
                })
                .expect("test associated type should exist");

            let source_stash = Stash::new();
            let mut cx = InferCtx::new(db, &source_stash, None);
            cx.local_crate = Some(krate);
            let bool_ty = cx.alloc_ty(Ty::Bool);
            let char_ty = cx.alloc_ty(Ty::Char);
            let empty_args = cx.stash_mut().alloc_slice(&[]);
            let alias = AliasTy::Associated(ProjectionTy {
                associated_ty,
                self_ty: bool_ty,
                trait_ref: TraitRef {
                    trait_sym: TraitSymbol::Local(iterable),
                    args: empty_args,
                },
                args: empty_args,
            });

            let caller_var = cx.fresh_ty_var();
            let mismatch_elements = cx.stash_mut().alloc_slice(&[caller_var, char_ty]);
            let mismatch = cx.alloc_ty(Ty::Tuple(mismatch_elements));
            let revision = cx.egraph.borrow().semantic_revision(Version::ROOT);
            assert!(matches!(
                apply_alias_relation(&cx, alias, mismatch),
                NormalizationProgress::No
            ));
            assert_eq!(cx.get_bound(caller_var), Bound::None);
            assert_eq!(
                cx.egraph.borrow().semantic_revision(Version::ROOT),
                revision,
                "a failed relation must discard its partial caller binding"
            );

            let expected_elements = cx.stash_mut().alloc_slice(&[caller_var, bool_ty]);
            let expected = cx.alloc_ty(Ty::Tuple(expected_elements));
            assert!(matches!(
                apply_alias_relation(&cx, alias, expected),
                NormalizationProgress::Yes
            ));
            assert_eq!(cx.get_bound(caller_var), Bound::Exactly(bool_ty));
        });
    }
}
