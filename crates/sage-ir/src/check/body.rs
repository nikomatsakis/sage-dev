use sage_stash::{Ptr, Stash, StashCopy, Stashed};

use crate::name::Name;
use crate::resolve::{Namespace, Resolver};
use crate::ribs::RibEntry;
use crate::span::RelativeSpan;
use crate::ty::{Binder, FnSig, InferVarIndex, Ty, TyData};
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
}

// ---------------------------------------------------------------------------
// BodyCtx
// ---------------------------------------------------------------------------

/// Unified body-checking context: resolves names and infers types in a
/// single CST walk, producing `TyExpr` nodes directly into the egraph's
/// stash.
pub struct BodyCtx<'a, 'db> {
    // Resolution
    pub resolver: Resolver<'db>,
    pub src: &'a Stash,

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

impl<'a, 'db> BodyCtx<'a, 'db> {
    pub fn new(db: &'db dyn crate::Db, src: &'a Stash, resolver: Resolver<'db>) -> Self {
        Self {
            resolver,
            src,
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
        &self.egraph.stash
    }

    pub fn stash_mut(&mut self) -> &mut Stash {
        &mut self.egraph.stash
    }

    // ------------------------------------------------------------------
    // Path resolution
    // ------------------------------------------------------------------

    pub fn resolve_path(
        &mut self,
        path: crate::cst::paths::PathCst<'db>,
        ns: Namespace,
    ) -> crate::tytree::Res<'db> {
        use crate::tytree::Res;

        let segments = &self.src[path.segments];
        if segments.is_empty() {
            return Res::Err;
        }

        let first = &segments[0];
        let rest = &segments[1..];

        if let Some(entry) = self.resolver.ribs.lookup(first.name, ns) {
            return match entry {
                RibEntry::Local(id) => {
                    if rest.is_empty() {
                        Res::Local(id)
                    } else {
                        Res::Err
                    }
                }
                RibEntry::Param(_) | RibEntry::SelfTy(_) => Res::Err,
                RibEntry::Sym(sym) => {
                    if rest.is_empty() {
                        Res::Def(sym)
                    } else {
                        let names: Vec<_> = segments.iter().map(|s| s.name).collect();
                        match self.resolver.resolve_segments(&names, ns) {
                            Ok(sym) => Res::Def(sym),
                            Err(_) => Res::Err,
                        }
                    }
                }
            };
        }

        let names: Vec<_> = segments.iter().map(|s| s.name).collect();
        match self.resolver.resolve_segments(&names, ns) {
            Ok(sym) => Res::Def(sym),
            Err(_) => Res::Err,
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
            .add(name, Namespace::Value, RibEntry::Local(id));
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
        let data = self.fresh_ty_var_data();
        self.egraph.alloc_ty(data)
    }

    pub fn fresh_ty_var_data(&mut self) -> TyData<'db> {
        let universe = self.current_universe;
        let idx = self.egraph.alloc_var(VarInfo { universe });
        TyData::InferVar(idx)
    }

    // ------------------------------------------------------------------
    // Type allocation
    // ------------------------------------------------------------------

    pub fn alloc_ty(&mut self, data: TyData<'db>) -> Ptr<Ty<'db>> {
        self.egraph.alloc_ty(data)
    }

    pub fn unit_ty(&mut self) -> Ptr<Ty<'db>> {
        let elems = self.egraph.stash.alloc_slice(&[]);
        self.egraph.alloc_ty(TyData::Tuple(elems))
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
        self.egraph.set_bound(ty, bound);
        if let TyData::InferVar(idx) = self.egraph.ty_data(ty) {
            self.runtime.wake_variable(idx);
        }
    }

    // ------------------------------------------------------------------
    // Core constraint operations
    // ------------------------------------------------------------------

    pub fn assume_eq(&mut self, a: Ptr<Ty<'db>>, b: Ptr<Ty<'db>>) {
        self.egraph.union(a, b);
    }

