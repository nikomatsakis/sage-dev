//! Signature lowering: `TypeRefAst` → `Ty`.
//!
//! `SigLowerCtx` reads from a syntactic signature stash and writes
//! resolved `Ty` nodes into a destination stash. Generic params are
//! tracked so references to them produce `BoundVar`.

use sage_stash::{Ptr, Stash, Stashed};

use crate::Db;
use crate::item::{EnumAst, FnAst, StructAst};
use crate::module::ModSymbol;
use crate::name::Name;
use crate::resolve::{Namespace, SourceRoot, resolve_name};
use crate::sig_ast::*;
use crate::symbol::Symbol;
use crate::ty::*;
use crate::types::Mutability;

// ---------------------------------------------------------------------------
// SigLowerCtx
// ---------------------------------------------------------------------------

struct SigLowerCtx<'a, 'db> {
    db: &'db dyn Db,
    src: &'a Stash,
    dst: &'a mut Stash,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
    generics: Vec<(Name<'db>, BoundVar)>,
    self_type: Option<Ty<'db>>,
}

impl<'a, 'db> SigLowerCtx<'a, 'db> {
    fn lower_type_ref(&mut self, ty_ptr: Ptr<TypeRefAst<'db>>) -> Ptr<Ty<'db>> {
        let ty = self.src[ty_ptr];
        self.lower_type_ref_data(ty)
    }

    fn lower_type_ref_data(&mut self, ty: TypeRefAst<'db>) -> Ptr<Ty<'db>> {
        match ty.kind {
            TypeRefAstKind::Path(path_ptr) => self.lower_path_type(self.src[path_ptr]),
            TypeRefAstKind::Reference(inner, m) => {
                let inner = self.lower_type_ref(inner);
                self.dst.alloc(Ty {
                    data: TyData::Ref(inner, m, Lifetime::Erased),
                })
            }
            TypeRefAstKind::Tuple(elems) => {
                let tys: Vec<_> = self.src[elems]
                    .iter()
                    .map(|e| {
                        let ptr = self.lower_type_ref_data(*e);
                        self.dst[ptr]
                    })
                    .collect();
                let elems = self.dst.alloc_slice(&tys);
                self.dst.alloc(Ty {
                    data: TyData::Tuple(elems),
                })
            }
            TypeRefAstKind::Slice(inner) => {
                let inner = self.lower_type_ref(inner);
                self.dst.alloc(Ty {
                    data: TyData::Slice(inner),
                })
            }
            TypeRefAstKind::Array(inner) => {
                let inner = self.lower_type_ref(inner);
                self.dst.alloc(Ty {
                    data: TyData::Array(inner, Const::Literal(0)),
                })
            }
            TypeRefAstKind::Never => self.dst.alloc(Ty {
                data: TyData::Never,
            }),
            TypeRefAstKind::Infer | TypeRefAstKind::Error => self.dst.alloc(Ty {
                data: TyData::Error,
            }),
        }
    }

    fn lower_path_type(&mut self, path: PathAst<'db>) -> Ptr<Ty<'db>> {
        let segments = &self.src[path.segments];
        if segments.is_empty() {
            return self.dst.alloc(Ty {
                data: TyData::Error,
            });
        }

        // Single-segment: check generic params first, then primitives, then resolve.
        if segments.len() == 1 {
            let seg = &segments[0];
            let name = seg.name;

            // Generic param?
            for (gname, bv) in &self.generics {
                if *gname == name {
                    return self.dst.alloc(Ty {
                        data: TyData::BoundVar(*bv),
                    });
                }
            }

            // Self type in impl blocks?
            if name.text(self.db) == "Self" {
                if let Some(self_ty) = self.self_type {
                    return self.dst.alloc(self_ty);
                }
            }

            // Primitive?
            if let Some(prim) = recognize_primitive(self.db, name) {
                return self.dst.alloc(Ty { data: prim });
            }

            // Resolve in module
            let type_args = self.lower_type_args(seg);
            return match resolve_name(
                self.db,
                self.module,
                self.source_root,
                name,
                Namespace::Type,
            ) {
                Ok(sym) => self.dst.alloc(Ty {
                    data: TyData::Adt(sym, type_args),
                }),
                Err(_) => self.dst.alloc(Ty {
                    data: TyData::Error,
                }),
            };
        }

        // Multi-segment: resolve the path.
        let names: Vec<_> = segments.iter().map(|s| s.name).collect();
        let salsa_path = crate::types::Path::new(self.db, names, path.span);
        let type_args = self.lower_type_args(segments.last().unwrap());
        match self
            .module
            .resolve_path(self.db, self.source_root, salsa_path, Namespace::Type)
        {
            Ok(sym) => self.dst.alloc(Ty {
                data: TyData::Adt(sym, type_args),
            }),
            Err(_) => self.dst.alloc(Ty {
                data: TyData::Error,
            }),
        }
    }

    fn lower_type_args(&mut self, seg: &PathSegmentAst<'db>) -> sage_stash::Slice<Ty<'db>> {
        let src_args = &self.src[seg.type_args];
        if src_args.is_empty() {
            return self.dst.alloc_slice(&[]);
        }
        let tys: Vec<_> = src_args
            .iter()
            .map(|a| {
                let ptr = self.lower_type_ref_data(*a);
                self.dst[ptr]
            })
            .collect();
        self.dst.alloc_slice(&tys)
    }
}

fn recognize_primitive<'db>(db: &'db dyn Db, name: Name<'db>) -> Option<TyData<'db>> {
    match name.text(db).as_str() {
        "bool" => Some(TyData::Bool),
        "char" => Some(TyData::Char),
        "str" => Some(TyData::Str),
        "i8" => Some(TyData::Int(IntTy::I8)),
        "i16" => Some(TyData::Int(IntTy::I16)),
        "i32" => Some(TyData::Int(IntTy::I32)),
        "i64" => Some(TyData::Int(IntTy::I64)),
        "i128" => Some(TyData::Int(IntTy::I128)),
        "isize" => Some(TyData::Int(IntTy::Isize)),
        "u8" => Some(TyData::Uint(UintTy::U8)),
        "u16" => Some(TyData::Uint(UintTy::U16)),
        "u32" => Some(TyData::Uint(UintTy::U32)),
        "u64" => Some(TyData::Uint(UintTy::U64)),
        "u128" => Some(TyData::Uint(UintTy::U128)),
        "usize" => Some(TyData::Uint(UintTy::Usize)),
        "f32" => Some(TyData::Float(FloatTy::F32)),
        "f64" => Some(TyData::Float(FloatTy::F64)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Helpers: build bound vars from generic params
// ---------------------------------------------------------------------------

fn build_generics_map<'db>(
    src: &Stash,
    generics: sage_stash::Slice<GenericParam<'db>>,
    dst: &mut Stash,
) -> (Vec<(Name<'db>, BoundVar)>, sage_stash::Slice<BoundVarInfo>) {
    let params = &src[generics];
    let mut map = Vec::new();
    let mut bound_vars = Vec::new();
    for (i, param) in params.iter().enumerate() {
        match param {
            GenericParam::Type { name, .. } => {
                map.push((
                    *name,
                    BoundVar {
                        binder_index: 0,
                        param_index: i as u32,
                    },
                ));
                bound_vars.push(BoundVarInfo {
                    kind: BoundVarKind::Type,
                });
            }
            GenericParam::Lifetime { name, .. } => {
                map.push((
                    *name,
                    BoundVar {
                        binder_index: 0,
                        param_index: i as u32,
                    },
                ));
                bound_vars.push(BoundVarInfo {
                    kind: BoundVarKind::Lifetime,
                });
            }
            GenericParam::Const { name, .. } => {
                map.push((
                    *name,
                    BoundVar {
                        binder_index: 0,
                        param_index: i as u32,
                    },
                ));
                bound_vars.push(BoundVarInfo {
                    kind: BoundVarKind::Const,
                });
            }
        }
    }
    let bound_vars = dst.alloc_slice(&bound_vars);
    (map, bound_vars)
}

// ---------------------------------------------------------------------------
// Signature queries
// ---------------------------------------------------------------------------

#[salsa::tracked(returns(ref))]
pub fn fn_signature<'db>(
    db: &'db dyn Db,
    fn_ast: FnAst<'db>,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
) -> Stashed<Binder<'db, FnSig<'db>>> {
    lower_fn_sig(db, fn_ast, module, source_root, None, &Stash::new())
}

/// Lower a function signature with an optional self type for impl-block methods.
///
/// `self_type_src` is the stash that owns any `Slice`/`Ptr` data inside `self_type`.
/// For free functions, pass `None` and an empty stash.
pub fn lower_fn_sig<'db>(
    db: &'db dyn Db,
    fn_ast: FnAst<'db>,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
    self_type: Option<Ty<'db>>,
    self_type_src: &Stash,
) -> Stashed<Binder<'db, FnSig<'db>>> {
    let sig_ast = fn_ast.signature(db);
    let src = sig_ast.stash();
    let data = &src[*sig_ast.root()];

    let mut dst = Stash::new();
    let (generics_map, bound_vars) = build_generics_map(src, data.generics, &mut dst);

    let copied_self_type = self_type.map(|ty| {
        use sage_stash::StashCopy;
        ty.stash_copy(self_type_src, &mut dst)
    });

    let mut cx = SigLowerCtx {
        db,
        src,
        dst: &mut dst,
        module,
        source_root,
        generics: generics_map,
        self_type: copied_self_type,
    };

    let param_tys: Vec<_> = src[data.params]
        .iter()
        .map(|p| {
            let ptr = cx.lower_type_ref(p.ty);
            cx.dst[ptr]
        })
        .collect();
    let params = cx.dst.alloc_slice(&param_tys);

    let ret = match data.ret_type {
        Some(ret_ptr) => cx.lower_type_ref(ret_ptr),
        None => {
            let unit = cx.dst.alloc_slice(&[]);
            cx.dst.alloc(Ty {
                data: TyData::Tuple(unit),
            })
        }
    };

    let fn_sig = FnSig { params, ret };
    let binder = Binder::new(fn_sig, bound_vars);
    Stashed::new(dst, binder)
}

#[salsa::tracked(returns(ref))]
pub fn struct_signature<'db>(
    db: &'db dyn Db,
    struct_ast: StructAst<'db>,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
) -> Stashed<Binder<'db, StructSig<'db>>> {
    let sig_ast = struct_ast.signature(db);
    let src = sig_ast.stash();
    let data = &src[*sig_ast.root()];

    let mut dst = Stash::new();
    let (generics_map, bound_vars) = build_generics_map(src, data.generics, &mut dst);

    let mut cx = SigLowerCtx {
        db,
        src,
        dst: &mut dst,
        module,
        source_root,
        generics: generics_map,
        self_type: None,
    };

    let field_sigs: Vec<_> = src[data.fields]
        .iter()
        .map(|f| {
            let ty = cx.lower_type_ref(f.ty);
            FieldSig { name: f.name, ty }
        })
        .collect();
    let fields = cx.dst.alloc_slice(&field_sigs);

    let struct_sig = StructSig { fields };
    let binder = Binder::new(struct_sig, bound_vars);
    Stashed::new(dst, binder)
}

#[salsa::tracked(returns(ref))]
pub fn enum_signature<'db>(
    db: &'db dyn Db,
    enum_ast: EnumAst<'db>,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
) -> Stashed<Binder<'db, EnumSig<'db>>> {
    let sig_ast = enum_ast.signature(db);
    let src = sig_ast.stash();
    let data = &src[*sig_ast.root()];

    let mut dst = Stash::new();
    let (generics_map, bound_vars) = build_generics_map(src, data.generics, &mut dst);

    let mut cx = SigLowerCtx {
        db,
        src,
        dst: &mut dst,
        module,
        source_root,
        generics: generics_map,
        self_type: None,
    };

    let variant_sigs: Vec<_> = src[data.variants]
        .iter()
        .map(|v| {
            let field_sigs: Vec<_> = src[v.fields]
                .iter()
                .map(|f| {
                    let ty = cx.lower_type_ref(f.ty);
                    FieldSig { name: f.name, ty }
                })
                .collect();
            let fields = cx.dst.alloc_slice(&field_sigs);
            VariantSig {
                name: v.name,
                fields,
            }
        })
        .collect();
    let variants = cx.dst.alloc_slice(&variant_sigs);

    let enum_sig = EnumSig { variants };
    let binder = Binder::new(enum_sig, bound_vars);
    Stashed::new(dst, binder)
}
