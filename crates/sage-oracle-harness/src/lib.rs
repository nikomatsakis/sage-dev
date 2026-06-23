#![feature(rustc_private)]

use std::path::{Path, PathBuf};

use rust_ref::{Crate, Expr, FieldExpr, FnItem, Item, Module, NormalizedDef, Stmt, Type};

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-fixtures/oracle")
}

#[derive(Debug)]
pub enum Fixture {
    SingleFile(PathBuf),
    Directory { entry: PathBuf, files: Vec<PathBuf> },
}

impl Fixture {
    pub fn name(&self) -> String {
        let base = fixtures_dir();
        match self {
            Fixture::SingleFile(path) => path
                .strip_prefix(&base)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string(),
            Fixture::Directory { entry, .. } => entry
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.strip_prefix(&base).ok())
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| entry.to_string_lossy().to_string()),
        }
    }

    pub fn oracle_output(&self) -> Crate<NormalizedDef> {
        let entry = match self {
            Fixture::SingleFile(path) => path.clone(),
            Fixture::Directory { entry, .. } => entry.clone(),
        };
        sage_oracle::analyze_file(&entry)
            .unwrap_or_else(|e| panic!("oracle failed on {}: {}", entry.display(), e))
    }

    pub fn sage_output(&self) -> Crate<NormalizedDef> {
        match self {
            Fixture::SingleFile(path) => {
                let source = std::fs::read_to_string(path).unwrap();
                sage_test_harness::with_test_crate(&source, |db, root| {
                    sage_emit::emit_module(db, root)
                })
            }
            Fixture::Directory { entry, files } => {
                let src_dir = entry.parent().unwrap();
                let pairs: Vec<(String, String)> = files
                    .iter()
                    .map(|f| {
                        let rel = f
                            .strip_prefix(src_dir)
                            .unwrap()
                            .to_string_lossy()
                            .to_string();
                        let content = std::fs::read_to_string(f).unwrap();
                        (rel, content)
                    })
                    .collect();
                let refs: Vec<(&str, &str)> = pairs
                    .iter()
                    .map(|(p, c)| (p.as_str(), c.as_str()))
                    .collect();
                sage_test_harness::with_test_crate_files(&refs, |db, root| {
                    sage_emit::emit_module(db, root)
                })
            }
        }
    }
}

pub fn discover_fixtures() -> Vec<Fixture> {
    let dir = fixtures_dir();
    let mut fixtures = Vec::new();
    discover_recursive(&dir, &mut fixtures);
    fixtures.sort_by(|a, b| a.name().cmp(&b.name()));
    fixtures
}

fn discover_recursive(dir: &Path, fixtures: &mut Vec<Fixture>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", dir.display(), e))
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.path());

    for entry in &entries {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            fixtures.push(Fixture::SingleFile(path));
        } else if path.is_dir() {
            let src_dir = path.join("src");
            let lib = src_dir.join("lib.rs");
            let main = src_dir.join("main.rs");
            if lib.exists() || main.exists() {
                let entry_file = if lib.exists() { lib } else { main };
                let files = collect_rs_files(&src_dir);
                fixtures.push(Fixture::Directory {
                    entry: entry_file,
                    files,
                });
            } else {
                discover_recursive(&path, fixtures);
            }
        }
    }
}

fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs_recursive(dir, &mut files);
    files.sort();
    files
}

