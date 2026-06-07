use sage_ir::ty::{InferVarIndex, IntTy, TyData};

use crate::bound::Bound;
use crate::egraph::VersionedEGraph;
use crate::infer_ctx::InferCtx;
use crate::version::{Universe, VarInfo, Version, VersionTree};

// ---------------------------------------------------------------------------
// Version tree tests
// ---------------------------------------------------------------------------

#[test]
fn version_tree_basic() {
    let mut tree = VersionTree::new();
    assert_eq!(tree.variable_count_at(Version::ROOT), InferVarIndex(0));

    let idx0 = tree.alloc_var(
        Version::ROOT,
        VarInfo {
            universe: Universe(1),
        },
    );
    assert_eq!(idx0, InferVarIndex(0));

    let idx1 = tree.alloc_var(
        Version::ROOT,
        VarInfo {
            universe: Universe(1),
        },
    );
    assert_eq!(idx1, InferVarIndex(1));

    assert_eq!(tree.variable_count_at(Version::ROOT), InferVarIndex(2));
    assert_eq!(tree.get_variable(Version::ROOT, idx0).universe, Universe(1));
}

#[test]
fn version_tree_branching() {
    let mut tree = VersionTree::new();
    tree.alloc_var(
        Version::ROOT,
        VarInfo {
            universe: Universe(1),
        },
    );
    tree.alloc_var(
        Version::ROOT,
        VarInfo {
            universe: Universe(1),
        },
    );

    let branch_a = tree.branch(Version::ROOT);
    let branch_b = tree.branch(Version::ROOT);

    assert_eq!(tree.get(branch_a).variable_start, InferVarIndex(2));
    assert_eq!(tree.get(branch_b).variable_start, InferVarIndex(2));

    let a0 = tree.alloc_var(
        branch_a,
        VarInfo {
            universe: Universe(2),
        },
    );
    let b0 = tree.alloc_var(
        branch_b,
        VarInfo {
            universe: Universe(2),
        },
    );

    assert_eq!(a0, InferVarIndex(2));
    assert_eq!(b0, InferVarIndex(2));

    assert_eq!(tree.get_variable(branch_a, a0).universe, Universe(2));
    assert_eq!(tree.get_variable(branch_b, b0).universe, Universe(2));

    assert_eq!(
        tree.get_variable(branch_a, InferVarIndex(0)).universe,
        Universe(1)
    );
}

#[test]
fn version_tree_remove() {
    let mut tree = VersionTree::new();
    let branch = tree.branch(Version::ROOT);
    tree.alloc_var(
        branch,
        VarInfo {
            universe: Universe(1),
        },
    );
    tree.remove(branch);
    assert!(tree.get(branch).removed);
}

// ---------------------------------------------------------------------------
// Egraph tests
// ---------------------------------------------------------------------------

#[test]
fn egraph_find_self() {
    let mut eg = VersionedEGraph::new();
    let ty = eg.alloc_ty(TyData::Bool);
    assert_eq!(eg.find(ty), ty);
}

#[test]
fn egraph_union_and_find() {
    let mut eg = VersionedEGraph::new();
    let a = eg.alloc_ty(TyData::Int(IntTy::I32));
    eg.alloc_var(VarInfo {
        universe: Universe(1),
    });
    let b = eg.alloc_ty(TyData::InferVar(InferVarIndex(0)));

    let repr = eg.union(b, a);
    assert_eq!(repr, a);
    assert_eq!(eg.find(b), a);
}

#[test]
fn egraph_versioned_union() {
    let mut eg = VersionedEGraph::new();
    eg.alloc_var(VarInfo {
        universe: Universe(1),
    });
    eg.alloc_var(VarInfo {
        universe: Universe(1),
    });

    let ty0 = eg.alloc_ty(TyData::InferVar(InferVarIndex(0)));
    let _ty1 = eg.alloc_ty(TyData::InferVar(InferVarIndex(1)));
    let concrete = eg.alloc_ty(TyData::Bool);

    let branch = eg.branch();
    eg.set_current_version(branch);
    eg.union(ty0, concrete);
    assert_eq!(eg.find(ty0), concrete);

    eg.set_current_version(Version::ROOT);
    assert_eq!(eg.find(ty0), ty0);

    eg.discard(branch);
    assert_eq!(eg.find(ty0), ty0);
}

#[test]
fn egraph_bounds_versioned() {
    let mut eg = VersionedEGraph::new();
    eg.alloc_var(VarInfo {
        universe: Universe(1),
    });
    let ty_var = eg.alloc_ty(TyData::InferVar(InferVarIndex(0)));
    let i32_ty = eg.alloc_ty(TyData::Int(IntTy::I32));

    assert_eq!(eg.get_bound(ty_var), Bound::None);

    let branch = eg.branch();
    eg.set_current_version(branch);
    eg.set_bound(ty_var, Bound::AtLeast(i32_ty));
    assert_eq!(eg.get_bound(ty_var), Bound::AtLeast(i32_ty));

    eg.set_current_version(Version::ROOT);
    assert_eq!(eg.get_bound(ty_var), Bound::None);
}

// ---------------------------------------------------------------------------
// InferCtx tests
// ---------------------------------------------------------------------------

#[test]
fn infer_ctx_fresh_var() {
    let mut ctx = InferCtx::new();
    let v0 = ctx.fresh_ty_var();
    let v1 = ctx.fresh_ty_var();
    assert_ne!(v0, v1);

    match ctx.egraph.ty_data(v0) {
        TyData::InferVar(idx) => assert_eq!(idx, InferVarIndex(0)),
        _ => panic!("expected InferVar"),
    }
    match ctx.egraph.ty_data(v1) {
        TyData::InferVar(idx) => assert_eq!(idx, InferVarIndex(1)),
        _ => panic!("expected InferVar"),
    }
}

