use rustc_ast::LitKind as AstLitKind;
use rustc_hir as hir;
use rustc_hir::def::{DefKind as HirDefKind, Res};
use rustc_middle::ty::{self, TyCtxt};
use rustc_span::def_id::{CRATE_DEF_ID, DefId, LocalDefId, LocalModDefId};

use rust_ref::*;

#[derive(Debug, Clone)]
pub enum OracleError {
    Io(String),
    NoOutput,
    CompileError(String),
}

impl std::fmt::Display for OracleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OracleError::Io(msg) => write!(f, "IO error: {}", msg),
            OracleError::NoOutput => write!(f, "no output produced"),
            OracleError::CompileError(msg) => write!(f, "compile error: {}", msg),
        }
    }
}

impl std::error::Error for OracleError {}

struct Emitter<'tcx> {
    tcx: TyCtxt<'tcx>,
    local_def_counter: u32,
    local_def_map: Vec<(DefId, u32)>,
}

struct LocalMap {
    entries: Vec<hir::HirId>,
}

impl LocalMap {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn register(&mut self, hir_id: hir::HirId) -> u32 {
        let index = self.entries.len() as u32;
        self.entries.push(hir_id);
        index
    }

    fn lookup(&self, hir_id: hir::HirId) -> u32 {
        self.entries
            .iter()
            .position(|&id| id == hir_id)
            .map(|i| i as u32)
            .unwrap_or(0)
    }
}

impl<'tcx> Emitter<'tcx> {
    fn new(tcx: TyCtxt<'tcx>) -> Self {
        Self {
            tcx,
            local_def_counter: 0,
            local_def_map: Vec::new(),
        }
    }

    fn assign_local_id(&mut self, def_id: DefId) -> u32 {
        let id = self.local_def_counter;
        self.local_def_counter += 1;
        self.local_def_map.push((def_id, id));
        id
    }

    fn normalize_def(&self, def_id: DefId) -> NormalizedDef {
        if let Some(&(_, id)) = self.local_def_map.iter().find(|(d, _)| *d == def_id) {
            NormalizedDef::Local(id)
        } else {
            NormalizedDef::External(self.def_path_for(def_id))
        }
    }

    fn def_path_for(&self, def_id: DefId) -> DefPath {
        let crate_name = self.tcx.crate_name(def_id.krate).to_string();
        let def_path = self.tcx.def_path(def_id);
        let segments = def_path
            .data
            .iter()
            .filter_map(|elem| {
                let name = elem.data.get_opt_name()?;
                let kind = match &elem.data {
                    rustc_hir::definitions::DefPathData::TypeNs(_) => {
                        let def_kind = self.tcx.def_kind(def_id);
                        match def_kind {
                            HirDefKind::Struct => rust_ref::DefKind::Struct,
                            HirDefKind::Enum => rust_ref::DefKind::Enum,
                            HirDefKind::Trait => rust_ref::DefKind::Trait,
                            HirDefKind::TyAlias => rust_ref::DefKind::TypeAlias,
                            HirDefKind::Mod => rust_ref::DefKind::Mod,
                            _ => rust_ref::DefKind::Struct,
                        }
                    }
                    rustc_hir::definitions::DefPathData::ValueNs(_) => rust_ref::DefKind::Fn,
                    _ => return None,
                };
                Some(DefPathSegment {
                    kind,
                    name: name.to_string(),
                })
            })
            .collect();
        DefPath {
            krate: crate_name,
            segments,
        }
    }

    fn emit_module(&mut self, module_def_id: LocalDefId) -> Module<NormalizedDef> {
        let def_id = module_def_id.to_def_id();
        let local_id = self.assign_local_id(def_id);

        let name = if module_def_id == CRATE_DEF_ID {
            String::new()
        } else {
            self.tcx.item_name(def_id).to_string()
        };

        let mod_def_id = LocalModDefId::new_unchecked(module_def_id);
        let hir_mod = self.tcx.hir_module_items(mod_def_id);
        let mut items = Vec::new();

        for item_id in hir_mod.free_items() {
            let item = self.tcx.hir_item(item_id);
            if let Some(ref_item) = self.emit_item(item) {
                items.push(ref_item);
            }
        }

        Module {
            def: NormalizedDef::Local(local_id),
            name,
            items,
        }
    }

    fn emit_item(&mut self, item: &'tcx hir::Item<'tcx>) -> Option<Item<NormalizedDef>> {
        match &item.kind {
            hir::ItemKind::Fn { sig, body, .. } => Some(Item::Fn(self.emit_fn_item(
                item.owner_id.def_id,
                sig,
                *body,
            ))),
            hir::ItemKind::Struct(_, _, variant) => Some(Item::Struct(
                self.emit_struct_item(item.owner_id.def_id, variant),
            )),
            hir::ItemKind::Enum(_, _, enum_def) => Some(Item::Enum(
                self.emit_enum_item(item.owner_id.def_id, enum_def),
            )),
            hir::ItemKind::Mod(..) => Some(Item::Mod(self.emit_module(item.owner_id.def_id))),
            _ => None,
        }
    }

