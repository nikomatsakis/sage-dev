use std::ops::{Deref, DerefMut};

use sage_stash::{Ptr, Slice, Stash, StashCopy, Stashed};

use crate::check::Check;
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
// Diagnostics
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Diagnostic<'db> {
    pub kind: DiagnosticKind<'db>,
}

#[derive(Clone, Debug)]
pub enum DiagnosticKind<'db> {
    TypeMismatch {
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

// ---------------------------------------------------------------------------
// BodyCtx
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
    diagnostics: Vec<Diagnostic<'db>>,
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
    pub fn new(db: &'db dyn crate::Db, src: &'a Stash, resolver: Resolver<'db>) -> Self {
        Self {
            check: Check::new(db, src, resolver),
            db,
            egraph: VersionedEGraph::new(),
            runtime: Runtime::new(),
            current_universe: Universe(1),
            locals: Vec::new(),
            local_vars: Vec::new(),
            diagnostics: Vec::new(),
        }
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

    pub fn resolve_path(&mut self, path: crate::cst::paths::Path<'db>, ns: Namespace) -> Res<'db> {
        let check = &mut self.check;
        let results = check.resolver.resolve_path(check.source_stash, path, ns);
        if results.len() > 1 {
            self.diagnostics.push(Diagnostic {
                kind: DiagnosticKind::AmbiguousName {
                    count: results.len(),
                },
            });
            return Res::Err;
        }
        match results.into_iter().next() {
            Some(Resolution::Sym(sym)) => Res::Def(sym),
            Some(Resolution::Local(id)) => Res::Local(id),
            Some(Resolution::Param(_) | Resolution::SelfTy(_) | Resolution::Error) | None => {
                Res::Err
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
        self.target_stash.alloc(ty)
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

    pub fn require_eq(&mut self, a: Ptr<Ty<'db>>, b: Ptr<Ty<'db>>) {
        use super::infer::skeleton::decompose;

        let a_canon = self.egraph.find_mut(a);
        let b_canon = self.egraph.find_mut(b);
        if a_canon == b_canon {
            return;
        }

        let dst = &self.check.target_stash;
        let a_data = dst[a_canon];
        let b_data = dst[b_canon];

        match (a_data, b_data) {
            (Ty::InferVar(_), Ty::InferVar(_)) => {
                self.egraph.union(dst, a_canon, b_canon);
                return;
            }
            (Ty::InferVar(idx), _) => {
                self.egraph.set_bound(dst, a_canon, Bound::Exactly(b_canon));
                self.egraph.union(dst, a_canon, b_canon);
                self.runtime.wake_variable(idx);
                return;
            }
            (_, Ty::InferVar(idx)) => {
                self.egraph.set_bound(dst, b_canon, Bound::Exactly(a_canon));
                self.egraph.union(dst, b_canon, a_canon);
                self.runtime.wake_variable(idx);
                return;
            }
            (Ty::Error, _) | (_, Ty::Error) => return,
            _ => {}
        }

        let da = decompose(dst, a_canon);
        let db_decomposed = decompose(dst, b_canon);

        if da.skeleton != db_decomposed.skeleton {
            self.report_type_mismatch(b_canon, a_canon);
            return;
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
            self.require_eq(ca, cb);
        }
    }

    pub fn require_sub(&mut self, a: Ptr<Ty<'db>>, b: Ptr<Ty<'db>>) {
        let a_canon = self.egraph.find_mut(a);
        let b_canon = self.egraph.find_mut(b);
        if a_canon == b_canon {
            return;
        }

        let a_data = self.target_stash[a_canon];
        let b_data = self.target_stash[b_canon];

        match (a_data, b_data) {
            (Ty::Never, _) => {}
            (Ty::Ref(inner_a, m_a, _), Ty::Ref(inner_b, m_b, _)) if m_a == m_b => {
                self.require_sub(inner_a, inner_b);
            }
            _ => self.require_eq(a_canon, b_canon),
        }
    }

    pub fn require_coerce(&mut self, a: Ptr<Ty<'db>>, b: Ptr<Ty<'db>>) {
        self.require_sub(a, b);
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
        let version = self.egraph.current_version();
        let var_count = self.egraph.version_tree().variable_count_at(version);

        let dst = &mut self.check.target_stash;
        for i in 0..var_count.0 {
            let idx = InferVarIndex(i);
            let ty = dst.alloc(Ty::InferVar(idx));
            let canon = self.egraph.find_mut(ty);

            if canon != ty {
                continue;
            }

            let bound = self.egraph.get_bound(ty);
            match bound {
                Bound::None => {
                    let error_ty = dst.alloc(Ty::Error);
                    self.egraph.set_bound(dst, ty, Bound::Exactly(error_ty));
                    self.egraph.union(dst, ty, error_ty);
                    self.diagnostics.push(Diagnostic {
                        kind: DiagnosticKind::UnresolvedInferVar { var: idx },
                    });
                }
                Bound::AtLeast(bound_ty) => {
                    self.egraph.set_bound(dst, ty, Bound::Exactly(bound_ty));
                    self.egraph.union(dst, ty, bound_ty);
                }
                Bound::Exactly(_) => {}
            }
        }

        self.runtime.wake_all();
        self.runtime.drain();
    }

    // ------------------------------------------------------------------
    // Diagnostics
    // ------------------------------------------------------------------

    pub fn report_type_mismatch(&mut self, expected: Ptr<Ty<'db>>, actual: Ptr<Ty<'db>>) {
        self.diagnostics.push(Diagnostic {
            kind: DiagnosticKind::TypeMismatch { expected, actual },
        });
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

    pub fn finish(self, root: Ptr<TyExpr<'db>>, span: RelativeSpan) -> TyBody<'db> {
        let mut stash = self.check.target_stash;
        let locals = stash.alloc_slice(&self.local_vars);
        let body = stash.alloc(TyBodyData { root, locals, span });
        Stashed::new(stash, body)
    }
}
