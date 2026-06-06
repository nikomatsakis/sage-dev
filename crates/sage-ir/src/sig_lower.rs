//! Signature lowering: `TypeRefAst` → `Ty`.
//!
//! `SigLowerCtx` reads from a syntactic signature stash and writes
//! resolved `Ty` nodes into a destination stash. Generic params are
//! tracked so references to them produce `TyData::Param`.

use sage_stash::{Ptr, Stash, Stashed};

use crate::Db;
use crate::generic_param::{AstGenericParam, GenericParam, GenericParamKind};
use crate::item::{EnumAst, FnAst, StructAst};
use crate::module::ModSymbol;
use crate::name::Name;
use crate::resolve::{Namespace, Resolver, SourceRoot};
use crate::ribs::{RibEntry, Ribs};
use crate::sig_ast::*;
use crate::symbol::{Intrinsic, Symbol, SymbolData};
use crate::ty::*;

// ---------------------------------------------------------------------------
// SigLowerCtx
// ---------------------------------------------------------------------------

struct SigLowerCtx<'a, 'db> {
    resolver: Resolver<'db>,
    module: ModSymbol<'db>,
    src: &'a Stash,
    dst: &'a mut Stash,
    ribs: Ribs<'db>,
}

impl<'a, 'db> SigLowerCtx<'a, 'db> {
    fn lower_ptr_type_ref(&mut self, ty_ptr: Ptr<TypeRefAst<'db>>) -> Ty<'db> {
        let ty = self.src[ty_ptr];
        self.lower_type_ref(ty)
    }

    fn lower_type_ref(&mut self, ty: TypeRefAst<'db>) -> Ty<'db> {
        match ty.kind {
            TypeRefAstKind::Path(path_ptr) => self.lower_path_type(self.src[path_ptr]),
            TypeRefAstKind::Reference(inner, m) => {
                let inner_ty = self.lower_ptr_type_ref(inner);
                let inner = self.dst.alloc(inner_ty);
                Ty {
                    data: TyData::Ref(inner, m, Lifetime::Erased),
                }
            }
            TypeRefAstKind::Tuple(elems) => {
                let tys: Vec<_> = self.src[elems]
                    .iter()
                    .map(|e| self.lower_type_ref(*e))
                    .collect();
                let ptrs: Vec<_> = tys.into_iter().map(|t| self.dst.alloc(t)).collect();
                let elems = self.dst.alloc_slice(&ptrs);
                Ty {
                    data: TyData::Tuple(elems),
                }
            }
            TypeRefAstKind::Slice(inner) => {
                let inner_ty = self.lower_ptr_type_ref(inner);
                let inner = self.dst.alloc(inner_ty);
                Ty {
                    data: TyData::Slice(inner),
                }
            }
            TypeRefAstKind::Array(inner) => {
                let inner_ty = self.lower_ptr_type_ref(inner);
                let inner = self.dst.alloc(inner_ty);
                Ty {
                    data: TyData::Array(inner, Const::Literal(0)),
                }
            }
            TypeRefAstKind::Never => Ty {
                data: TyData::Never,
            },
            TypeRefAstKind::Infer | TypeRefAstKind::Error => Ty {
                data: TyData::Error,
            },
        }
    }

    fn lower_path_type(&mut self, path: PathAst<'db>) -> Ty<'db> {
        let segments = &self.src[path.segments];
        if segments.is_empty() {
            return Ty {
                data: TyData::Error,
            };
        }

        let first = &segments[0];
        let rest = &segments[1..];
        let type_args = self.lower_type_args(segments.last().unwrap());

        // Check ribs for the first segment (generic params, Self).
        if let Some(entry) = self.ribs.lookup(first.name, Namespace::Type) {
            return match entry {
                RibEntry::Param(param) => {
                    if rest.is_empty() {
                        Ty {
                            data: TyData::Param(param),
                        }
                    } else {
                        // T::AssocType — not yet supported
                        Ty {
                            data: TyData::Error,
                        }
                    }
                }
                RibEntry::Sym(sym) => {
                    if rest.is_empty() {
                        self.symbol_to_ty(sym, type_args)
                    } else {
                        // Sym::Path — not yet supported in signatures
                        Ty {
                            data: TyData::Error,
                        }
                    }
                }
                RibEntry::SelfTy(self_ty) => {
                    if rest.is_empty() {
                        self_ty
                    } else {
                        // Self::Variant — not yet supported in signatures
                        Ty {
                            data: TyData::Error,
                        }
                    }
                }
                RibEntry::Local(_) => Ty {
                    data: TyData::Error,
                },
            };
        }

        // No rib hit — resolve via module-level path resolution.
        let names: Vec<_> = segments.iter().map(|s| s.name).collect();
        let sym = self
            .resolver
            .resolve_segments(self.module, &names, Namespace::Type);

        match sym {
            Ok(sym) => self.symbol_to_ty(sym, type_args),
            Err(_) => Ty {
                data: TyData::Error,
            },
        }
    }

    fn symbol_to_ty(
        &self,
        sym: Symbol<'db>,
        type_args: sage_stash::Slice<Ptr<Ty<'db>>>,
    ) -> Ty<'db> {
        match sym.data() {
            SymbolData::Intrinsic(intrinsic) => Ty {
                data: intrinsic_to_ty_data(intrinsic),
            },
            _ => Ty {
                data: TyData::Adt(sym, type_args),
            },
        }
    }

    fn lower_type_args(&mut self, seg: &PathSegmentAst<'db>) -> sage_stash::Slice<Ptr<Ty<'db>>> {
        let src_args = &self.src[seg.type_args];
        if src_args.is_empty() {
            return self.dst.alloc_slice(&[]);
        }
        let tys: Vec<_> = src_args.iter().map(|a| self.lower_type_ref(*a)).collect();
        let ptrs: Vec<_> = tys.into_iter().map(|t| self.dst.alloc(t)).collect();
        self.dst.alloc_slice(&ptrs)
    }
}

fn intrinsic_to_ty_data(intrinsic: Intrinsic) -> TyData<'static> {
    match intrinsic {
        Intrinsic::Bool => TyData::Bool,
        Intrinsic::Char => TyData::Char,
        Intrinsic::Str => TyData::Str,
        Intrinsic::Int(i) => TyData::Int(i),
        Intrinsic::Uint(u) => TyData::Uint(u),
        Intrinsic::Float(f) => TyData::Float(f),
    }
}

// ---------------------------------------------------------------------------
// Helpers: create AstGenericParam symbols and bind them in ribs
// ---------------------------------------------------------------------------

fn build_generics_ribs<'db>(
    db: &'db dyn Db,
    src: &Stash,
    generics: sage_stash::Slice<crate::sig_ast::GenericParam<'db>>,
    dst: &mut Stash,
    ribs: &mut Ribs<'db>,
    parent: Symbol<'db>,
) -> sage_stash::Slice<GenericParam<'db>> {
    let params = &src[generics];
    let mut generic_params = Vec::new();
    for (i, param) in params.iter().enumerate() {
        let (name, kind) = match param {
            crate::sig_ast::GenericParam::Type { name, .. } => (*name, GenericParamKind::Type),
            crate::sig_ast::GenericParam::Lifetime { name, .. } => {
                (*name, GenericParamKind::Lifetime)
            }
            crate::sig_ast::GenericParam::Const { name, .. } => (*name, GenericParamKind::Const),
        };
        let ast_param = AstGenericParam::new(db, kind, Some(name), parent, i as u32);
        let gp = GenericParam::Ast(ast_param);
        ribs.add(name, Namespace::Type, RibEntry::Param(gp));
        generic_params.push(gp);
    }
    dst.alloc_slice(&generic_params)
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
    let mut ribs = Ribs::new();
    ribs.push_scope();
    let parent = Symbol::ast(crate::item::ItemAst::Function(fn_ast));
    let generics = build_generics_ribs(db, src, data.generics, &mut dst, &mut ribs, parent);

