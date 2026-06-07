use sage_ir::Db;
use sage_ir::body::{BinaryOp, Literal};
use sage_ir::module::ModSymbol;
use sage_ir::name::Name;
use sage_ir::resolve::SourceRoot;
use sage_ir::resolved::*;
use sage_ir::sig_lower::struct_signature;
use sage_ir::symbol::SymbolData;
use sage_ir::ty::*;
use sage_ir::ty_fold::instantiate_struct_sig;
use sage_stash::{Ptr, Stash, StashCopy, Stashed};

use crate::infer_ctx::{Diagnostic, DiagnosticKind, InferCtx};

pub struct TypeCheckResult<'db> {
    pub diagnostics: Vec<Diagnostic<'db>>,
    stash: Stash,
}

impl<'db> TypeCheckResult<'db> {
    pub fn render_errors(&self, db: &'db dyn Db) -> Vec<String> {
        self.diagnostics
            .iter()
            .map(|d| render_diagnostic(db, &self.stash, d))
            .collect()
    }

    pub fn has_errors(&self) -> bool {
        !self.diagnostics.is_empty()
    }
}

pub fn type_check_body<'db>(
    db: &'db dyn Db,
    resolved: &ResolvedBody<'db>,
    sig: &Stashed<Binder<'db, FnSig<'db>>>,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
) -> TypeCheckResult<'db> {
    let mut ctx = InferCtx::new();

    let sig_data = import_signature(&mut ctx, sig);

    let body_stash = resolved.stash();
    let body = &body_stash[*resolved.root()];

    let locals = &body_stash[body.locals];
    for (i, _local_var) in locals.iter().enumerate() {
        if (i as u32) < sig_data.param_count {
            ctx.push_local(sig_data.params[i]);
        } else {
            let var = ctx.fresh_ty_var();
            ctx.push_local(var);
        }
    }

    let env = CheckEnv {
        db,
        module,
        source_root,
        ret_ty: sig_data.ret,
    };

    let body_expr = &body_stash[body.root];
    let body_ty = check_expr(&mut ctx, &env, body_stash, body_expr);
    ctx.require_coerce(body_ty, sig_data.ret);

    ctx.finalize();

    let (diagnostics, stash) = ctx.into_parts();
    TypeCheckResult { diagnostics, stash }
}

struct CheckEnv<'db> {
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
    ret_ty: Ptr<Ty<'db>>,
}

struct ImportedSig<'db> {
    params: Vec<Ptr<Ty<'db>>>,
    param_count: u32,
    ret: Ptr<Ty<'db>>,
}

fn import_signature<'db>(
    ctx: &mut InferCtx<'db>,
    sig: &Stashed<Binder<'db, FnSig<'db>>>,
) -> ImportedSig<'db> {
    let sig_stash = sig.stash();
    let binder = sig.root();
    let fn_sig = binder.value;

    let params: Vec<Ptr<Ty<'db>>> = sig_stash[fn_sig.params]
        .iter()
        .map(|p| copy_ty_into(ctx, sig_stash, *p))
        .collect();

    let ret = copy_ty_into(ctx, sig_stash, fn_sig.ret);

    let param_count = params.len() as u32;
    ImportedSig {
        params,
        param_count,
        ret,
    }
}

fn copy_ty_into<'db>(ctx: &mut InferCtx<'db>, src: &Stash, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
    ty.stash_copy(src, ctx.stash_mut())
}

// ---------------------------------------------------------------------------
// Expression checking
// ---------------------------------------------------------------------------

