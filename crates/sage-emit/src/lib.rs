use sage_ir::Db;
use sage_ir::cst::Mutability;
use sage_ir::cst::expr::{BinaryOp as SageBinaryOp, Literal as SageLiteral};
use sage_ir::symbol::{FnSymbol, ModSymbol, StructSymbol, Symbol, SymbolData};
use sage_ir::ty::{self, Ty};
use sage_ir::tytree::{PathResolution, TyBody, TyExprData, TyFieldInit, TyStmt, TyStmtKind};
use sage_stash::Stash;

use rust_ref::*;

pub fn emit_module<'db>(db: &'db dyn Db, module: ModSymbol<'db>) -> Crate<NormalizedDef> {
    let mut emitter = Emitter::new(db);
    let root = emitter.emit_mod(module);
    Crate { root }
}

struct Emitter<'db> {
    db: &'db dyn Db,
    local_def_counter: u32,
    local_def_map: Vec<(Symbol<'db>, u32)>,
}

impl<'db> Emitter<'db> {
    fn new(db: &'db dyn Db) -> Self {
        Self {
            db,
            local_def_counter: 0,
            local_def_map: Vec::new(),
        }
    }

    fn assign_local_id(&mut self, sym: Symbol<'db>) -> u32 {
        let id = self.local_def_counter;
        self.local_def_counter += 1;
        self.local_def_map.push((sym, id));
        id
    }

    fn normalize_def(&self, sym: Symbol<'db>) -> NormalizedDef {
        if let Some(&(_, id)) = self.local_def_map.iter().find(|(s, _)| *s == sym) {
            NormalizedDef::Local(id)
        } else {
            NormalizedDef::External(DefPath {
                krate: "?".to_string(),
                segments: vec![],
            })
        }
    }

    fn emit_mod(&mut self, module: ModSymbol<'db>) -> Module<NormalizedDef> {
        let sym: Symbol<'db> = module.into();
        let local_id = self.assign_local_id(sym);

        let name = sym
            .name(self.db)
            .map(|(n, _)| n.text(self.db).to_string())
            .unwrap_or_default();

        let expanded = module.expanded_module_items(self.db);
        let mut items = Vec::new();

        for &item_sym in expanded {
            if let Some(ref_item) = self.emit_item(item_sym) {
                items.push(ref_item);
            }
        }

        Module {
            def: NormalizedDef::Local(local_id),
            name,
            items,
        }
    }

    fn emit_item(&mut self, sym: Symbol<'db>) -> Option<Item<NormalizedDef>> {
        match sym.data(self.db) {
            SymbolData::FnSymbol(FnSymbol::Local(local_fn)) => {
                Some(Item::Fn(self.emit_fn(sym, local_fn)))
            }
            SymbolData::StructSymbol(StructSymbol::Local(local_struct)) => {
                Some(Item::Struct(self.emit_struct(sym, local_struct)))
            }
            SymbolData::ModSymbol(mod_sym) => Some(Item::Mod(self.emit_mod(mod_sym))),
            _ => None,
        }
    }

    fn emit_fn(
        &mut self,
        sym: Symbol<'db>,
        local_fn: sage_ir::local_syms::fns::LocalFnSym<'db>,
    ) -> FnItem<NormalizedDef> {
        let local_id = self.assign_local_id(sym);
        let name = local_fn.name(self.db).text(self.db).to_string();

        let sig = local_fn.sig(self.db);
        let (sig_stash, binder) = sig.open();
        let fn_sig = &binder.value;

        let params: Vec<Param<NormalizedDef>> = self.emit_fn_params(sig_stash, fn_sig, local_fn);
        let return_ty = self.emit_ty(sig_stash, sig_stash[fn_sig.ret]);

        let body = local_fn.body(self.db);
        let body_expr = self.emit_body(&body);

        FnItem {
            def: NormalizedDef::Local(local_id),
            name,
            params,
            return_ty,
            body: Some(body_expr),
        }
    }