    fn emit_fn_item(
        &mut self,
        def_id: LocalDefId,
        sig: &hir::FnSig<'tcx>,
        body_id: hir::BodyId,
    ) -> FnItem<NormalizedDef> {
        let local_id = self.assign_local_id(def_id.to_def_id());
        let name = self.tcx.item_name(def_id.to_def_id()).to_string();

        let body = self.tcx.hir_body(body_id);
        let typeck = self.tcx.typeck(def_id);

        let mut locals = LocalMap::new();

        let params: Vec<Param<NormalizedDef>> = body
            .params
            .iter()
            .zip(sig.decl.inputs.iter())
            .map(|(param, _ty_hir)| {
                let param_name = match param.pat.kind {
                    hir::PatKind::Binding(_, hir_id, ident, _) => {
                        locals.register(hir_id);
                        ident.name.to_string()
                    }
                    _ => "_".to_string(),
                };
                let param_ty = typeck.node_type(param.hir_id);
                Param {
                    name: param_name,
                    ty: self.emit_type(param_ty),
                }
            })
            .collect();

        let return_ty = match sig.decl.output {
            hir::FnRetTy::DefaultReturn(_) => Type::Unit,
            hir::FnRetTy::Return(_) => {
                let fn_sig = self.tcx.fn_sig(def_id).instantiate_identity().skip_binder();
                let ret_ty = self.tcx.normalize_erasing_regions(
                    ty::TypingEnv::post_analysis(self.tcx, def_id),
                    fn_sig.output(),
                );
                self.emit_type(ret_ty)
            }
        };

        let body_expr = self.emit_expr_with_locals(body.value, typeck, &mut locals);

        FnItem {
            def: NormalizedDef::Local(local_id),
            name,
            params,
            return_ty,
            body: Some(body_expr),
        }
    }

    fn emit_struct_item(
        &mut self,
        def_id: LocalDefId,
        variant: &hir::VariantData<'tcx>,
    ) -> StructItem<NormalizedDef> {
        let local_id = self.assign_local_id(def_id.to_def_id());
        let name = self.tcx.item_name(def_id.to_def_id()).to_string();

        let fields = variant
            .fields()
            .iter()
            .map(|field| {
                let field_name = field.ident.name.to_string();
                let field_ty = self.tcx.type_of(field.def_id).instantiate_identity();
                FieldDef {
                    name: field_name,
                    ty: self.emit_type(field_ty),
                }
            })
            .collect();

        StructItem {
            def: NormalizedDef::Local(local_id),
            name,
            fields,
        }
    }

    fn emit_enum_item(
        &mut self,
        def_id: LocalDefId,
        enum_def: &hir::EnumDef<'tcx>,
    ) -> EnumItem<NormalizedDef> {
        let local_id = self.assign_local_id(def_id.to_def_id());
        let name = self.tcx.item_name(def_id.to_def_id()).to_string();

        let variants = enum_def
            .variants
            .iter()
            .map(|variant| {
                let variant_def_id = variant.def_id.to_def_id();
                let variant_local_id = self.assign_local_id(variant_def_id);
                let variant_name = variant.ident.name.to_string();
                VariantDef {
                    def: NormalizedDef::Local(variant_local_id),
                    name: variant_name,
                    fields: vec![],
                }
            })
            .collect();

        EnumItem {
            def: NormalizedDef::Local(local_id),
            name,
            variants,
        }
    }

    fn emit_type(&self, ty: ty::Ty<'tcx>) -> Type<NormalizedDef> {
        match ty.kind() {
            ty::TyKind::Bool => Type::Primitive("bool".to_string()),
            ty::TyKind::Char => Type::Primitive("char".to_string()),
            ty::TyKind::Int(int_ty) => Type::Primitive(int_ty.name_str().to_string()),
            ty::TyKind::Uint(uint_ty) => Type::Primitive(uint_ty.name_str().to_string()),
            ty::TyKind::Float(float_ty) => Type::Primitive(float_ty.name_str().to_string()),
            ty::TyKind::Str => Type::Primitive("str".to_string()),
            ty::TyKind::Adt(adt_def, args) => {
                let target = self.normalize_def(adt_def.did());
                let type_args: Vec<_> = args.types().map(|t| self.emit_type(t)).collect();
                Type::Def { target, type_args }
            }
            ty::TyKind::Ref(_, inner_ty, mutability) => Type::Ref {
                mutable: mutability.is_mut(),
                ty: Box::new(self.emit_type(*inner_ty)),
            },
            ty::TyKind::Tuple(tys) => {
                if tys.is_empty() {
                    Type::Unit
                } else {
                    Type::Tuple(tys.iter().map(|t| self.emit_type(t)).collect())
                }
            }
            ty::TyKind::Never => Type::Primitive("!".to_string()),
            _ => Type::Primitive(format!("?{:?}", ty.kind())),
        }
    }