fn check_expr<'db>(
    ctx: &mut InferCtx<'db>,
    env: &CheckEnv<'db>,
    body_stash: &Stash,
    expr: &RExpr<'db>,
) -> Ptr<Ty<'db>> {
    match &expr.kind {
        RExprKind::Literal(lit) => check_literal(ctx, *lit),

        RExprKind::Path(res) => check_path(ctx, *res),

        RExprKind::Block(stmts, tail) => {
            let stmts_slice = &body_stash[*stmts];
            for stmt in stmts_slice {
                check_stmt(ctx, env, body_stash, stmt);
            }
            match tail {
                Some(tail_ptr) => {
                    let tail_expr = &body_stash[*tail_ptr];
                    check_expr(ctx, env, body_stash, tail_expr)
                }
                None => ctx.unit_ty(),
            }
        }

        RExprKind::Binary(lhs_ptr, op, rhs_ptr) => {
            let lhs_expr = &body_stash[*lhs_ptr];
            let rhs_expr = &body_stash[*rhs_ptr];
            let lhs_ty = check_expr(ctx, env, body_stash, lhs_expr);
            let rhs_ty = check_expr(ctx, env, body_stash, rhs_expr);
            check_binary_op(ctx, *op, lhs_ty, rhs_ty)
        }

        RExprKind::Unary(_op, operand_ptr) => {
            let operand_expr = &body_stash[*operand_ptr];
            check_expr(ctx, env, body_stash, operand_expr)
        }

        RExprKind::If(cond_ptr, then_ptr, else_ptr) => {
            let cond_expr = &body_stash[*cond_ptr];
            let cond_ty = check_expr(ctx, env, body_stash, cond_expr);
            let bool_ty = ctx.alloc_ty(TyData::Bool);
            ctx.require_eq(cond_ty, bool_ty);

            let result_ty = ctx.fresh_ty_var();
            let then_expr = &body_stash[*then_ptr];
            let then_ty = check_expr(ctx, env, body_stash, then_expr);
            ctx.require_coerce(then_ty, result_ty);

            match else_ptr {
                Some(else_p) => {
                    let else_expr = &body_stash[*else_p];
                    let else_ty = check_expr(ctx, env, body_stash, else_expr);
                    ctx.require_coerce(else_ty, result_ty);
                }
                None => {
                    let unit = ctx.unit_ty();
                    ctx.require_eq(result_ty, unit);
                }
            }
            result_ty
        }

        RExprKind::Match(scrutinee_ptr, arms) => {
            let scrutinee_expr = &body_stash[*scrutinee_ptr];
            let _scrutinee_ty = check_expr(ctx, env, body_stash, scrutinee_expr);
            let result_ty = ctx.fresh_ty_var();

            let arms_slice = &body_stash[*arms];
            for arm in arms_slice {
                let arm_body = &body_stash[arm.body];
                let arm_ty = check_expr(ctx, env, body_stash, arm_body);
                ctx.require_coerce(arm_ty, result_ty);
            }
            result_ty
        }

        RExprKind::Return(val) => {
            if let Some(val_ptr) = val {
                let val_expr = &body_stash[*val_ptr];
                let val_ty = check_expr(ctx, env, body_stash, val_expr);
                ctx.require_coerce(val_ty, env.ret_ty);
            }
            ctx.alloc_ty(TyData::Never)
        }

        RExprKind::Assign(lhs_ptr, rhs_ptr) => {
            let lhs_expr = &body_stash[*lhs_ptr];
            let rhs_expr = &body_stash[*rhs_ptr];
            let lhs_ty = check_place_expr(ctx, env, body_stash, lhs_expr);
            let rhs_ty = check_expr(ctx, env, body_stash, rhs_expr);
            ctx.require_coerce(rhs_ty, lhs_ty);
            ctx.unit_ty()
        }

        RExprKind::Tuple(elems) => {
            let elem_tys: Vec<Ptr<Ty<'db>>> = body_stash[*elems]
                .iter()
                .map(|e| check_expr(ctx, env, body_stash, e))
                .collect();
            let elems_slice = ctx.stash_mut().alloc_slice(&elem_tys);
            ctx.alloc_ty(TyData::Tuple(elems_slice))
        }

        RExprKind::Array(elems) => {
            let result_ty = ctx.fresh_ty_var();
            for elem in &body_stash[*elems] {
                let elem_ty = check_expr(ctx, env, body_stash, elem);
                ctx.require_coerce(elem_ty, result_ty);
            }
            ctx.alloc_ty(TyData::Slice(result_ty))
        }

        RExprKind::Ref(inner_ptr, mutability) => {
            let inner_expr = &body_stash[*inner_ptr];
            let inner_ty = check_expr(ctx, env, body_stash, inner_expr);
            ctx.alloc_ty(TyData::Ref(inner_ty, *mutability, Lifetime::Erased))
        }

        RExprKind::StructLit(res, fields) => check_struct_lit(ctx, env, body_stash, *res, *fields),

        RExprKind::Field(obj_ptr, field_name) => {
            let obj_expr = &body_stash[*obj_ptr];
            let obj_ty = check_expr(ctx, env, body_stash, obj_expr);
            check_field_access(ctx, env, obj_ty, *field_name)
        }

        RExprKind::Loop(body_ptr) => {
            let body_expr = &body_stash[*body_ptr];
            let _ = check_expr(ctx, env, body_stash, body_expr);
            ctx.alloc_ty(TyData::Never)
        }

        RExprKind::While(cond_ptr, body_ptr) => {
            let cond_expr = &body_stash[*cond_ptr];
            let cond_ty = check_expr(ctx, env, body_stash, cond_expr);
            let bool_ty = ctx.alloc_ty(TyData::Bool);
            ctx.require_eq(cond_ty, bool_ty);
            let body_expr = &body_stash[*body_ptr];
            let _ = check_expr(ctx, env, body_stash, body_expr);
            ctx.unit_ty()
        }

        RExprKind::WhileLet(_, scrutinee_ptr, body_ptr) => {
            let scrutinee_expr = &body_stash[*scrutinee_ptr];
            let _ = check_expr(ctx, env, body_stash, scrutinee_expr);
            let body_expr = &body_stash[*body_ptr];
            let _ = check_expr(ctx, env, body_stash, body_expr);
            ctx.unit_ty()
        }

        RExprKind::For(_pat, iter_ptr, body_ptr) => {
            let iter_expr = &body_stash[*iter_ptr];
            let _ = check_expr(ctx, env, body_stash, iter_expr);
            let body_expr = &body_stash[*body_ptr];
            let _ = check_expr(ctx, env, body_stash, body_expr);
            ctx.unit_ty()
        }

        RExprKind::Break(_) | RExprKind::Continue => ctx.alloc_ty(TyData::Never),

        RExprKind::Cast(_expr_ptr, _ty_ast) => {
            // TODO: check the inner expression, resolve the target type
            ctx.fresh_ty_var()
        }

        RExprKind::IfLet(_pat, scrutinee_ptr, then_ptr, else_ptr) => {
            let scrutinee_expr = &body_stash[*scrutinee_ptr];
            let _ = check_expr(ctx, env, body_stash, scrutinee_expr);

            let result_ty = ctx.fresh_ty_var();
            let then_expr = &body_stash[*then_ptr];
            let then_ty = check_expr(ctx, env, body_stash, then_expr);
            ctx.require_coerce(then_ty, result_ty);

            if let Some(else_p) = else_ptr {
                let else_expr = &body_stash[*else_p];
                let else_ty = check_expr(ctx, env, body_stash, else_expr);
                ctx.require_coerce(else_ty, result_ty);
            } else {
                let unit = ctx.unit_ty();
                ctx.require_eq(result_ty, unit);
            }
            result_ty
        }

        RExprKind::Call(_, _) => todo!("function call type checking"),
        RExprKind::MethodCall(_, _, _) => todo!("method call type checking"),
        RExprKind::Index(_, _) => todo!("index expression type checking"),
        RExprKind::Closure(_, _) => todo!("closure type checking"),
        RExprKind::Await(_) => todo!("await type checking"),
        RExprKind::Try(_) => todo!("try operator type checking"),
        RExprKind::Range(_, _) => todo!("range expression type checking"),
        RExprKind::MacroCall(_, _) => todo!("macro call type checking"),
        RExprKind::Missing => ctx.alloc_ty(TyData::Error),
    }
}