    if let Some(ty) = self_type {
        use sage_stash::StashCopy;
        let copied = ty.stash_copy(self_type_src, &mut dst);
        let self_name = Name::new(db, "Self".to_owned());
        ribs.add(self_name, Namespace::Type, RibEntry::SelfTy(copied));
    }

    let mut cx = SigLowerCtx {
        resolver: Resolver::new(db, source_root),
        module,
        src,
        dst: &mut dst,
        ribs,
    };

    let param_tys: Vec<_> = src[data.params]
        .iter()
        .map(|p| cx.lower_ptr_type_ref(p.ty))
        .collect();
    let param_ptrs: Vec<_> = param_tys.into_iter().map(|t| cx.dst.alloc(t)).collect();
    let params = cx.dst.alloc_slice(&param_ptrs);

    let ret_ty = match data.ret_type {
        Some(ret_ptr) => cx.lower_ptr_type_ref(ret_ptr),
        None => {
            let unit = cx.dst.alloc_slice(&[]);
            Ty {
                data: TyData::Tuple(unit),
            }
        }
    };
    let ret = cx.dst.alloc(ret_ty);

    let fn_sig = FnSig { params, ret };
    let binder = Binder::new(fn_sig, generics);
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
    let mut ribs = Ribs::new();
    ribs.push_scope();
    let parent = Symbol::ast(crate::item::ItemAst::Struct(struct_ast));
    let generics = build_generics_ribs(db, src, data.generics, &mut dst, &mut ribs, parent);

    let mut cx = SigLowerCtx {
        resolver: Resolver::new(db, source_root),
        module,
        src,
        dst: &mut dst,
        ribs,
    };

    let field_sigs: Vec<_> = src[data.fields]
        .iter()
        .map(|f| {
            let ty_val = cx.lower_ptr_type_ref(f.ty);
            let ty = cx.dst.alloc(ty_val);
            FieldSig { name: f.name, ty }
        })
        .collect();
    let fields = cx.dst.alloc_slice(&field_sigs);

    let struct_sig = StructSig { fields };
    let binder = Binder::new(struct_sig, generics);
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
    let mut ribs = Ribs::new();
    ribs.push_scope();
    let parent = Symbol::ast(crate::item::ItemAst::Enum(enum_ast));
    let generics = build_generics_ribs(db, src, data.generics, &mut dst, &mut ribs, parent);

    let mut cx = SigLowerCtx {
        resolver: Resolver::new(db, source_root),
        module,
        src,
        dst: &mut dst,
        ribs,
    };

    let variant_sigs: Vec<_> = src[data.variants]
        .iter()
        .map(|v| {
            let field_sigs: Vec<_> = src[v.fields]
                .iter()
                .map(|f| {
                    let ty_val = cx.lower_ptr_type_ref(f.ty);
                    let ty = cx.dst.alloc(ty_val);
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
    let binder = Binder::new(enum_sig, generics);
    Stashed::new(dst, binder)
}
