use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

// --- cargo metadata types (only the fields we need) ---

#[derive(Deserialize)]
pub struct Metadata {
    pub packages: Vec<Package>,
    pub workspace_members: Vec<String>,
    pub resolve: Resolve,
    pub target_directory: PathBuf,
    pub workspace_root: PathBuf,
}

#[derive(Deserialize)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub manifest_path: PathBuf,
    pub targets: Vec<Target>,
}

#[derive(Deserialize)]
pub struct Target {
    pub name: String,
    pub kind: Vec<String>,
    pub src_path: PathBuf,
}

#[derive(Deserialize)]
pub struct Resolve {
    pub nodes: Vec<ResolveNode>,
}

#[derive(Deserialize)]
pub struct ResolveNode {
    pub id: String,
    pub deps: Vec<ResolveDep>,
}

#[derive(Deserialize)]
pub struct ResolveDep {
    pub name: String,
    pub pkg: String,
    pub dep_kinds: Vec<DepKindInfo>,
}

#[derive(Deserialize)]
pub struct DepKindInfo {
    pub kind: Option<String>,
}

// --- cargo build artifact message ---

#[derive(Deserialize)]
pub struct BuildMessage {
    pub reason: String,
    pub package_id: Option<String>,
    pub target: Option<BuildTarget>,
    pub filenames: Option<Vec<PathBuf>>,
}

#[derive(Deserialize)]
pub struct BuildTarget {
    pub name: String,
    pub kind: Vec<String>,
}

// --- public API ---

pub struct WorkspaceInfo {
    pub root: PathBuf,
    pub target_dir: PathBuf,
    pub selected: Vec<SelectedCrate>,
    /// crate_name -> rlib path, for all external deps of selected crates
    pub extern_rlibs: HashMap<String, PathBuf>,
}

pub struct SelectedCrate {
    pub name: String,
    pub src_path: PathBuf,
    pub manifest_dir: PathBuf,
}

pub fn load_workspace(manifest_dir: &Path, selected_packages: &[String]) -> WorkspaceInfo {
    let meta = run_cargo_metadata(manifest_dir);
    let ws_member_ids: HashSet<&str> = meta.workspace_members.iter().map(|s| s.as_str()).collect();

    // Build package lookup
    let pkg_by_id: HashMap<&str, &Package> =
        meta.packages.iter().map(|p| (p.id.as_str(), p)).collect();

    // Determine selected crates
    let selected: Vec<SelectedCrate> = meta
        .packages
        .iter()
        .filter(|p| ws_member_ids.contains(p.id.as_str()))
        .filter(|p| selected_packages.is_empty() || selected_packages.iter().any(|s| s == &p.name))
        .filter_map(|p| {
            // Prefer lib target, fall back to first target (bin crates)
            let target = p
                .targets
                .iter()
                .find(|t| t.kind.iter().any(|k| k == "lib" || k == "proc-macro"))
                .or_else(|| p.targets.first())?;
            Some(SelectedCrate {
                name: p.name.clone(),
                src_path: target.src_path.clone(),
                manifest_dir: p.manifest_path.parent().unwrap().to_path_buf(),
            })
        })
        .collect();

    // Collect all external dep package IDs needed by selected crates
    let selected_ids: HashSet<&str> = meta
        .packages
        .iter()
        .filter(|p| {
            selected.iter().any(|s| s.name == p.name) && ws_member_ids.contains(p.id.as_str())
        })
        .map(|p| p.id.as_str())
        .collect();

    let node_by_id: HashMap<&str, &ResolveNode> = meta
        .resolve
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n))
        .collect();

    // Identify proc-macro packages (these are host-side, not target-side)
    let proc_macro_ids: HashSet<&str> = meta
        .packages
        .iter()
        .filter(|p| {
            p.targets
                .iter()
                .any(|t| t.kind.iter().any(|k| k == "proc-macro"))
        })
        .map(|p| p.id.as_str())
        .collect();

    let mut extern_pkg_ids: HashSet<&str> = HashSet::new();
    let mut queue: Vec<&str> = Vec::new();

    // Seed with direct non-workspace, non-proc-macro deps of selected crates
    for &sel_id in &selected_ids {
        if let Some(node) = node_by_id.get(sel_id) {
            for dep in &node.deps {
                let is_normal = dep.dep_kinds.iter().any(|dk| dk.kind.is_none());
                if is_normal
                    && !ws_member_ids.contains(dep.pkg.as_str())
                    && !proc_macro_ids.contains(dep.pkg.as_str())
                {
                    if extern_pkg_ids.insert(&dep.pkg) {
                        queue.push(&dep.pkg);
                    }
                }
            }
        }
    }

    // Transitively collect all external deps (skipping proc-macros)
    while let Some(pkg_id) = queue.pop() {
        if let Some(node) = node_by_id.get(pkg_id) {
            for dep in &node.deps {
                let is_normal = dep.dep_kinds.iter().any(|dk| dk.kind.is_none());
                if is_normal
                    && !proc_macro_ids.contains(dep.pkg.as_str())
                    && extern_pkg_ids.insert(&dep.pkg)
                {
                    queue.push(&dep.pkg);
                }
            }
        }
    }

    // Build external deps and collect rlib paths
    let extern_rlibs = build_and_collect_rlibs(
        manifest_dir,
        &meta.target_directory,
        &extern_pkg_ids,
        &pkg_by_id,
    );

    WorkspaceInfo {
        root: meta.workspace_root,
        target_dir: meta.target_directory,
        selected,
        extern_rlibs,
    }
}

fn run_cargo_metadata(manifest_dir: &Path) -> Metadata {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(manifest_dir)
        .output()
        .expect("failed to run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("failed to parse cargo metadata")
}

fn build_and_collect_rlibs(
    manifest_dir: &Path,
    target_dir: &Path,
    extern_pkg_ids: &HashSet<&str>,
    pkg_by_id: &HashMap<&str, &Package>,
) -> HashMap<String, PathBuf> {
    eprintln!(
        "sage: building {} external dependencies...",
        extern_pkg_ids.len()
    );

    // Build the whole workspace (deps get built as side effect).
    // We use `cargo check` which produces .rmeta but not full rlibs.
    // Actually we need rlibs for --extern, so use `cargo build`.
    let output = Command::new("cargo")
        .args(["build", "--message-format=json"])
        .current_dir(manifest_dir)
        .output()
        .expect("failed to run cargo build");

    if !output.status.success() {
        eprintln!(
            "sage: cargo build stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("cargo build failed");
    }

    // Parse artifact messages to find rlib paths
    let mut rlibs: HashMap<String, PathBuf> = HashMap::new();
    let wanted_names: HashSet<&str> = extern_pkg_ids
        .iter()
        .filter_map(|id| pkg_by_id.get(id).map(|p| p.name.as_str()))
        .collect();

    for line in output.stdout.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_slice::<BuildMessage>(line) else {
            continue;
        };
        if msg.reason != "compiler-artifact" {
            continue;
        }
        let Some(target) = &msg.target else { continue };
        if !target.kind.iter().any(|k| k == "lib") {
            continue;
        }
        let Some(filenames) = &msg.filenames else {
            continue;
        };
        let Some(rlib) = filenames
            .iter()
            .find(|f| f.extension().is_some_and(|e| e == "rlib"))
        else {
            continue;
        };

        // Match by crate name (underscored)
        let crate_name = target.name.replace('-', "_");
        if wanted_names.contains(target.name.as_str())
            || wanted_names
                .iter()
                .any(|&n| n.replace('-', "_") == crate_name)
        {
            rlibs.insert(crate_name, rlib.clone());
        }
    }

    rlibs
}