fn check_place_expr<'db>(
    ctx: &mut InferCtx<'db>,
    env: &CheckEnv<'db>,
    body_stash: &Stash,
    expr: &RExpr<'db>,
) -> Ptr<Ty<'db>> {
    check_expr(ctx, env, body_stash, expr)
}

// ---------------------------------------------------------------------------
// Statement checking
// ---------------------------------------------------------------------------

fn check_stmt<'db>(
    ctx: &mut InferCtx<'db>,
    env: &CheckEnv<'db>,
    body_stash: &Stash,
    stmt: &RStmt<'db>,
) {
    match &stmt.kind {
        RStmtKind::Let(pat_ptr, _ty_annot, init) => {
            let pat = &body_stash[*pat_ptr];
            let declared_ty = ctx.local_type(extract_bind_local(pat));

            if let Some(init_ptr) = init {
                let init_expr = &body_stash[*init_ptr];
                let init_ty = check_expr(ctx, env, body_stash, init_expr);
                ctx.require_coerce(init_ty, declared_ty);
            }
        }
        RStmtKind::Expr(expr_ptr) => {
            let expr = &body_stash[*expr_ptr];
            check_expr(ctx, env, body_stash, expr);
        }
    }
}

fn extract_bind_local(pat: &RPat) -> u32 {
    match pat.kind {
        RPatKind::Bind(LocalId(id), _) => id,
        RPatKind::Wildcard
        | RPatKind::Path(_)
        | RPatKind::Tuple(_)
        | RPatKind::Struct(_, _)
        | RPatKind::TupleStruct(_, _)
        | RPatKind::Ref(_, _)
        | RPatKind::Literal(_)
        | RPatKind::Or(_)
        | RPatKind::Rest
        | RPatKind::Missing => 0,
    }
}