    fn emit_fn_params(
        &self,
        sig_stash: &Stash,
        fn_sig: &ty::FnSig<'db>,
        local_fn: sage_ir::local_syms::fns::LocalFnSym<'db>,
    ) -> Vec<Param<NormalizedDef>> {
        let (cst_stash, cst) = local_fn.cst(self.db).open_deref();
        let cst_params = &cst_stash[cst.params];
        let sig_params = &sig_stash[fn_sig.params];

        sig_params
            .iter()
            .enumerate()
            .map(|(i, &ty_ptr)| {
                let param_name = if i < cst_params.len() {
                    cst_params[i]
                        .name
                        .map(|n| n.text(self.db).to_string())
                        .unwrap_or_else(|| "_".to_string())
                } else {
                    "_".to_string()
                };
                let ty = self.emit_ty(sig_stash, sig_stash[ty_ptr]);
                Param {
                    name: param_name,
                    ty,
                }
            })
            .collect()
    }

    fn emit_struct(
        &mut self,
        sym: Symbol<'db>,
        local_struct: sage_ir::local_syms::structs::LocalStructSym<'db>,
    ) -> StructItem<NormalizedDef> {
        let local_id = self.assign_local_id(sym);
        let name = local_struct.name(self.db).text(self.db).to_string();

        let fields_stashed = local_struct.fields(self.db);
        let (stash, struct_fields) = fields_stashed.open();
        let field_sigs = &stash[struct_fields.fields];

        let fields: Vec<FieldDef<NormalizedDef>> = field_sigs
            .iter()
            .map(|f| {
                let field_name = f.name.text(self.db).to_string();
                let ty = self.emit_ty(stash, stash[f.ty]);
                FieldDef {
                    name: field_name,
                    ty,
                }
            })
            .collect();

        StructItem {
            def: NormalizedDef::Local(local_id),
            name,
            fields,
        }
    }

    fn emit_ty(&self, stash: &Stash, ty: Ty<'db>) -> Type<NormalizedDef> {
        match ty {
            Ty::Bool => Type::Primitive("bool".to_string()),
            Ty::Char => Type::Primitive("char".to_string()),
            Ty::Str => Type::Primitive("str".to_string()),
            Ty::Int(int_ty) => Type::Primitive(
                match int_ty {
                    ty::IntTy::I8 => "i8",
                    ty::IntTy::I16 => "i16",
                    ty::IntTy::I32 => "i32",
                    ty::IntTy::I64 => "i64",
                    ty::IntTy::I128 => "i128",
                    ty::IntTy::Isize => "isize",
                }
                .to_string(),
            ),
            Ty::Uint(uint_ty) => Type::Primitive(
                match uint_ty {
                    ty::UintTy::U8 => "u8",
                    ty::UintTy::U16 => "u16",
                    ty::UintTy::U32 => "u32",
                    ty::UintTy::U64 => "u64",
                    ty::UintTy::U128 => "u128",
                    ty::UintTy::Usize => "usize",
                }
                .to_string(),
            ),
            Ty::Float(float_ty) => Type::Primitive(
                match float_ty {
                    ty::FloatTy::F32 => "f32",
                    ty::FloatTy::F64 => "f64",
                }
                .to_string(),
            ),
            Ty::Adt(sym, type_args) => {
                let target = self.normalize_def(sym);
                let args: Vec<_> = stash[type_args]
                    .iter()
                    .map(|&ty_ptr| self.emit_ty(stash, stash[ty_ptr]))
                    .collect();
                Type::Def {
                    target,
                    type_args: args,
                }
            }
            Ty::Ref(inner_ptr, mutability, _) => Type::Ref {
                mutable: matches!(mutability, Mutability::Mut),
                ty: Box::new(self.emit_ty(stash, stash[inner_ptr])),
            },
            Ty::Tuple(elems) => {
                let elem_slice = &stash[elems];
                if elem_slice.is_empty() {
                    Type::Unit
                } else {
                    Type::Tuple(
                        elem_slice
                            .iter()
                            .map(|&ptr| self.emit_ty(stash, stash[ptr]))
                            .collect(),
                    )
                }
            }
            Ty::Never => Type::Primitive("!".to_string()),
            _ => Type::Primitive(format!("?{:?}", ty)),
        }
    }