    fn emit_expr_with_locals(
        &self,
        expr: &'tcx hir::Expr<'tcx>,
        typeck: &'tcx ty::TypeckResults<'tcx>,
        locals: &mut LocalMap,
    ) -> Expr<NormalizedDef> {
        let expr_ty = typeck.expr_ty(expr);

        match &expr.kind {
            hir::ExprKind::Path(qpath) => {
                let res = typeck.qpath_res(qpath, expr.hir_id);
                match res {
                    Res::Local(hir_id) => {
                        let name = match qpath {
                            hir::QPath::Resolved(_, path) => path
                                .segments
                                .last()
                                .map_or("_".to_string(), |s| s.ident.name.to_string()),
                            _ => "_".to_string(),
                        };
                        let index = locals.lookup(hir_id);
                        Expr::Local { name, index }
                    }
                    Res::Def(HirDefKind::Fn | HirDefKind::AssocFn, def_id) => Expr::Call {
                        target: self.normalize_def(def_id),
                        args: vec![],
                        ty: self.emit_type(expr_ty),
                    },
                    _ => Expr::Local {
                        name: format!("?{:?}", res),
                        index: 0,
                    },
                }
            }
            hir::ExprKind::Lit(lit) => {
                let (kind, value) = self.emit_literal(&lit.node);
                Expr::Literal { kind, value }
            }
            hir::ExprKind::Binary(op, lhs, rhs) => {
                let bin_op = self.emit_bin_op(op.node);
                Expr::BinaryOp {
                    op: bin_op,
                    lhs: Box::new(self.emit_expr_with_locals(lhs, typeck, locals)),
                    rhs: Box::new(self.emit_expr_with_locals(rhs, typeck, locals)),
                    ty: self.emit_type(expr_ty),
                }
            }
            hir::ExprKind::Call(callee, args) => {
                if let hir::ExprKind::Path(qpath) = &callee.kind {
                    let res = typeck.qpath_res(qpath, callee.hir_id);
                    if let Some(def_id) = res.opt_def_id() {
                        let target_def_id = match self.tcx.def_kind(def_id) {
                            HirDefKind::Ctor(..) => self.tcx.parent(def_id),
                            _ => def_id,
                        };
                        let emitted_args: Vec<_> = args
                            .iter()
                            .map(|a| self.emit_expr_with_locals(a, typeck, locals))
                            .collect();
                        return Expr::Call {
                            target: self.normalize_def(target_def_id),
                            args: emitted_args,
                            ty: self.emit_type(expr_ty),
                        };
                    }
                }
                let emitted_args: Vec<_> = args
                    .iter()
                    .map(|a| self.emit_expr_with_locals(a, typeck, locals))
                    .collect();
                Expr::Call {
                    target: NormalizedDef::External(DefPath {
                        krate: "?".to_string(),
                        segments: vec![],
                    }),
                    args: emitted_args,
                    ty: self.emit_type(expr_ty),
                }
            }
            hir::ExprKind::Struct(qpath, fields, _base) => {
                let res = typeck.qpath_res(qpath, expr.hir_id);
                let target = match res.opt_def_id() {
                    Some(did) => {
                        let adt_def_id = match self.tcx.def_kind(did) {
                            HirDefKind::Ctor(..) => self.tcx.parent(did),
                            _ => did,
                        };
                        self.normalize_def(adt_def_id)
                    }
                    None => NormalizedDef::External(DefPath {
                        krate: "?".to_string(),
                        segments: vec![],
                    }),
                };
                let emitted_fields: Vec<_> = fields
                    .iter()
                    .map(|f| FieldExpr {
                        name: f.ident.name.to_string(),
                        value: self.emit_expr_with_locals(f.expr, typeck, locals),
                    })
                    .collect();
                Expr::StructLit {
                    target,
                    fields: emitted_fields,
                    ty: self.emit_type(expr_ty),
                }
            }
            hir::ExprKind::Field(base, field) => Expr::Field {
                expr: Box::new(self.emit_expr_with_locals(base, typeck, locals)),
                field_name: field.name.to_string(),
                ty: self.emit_type(expr_ty),
            },
            hir::ExprKind::Block(block, _) => {
                let stmts: Vec<_> = block
                    .stmts
                    .iter()
                    .filter_map(|s| self.emit_stmt_with_locals(s, typeck, locals))
                    .collect();
                let tail = block
                    .expr
                    .map(|e| Box::new(self.emit_expr_with_locals(e, typeck, locals)));
                Expr::Block {
                    stmts,
                    tail,
                    ty: self.emit_type(expr_ty),
                }
            }
            hir::ExprKind::Unary(hir::UnOp::Deref, inner) => Expr::Deref {
                expr: Box::new(self.emit_expr_with_locals(inner, typeck, locals)),
                ty: self.emit_type(expr_ty),
            },
            hir::ExprKind::AddrOf(_, mutability, inner) => Expr::Ref {
                mutable: mutability.is_mut(),
                expr: Box::new(self.emit_expr_with_locals(inner, typeck, locals)),
                ty: self.emit_type(expr_ty),
            },
            hir::ExprKind::DropTemps(inner) => self.emit_expr_with_locals(inner, typeck, locals),
            _ => Expr::Literal {
                kind: LiteralKind::Str,
                value: "?unsupported".to_string(),
            },
        }
    }

