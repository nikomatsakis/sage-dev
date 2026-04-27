#![feature(rustc_private)]

extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;

mod metadata;

use std::collections::HashMap;
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

    let ws = metadata::load_workspace(&cwd, &cli.p);

    eprintln!(
        "sage: {} workspace crate(s) selected, {} direct deps",
        ws.selected.len(),
        ws.direct_dep_rlibs.len(),
    );

    if ws.selected.is_empty() {
        eprintln!("sage: no workspace crates matched");
        return;
    }

    run_stub_driver(&ws.deps_dir, &ws.direct_dep_rlibs);

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

fn run_stub_driver(deps_dir: &Path, direct_dep_rlibs: &HashMap<String, PathBuf>) {
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

    // Generate stub with extern crate for each direct dep
    let stub_dir = std::env::temp_dir().join("sage-stub");
    std::fs::create_dir_all(&stub_dir).unwrap();
    let stub_path = stub_dir.join("lib.rs");
    let mut stub_src = String::from("#![crate_type = \"lib\"]\n#![allow(unused_extern_crates)]\n");
    for name in direct_dep_rlibs.keys() {
        stub_src.push_str(&format!("extern crate {name};\n"));
    }
    std::fs::write(&stub_path, &stub_src).unwrap();

    // Match cargo's pattern:
    //   -L dependency=<target/debug/deps>   (search path for transitive deps)
    //   --extern name=<path>                (only direct deps)
    let mut args: Vec<String> = vec![
        "sage".into(),
        stub_path.to_string_lossy().into_owned(),
        "--edition=2021".into(),
        "--crate-type=lib".into(),
        format!("--sysroot={sysroot}"),
        format!("-Ldependency={}", deps_dir.display()),
    ];

    for (name, path) in direct_dep_rlibs {
        args.push(format!("--extern={name}={}", path.display()));
    }

    let mut driver = SageDriver;
    let _ = rustc_driver::catch_fatal_errors(|| {
        rustc_driver::run_compiler(&args, &mut driver);
    });
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