    fn emit_body(&self, body: &TyBody<'db>) -> Expr<NormalizedDef> {
        let (stash, body_data) = body.open_deref();
        let root_expr = stash[body_data.root];
        let locals = &stash[body_data.locals];
        self.emit_expr(stash, &root_expr, locals)
    }

    fn emit_expr(
        &self,
        stash: &Stash,
        expr: &sage_ir::tytree::TyExpr<'db>,
        locals: &[sage_ir::tytree::LocalVar<'db>],
    ) -> Expr<NormalizedDef> {
        let expr_ty = self.emit_ty(stash, stash[expr.ty]);

        match &expr.data {
            TyExprData::Literal(lit) => {
                let (kind, value) = self.emit_literal(lit);
                Expr::Literal { kind, value }
            }
            TyExprData::Path(res) => match res {
                PathResolution::Local(local_id) => {
                    let name = if (local_id.0 as usize) < locals.len() {
                        locals[local_id.0 as usize].name.text(self.db).to_string()
                    } else {
                        "_".to_string()
                    };
                    Expr::Local {
                        name,
                        index: local_id.0,
                    }
                }
                PathResolution::Def(sym) => {
                    let target = self.normalize_def(*sym);
                    Expr::Call {
                        target,
                        args: vec![],
                        ty: expr_ty,
                    }
                }
                PathResolution::Err => Expr::Local {
                    name: "?err".to_string(),
                    index: 0,
                },
            },
            TyExprData::Block(stmts_slice, tail) => {
                let stmts: Vec<_> = stash[*stmts_slice]
                    .iter()
                    .filter_map(|s| self.emit_stmt(stash, s, locals))
                    .collect();
                let tail_expr = tail.map(|ptr| {
                    let tail_e = &stash[ptr];
                    Box::new(self.emit_expr(stash, tail_e, locals))
                });
                Expr::Block {
                    stmts,
                    tail: tail_expr,
                    ty: expr_ty,
                }
            }
            TyExprData::Call(callee_ptr, args_slice) => {
                let callee = &stash[*callee_ptr];
                let args: Vec<_> = stash[*args_slice]
                    .iter()
                    .map(|&arg_ptr| {
                        let arg = &stash[arg_ptr];
                        self.emit_expr(stash, arg, locals)
                    })
                    .collect();

                match &callee.data {
                    TyExprData::Path(PathResolution::Def(sym)) => {
                        let target = self.normalize_def(*sym);
                        Expr::Call {
                            target,
                            args,
                            ty: expr_ty,
                        }
                    }
                    _ => {
                        let target = NormalizedDef::External(DefPath {
                            krate: "?".to_string(),
                            segments: vec![],
                        });
                        Expr::Call {
                            target,
                            args,
                            ty: expr_ty,
                        }
                    }
                }
            }
            TyExprData::StructLit(res, fields_slice) => {
                let target = match res {
                    PathResolution::Def(sym) => self.normalize_def(*sym),
                    _ => NormalizedDef::External(DefPath {
                        krate: "?".to_string(),
                        segments: vec![],
                    }),
                };
                let fields: Vec<_> = stash[*fields_slice]
                    .iter()
                    .map(|f: &TyFieldInit<'db>| {
                        let value_expr = &stash[f.value];
                        FieldExpr {
                            name: f.name.text(self.db).to_string(),
                            value: self.emit_expr(stash, value_expr, locals),
                        }
                    })
                    .collect();
                Expr::StructLit {
                    target,
                    fields,
                    ty: expr_ty,
                }
            }
            TyExprData::Field(base_ptr, field_name) => {
                let base = &stash[*base_ptr];
                Expr::Field {
                    expr: Box::new(self.emit_expr(stash, base, locals)),
                    field_name: field_name.text(self.db).to_string(),
                    ty: expr_ty,
                }
            }
            TyExprData::Binary(lhs_ptr, op, rhs_ptr) => {
                let lhs = &stash[*lhs_ptr];
                let rhs = &stash[*rhs_ptr];
                Expr::BinaryOp {
                    op: self.emit_bin_op(*op),
                    lhs: Box::new(self.emit_expr(stash, lhs, locals)),
                    rhs: Box::new(self.emit_expr(stash, rhs, locals)),
                    ty: expr_ty,
                }
            }
            TyExprData::Unary(sage_ir::cst::expr::UnaryOp::Deref, inner_ptr) => {
                let inner = &stash[*inner_ptr];
                Expr::Deref {
                    expr: Box::new(self.emit_expr(stash, inner, locals)),
                    ty: expr_ty,
                }
            }
            TyExprData::Ref(inner_ptr, mutability) => {
                let inner = &stash[*inner_ptr];
                Expr::Ref {
                    mutable: matches!(mutability, Mutability::Mut),
                    expr: Box::new(self.emit_expr(stash, inner, locals)),
                    ty: expr_ty,
                }
            }
            _ => Expr::Literal {
                kind: LiteralKind::Str,
                value: "?unsupported".to_string(),
            },
        }
    }