    fn emit_stmt_with_locals(
        &self,
        stmt: &'tcx hir::Stmt<'tcx>,
        typeck: &'tcx ty::TypeckResults<'tcx>,
        locals: &mut LocalMap,
    ) -> Option<Stmt<NormalizedDef>> {
        match &stmt.kind {
            hir::StmtKind::Let(local) => {
                let (name, index) = match local.pat.kind {
                    hir::PatKind::Binding(_, hir_id, ident, _) => {
                        let idx = locals.register(hir_id);
                        (ident.name.to_string(), idx)
                    }
                    _ => ("_".to_string(), 0),
                };
                let ty = self.emit_type(typeck.node_type(local.pat.hir_id));
                let init = local
                    .init
                    .map(|e| self.emit_expr_with_locals(e, typeck, locals));
                Some(Stmt::Let {
                    name,
                    index,
                    ty,
                    init,
                })
            }
            hir::StmtKind::Expr(e) | hir::StmtKind::Semi(e) => {
                Some(Stmt::Expr(self.emit_expr_with_locals(e, typeck, locals)))
            }
            hir::StmtKind::Item(_) => None,
        }
    }

    fn emit_literal(&self, lit: &AstLitKind) -> (LiteralKind, String) {
        match lit {
            AstLitKind::Int(val, _) => (LiteralKind::Int, val.to_string()),
            AstLitKind::Float(sym, _) => (LiteralKind::Float, sym.to_string()),
            AstLitKind::Bool(b) => (LiteralKind::Bool, b.to_string()),
            AstLitKind::Char(c) => (LiteralKind::Char, c.to_string()),
            AstLitKind::Str(sym, _) => (LiteralKind::Str, sym.to_string()),
            AstLitKind::ByteStr(_, _) => (LiteralKind::Str, "<bytestr>".to_string()),
            AstLitKind::Byte(b) => (LiteralKind::Int, b.to_string()),
            AstLitKind::CStr(_, _) => (LiteralKind::Str, "<cstr>".to_string()),
            AstLitKind::Err(_) => (LiteralKind::Str, "<error>".to_string()),
        }
    }

    fn emit_bin_op(&self, op: hir::BinOpKind) -> BinOp {
        match op {
            hir::BinOpKind::Add => BinOp::Add,
            hir::BinOpKind::Sub => BinOp::Sub,
            hir::BinOpKind::Mul => BinOp::Mul,
            hir::BinOpKind::Div => BinOp::Div,
            hir::BinOpKind::Rem => BinOp::Rem,
            hir::BinOpKind::Eq => BinOp::Eq,
            hir::BinOpKind::Ne => BinOp::Ne,
            hir::BinOpKind::Lt => BinOp::Lt,
            hir::BinOpKind::Le => BinOp::Le,
            hir::BinOpKind::Gt => BinOp::Gt,
            hir::BinOpKind::Ge => BinOp::Ge,
            hir::BinOpKind::And => BinOp::And,
            hir::BinOpKind::Or => BinOp::Or,
            hir::BinOpKind::BitAnd => BinOp::BitAnd,
            hir::BinOpKind::BitOr => BinOp::BitOr,
            hir::BinOpKind::BitXor => BinOp::BitXor,
            hir::BinOpKind::Shl => BinOp::Shl,
            hir::BinOpKind::Shr => BinOp::Shr,
        }
    }
}

pub fn emit_crate(tcx: TyCtxt<'_>) -> Result<Crate<NormalizedDef>, OracleError> {
    if tcx.dcx().has_errors().is_some() {
        return Err(OracleError::CompileError(
            "input file has compilation errors".to_string(),
        ));
    }

    let mut emitter = Emitter::new(tcx);
    let root = emitter.emit_module(CRATE_DEF_ID);

    Ok(Crate { root })
}
