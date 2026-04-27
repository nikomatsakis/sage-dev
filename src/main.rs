#![feature(rustc_private)]

extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;

mod metadata;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use clap::Parser;
use rustc_driver::{Callbacks, Compilation};
use rustc_hir::def::DefKind;
use rustc_hir::def_id::{CrateNum, DefId};
use rustc_interface::interface;
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::CRATE_DEF_INDEX;

#[derive(Parser)]
#[command(name = "sage", about = "Fast Rust analysis tool")]
struct Cli {
    /// Select workspace crates to analyze (default: all)
    #[arg(short, long = "package", value_name = "CRATE")]
    p: Vec<String>,
}

fn main() {
    let cli = Cli::parse();
    let cwd = std::env::current_dir().expect("no cwd");

    // 1. Load workspace metadata and build external deps
    let ws = metadata::load_workspace(&cwd, &cli.p);

    eprintln!(
        "sage: {} workspace crate(s) selected, {} external dep rlibs collected",
        ws.selected.len(),
        ws.extern_rlibs.len(),
    );

    if ws.selected.is_empty() {
        eprintln!("sage: no workspace crates matched");
        return;
    }

    // 2. Run the stub driver to load dep metadata
    run_stub_driver(&ws.extern_rlibs, &ws.dep_graph);

    // 3. tree-sitter parse each selected workspace crate
    for krate in &ws.selected {
        parse_workspace_crate(krate);
    }
}

// --- rustc_driver stub ---

struct SageDriver;

impl Callbacks for SageDriver {
    fn after_expansion<'tcx>(
        &mut self,
        _compiler: &interface::Compiler,
        tcx: TyCtxt<'tcx>,
    ) -> Compilation {
        print_dep_stats(tcx);
        Compilation::Stop
    }
}

fn print_dep_stats(tcx: TyCtxt<'_>) {
    let crates: Vec<CrateNum> = tcx.crates(()).to_vec();
    println!(
        "\n=== Dependency stats ({} crates loaded) ===",
        crates.len()
    );

    let mut total_by_kind: HashMap<DefKind, usize> = HashMap::new();

    for &cnum in &crates {
        let name = tcx.crate_name(cnum);
        let root = DefId {
            krate: cnum,
            index: CRATE_DEF_INDEX,
        };
        let mut counts: HashMap<DefKind, usize> = HashMap::new();
        count_items(
            tcx,
            root,
            &mut counts,
            &mut std::collections::HashSet::new(),
        );
        let total: usize = counts.values().sum();
        println!("  {name}: {total} items");
        for (kind, count) in &counts {
            *total_by_kind.entry(*kind).or_default() += count;
        }
    }

    println!("\nTotals by kind:");
    let mut sorted: Vec<_> = total_by_kind.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    for (kind, count) in sorted {
        println!("  {kind:?}: {count}");
    }
}

fn count_items(
    tcx: TyCtxt<'_>,
    module: DefId,
    counts: &mut HashMap<DefKind, usize>,
    visited: &mut std::collections::HashSet<DefId>,
) {
    for child in tcx.module_children(module) {
        let Some(did) = child.res.opt_def_id() else {
            continue;
        };
        if did.krate != module.krate || !visited.insert(did) {
            continue;
        }
        let kind = tcx.def_kind(did);
        *counts.entry(kind).or_default() += 1;
        if kind == DefKind::Mod {
            count_items(tcx, did, counts, visited);
        }
    }
}

