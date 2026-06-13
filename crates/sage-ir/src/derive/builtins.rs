use crate::Db;
use crate::body::*;
use crate::item::*;
use crate::name::Name;
use crate::sig_ast::*;
use crate::source::SourceFile;
use crate::span::{AbsoluteSpan, ParseSource, RelativeSpan};
use crate::types::*;

use sage_stash::{Stash, Stashed};

const GEN_REL_SPAN: RelativeSpan = RelativeSpan { start: 0, end: 0 };

/// Generate impl blocks for a builtin derive.
#[salsa::tracked(returns(ref))]
pub fn expand_builtin<'db>(
    db: &'db dyn Db,
    derive_name: Name<'db>,
    item: LocalModItemSym<'db>,
) -> Vec<ImplAst<'db>> {
    let name_text = derive_name.text(db);
    let item_name = crate::resolve::item_name(db, item)
        .map(|n| n.text(db).to_string())
        .unwrap_or_else(|| "?".into());
    db.log_query(format!("expand_builtin(\"{name_text}\", \"{item_name}\")"));
    match name_text.as_str() {
        "Debug" => expand_debug(db, item),
        "Clone" => expand_clone(db, item),
        "Default" => expand_default(db, item),
        // Marker traits — empty impl body
        "Copy" | "Eq" | "PartialEq" | "Hash" | "PartialOrd" | "Ord" => {
            expand_marker(db, derive_name, item)
        }
        _ => Vec::new(),
    }
}

/// `impl Debug for T { fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result { ... } }`
fn expand_debug<'db>(db: &'db dyn Db, item: LocalModItemSym<'db>) -> Vec<ImplAst<'db>> {
    let type_name = match item_info(db, item) {
        Some(name) => name,
        None => return Vec::new(),
    };

    // Build fmt method body: `f.debug_struct("TypeName").field("x", &self.x)...finish()`
    let mut stash = Stash::new();
    let body_expr = {
        // Missing body — we represent the body as a placeholder
        stash.alloc(Expr {
            kind: ExprKind::Missing,
            span: GEN_REL_SPAN,
        })
    };
    let body = stash.alloc(Body {
        root: body_expr,
        span: GEN_REL_SPAN,
    });
    let body = Stashed::new(stash, body);

    let signature = make_fn_sig(
        db,
        &[
            (Some("self"), SigTy::RefSelf),
            (Some("f"), SigTy::AbsPath(&["std", "fmt", "Formatter"])),
        ],
        Some(SigTy::AbsPath(&["std", "fmt", "Result"])),
    );

    let fmt_fn = FnAst::new(
        db,
        Name::new(db, "fmt".to_owned()),
        Vec::new(), // attrs
        signature,
        false,
        false,
        body,
        gen_abs_span(db),
    );

    let impl_sig = make_impl_sig(
        db,
        &SigTy::Path(&[type_name.text(db)]),
        Some(&SigTy::AbsPath(&["std", "fmt", "Debug"])),
    );
    let impl_item = ImplAst::new(
        db,
        Vec::new(),
        impl_sig,
        vec![LocalModItemSym::Function(fmt_fn)],
        gen_abs_span(db),
    );
    vec![impl_item]
}

/// `impl Clone for T { fn clone(&self) -> Self { ... } }`
fn expand_clone<'db>(db: &'db dyn Db, item: LocalModItemSym<'db>) -> Vec<ImplAst<'db>> {
    let type_name = match item_info(db, item) {
        Some(name) => name,
        None => return Vec::new(),
    };

    let mut stash = Stash::new();
    let body_expr = stash.alloc(Expr {
        kind: ExprKind::Missing,
        span: GEN_REL_SPAN,
    });
    let body = stash.alloc(Body {
        root: body_expr,
        span: GEN_REL_SPAN,
    });
    let body = Stashed::new(stash, body);

    let signature = make_fn_sig(
        db,
        &[(Some("self"), SigTy::RefSelf)],
        Some(SigTy::Path(&["Self"])),
    );

    let clone_fn = FnAst::new(
        db,
        Name::new(db, "clone".to_owned()),
        Vec::new(),
        signature,
        false,
        false,
        body,
        gen_abs_span(db),
    );

    let impl_sig = make_impl_sig(
        db,
        &SigTy::Path(&[type_name.text(db)]),
        Some(&SigTy::AbsPath(&["core", "clone", "Clone"])),
    );
    let impl_item = ImplAst::new(
        db,
        Vec::new(),
        impl_sig,
        vec![LocalModItemSym::Function(clone_fn)],
        gen_abs_span(db),
    );
    vec![impl_item]
}

