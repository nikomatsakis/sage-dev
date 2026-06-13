use crate::ty::{InferVarIndex, Ty, TyData};
use sage_stash::{Ptr, Stash};

use super::bound::Bound;
use super::egraph::VersionedEGraph;
use super::runtime::Runtime;
use super::version::{Universe, VarInfo, Version};

/// The inference context — owns the egraph, runtime, and typing state.
pub struct InferCtx<'db> {
    pub db: &'db dyn crate::Db,
    pub egraph: VersionedEGraph<'db>,
    pub runtime: Runtime,
    current_universe: Universe,
    locals: Vec<Ptr<Ty<'db>>>,
    diagnostics: Vec<Diagnostic<'db>>,
}

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

impl<'db> InferCtx<'db> {
    pub fn new() -> Self {
        Self {
            egraph: VersionedEGraph::new(),
            runtime: Runtime::new(),
            current_universe: Universe(1),
            locals: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Stash access
    // -----------------------------------------------------------------------

    pub fn stash(&self) -> &Stash {
        &self.egraph.stash
    }

    pub fn stash_mut(&mut self) -> &mut Stash {
        &mut self.egraph.stash
    }

    // -----------------------------------------------------------------------
    // Variable allocation
    // -----------------------------------------------------------------------

    pub fn fresh_ty_var(&mut self) -> Ptr<Ty<'db>> {
        let data = self.fresh_ty_var_data();
        self.egraph.alloc_ty(data)
    }

    pub fn fresh_ty_var_data(&mut self) -> TyData<'db> {
        let universe = self.current_universe;
        let idx = self.egraph.alloc_var(VarInfo { universe });
        TyData::InferVar(idx)
    }

    pub fn var_universe(&self, idx: InferVarIndex) -> Universe {
        self.egraph.get_var_info(idx).universe
    }

    // -----------------------------------------------------------------------
    // Locals
    // -----------------------------------------------------------------------

    pub fn push_local(&mut self, ty: Ptr<Ty<'db>>) -> u32 {
        let id = self.locals.len() as u32;
        self.locals.push(ty);
        id
    }

    pub fn local_type(&self, id: u32) -> Ptr<Ty<'db>> {
        self.locals[id as usize]
    }

    // -----------------------------------------------------------------------
    // Universe management
    // -----------------------------------------------------------------------

    pub fn current_universe(&self) -> Universe {
        self.current_universe
    }

    pub fn push_universe(&mut self) -> Universe {
        self.current_universe = Universe(self.current_universe.0 + 1);
        self.current_universe
    }

    pub fn pop_universe(&mut self) -> Universe {
        self.current_universe = Universe(self.current_universe.0 - 1);
        self.current_universe
    }

    // -----------------------------------------------------------------------
    // Egraph operations
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // Core constraint operations
    // -----------------------------------------------------------------------

    pub fn assume_eq(&mut self, a: Ptr<Ty<'db>>, b: Ptr<Ty<'db>>) {
        self.egraph.union(a, b);
    }

    /// Check that `a` and `b` can be equal. Structural descent; sets bounds
    /// on inference vars; reports errors on mismatch.
    /// Convention: `b` is the "expected" type for error reporting.
    pub fn require_eq(&mut self, a: Ptr<Ty<'db>>, b: Ptr<Ty<'db>>) {
        use super::skeleton::decompose;

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
        let db = decompose(&self.egraph.stash, b_canon);

        if da.skeleton != db.skeleton {
            self.report_type_mismatch(b_canon, a_canon);
            return;
        }

        assert_eq!(
            da.children.len(),
            db.children.len(),
            "same skeleton but different child counts"
        );

        for (ca, cb) in da.children.into_iter().zip(db.children.into_iter()) {
            self.require_eq(ca, cb);
        }
    }

    /// Subtyping check.
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

    /// Coercion check — delegates to subtyping for now.
    pub fn require_coerce(&mut self, a: Ptr<Ty<'db>>, b: Ptr<Ty<'db>>) {
        self.require_sub(a, b);
    }

    // -----------------------------------------------------------------------
    // Type allocation
    // -----------------------------------------------------------------------

    pub fn alloc_ty(&mut self, data: TyData<'db>) -> Ptr<Ty<'db>> {
        self.egraph.alloc_ty(data)
    }

    pub fn unit_ty(&mut self) -> Ptr<Ty<'db>> {
        let elems = self.egraph.stash.alloc_slice(&[]);
        self.egraph.alloc_ty(TyData::Tuple(elems))
    }

    // -----------------------------------------------------------------------
    // Versioning
    // -----------------------------------------------------------------------

    pub fn branch(&mut self) -> Version {
        self.egraph.branch()
    }

    pub fn switch_to(&mut self, v: Version) {
        self.egraph.set_current_version(v);
    }

    pub fn discard_branch(&mut self, v: Version) {
        self.egraph.discard(v);
    }

    // -----------------------------------------------------------------------
    // Finalization
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // Diagnostics
    // -----------------------------------------------------------------------

    pub fn report_type_mismatch(&mut self, expected: Ptr<Ty<'db>>, actual: Ptr<Ty<'db>>) {
        self.diagnostics.push(Diagnostic {
            kind: DiagnosticKind::TypeMismatch { expected, actual },
        });
    }

    pub fn diagnostics(&self) -> &[Diagnostic<'db>] {
        &self.diagnostics
    }

    pub fn take_diagnostics(self) -> Vec<Diagnostic<'db>> {
        self.diagnostics
    }

    pub fn into_parts(self) -> (Vec<Diagnostic<'db>>, Stash) {
        (self.diagnostics, self.egraph.stash)
    }

    pub fn has_errors(&self) -> bool {
        !self.diagnostics.is_empty()
    }
}