// ---------------------------------------------------------------------------
// Struct literal and field access
// ---------------------------------------------------------------------------

fn check_struct_lit<'db>(
    ctx: &mut InferCtx<'db>,
    env: &CheckEnv<'db>,
    body_stash: &Stash,
    res: Res<'db>,
    fields: sage_stash::Slice<RFieldInit<'db>>,
) -> Ptr<Ty<'db>> {
    let Res::Def(sym) = res else {
        return ctx.alloc_ty(TyData::Error);
    };

    let SymbolData::Struct(struct_sym) = sym.data() else {
        return ctx.alloc_ty(TyData::Error);
    };

    // TODO(symbol-signatures RFD): use symbol-level query instead of as_ast()
    let Some(struct_ast) = struct_sym.as_ast() else {
        return ctx.alloc_ty(TyData::Error);
    };

    let sig = struct_signature(env.db, struct_ast, env.module, env.source_root);
    let sig_stash = sig.stash();
    let binder = sig.root();

    let type_args: Vec<Ty<'db>> = sig_stash[binder.generics]
        .iter()
        .map(|_| Ty {
            data: ctx.fresh_ty_var_data(),
        })
        .collect();

    let instantiated =
        instantiate_struct_sig(sig_stash, ctx.stash_mut(), binder, type_args.clone());

    let inst_fields = &ctx.stash()[instantiated.fields].to_vec();
    let lit_fields = &body_stash[fields];

    for lit_field in lit_fields {
        let field_val_ty = check_expr(ctx, env, body_stash, &body_stash[lit_field.value]);

        if let Some(sig_field) = inst_fields.iter().find(|f| f.name == lit_field.name) {
            ctx.require_coerce(field_val_ty, sig_field.ty);
        }
    }

    let type_arg_ptrs: Vec<Ptr<Ty<'db>>> = type_args
        .iter()
        .map(|ty| ctx.stash_mut().alloc(*ty))
        .collect();
    let args_slice = ctx.stash_mut().alloc_slice(&type_arg_ptrs);
    ctx.alloc_ty(TyData::Adt(sym, args_slice))
}

fn check_field_access<'db>(
    ctx: &mut InferCtx<'db>,
    env: &CheckEnv<'db>,
    obj_ty: Ptr<Ty<'db>>,
    field_name: Name<'db>,
) -> Ptr<Ty<'db>> {
    let obj_canon = ctx.find_mut(obj_ty);
    let obj_data = ctx.stash()[obj_canon].data;

    let TyData::Adt(sym, type_args_slice) = obj_data else {
        return ctx.fresh_ty_var();
    };

    let SymbolData::Struct(struct_sym) = sym.data() else {
        return ctx.fresh_ty_var();
    };

    // TODO(symbol-signatures RFD): use symbol-level query instead of as_ast()
    let Some(struct_ast) = struct_sym.as_ast() else {
        return ctx.fresh_ty_var();
    };

    let sig = struct_signature(env.db, struct_ast, env.module, env.source_root);
    let sig_stash = sig.stash();
    let binder = sig.root();

    let type_arg_ptrs: Vec<Ptr<Ty<'db>>> = ctx.stash()[type_args_slice].to_vec();
    let type_args: Vec<Ty<'db>> = type_arg_ptrs.iter().map(|p| ctx.stash()[*p]).collect();

    let instantiated = instantiate_struct_sig(sig_stash, ctx.stash_mut(), binder, type_args);

    let inst_fields = &ctx.stash()[instantiated.fields].to_vec();
    if let Some(sig_field) = inst_fields.iter().find(|f| f.name == field_name) {
        sig_field.ty
    } else {
        ctx.fresh_ty_var()
    }
}

// ---------------------------------------------------------------------------
// Literals and paths
// ---------------------------------------------------------------------------

fn check_literal<'db>(ctx: &mut InferCtx<'db>, lit: Literal) -> Ptr<Ty<'db>> {
    match lit {
        Literal::Bool(_) => ctx.alloc_ty(TyData::Bool),
        Literal::Int => ctx.fresh_ty_var(),
        Literal::Float => ctx.fresh_ty_var(),
        Literal::String => {
            let str_ty = ctx.alloc_ty(TyData::Str);
            ctx.alloc_ty(TyData::Ref(
                str_ty,
                sage_ir::types::Mutability::Shared,
                Lifetime::Static,
            ))
        }
        Literal::Char => ctx.alloc_ty(TyData::Char),
    }
}