fn run_stub_driver(
    extern_rlibs: &HashMap<String, PathBuf>,
    dep_graph: &HashMap<String, Vec<String>>,
) {
    let sysroot = String::from_utf8(
        std::process::Command::new("rustc")
            .arg("--print=sysroot")
            .output()
            .expect("rustc not found")
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();

    // Discover crate names in the sysroot. Any crate that IS a sysroot crate
    // or transitively depends on one will conflict (cargo built against
    // crates.io versions which don't match the sysroot builds).
    let sysroot_crates = discover_sysroot_crates(&sysroot);
    let tainted = compute_tainted_crates(&sysroot_crates, dep_graph);

    let filtered: HashMap<&String, &PathBuf> = extern_rlibs
        .iter()
        .filter(|(name, _)| !tainted.contains(name.as_str()))
        .collect();

    eprintln!(
        "sage: {} deps provided to driver ({} skipped due to sysroot conflicts)",
        filtered.len(),
        extern_rlibs.len() - filtered.len(),
    );

    // Generate a stub with `extern crate` for every dep so rustc loads them
    let stub_dir = std::env::temp_dir().join("sage-stub");
    std::fs::create_dir_all(&stub_dir).unwrap();
    let stub_path = stub_dir.join("lib.rs");
    let mut stub_src = String::from("#![crate_type = \"lib\"]\n#![allow(unused_extern_crates)]\n");
    for name in filtered.keys() {
        stub_src.push_str(&format!("extern crate {name};\n"));
    }
    std::fs::write(&stub_path, &stub_src).unwrap();

    let mut args: Vec<String> = vec![
        "sage".into(),
        stub_path.to_string_lossy().into_owned(),
        "--edition=2021".into(),
        "--crate-type=lib".into(),
        format!("--sysroot={sysroot}"),
    ];

    for (name, path) in &filtered {
        args.push(format!("--extern={name}={}", path.display()));
    }

    let mut driver = SageDriver;
    let _ = rustc_driver::catch_fatal_errors(|| {
        rustc_driver::run_compiler(&args, &mut driver);
    });
}

fn discover_sysroot_crates(sysroot: &str) -> HashSet<String> {
    let lib_dir = Path::new(sysroot)
        .join("lib/rustlib")
        .join(
            std::env::consts::ARCH.to_string()
                + "-"
                + match std::env::consts::OS {
                    "macos" => "apple-darwin",
                    "linux" => "unknown-linux-gnu",
                    "windows" => "pc-windows-msvc",
                    os => os,
                },
        )
        .join("lib");

    let mut crates = HashSet::new();
    let Ok(entries) = std::fs::read_dir(&lib_dir) else {
        return crates;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(rest) = name.strip_prefix("lib") {
            if rest.ends_with(".rlib") || rest.ends_with(".rmeta") {
                if let Some(dash_pos) = rest.rfind('-') {
                    let crate_name = &rest[..dash_pos];
                    crates.insert(crate_name.to_string());
                }
            }
        }
    }
    crates
}

/// Compute the set of crate names that are "tainted" by the sysroot:
/// either they ARE sysroot crates, or they transitively depend on one.
fn compute_tainted_crates(
    sysroot_crates: &HashSet<String>,
    dep_graph: &HashMap<String, Vec<String>>,
) -> HashSet<String> {
    let mut tainted = sysroot_crates.clone();
    // Fixed-point: keep marking crates as tainted if any dep is tainted
    loop {
        let mut changed = false;
        for (name, deps) in dep_graph {
            if tainted.contains(name) {
                continue;
            }
            if deps.iter().any(|d| tainted.contains(d)) {
                tainted.insert(name.clone());
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    tainted
}

// --- tree-sitter workspace parsing ---

fn parse_workspace_crate(krate: &metadata::SelectedCrate) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("failed to set tree-sitter language");

    let rs_files = collect_rs_files(&krate.manifest_dir.join("src"));
    let mut total_nodes = 0usize;
    let mut total_lines = 0usize;
    let mut node_kinds: HashMap<&'static str, usize> = HashMap::new();

    for path in &rs_files {
        let source = std::fs::read_to_string(path).expect("failed to read file");
        total_lines += source.lines().count();
        let Some(tree) = parser.parse(&source, None) else {
            continue;
        };
        count_tree_nodes(tree.root_node(), &mut total_nodes, &mut node_kinds);
    }

    println!(
        "\n=== Workspace crate: {} ({} files, {} lines) ===",
        krate.name,
        rs_files.len(),
        total_lines,
    );
    println!("  Total AST nodes: {total_nodes}");

    let mut sorted: Vec<_> = node_kinds.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    println!("  Top node kinds:");
    for (kind, count) in sorted.iter().take(15) {
        println!("    {kind}: {count}");
    }
}

fn count_tree_nodes<'a>(
    node: tree_sitter::Node<'a>,
    total: &mut usize,
    kinds: &mut HashMap<&'static str, usize>,
) {
    *total += 1;
    // tree-sitter node kinds are static strings from the grammar
    let kind: &'static str = unsafe { std::mem::transmute(node.kind()) };
    *kinds.entry(kind).or_default() += 1;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count_tree_nodes(child, total, kinds);
    }
}

fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return files;
    }
    for entry in std::fs::read_dir(dir).expect("failed to read dir") {
        let entry = entry.expect("failed to read entry");
        let path = entry.path();
        if path.is_dir() {
            files.extend(collect_rs_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
    files
}
