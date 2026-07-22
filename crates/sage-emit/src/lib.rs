use sage_ir::Db;
use sage_ir::cst::Mutability;
use sage_ir::cst::expr::{BinaryOp as SageBinaryOp, Literal as SageLiteral};
use sage_ir::local_syms::enums::{LocalEnumSym, enum_variants};
use sage_ir::span::ParseSource;
use sage_ir::symbol::{
    EnumSymbol, FnSymbol, ImplSymbol, ModSymbol, StructSymbol, Symbol, SymbolData, TraitSymbol,
    TypeAliasSymbol,
};
use sage_ir::ty::{self, AliasTy, TraitItemDef, Ty};
use sage_ir::tytree::{PathResolution, TyBody, TyExprData, TyFieldInit, TyStmt, TyStmtKind};
use sage_stash::Stash;

use rust_ref::*;

pub fn emit_module<'db>(db: &'db dyn Db, module: ModSymbol<'db>) -> Crate<NormalizedDef> {
    let mut emitter = Emitter::new(db);
    emitter.pre_register_mod(module);
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

    fn register_local_def(&mut self, sym: Symbol<'db>) {
        assert!(
            !self
                .local_def_map
                .iter()
                .any(|(registered, _)| *registered == sym),
            "local definition registered twice: {sym:?}"
        );
        let id = self.local_def_counter;
        self.local_def_counter += 1;
        self.local_def_map.push((sym, id));
    }

    fn local_id(&self, sym: Symbol<'db>) -> u32 {
        self.local_def_map
            .iter()
            .find_map(|(registered, id)| (*registered == sym).then_some(*id))
            .expect("emitted local definition must be pre-registered")
    }

    fn pre_register_mod(&mut self, module: ModSymbol<'db>) {
        let module_symbol: Symbol<'db> = module.into();
        self.register_local_def(module_symbol);

        for &item in module.expanded_module_items(self.db) {
            match item.data(self.db) {
                SymbolData::FnSymbol(FnSymbol::Local(_))
                | SymbolData::StructSymbol(StructSymbol::Local(_))
                | SymbolData::TypeAliasSymbol(sage_ir::symbol::TypeAliasSymbol::Local(_)) => {
                    self.register_local_def(item);
                }
                SymbolData::EnumSymbol(EnumSymbol::Local(local_enum)) => {
                    self.register_local_def(item);
                    for &variant in enum_variants(self.db, local_enum) {
                        if matches!(variant.data(self.db), SymbolData::VariantSymbol(_)) {
                            self.register_local_def(variant);
                        }
                    }
                }
                SymbolData::ModSymbol(ModSymbol::Local(local_module)) => {
                    self.pre_register_mod(ModSymbol::Local(local_module));
                }
                SymbolData::ImplSymbol(ImplSymbol::Local(local_impl)) => {
                    for function in self.source_impl_functions(local_impl) {
                        self.register_local_def(function.into());
                    }
                }
                SymbolData::TraitSymbol(TraitSymbol::Local(local_trait)) => {
                    let items = local_trait.items(self.db);
                    let associated_types: Vec<_> = items.stash()[items.root().value]
                        .iter()
                        .filter_map(|item| match *item {
                            TraitItemDef::Type(TypeAliasSymbol::Local(associated_type)) => {
                                Some(associated_type)
                            }
                            TraitItemDef::Type(TypeAliasSymbol::Ext(_))
                            | TraitItemDef::Function(_)
                            | TraitItemDef::Const(_) => None,
                        })
                        .collect();
                    for associated_type in associated_types {
                        self.register_local_def(associated_type.into());
                    }
                }
                SymbolData::FnSymbol(FnSymbol::Ext(_))
                | SymbolData::StructSymbol(StructSymbol::Ext(_))
                | SymbolData::EnumSymbol(EnumSymbol::Ext(_))
                | SymbolData::ModSymbol(ModSymbol::Ext(_))
                | SymbolData::VariantSymbol(_)
                | SymbolData::VariantCtorSymbol(_)
                | SymbolData::TraitSymbol(TraitSymbol::Ext(_))
                | SymbolData::TypeAliasSymbol(sage_ir::symbol::TypeAliasSymbol::Ext(_))
                | SymbolData::ConstSymbol(_)
                | SymbolData::StaticSymbol(_)
                | SymbolData::ImplSymbol(ImplSymbol::Ext(_))
                | SymbolData::MacroDefSymbol(_)
                | SymbolData::UseSymbol(_)
                | SymbolData::IntrinsicTypeSymbol(_)
                | SymbolData::MacroInvocationSymbol(_) => {}
            }
        }
    }

    fn normalize_def(&self, sym: Symbol<'db>) -> NormalizedDef {
        // For variant ctor symbols, map to the parent variant (mirrors rustc's behavior)
        let lookup_sym = match sym.data(self.db) {
            SymbolData::VariantCtorSymbol(sage_ir::symbol::VariantCtorSymbol::Local(ctor)) => {
                ctor.variant(self.db).into()
            }
            _ => sym,
        };
        if let Some(&(_, id)) = self.local_def_map.iter().find(|(s, _)| *s == lookup_sym) {
            NormalizedDef::Local(id)
        } else {
            self.external_def_path(lookup_sym)
        }
    }

    fn external_def_path(&self, sym: Symbol<'db>) -> NormalizedDef {
        let ext = match sym.data(self.db) {
            SymbolData::FnSymbol(sage_ir::symbol::FnSymbol::Ext(e)) => e,
            SymbolData::StructSymbol(sage_ir::symbol::StructSymbol::Ext(e)) => e,
            SymbolData::EnumSymbol(sage_ir::symbol::EnumSymbol::Ext(e)) => e,
            SymbolData::VariantSymbol(sage_ir::symbol::VariantSymbol::Ext(e)) => e,
            SymbolData::VariantCtorSymbol(sage_ir::symbol::VariantCtorSymbol::Ext(e)) => e,
            SymbolData::TraitSymbol(sage_ir::symbol::TraitSymbol::Ext(e)) => e,
            SymbolData::ModSymbol(sage_ir::symbol::ModSymbol::Ext(e)) => e,
            SymbolData::TypeAliasSymbol(sage_ir::symbol::TypeAliasSymbol::Ext(e)) => e,
            SymbolData::ConstSymbol(sage_ir::symbol::ConstSymbol::Ext(e)) => e,
            SymbolData::StaticSymbol(sage_ir::symbol::StaticSymbol::Ext(e)) => e,
            _ => {
                return NormalizedDef::External(DefPath {
                    krate: "?".to_string(),
                    segments: vec![],
                });
            }
        };

        self.ext_to_def_path(ext)
    }

    fn ext_to_def_path(&self, ext: sage_ir::symbol::SymExt<'db>) -> NormalizedDef {
        let Some(sdp) = self
            .db
            .tcx()
            .structured_def_path(ext.crate_num(self.db), ext.def_index(self.db))
        else {
            return NormalizedDef::External(DefPath {
                krate: "?".to_string(),
                segments: vec![],
            });
        };

        let segments = sdp
            .segments
            .into_iter()
            .map(|seg| {
                let kind = reference_def_kind(seg.kind).unwrap_or_else(|| {
                    panic!(
                        "Sage cannot represent external path segment kind {:?} for {}",
                        seg.kind, seg.name
                    )
                });
                DefPathSegment {
                    kind,
                    name: seg.name,
                }
            })
            .collect();

        NormalizedDef::External(DefPath {
            krate: sdp.krate,
            segments,
        })
    }

    fn emit_mod(&mut self, module: ModSymbol<'db>) -> Module<NormalizedDef> {
        let sym: Symbol<'db> = module.into();
        let local_id = self.local_id(sym);

        let name = sym
            .name(self.db)
            .map(|(n, _)| n.text(self.db).to_string())
            .unwrap_or_default();

        let expanded = module.expanded_module_items(self.db);
        let mut items = Vec::new();

        for &item_sym in expanded {
            if let SymbolData::ImplSymbol(ImplSymbol::Local(local_impl)) = item_sym.data(self.db) {
                for function in self.source_impl_functions(local_impl) {
                    items.push(Item::Fn(self.emit_fn(function.into(), function)));
                }
                continue;
            }
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

    // ANCHOR: example_sage_source_impl_functions
    fn source_impl_functions(
        &self,
        local_impl: sage_ir::local_syms::impls::LocalImplSym<'db>,
    ) -> Vec<sage_ir::local_syms::fns::LocalFnSym<'db>> {
        let items = local_impl.items(self.db);
        items.stash()[items.root().value]
            .iter()
            .filter_map(|item| match *item {
                TraitItemDef::Function(FnSymbol::Local(function))
                    if !matches!(function.span(self.db).source, ParseSource::Derive(_)) =>
                {
                    Some(function)
                }
                TraitItemDef::Function(FnSymbol::Local(_))
                | TraitItemDef::Function(FnSymbol::Ext(_))
                | TraitItemDef::Type(_)
                | TraitItemDef::Const(_) => None,
            })
            .collect()
    }
    // ANCHOR_END: example_sage_source_impl_functions

    fn emit_item(&mut self, sym: Symbol<'db>) -> Option<Item<NormalizedDef>> {
        match sym.data(self.db) {
            SymbolData::FnSymbol(FnSymbol::Local(local_fn)) => {
                Some(Item::Fn(self.emit_fn(sym, local_fn)))
            }
            SymbolData::StructSymbol(StructSymbol::Local(local_struct)) => {
                Some(Item::Struct(self.emit_struct(sym, local_struct)))
            }
            SymbolData::EnumSymbol(EnumSymbol::Local(local_enum)) => {
                Some(Item::Enum(self.emit_enum(sym, local_enum)))
            }
            SymbolData::ModSymbol(mod_sym) => Some(Item::Mod(self.emit_mod(mod_sym))),
            SymbolData::FnSymbol(FnSymbol::Ext(_))
            | SymbolData::StructSymbol(StructSymbol::Ext(_))
            | SymbolData::EnumSymbol(EnumSymbol::Ext(_))
            | SymbolData::VariantSymbol(_)
            | SymbolData::VariantCtorSymbol(_)
            | SymbolData::TraitSymbol(_)
            | SymbolData::TypeAliasSymbol(_)
            | SymbolData::ConstSymbol(_)
            | SymbolData::StaticSymbol(_)
            | SymbolData::ImplSymbol(_)
            | SymbolData::MacroDefSymbol(_)
            | SymbolData::UseSymbol(_)
            | SymbolData::IntrinsicTypeSymbol(_)
            | SymbolData::MacroInvocationSymbol(_) => None,
        }
    }

    fn emit_fn(
        &mut self,
        sym: Symbol<'db>,
        local_fn: sage_ir::local_syms::fns::LocalFnSym<'db>,
    ) -> FnItem<NormalizedDef> {
        let local_id = self.local_id(sym);
        let name = local_fn.name(self.db).text(self.db).to_string();

        let sig = local_fn.sig(self.db);
        let (sig_stash, binder) = sig.open();
        let fn_sig = &binder.value;

        let params: Vec<Param<NormalizedDef>> = self.emit_fn_params(sig_stash, fn_sig, local_fn);
        let return_ty = self.emit_ty(sig_stash, sig_stash[fn_sig.ret]);

        let checked = local_fn.body(self.db);
        let body_expr = self.emit_body(&checked.body);

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

        let mut params = Vec::new();
        if let Some(receiver) = fn_sig.receiver {
            let owner_ty = self.emit_ty(sig_stash, sig_stash[receiver.owner_self_ty]);
            let ty = match receiver.form {
                ty::MethodReceiver::Value { .. } => owner_ty,
                ty::MethodReceiver::Ref { mutability } => Type::Ref {
                    mutable: matches!(mutability, Mutability::Mut),
                    ty: Box::new(owner_ty),
                },
            };
            let name = cst_params
                .first()
                .and_then(|parameter| parameter.name)
                .map(|name| name.text(self.db).to_string())
                .unwrap_or_else(|| "self".to_string());
            params.push(Param { name, ty });
        }

        let cst_offset = usize::from(fn_sig.receiver.is_some());
        params.extend(sig_params.iter().enumerate().map(|(i, &ty_ptr)| {
            let param_name = if i + cst_offset < cst_params.len() {
                cst_params[i + cst_offset]
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
        }));
        params
    }

    fn emit_struct(
        &mut self,
        sym: Symbol<'db>,
        local_struct: sage_ir::local_syms::structs::LocalStructSym<'db>,
    ) -> StructItem<NormalizedDef> {
        let local_id = self.local_id(sym);
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

    fn emit_enum(
        &mut self,
        sym: Symbol<'db>,
        local_enum: LocalEnumSym<'db>,
    ) -> EnumItem<NormalizedDef> {
        let local_id = self.local_id(sym);
        let name = local_enum.name(self.db).text(self.db).to_string();

        let variant_syms = enum_variants(self.db, local_enum);
        let mut variants = Vec::new();
        for &variant_sym in variant_syms {
            match variant_sym.data(self.db) {
                SymbolData::VariantSymbol(_) => {
                    variants.push(self.emit_variant(variant_sym));
                }
                SymbolData::VariantCtorSymbol(_) => {}
                _ => {}
            }
        }

        EnumItem {
            def: NormalizedDef::Local(local_id),
            name,
            variants,
        }
    }

    fn emit_variant(&mut self, sym: Symbol<'db>) -> VariantDef<NormalizedDef> {
        let local_id = self.local_id(sym);
        let name = sym
            .name(self.db)
            .map(|(n, _)| n.text(self.db).to_string())
            .unwrap_or_default();

        VariantDef {
            def: NormalizedDef::Local(local_id),
            name,
            fields: vec![],
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
            Ty::Alias(alias) => {
                let (kind, target, type_args) = match alias {
                    AliasTy::Named(alias) => {
                        (AliasKind::Named, alias.def, stash[alias.args].to_vec())
                    }
                    AliasTy::Associated(projection) => {
                        let mut arguments = vec![projection.self_ty];
                        arguments.extend(stash[projection.trait_ref.args].iter().copied());
                        arguments.extend(stash[projection.args].iter().copied());
                        (AliasKind::Associated, projection.associated_ty, arguments)
                    }
                    AliasTy::Opaque(alias) => {
                        (AliasKind::Opaque, alias.def, stash[alias.args].to_vec())
                    }
                };
                Type::Alias {
                    kind,
                    target: self.normalize_def(target.into()),
                    type_args: type_args
                        .into_iter()
                        .map(|argument| self.emit_ty(stash, stash[argument]))
                        .collect(),
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
            Ty::Slice(_)
            | Ty::Array(_, _)
            | Ty::FnPtr(_, _)
            | Ty::Param(_)
            | Ty::InferVar(_)
            | Ty::Error(_) => Type::Primitive(format!("?{:?}", ty)),
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
                PathResolution::Error(_) => Expr::Local {
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
            TyExprData::ResolvedCall(target, args_slice) => {
                let args = stash[*args_slice]
                    .iter()
                    .map(|&argument| self.emit_expr(stash, &stash[argument], locals))
                    .collect();
                Expr::Call {
                    target: self.normalize_def(target.function.into()),
                    args,
                    ty: expr_ty,
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
            TyExprData::Field(base_ptr, field) => {
                let base = &stash[*base_ptr];
                let owner = match field.owner {
                    sage_ir::tytree::FieldOwner::Struct(owner) => self.normalize_def(owner.into()),
                    sage_ir::tytree::FieldOwner::Variant(owner) => self.normalize_def(owner.into()),
                };
                Expr::Field {
                    expr: Box::new(self.emit_expr(stash, base, locals)),
                    field: FieldId {
                        owner,
                        index: field.index,
                    },
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
            TyExprData::Unary(sage_ir::cst::expr::UnaryOp::Deref, inner_ptr)
            | TyExprData::Deref(inner_ptr) => {
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
            SageLiteral::Int(name) => (LiteralKind::Int, name.text(self.db).clone()),
            SageLiteral::Float(name) => (LiteralKind::Float, name.text(self.db).clone()),
            SageLiteral::String(name) => (LiteralKind::Str, name.text(self.db).clone()),
            SageLiteral::Bool(b) => (LiteralKind::Bool, b.to_string()),
            SageLiteral::Char(name) => (LiteralKind::Char, name.text(self.db).clone()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use sage_ir::ty::{ProjectionTy, TraitRef};
    use sage_test_harness::with_test_crate;

    #[test]
    fn local_associated_alias_emits_registered_identity_and_ordered_inputs() {
        with_test_crate("trait Marker<T> { type Item; }", |db, root| {
            let [symbol] = root.expanded_module_items(db) else {
                panic!("expected one trait")
            };
            let trait_symbol = match symbol.data(db) {
                SymbolData::TraitSymbol(trait_symbol) => trait_symbol,
                other => panic!("expected trait, found {other:?}"),
            };
            let items = trait_symbol
                .items(db)
                .expect("trait items should be complete");
            let [item] = &items.stash()[items.root().value] else {
                panic!("expected one associated type")
            };
            let associated_ty = match item {
                TraitItemDef::Type(associated_ty) => *associated_ty,
                other => panic!("expected associated type, found {other:?}"),
            };

            let mut emitter = Emitter::new(db);
            emitter.pre_register_mod(root);
            let mut stash = Stash::new();
            let self_ty = stash.alloc(Ty::Uint(ty::UintTy::U8));
            let trait_argument = stash.alloc(Ty::Bool);
            let trait_args = stash.alloc_slice(&[trait_argument]);
            let item_args = stash.alloc_slice(&[]);
            let projection = Ty::Alias(AliasTy::Associated(ProjectionTy {
                associated_ty,
                self_ty,
                trait_ref: TraitRef {
                    trait_sym: trait_symbol,
                    args: trait_args,
                },
                args: item_args,
            }));

            assert_eq!(
                emitter.emit_ty(&stash, projection),
                Type::Alias {
                    kind: AliasKind::Associated,
                    target: NormalizedDef::Local(1),
                    type_args: vec![
                        Type::Primitive("u8".to_owned()),
                        Type::Primitive("bool".to_owned()),
                    ],
                }
            );
        });
    }
}

fn reference_def_kind(kind: sage_ir::symbol::SymExtKind) -> Option<DefKind> {
    use sage_ir::symbol::SymExtKind;

    Some(match kind {
        SymExtKind::Fn => DefKind::Fn,
        SymExtKind::Struct | SymExtKind::TupleStructCtor => DefKind::Struct,
        SymExtKind::Enum => DefKind::Enum,
        SymExtKind::Variant | SymExtKind::VariantCtor => DefKind::Variant,
        SymExtKind::Trait => DefKind::Trait,
        SymExtKind::Impl => DefKind::Impl,
        SymExtKind::Mod => DefKind::Mod,
        SymExtKind::TypeAlias => DefKind::TypeAlias,
        SymExtKind::Const => DefKind::Const,
        SymExtKind::Static => DefKind::Static,
        SymExtKind::MacroDef | SymExtKind::Use | SymExtKind::Other => return None,
    })
}
