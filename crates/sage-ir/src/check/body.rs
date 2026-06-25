use std::ops::{Deref, DerefMut};

use sage_stash::{Ptr, Slice, Stash, StashCopy, Stashed};

use crate::check::Check;
use crate::diagnostic::{Diagnostic, ErrorReported, Span};
use crate::display::TyDisplay;
use crate::local_syms::LocalModItemSym;
use crate::name::Name;
use crate::resolve::{Namespace, Resolution, Resolver};
use crate::span::RelativeSpan;
use crate::ty::{Binder, FnSig, InferVarIndex, Ty};
use crate::tytree::*;
use crate::tytree::{LocalId, LocalVar};

use super::infer::bound::Bound;
use super::infer::egraph::VersionedEGraph;
use super::infer::runtime::Runtime;
use super::infer::version::{Universe, VarInfo, Version};

// ---------------------------------------------------------------------------
// TypeError — the structured error that flows through Result during checking
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct TypeError<'db> {
    pub kind: TypeErrorKind<'db>,
    pub span: RelativeSpan,
    pub context: Vec<ErrorContext>,
}

#[derive(Clone, Debug)]
pub enum TypeErrorKind<'db> {
    Mismatch {
        expected: Ptr<Ty<'db>>,
        actual: Ptr<Ty<'db>>,
    },
    UnresolvedInferVar {
        var: InferVarIndex,
    },
    AmbiguousName {
        count: usize,
    },
}

/// Contextual information about *why* a type constraint was required.
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

impl<'db> TypeError<'db> {
    pub fn with_context(mut self, context: ErrorContext) -> Self {
        self.context.push(context);
        self
    }

    pub fn to_diagnostic(&self, cx: &BodyCheck<'_, 'db>) -> Diagnostic<'db> {
        let span = cx.span(self.span);
        match &self.kind {
            TypeErrorKind::Mismatch { expected, actual } => {
                let expected_str = TyDisplay::new(cx.db, cx.stash(), *expected).to_string();
                let actual_str = TyDisplay::new(cx.db, cx.stash(), *actual).to_string();
                let msg = format!(
                    "type mismatch: expected `{}`, found `{}`",
                    expected_str, actual_str,
                );
                let mut diag = Diagnostic::error(span.clone(), &msg)
                    .label(span, format!("found `{actual_str}`"));

                for ctx in &self.context {
                    match ctx {
                        ErrorContext::ReturnType { ret_span } => {
                            diag = diag.secondary(
                                cx.span(*ret_span),
                                format!("expected `{expected_str}` because of return type"),
                            );
                        }
                        ErrorContext::Argument { index, call_span } => {
                            diag = diag.secondary(
                                cx.span(*call_span),
                                format!("expected `{expected_str}` for argument {}", index + 1),
                            );
                        }
                        ErrorContext::FieldInit { field_span } => {
                            diag = diag.secondary(
                                cx.span(*field_span),
                                format!("expected `{expected_str}` for this field"),
                            );
                        }
                    }
                }

                diag
            }
            TypeErrorKind::UnresolvedInferVar { var } => {
                Diagnostic::error(span, format!("could not infer type for ?{}", var.0))
            }
            TypeErrorKind::AmbiguousName { count } => {
                Diagnostic::error(span, format!("ambiguous name: {count} candidates"))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// BodyCheck
// ---------------------------------------------------------------------------

/// Unified body-checking context: resolves names and infers types in a
/// single CST walk, producing `TyExpr` nodes directly into the egraph's
/// stash.
pub struct BodyCheck<'a, 'db> {
    pub(crate) check: Check<'a, 'db>,

    // Inference engine
    pub db: &'db dyn crate::Db,
    pub egraph: VersionedEGraph<'db>,
    pub runtime: Runtime,
    current_universe: Universe,

    // Body state
    locals: Vec<Ptr<Ty<'db>>>,
    local_vars: Vec<LocalVar<'db>>,
    infer_var_ptrs: Vec<Ptr<Ty<'db>>>,
    diagnostics: Vec<Diagnostic<'db>>,

    /// The item being checked — used to anchor relative spans.
    current_sym: LocalModItemSym<'db>,
}

impl<'a, 'db> DerefMut for BodyCheck<'a, 'db> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.check
    }
}

impl<'a, 'db> Deref for BodyCheck<'a, 'db> {
    type Target = Check<'a, 'db>;

    fn deref(&self) -> &Self::Target {
        &self.check
    }
}

impl<'a, 'db> BodyCheck<'a, 'db> {
    pub fn new(
        db: &'db dyn crate::Db,
        src: &'a Stash,
        resolver: Resolver<'db>,
        current_sym: LocalModItemSym<'db>,
    ) -> Self {
        Self {
            check: Check::new(db, src, resolver),
            db,
            egraph: VersionedEGraph::new(),
            runtime: Runtime::new(),
            current_universe: Universe(1),
            locals: Vec::new(),
            local_vars: Vec::new(),
            infer_var_ptrs: Vec::new(),
            diagnostics: Vec::new(),
            current_sym,
        }
    }