fn check_path<'db>(ctx: &mut InferCtx<'db>, res: Res<'db>) -> Ptr<Ty<'db>> {
    match res {
        Res::Local(LocalId(id)) => ctx.local_type(id),
        Res::Def(_) => todo!("path to definition (function, constant, variant)"),
        Res::Err => ctx.alloc_ty(TyData::Error),
    }
}

// ---------------------------------------------------------------------------
// Binary operations
// ---------------------------------------------------------------------------

fn check_binary_op<'db>(
    ctx: &mut InferCtx<'db>,
    op: BinaryOp,
    lhs_ty: Ptr<Ty<'db>>,
    rhs_ty: Ptr<Ty<'db>>,
) -> Ptr<Ty<'db>> {
    ctx.require_eq(rhs_ty, lhs_ty);

    match op {
        BinaryOp::Eq
        | BinaryOp::Ne
        | BinaryOp::Lt
        | BinaryOp::Le
        | BinaryOp::Gt
        | BinaryOp::Ge
        | BinaryOp::And
        | BinaryOp::Or => ctx.alloc_ty(TyData::Bool),

        BinaryOp::Add
        | BinaryOp::Sub
        | BinaryOp::Mul
        | BinaryOp::Div
        | BinaryOp::Rem
        | BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::Shl
        | BinaryOp::Shr => lhs_ty,
    }
}

// ---------------------------------------------------------------------------
// Diagnostic rendering
// ---------------------------------------------------------------------------

fn render_diagnostic(db: &dyn Db, stash: &Stash, diag: &Diagnostic) -> String {
    match &diag.kind {
        DiagnosticKind::TypeMismatch { expected, actual } => {
            format!(
                "type mismatch: expected `{}`, found `{}`",
                fmt_ty(db, stash, *expected),
                fmt_ty(db, stash, *actual)
            )
        }
        DiagnosticKind::UnresolvedInferVar { var } => {
            format!("could not infer type for ?{}", var.0)
        }
    }
}

fn fmt_ty(db: &dyn Db, stash: &Stash, ty: Ptr<Ty>) -> String {
    match stash[ty].data {
        TyData::Bool => "bool".to_owned(),
        TyData::Char => "char".to_owned(),
        TyData::Int(i) => match i {
            IntTy::I8 => "i8",
            IntTy::I16 => "i16",
            IntTy::I32 => "i32",
            IntTy::I64 => "i64",
            IntTy::I128 => "i128",
            IntTy::Isize => "isize",
        }
        .to_owned(),
        TyData::Uint(u) => match u {
            UintTy::U8 => "u8",
            UintTy::U16 => "u16",
            UintTy::U32 => "u32",
            UintTy::U64 => "u64",
            UintTy::U128 => "u128",
            UintTy::Usize => "usize",
        }
        .to_owned(),
        TyData::Float(f) => match f {
            FloatTy::F32 => "f32",
            FloatTy::F64 => "f64",
        }
        .to_owned(),
        TyData::Str => "str".to_owned(),
        TyData::Never => "!".to_owned(),
        TyData::Error => "<error>".to_owned(),
        TyData::InferVar(idx) => format!("?{}", idx.0),
        TyData::Param(p) => format!("{p:?}"),
        TyData::Tuple(elems) => {
            let items: Vec<String> = stash[elems].iter().map(|e| fmt_ty(db, stash, *e)).collect();
            format!("({})", items.join(", "))
        }
        TyData::Ref(inner, m, _) => {
            let prefix = match m {
                sage_ir::types::Mutability::Shared => "&",
                sage_ir::types::Mutability::Mut => "&mut ",
            };
            format!("{prefix}{}", fmt_ty(db, stash, inner))
        }
        TyData::Adt(sym, args) => {
            let name = sym
                .name(db)
                .map_or_else(|| "?".to_owned(), |n| n.text(db).clone());
            let type_args: Vec<String> =
                stash[args].iter().map(|a| fmt_ty(db, stash, *a)).collect();
            if type_args.is_empty() {
                name
            } else {
                format!("{name}<{}>", type_args.join(", "))
            }
        }
        TyData::Slice(inner) => format!("[{}]", fmt_ty(db, stash, inner)),
        TyData::Array(inner, _) => format!("[{}; _]", fmt_ty(db, stash, inner)),
        TyData::FnPtr(params, ret) => {
            let ps: Vec<String> = stash[params]
                .iter()
                .map(|p| fmt_ty(db, stash, *p))
                .collect();
            format!("fn({}) -> {}", ps.join(", "), fmt_ty(db, stash, ret))
        }
    }
}