fn collect_rs_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        } else if path.is_dir() {
            collect_rs_recursive(&path, files);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Normalization
// ═══════════════════════════════════════════════════════════════════════════

pub fn strip_bodies(krate: &Crate<NormalizedDef>) -> Crate<NormalizedDef> {
    Crate {
        root: strip_bodies_module(&krate.root),
    }
}

fn strip_bodies_module(module: &Module<NormalizedDef>) -> Module<NormalizedDef> {
    Module {
        def: module.def.clone(),
        name: module.name.clone(),
        items: module.items.iter().map(strip_bodies_item).collect(),
    }
}

fn strip_bodies_item(item: &Item<NormalizedDef>) -> Item<NormalizedDef> {
    match item {
        Item::Fn(f) => Item::Fn(FnItem {
            def: f.def.clone(),
            name: f.name.clone(),
            params: f.params.clone(),
            return_ty: f.return_ty.clone(),
            body: None,
        }),
        Item::Struct(s) => Item::Struct(s.clone()),
        Item::Mod(m) => Item::Mod(strip_bodies_module(m)),
    }
}

pub fn normalize_pair(
    oracle: &Crate<NormalizedDef>,
    sage: &Crate<NormalizedDef>,
) -> (Crate<NormalizedDef>, Crate<NormalizedDef>) {
    let (o_root, s_root) = normalize_module_pair(&oracle.root, &sage.root);
    (Crate { root: o_root }, Crate { root: s_root })
}

fn normalize_module_pair(
    oracle: &Module<NormalizedDef>,
    sage: &Module<NormalizedDef>,
) -> (Module<NormalizedDef>, Module<NormalizedDef>) {
    if oracle.items.len() != sage.items.len() {
        return (oracle.clone(), sage.clone());
    }
    let items: Vec<_> = oracle
        .items
        .iter()
        .zip(sage.items.iter())
        .map(|(o, s)| normalize_item_pair(o, s))
        .collect();
    let (o_items, s_items): (Vec<_>, Vec<_>) = items.into_iter().unzip();
    (
        Module {
            def: oracle.def.clone(),
            name: oracle.name.clone(),
            items: o_items,
        },
        Module {
            def: sage.def.clone(),
            name: sage.name.clone(),
            items: s_items,
        },
    )
}

fn normalize_item_pair(
    oracle: &Item<NormalizedDef>,
    sage: &Item<NormalizedDef>,
) -> (Item<NormalizedDef>, Item<NormalizedDef>) {
    match (oracle, sage) {
        (Item::Fn(o), Item::Fn(s)) => {
            let (o_body, s_body) = match (o.body.as_ref(), s.body.as_ref()) {
                (Some(ob), Some(sb)) => {
                    let (o2, s2) = normalize_expr_pair(ob, sb);
                    (Some(o2), Some(s2))
                }
                _ => (o.body.clone(), s.body.clone()),
            };
            (
                Item::Fn(FnItem {
                    def: o.def.clone(),
                    name: o.name.clone(),
                    params: o.params.clone(),
                    return_ty: o.return_ty.clone(),
                    body: o_body,
                }),
                Item::Fn(FnItem {
                    def: s.def.clone(),
                    name: s.name.clone(),
                    params: s.params.clone(),
                    return_ty: s.return_ty.clone(),
                    body: s_body,
                }),
            )
        }
        (Item::Struct(o), Item::Struct(s)) => (Item::Struct(o.clone()), Item::Struct(s.clone())),
        (Item::Mod(o), Item::Mod(s)) => {
            let (om, sm) = normalize_module_pair(o, s);
            (Item::Mod(om), Item::Mod(sm))
        }
        _ => (oracle.clone(), sage.clone()),
    }
}

fn is_infer_var(ty: &Type<NormalizedDef>) -> bool {
    matches!(ty, Type::Primitive(s) if s.starts_with("?InferVar"))
}

fn normalize_type_pair(
    oracle_ty: &Type<NormalizedDef>,
    sage_ty: &Type<NormalizedDef>,
) -> (Type<NormalizedDef>, Type<NormalizedDef>) {
    if is_infer_var(sage_ty) {
        let placeholder = Type::Primitive("_".to_string());
        (placeholder.clone(), placeholder)
    } else {
        (oracle_ty.clone(), sage_ty.clone())
    }
}

fn normalize_expr_pair(
    oracle: &Expr<NormalizedDef>,
    sage: &Expr<NormalizedDef>,
) -> (Expr<NormalizedDef>, Expr<NormalizedDef>) {
    match (oracle, sage) {
        (Expr::Literal { kind: ok, .. }, Expr::Literal { kind: sk, .. }) => (
            Expr::Literal {
                kind: ok.clone(),
                value: String::new(),
            },
            Expr::Literal {
                kind: sk.clone(),
                value: String::new(),
            },
        ),
        (
            Expr::BinaryOp {
                op: oo,
                lhs: ol,
                rhs: or,
                ty: oty,
            },
            Expr::BinaryOp {
                op: so,
                lhs: sl,
                rhs: sr,
                ty: sty,
            },
        ) => {
            let (ol2, sl2) = normalize_expr_pair(ol, sl);
            let (or2, sr2) = normalize_expr_pair(or, sr);
            let (oty2, sty2) = normalize_type_pair(oty, sty);
            (
                Expr::BinaryOp {
                    op: oo.clone(),
                    lhs: Box::new(ol2),
                    rhs: Box::new(or2),
                    ty: oty2,
                },
                Expr::BinaryOp {
                    op: so.clone(),
                    lhs: Box::new(sl2),
                    rhs: Box::new(sr2),
                    ty: sty2,
                },
            )
        }
        (
            Expr::Call {
                target: ot,
                args: oa,
                ty: oty,
            },
            Expr::Call {
                target: st,
                args: sa,
                ty: sty,
            },
        ) => {
            if oa.len() != sa.len() {
                return (oracle.clone(), sage.clone());
            }
            let args: Vec<_> = oa
                .iter()
                .zip(sa.iter())
                .map(|(o, s)| normalize_expr_pair(o, s))
                .collect();
            let (oa2, sa2): (Vec<_>, Vec<_>) = args.into_iter().unzip();
            let (oty2, sty2) = normalize_type_pair(oty, sty);
            (
                Expr::Call {
                    target: ot.clone(),
                    args: oa2,
                    ty: oty2,
                },
                Expr::Call {
                    target: st.clone(),
                    args: sa2,
                    ty: sty2,
                },
            )
        }
        (
            Expr::StructLit {
                target: ot,
                fields: of,
                ty: oty,
            },
            Expr::StructLit {
                target: st,
                fields: sf,
                ty: sty,
            },
        ) => {
            if of.len() != sf.len() {
                return (oracle.clone(), sage.clone());
            }
            let fields: Vec<_> = of
                .iter()
                .zip(sf.iter())
                .map(|(o, s)| {
                    let (ov, sv) = normalize_expr_pair(&o.value, &s.value);
                    (
                        FieldExpr {
                            name: o.name.clone(),
                            value: ov,
                        },
                        FieldExpr {
                            name: s.name.clone(),
                            value: sv,
                        },
                    )
                })
                .collect();
            let (of2, sf2): (Vec<_>, Vec<_>) = fields.into_iter().unzip();
            let (oty2, sty2) = normalize_type_pair(oty, sty);
            (
                Expr::StructLit {
                    target: ot.clone(),
                    fields: of2,
                    ty: oty2,
                },
                Expr::StructLit {
                    target: st.clone(),
                    fields: sf2,
                    ty: sty2,
                },
            )
        }
        (
            Expr::Field {
                expr: oe,
                field_name: ofn,
                ty: oty,
            },
            Expr::Field {
                expr: se,
                field_name: sfn,
                ty: sty,
            },
        ) => {
            let (oe2, se2) = normalize_expr_pair(oe, se);
            let (oty2, sty2) = normalize_type_pair(oty, sty);
            (
                Expr::Field {
                    expr: Box::new(oe2),
                    field_name: ofn.clone(),
                    ty: oty2,
                },
                Expr::Field {
                    expr: Box::new(se2),
                    field_name: sfn.clone(),
                    ty: sty2,
                },
            )
        }
        (
            Expr::Block {
                stmts: os,
                tail: ot,
                ty: oty,
            },
            Expr::Block {
                stmts: ss,
                tail: st,
                ty: sty,
            },
        ) => {
            if os.len() != ss.len() {
                return (oracle.clone(), sage.clone());
            }
            let stmts: Vec<_> = os
                .iter()
                .zip(ss.iter())
                .map(|(o, s)| normalize_stmt_pair(o, s))
                .collect();
            let (os2, ss2): (Vec<_>, Vec<_>) = stmts.into_iter().unzip();
            let (ot2, st2) = match (ot.as_ref(), st.as_ref()) {
                (Some(o), Some(s)) => {
                    let (o2, s2) = normalize_expr_pair(o, s);
                    (Some(Box::new(o2)), Some(Box::new(s2)))
                }
                _ => (ot.clone(), st.clone()),
            };
            let (oty2, sty2) = normalize_type_pair(oty, sty);
            (
                Expr::Block {
                    stmts: os2,
                    tail: ot2,
                    ty: oty2,
                },
                Expr::Block {
                    stmts: ss2,
                    tail: st2,
                    ty: sty2,
                },
            )
        }
        (Expr::Deref { expr: oe, ty: oty }, Expr::Deref { expr: se, ty: sty }) => {
            let (oe2, se2) = normalize_expr_pair(oe, se);
            let (oty2, sty2) = normalize_type_pair(oty, sty);
            (
                Expr::Deref {
                    expr: Box::new(oe2),
                    ty: oty2,
                },
                Expr::Deref {
                    expr: Box::new(se2),
                    ty: sty2,
                },
            )
        }
        (
            Expr::Ref {
                mutable: om,
                expr: oe,
                ty: oty,
            },
            Expr::Ref {
                mutable: sm,
                expr: se,
                ty: sty,
            },
        ) => {
            let (oe2, se2) = normalize_expr_pair(oe, se);
            let (oty2, sty2) = normalize_type_pair(oty, sty);
            (
                Expr::Ref {
                    mutable: *om,
                    expr: Box::new(oe2),
                    ty: oty2,
                },
                Expr::Ref {
                    mutable: *sm,
                    expr: Box::new(se2),
                    ty: sty2,
                },
            )
        }
        (Expr::Local { .. }, Expr::Local { .. }) => (oracle.clone(), sage.clone()),
        _ => (oracle.clone(), sage.clone()),
    }
}

fn normalize_stmt_pair(
    oracle: &Stmt<NormalizedDef>,
    sage: &Stmt<NormalizedDef>,
) -> (Stmt<NormalizedDef>, Stmt<NormalizedDef>) {
    match (oracle, sage) {
        (
            Stmt::Let {
                name: on,
                index: oi,
                ty: oty,
                init: oinit,
            },
            Stmt::Let {
                name: sn,
                index: si,
                ty: sty,
                init: sinit,
            },
        ) => {
            let (oty2, sty2) = normalize_type_pair(oty, sty);
            let (oinit2, sinit2) = match (oinit.as_ref(), sinit.as_ref()) {
                (Some(o), Some(s)) => {
                    let (o2, s2) = normalize_expr_pair(o, s);
                    (Some(o2), Some(s2))
                }
                _ => (oinit.clone(), sinit.clone()),
            };
            (
                Stmt::Let {
                    name: on.clone(),
                    index: *oi,
                    ty: oty2,
                    init: oinit2,
                },
                Stmt::Let {
                    name: sn.clone(),
                    index: *si,
                    ty: sty2,
                    init: sinit2,
                },
            )
        }
        (Stmt::Expr(o), Stmt::Expr(s)) => {
            let (o2, s2) = normalize_expr_pair(o, s);
            (Stmt::Expr(o2), Stmt::Expr(s2))
        }
        _ => (oracle.clone(), sage.clone()),
    }
}

pub fn assert_crates_eq(
    fixture_name: &str,
    lhs: &Crate<NormalizedDef>,
    rhs: &Crate<NormalizedDef>,
) -> Result<(), String> {
    let lhs_json = serde_json::to_value(lhs).unwrap();
    let rhs_json = serde_json::to_value(rhs).unwrap();

    if lhs_json == rhs_json {
        return Ok(());
    }

    let diff = assert_json_diff::assert_json_matches_no_panic(
        &lhs_json,
        &rhs_json,
        assert_json_diff::Config::new(assert_json_diff::CompareMode::Strict),
    );
    match diff {
        Ok(()) => Ok(()),
        Err(msg) => Err(format!(
            "fixture '{}' diverges between oracle and sage:\n{}",
            fixture_name, msg
        )),
    }
}