    // ------------------------------------------------------------------
    // Span helpers
    // ------------------------------------------------------------------

    pub fn span(&self, relative: RelativeSpan) -> Span<'db> {
        Span::Relative(self.current_sym, relative)
    }

    // ------------------------------------------------------------------
    // Stash access
    // ------------------------------------------------------------------

    pub fn stash(&self) -> &Stash {
        &self.target_stash
    }

    pub fn stash_mut(&mut self) -> &mut Stash {
        &mut self.target_stash
    }

    // ------------------------------------------------------------------
    // Path resolution
    // ------------------------------------------------------------------

    pub fn resolve_path(
        &mut self,
        path: crate::cst::paths::Path<'db>,
        ns: Namespace,
        span: RelativeSpan,
    ) -> Res<'db> {
        let check = &mut self.check;
        let results = check.resolver.resolve_path(check.source_stash, path, ns);
        if results.len() > 1 {
            let err = TypeError {
                kind: TypeErrorKind::AmbiguousName {
                    count: results.len(),
                },
                span,
                context: Vec::new(),
            };
            let e = self.catch(err);
            return Res::Error(e);
        }
        match results.into_iter().next() {
            Some(Resolution::Sym(sym)) => Res::Def(sym),
            Some(Resolution::Local(id)) => Res::Local(id),
            Some(Resolution::Param(_) | Resolution::SelfTy(_) | Resolution::Error) | None => {
                let e = self.report(Diagnostic::error(self.span(span), "unresolved name"));
                Res::Error(e)
            }
        }
    }

    // ------------------------------------------------------------------
    // Locals
    // ------------------------------------------------------------------

    pub fn add_binding(&mut self, name: Name<'db>, span: RelativeSpan) -> LocalId {
        let id = LocalId(self.local_vars.len() as u32);
        self.local_vars.push(LocalVar { name, span });
        let var = self.fresh_ty_var();
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

    pub fn local_type(&self, id: u32) -> Ptr<Ty<'db>> {
        self.locals[id as usize]
    }

    // ------------------------------------------------------------------
    // Variable allocation
    // ------------------------------------------------------------------

    pub fn fresh_ty_var(&mut self) -> Ptr<Ty<'db>> {
        let ty = self.fresh_ty_var_data();
        let ptr = self.target_stash.alloc(ty);
        self.infer_var_ptrs.push(ptr);
        ptr
    }

    pub fn fresh_ty_var_data(&mut self) -> Ty<'db> {
        let universe = self.current_universe;
        let idx = self.egraph.alloc_var(VarInfo { universe });
        Ty::InferVar(idx)
    }

    // ------------------------------------------------------------------
    // Type allocation
    // ------------------------------------------------------------------

    pub fn unit_ty(&mut self) -> Ptr<Ty<'db>> {
        let elems = self.target_stash.alloc_slice(&[]);
        self.target_stash.alloc(Ty::Tuple(elems))
    }

    // ------------------------------------------------------------------
    // Egraph operations
    // ------------------------------------------------------------------

    pub fn find(&self, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        self.egraph.find(ty)
    }

    pub fn find_mut(&mut self, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        self.egraph.find_mut(ty)
    }

    pub fn get_bound(&self, ty: Ptr<Ty<'db>>) -> Bound<'db> {
        self.egraph.get_bound(ty)
    }

    pub fn set_bound(&mut self, ty: Ptr<Ty<'db>>, bound: Bound<'db>) {
        self.egraph.set_bound(&self.check.target_stash, ty, bound);
        if let Ty::InferVar(idx) = self.target_stash[ty] {
            self.runtime.wake_variable(idx);
        }
    }

    // ------------------------------------------------------------------
    // Core constraint operations
    // ------------------------------------------------------------------

    pub fn assume_eq(&mut self, a: Ptr<Ty<'db>>, b: Ptr<Ty<'db>>) {
        self.egraph.union(&self.check.target_stash, a, b);
    }

    pub fn require_eq(
        &mut self,
        a: Ptr<Ty<'db>>,
        b: Ptr<Ty<'db>>,
        span: RelativeSpan,
    ) -> Result<(), TypeError<'db>> {
        use super::infer::skeleton::decompose;

        let a_canon = self.egraph.find_mut(a);
        let b_canon = self.egraph.find_mut(b);
        if a_canon == b_canon {
            return Ok(());
        }

        let dst = &self.check.target_stash;
        let a_data = dst[a_canon];
        let b_data = dst[b_canon];

        match (a_data, b_data) {
            (Ty::InferVar(_), Ty::InferVar(_)) => {
                self.egraph.union(dst, a_canon, b_canon);
                return Ok(());
            }
            (Ty::InferVar(idx), _) => {
                self.egraph.set_bound(dst, a_canon, Bound::Exactly(b_canon));
                self.egraph.union(dst, a_canon, b_canon);
                self.runtime.wake_variable(idx);
                return Ok(());
            }
            (_, Ty::InferVar(idx)) => {
                self.egraph.set_bound(dst, b_canon, Bound::Exactly(a_canon));
                self.egraph.union(dst, b_canon, a_canon);
                self.runtime.wake_variable(idx);
                return Ok(());
            }
            (Ty::Error(_), _) | (_, Ty::Error(_)) => return Ok(()),
            _ => {}
        }

        let da = decompose(dst, a_canon);
        let db_decomposed = decompose(dst, b_canon);

        if da.skeleton != db_decomposed.skeleton {
            return Err(TypeError {
                kind: TypeErrorKind::Mismatch {
                    expected: b_canon,
                    actual: a_canon,
                },
                span,
                context: Vec::new(),
            });
        }

        assert_eq!(
            da.children.len(),
            db_decomposed.children.len(),
            "same skeleton but different child counts"
        );

        for (ca, cb) in da
            .children
            .into_iter()
            .zip(db_decomposed.children.into_iter())
        {
            self.require_eq(ca, cb, span)?;
        }

        Ok(())
    }

    pub fn require_sub(
        &mut self,
        a: Ptr<Ty<'db>>,
        b: Ptr<Ty<'db>>,
        span: RelativeSpan,
    ) -> Result<(), TypeError<'db>> {
        let a_canon = self.egraph.find_mut(a);
        let b_canon = self.egraph.find_mut(b);
        if a_canon == b_canon {
            return Ok(());
        }

        let a_data = self.target_stash[a_canon];
        let b_data = self.target_stash[b_canon];

        match (a_data, b_data) {
            (Ty::Never, _) => Ok(()),
            (Ty::Ref(inner_a, m_a, _), Ty::Ref(inner_b, m_b, _)) if m_a == m_b => {
                self.require_sub(inner_a, inner_b, span)
            }
            _ => self.require_eq(a_canon, b_canon, span),
        }
    }

    pub fn require_coerce(
        &mut self,
        a: Ptr<Ty<'db>>,
        b: Ptr<Ty<'db>>,
        span: RelativeSpan,
    ) -> Result<(), TypeError<'db>> {
        self.require_sub(a, b, span)
    }

    // ------------------------------------------------------------------
    // Versioning
    // ------------------------------------------------------------------

    pub fn branch(&mut self) -> Version {
        self.egraph.branch()
    }

    pub fn switch_to(&mut self, v: Version) {
        self.egraph.set_current_version(v);
    }

    pub fn discard_branch(&mut self, v: Version) {
        self.egraph.discard(v);
    }

    // ------------------------------------------------------------------
    // Finalization
    // ------------------------------------------------------------------

    pub fn finalize(&mut self) {
        // First pass: identify unresolved variables and resolve AtLeast bounds.
        let mut unresolved_vars = Vec::new();

        let dst = &mut self.check.target_stash;
        for i in 0..self.infer_var_ptrs.len() {
            let ty = self.infer_var_ptrs[i];
            let canon = self.egraph.find_mut(ty);

            if canon != ty {
                continue;
            }

            let bound = self.egraph.get_bound(ty);
            match bound {
                Bound::None => {
                    let idx = match dst[ty] {
                        Ty::InferVar(idx) => idx,
                        _ => continue,
                    };
                    unresolved_vars.push((i, idx));
                }
                Bound::AtLeast(bound_ty) => {
                    self.egraph.set_bound(dst, ty, Bound::Exactly(bound_ty));
                    self.egraph.union(dst, ty, bound_ty);
                }
                Bound::Exactly(_) => {}
            }
        }

        // Second pass: emit diagnostics and set error types for unresolved vars.
        for (i, idx) in unresolved_vars {
            let span = RelativeSpan { start: 0, end: 0 };
            let err = TypeError {
                kind: TypeErrorKind::UnresolvedInferVar { var: idx },
                span,
                context: Vec::new(),
            };
            let diag = err.to_diagnostic(self);
            let e = self.report(diag);

            let ty = self.infer_var_ptrs[i];
            let dst = &mut self.check.target_stash;
            let error_ty = dst.alloc(Ty::Error(e));
            self.egraph.set_bound(dst, ty, Bound::Exactly(error_ty));
            self.egraph.union(dst, ty, error_ty);
        }

        self.runtime.wake_all();
        self.runtime.drain();
    }

    // ------------------------------------------------------------------
    // Diagnostics
    // ------------------------------------------------------------------

    pub fn report(&mut self, diag: Diagnostic<'db>) -> ErrorReported {
        self.diagnostics.push(diag);
        ErrorReported::mint()
    }

    /// Catch a TypeError: convert to Diagnostic (rendering types now) and report.
    pub fn catch(&mut self, err: TypeError<'db>) -> ErrorReported {
        let diag = err.to_diagnostic(self);
        self.report(diag)
    }

    pub fn report_type_mismatch(
        &mut self,
        expected: Ptr<Ty<'db>>,
        actual: Ptr<Ty<'db>>,
        span: RelativeSpan,
    ) {
        let err = TypeError {
            kind: TypeErrorKind::Mismatch { expected, actual },
            span,
            context: Vec::new(),
        };
        self.catch(err);
    }

    pub fn diagnostics(&self) -> &[Diagnostic<'db>] {
        &self.diagnostics
    }

    pub fn has_errors(&self) -> bool {
        !self.diagnostics.is_empty()
    }

    // ------------------------------------------------------------------
    // TyExpr allocation
    // ------------------------------------------------------------------

    pub fn alloc_ty(&mut self, ty: Ty<'db>) -> Ptr<Ty<'db>> {
        self.target_stash.alloc(ty)
    }

    pub fn alloc_expr(
        &mut self,
        data: TyExprData<'db>,
        ty: Ptr<Ty<'db>>,
        span: RelativeSpan,
    ) -> Ptr<TyExpr<'db>> {
        self.target_stash.alloc(TyExpr { data, ty, span })
    }

    // ------------------------------------------------------------------
    // Signature import
    // ------------------------------------------------------------------

    /// Import a function signature into the body's stash (identity
    /// instantiation — copies param/return types from the sig stash).
    pub fn import_fn_sig(&mut self, sig: &Stashed<Binder<'db, FnSig<'db>>>) -> FnSig<'db> {
        let sig_stash = sig.stash();
        let binder = sig.root();
        let fn_sig = binder.value;

        let params: smallvec::SmallVec<[Ptr<Ty<'db>>; 16]> = sig_stash[fn_sig.params]
            .iter()
            .map(|p| p.stash_copy(sig_stash, &mut self.target_stash))
            .collect();
        let params = self.target_stash.alloc_slice(&params);

        let ret = fn_sig.ret.stash_copy(sig_stash, &mut self.target_stash);

        FnSig { params, ret }
    }

    /// Bind function parameters as locals with their sig-declared types.
    pub fn bind_params(
        &mut self,
        param_tys: Slice<Ptr<Ty<'db>>>,
        params_cst: Slice<crate::cst::fns::ParamCst<'db>>,
    ) {
        for index in 0..self.source_stash[params_cst].len() {
            let param_cst = self.source_stash[params_cst][index];
            let param_ty = self.target_stash[param_tys][index];

            if let Some(name) = param_cst.name {
                let id = LocalId(self.local_vars.len() as u32);
                self.local_vars.push(LocalVar {
                    name,
                    span: param_cst.span,
                });
                self.locals.push(param_ty);
                self.resolver
                    .ribs
                    .add(name, Namespace::Value, Resolution::Local(id));
            }
        }
    }

    // ------------------------------------------------------------------
    // Type resolution (post-finalize)
    // ------------------------------------------------------------------

    /// Rewrite all InferVar entries in the stash with their resolved types.
    /// Must be called after `finalize()` so that the egraph has resolved all vars.
    pub fn resolve_types(&mut self) {
        let version = self.egraph.current_version();
        let var_count = self.egraph.version_tree().variable_count_at(version);

        let dst = &mut self.check.target_stash;
        for i in 0..var_count.0 {
            let idx = InferVarIndex(i);
            let ty_ptr = dst.alloc(Ty::InferVar(idx));
            let resolved = self.egraph.find_mut(ty_ptr);
            if resolved != ty_ptr {
                let resolved_ty = dst[resolved];
                dst[ty_ptr] = resolved_ty;
            }
        }
    }

    // ------------------------------------------------------------------
    // Finish
    // ------------------------------------------------------------------

    pub fn finish(self, root: Ptr<TyExpr<'db>>, span: RelativeSpan) -> CheckedBody<'db> {
        let mut stash = self.check.target_stash;
        let locals = stash.alloc_slice(&self.local_vars);
        let body_data = stash.alloc(TyBodyData { root, locals, span });
        CheckedBody {
            body: Stashed::new(stash, body_data),
            diagnostics: self.diagnostics,
        }
    }
}