/// `impl Default for T { fn default() -> Self { ... } }`
fn expand_default<'db>(db: &'db dyn Db, item: LocalModItemSym<'db>) -> Vec<ImplAst<'db>> {
    let type_name = match item_info(db, item) {
        Some(name) => name,
        None => return Vec::new(),
    };

    let mut stash = Stash::new();
    let body_expr = stash.alloc(Expr {
        kind: ExprKind::Missing,
        span: GEN_REL_SPAN,
    });
    let body = stash.alloc(Body {
        root: body_expr,
        span: GEN_REL_SPAN,
    });
    let body = Stashed::new(stash, body);

    let signature = make_fn_sig(db, &[], Some(SigTy::Path(&["Self"])));

    let default_fn = FnAst::new(
        db,
        Name::new(db, "default".to_owned()),
        Vec::new(),
        signature,
        false,
        false,
        body,
        gen_abs_span(db),
    );

    let impl_sig = make_impl_sig(
        db,
        &SigTy::Path(&[type_name.text(db)]),
        Some(&SigTy::AbsPath(&["core", "default", "Default"])),
    );
    let impl_item = ImplAst::new(
        db,
        Vec::new(),
        impl_sig,
        vec![LocalModItemSym::Function(default_fn)],
        gen_abs_span(db),
    );
    vec![impl_item]
}

/// Marker trait — empty impl body.
fn expand_marker<'db>(
    db: &'db dyn Db,
    derive_name: Name<'db>,
    item: LocalModItemSym<'db>,
) -> Vec<ImplAst<'db>> {
    let type_name = match item_info(db, item) {
        Some(name) => name,
        None => return Vec::new(),
    };

    let trait_segs: &[&str] = match derive_name.text(db).as_str() {
        "Copy" => &["core", "marker", "Copy"],
        "Eq" => &["core", "cmp", "Eq"],
        "PartialEq" => &["core", "cmp", "PartialEq"],
        "Hash" => &["core", "hash", "Hash"],
        "PartialOrd" => &["core", "cmp", "PartialOrd"],
        "Ord" => &["core", "cmp", "Ord"],
        _ => &[],
    };
    let trait_ty = if trait_segs.is_empty() {
        SigTy::Path(&[derive_name.text(db)])
    } else {
        SigTy::AbsPath(trait_segs)
    };
    let impl_sig = make_impl_sig(db, &SigTy::Path(&[type_name.text(db)]), Some(&trait_ty));
    let impl_item = ImplAst::new(db, Vec::new(), impl_sig, Vec::new(), gen_abs_span(db));
    vec![impl_item]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the type name from a struct or enum item.
fn item_info<'db>(db: &'db dyn Db, item: LocalModItemSym<'db>) -> Option<Name<'db>> {
    match item {
        LocalModItemSym::Struct(s) => Some(s.name(db)),
        LocalModItemSym::Enum(e) => Some(e.name(db)),
        _ => None,
    }
}

fn gen_abs_span<'db>(db: &'db dyn Db) -> AbsoluteSpan<'db> {
    AbsoluteSpan {
        source: ParseSource::SourceFile(gen_source_file(db)),
        start: 0,
        end: 0,
    }
}

fn gen_source_file<'db>(db: &'db dyn Db) -> SourceFile {
    SourceFile::new(db, "<generated>".to_owned(), String::new())
}