    fn emit_stmt(
        &self,
        stash: &Stash,
        stmt: &TyStmt<'db>,
        locals: &[sage_ir::tytree::LocalVar<'db>],
    ) -> Option<Stmt<NormalizedDef>> {
        match &stmt.kind {
            TyStmtKind::Let(pat_ptr, ty_ptr, init_ptr) => {
                let pat = &stash[*pat_ptr];
                let (name, index) = match &pat.kind {
                    sage_ir::tytree::TyPatKind::Bind(local_id, _) => {
                        let n = if (local_id.0 as usize) < locals.len() {
                            locals[local_id.0 as usize].name.text(self.db).to_string()
                        } else {
                            "_".to_string()
                        };
                        (n, local_id.0)
                    }
                    _ => ("_".to_string(), 0),
                };
                let ty = match ty_ptr {
                    Some(ptr) => self.emit_ty(stash, stash[*ptr]),
                    None => self.emit_ty(stash, stash[pat.ty]),
                };
                let init = init_ptr.map(|ptr| {
                    let init_expr = &stash[ptr];
                    self.emit_expr(stash, init_expr, locals)
                });
                Some(Stmt::Let {
                    name,
                    index,
                    ty,
                    init,
                })
            }
            TyStmtKind::Expr(expr_ptr) => {
                let e = &stash[*expr_ptr];
                Some(Stmt::Expr(self.emit_expr(stash, e, locals)))
            }
        }
    }

    fn emit_literal(&self, lit: &SageLiteral) -> (LiteralKind, String) {
        match lit {
            SageLiteral::Int => (LiteralKind::Int, "0".to_string()),
            SageLiteral::Float => (LiteralKind::Float, "0.0".to_string()),
            SageLiteral::String => (LiteralKind::Str, "".to_string()),
            SageLiteral::Bool(b) => (LiteralKind::Bool, b.to_string()),
            SageLiteral::Char => (LiteralKind::Char, "".to_string()),
        }
    }

    fn emit_bin_op(&self, op: SageBinaryOp) -> BinOp {
        match op {
            SageBinaryOp::Add => BinOp::Add,
            SageBinaryOp::Sub => BinOp::Sub,
            SageBinaryOp::Mul => BinOp::Mul,
            SageBinaryOp::Div => BinOp::Div,
            SageBinaryOp::Rem => BinOp::Rem,
            SageBinaryOp::Eq => BinOp::Eq,
            SageBinaryOp::Ne => BinOp::Ne,
            SageBinaryOp::Lt => BinOp::Lt,
            SageBinaryOp::Le => BinOp::Le,
            SageBinaryOp::Gt => BinOp::Gt,
            SageBinaryOp::Ge => BinOp::Ge,
            SageBinaryOp::And => BinOp::And,
            SageBinaryOp::Or => BinOp::Or,
            SageBinaryOp::BitAnd => BinOp::BitAnd,
            SageBinaryOp::BitOr => BinOp::BitOr,
            SageBinaryOp::BitXor => BinOp::BitXor,
            SageBinaryOp::Shl => BinOp::Shl,
            SageBinaryOp::Shr => BinOp::Shr,
        }
    }
}