    pub fn require_eq(&mut self, a: Ptr<Ty<'db>>, b: Ptr<Ty<'db>>) {
        use super::infer::skeleton::decompose;

        let a_canon = self.egraph.find_mut(a);
        let b_canon = self.egraph.find_mut(b);
        if a_canon == b_canon {
            return;
        }

        let a_data = self.egraph.ty_data(a_canon);
        let b_data = self.egraph.ty_data(b_canon);

        match (a_data, b_data) {
            (TyData::InferVar(_), TyData::InferVar(_)) => {
                self.egraph.union(a_canon, b_canon);
                return;
            }
            (TyData::InferVar(idx), _) => {
                self.egraph.set_bound(a_canon, Bound::Exactly(b_canon));
                self.egraph.union(a_canon, b_canon);
                self.runtime.wake_variable(idx);
                return;
            }
            (_, TyData::InferVar(idx)) => {
                self.egraph.set_bound(b_canon, Bound::Exactly(a_canon));
                self.egraph.union(b_canon, a_canon);
                self.runtime.wake_variable(idx);
                return;
            }
            (TyData::Error, _) | (_, TyData::Error) => return,
            _ => {}
        }

        let da = decompose(&self.egraph.stash, a_canon);
        let db_decomposed = decompose(&self.egraph.stash, b_canon);

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

        let a_data = self.egraph.ty_data(a_canon);
        let b_data = self.egraph.ty_data(b_canon);

        match (a_data, b_data) {
            (TyData::Never, _) => {}
            (TyData::Ref(inner_a, m_a, _), TyData::Ref(inner_b, m_b, _)) if m_a == m_b => {
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

        for i in 0..var_count.0 {
            let idx = InferVarIndex(i);
            let ty = self.egraph.alloc_ty(TyData::InferVar(idx));
            let canon = self.egraph.find_mut(ty);

            if canon != ty {
                continue;
            }

            let bound = self.egraph.get_bound(ty);
            match bound {
                Bound::None => {
                    let error_ty = self.egraph.alloc_ty(TyData::Error);
                    self.egraph.set_bound(ty, Bound::Exactly(error_ty));
                    self.egraph.union(ty, error_ty);
                    self.diagnostics.push(Diagnostic {
                        kind: DiagnosticKind::UnresolvedInferVar { var: idx },
                    });
                }
                Bound::AtLeast(bound_ty) => {
                    self.egraph.set_bound(ty, Bound::Exactly(bound_ty));
                    self.egraph.union(ty, bound_ty);
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

    pub fn alloc_expr(
        &mut self,
        kind: TyExprKind<'db>,
        ty: Ptr<Ty<'db>>,
        span: RelativeSpan,
    ) -> Ptr<TyExpr<'db>> {
        self.egraph.stash.alloc(TyExpr { kind, ty, span })
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

        let params: Vec<Ptr<Ty<'db>>> = sig_stash[fn_sig.params]
            .iter()
            .map(|p| p.stash_copy(sig_stash, &mut self.egraph.stash))
            .collect();

        let ret = fn_sig.ret.stash_copy(sig_stash, &mut self.egraph.stash);

        FnSig { params, ret }
    }

    /// Bind function parameters as locals with their sig-declared types.
    pub fn bind_params(
        &mut self,
        param_tys: &[Ptr<Ty<'db>>],
        params_cst: &[crate::cst::fns::ParamCst<'db>],
    ) {
        for (param_cst, &ty) in params_cst.iter().zip(param_tys) {
            if let Some(name) = param_cst.name {
                let id = LocalId(self.local_vars.len() as u32);
                self.local_vars.push(LocalVar {
                    name,
                    span: param_cst.span,
                });
                self.locals.push(ty);
                self.resolver
                    .ribs
                    .add(name, Namespace::Value, RibEntry::Local(id));
            }
        }
    }

    // ------------------------------------------------------------------
    // Finish
    // ------------------------------------------------------------------

    pub fn finish(self, root: Ptr<TyExpr<'db>>, span: RelativeSpan) -> TyBody<'db> {
        let mut stash = self.egraph.stash;
        let locals = stash.alloc_slice(&self.local_vars);
        let body = stash.alloc(TyBodyData { root, locals, span });
        Stashed::new(stash, body)
    }
}