#[test]
fn infer_ctx_require_eq_var_concrete() {
    let mut ctx = InferCtx::new();
    let var = ctx.fresh_ty_var();
    let i32_ty = ctx.alloc_ty(TyData::Int(IntTy::I32));

    ctx.require_eq(var, i32_ty);

    let canon = ctx.find(var);
    assert_eq!(canon, i32_ty);
    assert_eq!(ctx.get_bound(var), Bound::Exactly(i32_ty));
}

#[test]
fn infer_ctx_require_eq_var_var() {
    let mut ctx = InferCtx::new();
    let v0 = ctx.fresh_ty_var();
    let v1 = ctx.fresh_ty_var();

    ctx.require_eq(v0, v1);

    assert_eq!(ctx.find(v0), ctx.find(v1));
}

#[test]
fn infer_ctx_require_eq_propagates() {
    let mut ctx = InferCtx::new();
    let v0 = ctx.fresh_ty_var();
    let v1 = ctx.fresh_ty_var();
    let i32_ty = ctx.alloc_ty(TyData::Int(IntTy::I32));

    ctx.require_eq(v0, v1);
    ctx.require_eq(v1, i32_ty);

    assert_eq!(ctx.find(v0), i32_ty);
    assert_eq!(ctx.find(v1), i32_ty);
}

#[test]
fn infer_ctx_type_mismatch() {
    let mut ctx = InferCtx::new();
    let i32_ty = ctx.alloc_ty(TyData::Int(IntTy::I32));
    let bool_ty = ctx.alloc_ty(TyData::Bool);

    ctx.require_eq(i32_ty, bool_ty);

    assert!(ctx.has_errors());
}

#[test]
fn infer_ctx_never_subtypes() {
    let mut ctx = InferCtx::new();
    let never_ty = ctx.alloc_ty(TyData::Never);
    let i32_ty = ctx.alloc_ty(TyData::Int(IntTy::I32));

    ctx.require_sub(never_ty, i32_ty);

    assert!(!ctx.has_errors());
}

#[test]
fn infer_ctx_finalize_unresolved() {
    let mut ctx = InferCtx::new();
    let var = ctx.fresh_ty_var();

    ctx.finalize();

    let canon = ctx.find(var);
    assert_eq!(ctx.egraph.ty_data(canon), TyData::Error);
    assert!(ctx.has_errors());
}

#[test]
fn infer_ctx_finalize_at_least() {
    let mut ctx = InferCtx::new();
    let var = ctx.fresh_ty_var();
    let i32_ty = ctx.alloc_ty(TyData::Int(IntTy::I32));

    ctx.set_bound(var, Bound::AtLeast(i32_ty));
    ctx.finalize();

    let canon = ctx.find(var);
    assert_eq!(canon, i32_ty);
    assert!(!ctx.has_errors());
}

#[test]
fn infer_ctx_universe() {
    let mut ctx = InferCtx::new();
    assert_eq!(ctx.current_universe(), Universe(1));

    ctx.push_universe();
    assert_eq!(ctx.current_universe(), Universe(2));

    let var = ctx.fresh_ty_var();
    match ctx.egraph.ty_data(var) {
        TyData::InferVar(idx) => {
            assert_eq!(ctx.var_universe(idx), Universe(2));
        }
        _ => panic!("expected InferVar"),
    }

    ctx.pop_universe();
    assert_eq!(ctx.current_universe(), Universe(1));
}

// ---------------------------------------------------------------------------
// Integration
// ---------------------------------------------------------------------------

#[test]
fn forward_propagation_through_vars() {
    let mut ctx = InferCtx::new();
    let x_ty = ctx.fresh_ty_var();
    let y_ty = ctx.fresh_ty_var();
    let i32_ty = ctx.alloc_ty(TyData::Int(IntTy::I32));

    ctx.require_eq(x_ty, i32_ty);
    ctx.require_eq(y_ty, x_ty);

    assert_eq!(ctx.find(y_ty), i32_ty);
    assert!(!ctx.has_errors());
}

#[test]
fn backward_propagation_through_vars() {
    let mut ctx = InferCtx::new();
    let x_ty = ctx.fresh_ty_var();
    let y_ty = ctx.fresh_ty_var();
    let i32_ty = ctx.alloc_ty(TyData::Int(IntTy::I32));

    ctx.require_eq(y_ty, x_ty);
    ctx.require_eq(x_ty, i32_ty);

    assert_eq!(ctx.find(y_ty), i32_ty);
}

#[test]
fn speculative_branch_rollback() {
    let mut ctx = InferCtx::new();
    let var = ctx.fresh_ty_var();
    let i32_ty = ctx.alloc_ty(TyData::Int(IntTy::I32));
    let bool_ty = ctx.alloc_ty(TyData::Bool);

    let branch_a = ctx.branch();
    ctx.switch_to(branch_a);
    ctx.require_eq(var, i32_ty);
    assert_eq!(ctx.find(var), i32_ty);

    ctx.switch_to(Version::ROOT);
    ctx.discard_branch(branch_a);
    assert_eq!(ctx.find(var), var);

    let branch_b = ctx.branch();
    ctx.switch_to(branch_b);
    ctx.require_eq(var, bool_ty);
    assert_eq!(ctx.find(var), bool_ty);
}