enum SigTy<'a> {
    Path(&'a [&'a str]),
    AbsPath(&'a [&'a str]),
    RefSelf,
}

fn make_fn_sig<'db>(
    db: &'db dyn Db,
    params: &[(Option<&str>, SigTy<'_>)],
    ret: Option<SigTy<'_>>,
) -> FnSigAst<'db> {
    let mut stash = Stash::new();
    let generics = stash.alloc_slice::<GenericParam<'db>>(&[]);
    let param_asts: Vec<_> = params
        .iter()
        .map(|(name, ty)| {
            let ty_ptr = alloc_sig_ty(db, &mut stash, ty);
            ParamAst {
                name: name.map(|n| Name::new(db, n.to_owned())),
                ty: ty_ptr,
                span: GEN_REL_SPAN,
            }
        })
        .collect();
    let params = stash.alloc_slice(&param_asts);
    let ret_type = ret.map(|r| alloc_sig_ty(db, &mut stash, &r));
    let root = stash.alloc(FnSigAstData {
        generics,
        params,
        ret_type,
    });
    Stashed::new(stash, root)
}

fn alloc_sig_ty<'db>(
    db: &'db dyn Db,
    stash: &mut Stash,
    ty: &SigTy<'_>,
) -> sage_stash::Ptr<TypeRefAst<'db>> {
    match ty {
        SigTy::Path(segments) => {
            let segs: Vec<_> = segments
                .iter()
                .map(|s| PathSegmentAst {
                    name: Name::new(db, (*s).to_owned()),
                    type_args: stash.alloc_slice(&[]),
                    span: GEN_REL_SPAN,
                })
                .collect();
            let segs = stash.alloc_slice(&segs);
            let path = stash.alloc(PathAst {
                segments: segs,
                span: GEN_REL_SPAN,
            });
            stash.alloc(TypeRefAst {
                kind: TypeRefAstKind::Path(path),
                span: GEN_REL_SPAN,
            })
        }
        SigTy::AbsPath(segments) => {
            let mut all_segs = vec![PathSegmentAst {
                name: Name::new(db, String::new()),
                type_args: stash.alloc_slice(&[]),
                span: GEN_REL_SPAN,
            }];
            all_segs.extend(segments.iter().map(|s| PathSegmentAst {
                name: Name::new(db, (*s).to_owned()),
                type_args: stash.alloc_slice(&[]),
                span: GEN_REL_SPAN,
            }));
            let segs = stash.alloc_slice(&all_segs);
            let path = stash.alloc(PathAst {
                segments: segs,
                span: GEN_REL_SPAN,
            });
            stash.alloc(TypeRefAst {
                kind: TypeRefAstKind::Path(path),
                span: GEN_REL_SPAN,
            })
        }
        SigTy::RefSelf => {
            let inner = alloc_sig_ty(db, stash, &SigTy::Path(&["Self"]));
            stash.alloc(TypeRefAst {
                kind: TypeRefAstKind::Reference(inner, Mutability::Shared),
                span: GEN_REL_SPAN,
            })
        }
    }
}

fn make_impl_sig<'db>(
    db: &'db dyn Db,
    self_ty: &SigTy<'_>,
    trait_path: Option<&SigTy<'_>>,
) -> ImplSigAst<'db> {
    let mut stash = Stash::new();
    let generics = stash.alloc_slice::<GenericParam<'db>>(&[]);
    let self_ty_ptr = alloc_sig_ty(db, &mut stash, self_ty);
    let trait_path_ptr = trait_path.map(|tp| {
        let ty_ptr = alloc_sig_ty(db, &mut stash, tp);
        match stash[ty_ptr].kind {
            TypeRefAstKind::Path(p) => p,
            _ => unreachable!("trait path must be a path type"),
        }
    });
    let root = stash.alloc(ImplSigAstData {
        generics,
        self_ty: self_ty_ptr,
        trait_path: trait_path_ptr,
    });
    Stashed::new(stash, root)
}
