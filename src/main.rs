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
#[command(name = "cargo")]
struct Cargo {
    #[command(subcommand)]
    cmd: CargoCmd,
}

#[derive(clap::Subcommand)]
enum CargoCmd {
    /// Fast Rust analysis tool
    Sage {
        /// Select workspace crates to analyze (default: all)
        #[arg(short, long = "package", value_name = "CRATE")]
        p: Vec<String>,
    },
}

fn main() {
    let Cargo {
        cmd: CargoCmd::Sage { p },
    } = Cargo::parse();
    let cwd = std::env::current_dir().expect("no cwd");

    let ws = metadata::load_workspace(&cwd, &p);

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
    let sysroot = metadata::our_sysroot();

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

    println!(
        "\n=== Workspace crate: {} ({} files) ===",
        krate.name,
        rs_files.len(),
    );

    for path in &rs_files {
        let source = std::fs::read_to_string(path).expect("failed to read file");
        let Some(tree) = parser.parse(&source, None) else {
            continue;
        };
        let rel = path.strip_prefix(&krate.manifest_dir).unwrap_or(path);
        println!(
            "\n  --- {} ({} lines) ---",
            rel.display(),
            source.lines().count()
        );
        print_top_level_items(tree.root_node(), &source, 2);
    }
}

fn print_top_level_items(node: tree_sitter::Node<'_>, source: &str, indent: usize) {
    let pad = "  ".repeat(indent);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "use_declaration" => {
                let text = child_text(child, source);
                println!("{pad}use {text}");
            }
            "function_item" => {
                let name = named_child_text(child, "name", source).unwrap_or("?");
                let is_async = child
                    .children(&mut child.walk())
                    .any(|c| c.kind() == "async");
                let attrs = collect_attrs(child, source);
                let attr_str = if attrs.is_empty() {
                    String::new()
                } else {
                    format!(" {}", attrs.join(" "))
                };
                let async_str = if is_async { "async " } else { "" };
                println!("{pad}{async_str}fn {name}{attr_str}");
            }
            "struct_item" => {
                let name = named_child_text(child, "name", source).unwrap_or("?");
                let attrs = collect_attrs(child, source);
                let fields = count_child_kind(child, "field_declaration");
                let attr_str = if attrs.is_empty() {
                    String::new()
                } else {
                    format!(" {}", attrs.join(" "))
                };
                println!("{pad}struct {name} ({fields} fields){attr_str}");
            }
            "enum_item" => {
                let name = named_child_text(child, "name", source).unwrap_or("?");
                let variants = count_child_kind(child, "enum_variant");
                let attrs = collect_attrs(child, source);
                let attr_str = if attrs.is_empty() {
                    String::new()
                } else {
                    format!(" {}", attrs.join(" "))
                };
                println!("{pad}enum {name} ({variants} variants){attr_str}");
            }
            "impl_item" => {
                let type_name = named_child_text(child, "type", source).unwrap_or("?");
                let trait_node = child.child_by_field_name("trait");
                let methods = count_child_kind(child, "function_item");
                if let Some(t) = trait_node {
                    let trait_text = node_text(t, source);
                    println!("{pad}impl {trait_text} for {type_name} ({methods} methods)");
                } else {
                    println!("{pad}impl {type_name} ({methods} methods)");
                }
            }
            "trait_item" => {
                let name = named_child_text(child, "name", source).unwrap_or("?");
                let methods = count_child_kind(child, "function_signature_item")
                    + count_child_kind(child, "function_item");
                println!("{pad}trait {name} ({methods} methods)");
            }
            "type_item" => {
                let name = named_child_text(child, "name", source).unwrap_or("?");
                println!("{pad}type {name}");
            }
            "const_item" => {
                let name = named_child_text(child, "name", source).unwrap_or("?");
                println!("{pad}const {name}");
            }
            "static_item" => {
                let name = named_child_text(child, "name", source).unwrap_or("?");
                println!("{pad}static {name}");
            }
            "mod_item" => {
                let name = named_child_text(child, "name", source).unwrap_or("?");
                let has_body = child.child_by_field_name("body").is_some();
                if has_body {
                    println!("{pad}mod {name} {{");
                    if let Some(body) = child.child_by_field_name("body") {
                        print_top_level_items(body, source, indent + 1);
                    }
                    println!("{pad}}}");
                } else {
                    println!("{pad}mod {name};");
                }
            }
            "macro_invocation" => {
                let macro_name = child
                    .child_by_field_name("macro")
                    .map(|n| node_text(n, source))
                    .unwrap_or("?");
                println!("{pad}{macro_name}!(...)");
            }
            "macro_definition" => {
                let name = named_child_text(child, "name", source).unwrap_or("?");
                println!("{pad}macro_rules! {name}");
            }
            "attribute_item" | "inner_attribute_item" => {
                // skip standalone attributes, they'll be collected with their item
            }
            "line_comment" | "block_comment" => {}
            _ => {}
        }
    }
}

fn node_text<'a>(node: tree_sitter::Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

fn named_child_text<'a>(
    node: tree_sitter::Node<'_>,
    field: &str,
    source: &'a str,
) -> Option<&'a str> {
    node.child_by_field_name(field)
        .map(|n| node_text(n, source))
}

fn child_text<'a>(node: tree_sitter::Node<'_>, source: &'a str) -> &'a str {
    // For use declarations, grab everything after "use " and before ";"
    let text = node_text(node, source);
    text.trim_start_matches("use ").trim_end_matches(';').trim()
}

fn collect_attrs(node: tree_sitter::Node<'_>, source: &str) -> Vec<String> {
    let mut attrs = Vec::new();
    // Look at preceding siblings for attribute_item nodes
    let mut sib = node.prev_sibling();
    while let Some(s) = sib {
        if s.kind() == "attribute_item" {
            attrs.push(node_text(s, source).to_string());
        } else if s.kind() != "line_comment" && s.kind() != "block_comment" {
            break;
        }
        sib = s.prev_sibling();
    }
    attrs.reverse();
    attrs
}

fn count_child_kind(node: tree_sitter::Node<'_>, kind: &str) -> usize {
    let mut count = 0;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            count += 1;
        }
        let mut c2 = child.walk();
        for grandchild in child.children(&mut c2) {
            if grandchild.kind() == kind {
                count += 1;
            }
        }
    }
    count
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
