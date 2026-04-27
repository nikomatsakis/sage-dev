use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    resolve: Resolve,
    target_directory: PathBuf,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    manifest_path: PathBuf,
    targets: Vec<Target>,
}

#[derive(Deserialize)]
struct Target {
    name: String,
    kind: Vec<String>,
}

#[derive(Deserialize)]
struct Resolve {
    nodes: Vec<ResolveNode>,
}

#[derive(Deserialize)]
struct ResolveNode {
    id: String,
    deps: Vec<ResolveDep>,
}

#[derive(Deserialize)]
struct ResolveDep {
    name: String,
    pkg: String,
    dep_kinds: Vec<DepKindInfo>,
}

#[derive(Deserialize)]
struct DepKindInfo {
    kind: Option<String>,
}

// --- cargo build artifact message ---

#[derive(Deserialize)]
struct BuildMessage {
    reason: String,
    target: Option<BuildTarget>,
    filenames: Option<Vec<PathBuf>>,
}

#[derive(Deserialize)]
struct BuildTarget {
    name: String,
    kind: Vec<String>,
}

// --- public API ---

pub struct WorkspaceInfo {
    pub selected: Vec<SelectedCrate>,
    /// -L dependency search path (target/debug/deps)
    pub deps_dir: PathBuf,
    /// Direct dep crate_name -> rlib path (only direct deps of selected crates)
    pub direct_dep_rlibs: HashMap<String, PathBuf>,
}

pub struct SelectedCrate {
    pub name: String,
    pub manifest_dir: PathBuf,
}

pub fn load_workspace(manifest_dir: &Path, selected_packages: &[String]) -> WorkspaceInfo {
    let meta = run_cargo_metadata(manifest_dir);
    let ws_member_ids: HashSet<&str> = meta.workspace_members.iter().map(|s| s.as_str()).collect();

    let pkg_by_id: HashMap<&str, &Package> =
        meta.packages.iter().map(|p| (p.id.as_str(), p)).collect();

    // Determine selected crates
    let selected: Vec<SelectedCrate> = meta
        .packages
        .iter()
        .filter(|p| ws_member_ids.contains(p.id.as_str()))
        .filter(|p| selected_packages.is_empty() || selected_packages.iter().any(|s| s == &p.name))
        .filter_map(|p| {
            p.targets.first()?;
            Some(SelectedCrate {
                name: p.name.clone(),
                manifest_dir: p.manifest_path.parent().unwrap().to_path_buf(),
            })
        })
        .collect();

    // Find direct (normal) deps of selected workspace crates
    let selected_ids: HashSet<&str> = meta
        .packages
        .iter()
        .filter(|p| {
            ws_member_ids.contains(p.id.as_str()) && selected.iter().any(|s| s.name == p.name)
        })
        .map(|p| p.id.as_str())
        .collect();

    let node_by_id: HashMap<&str, &ResolveNode> = meta
        .resolve
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n))
        .collect();

    let mut direct_dep_names: HashSet<String> = HashSet::new();
    for &sel_id in &selected_ids {
        if let Some(node) = node_by_id.get(sel_id) {
            for dep in &node.deps {
                let is_normal = dep.dep_kinds.iter().any(|dk| dk.kind.is_none());
                if is_normal && !ws_member_ids.contains(dep.pkg.as_str()) {
                    // Use the `name` field — this is the extern crate name cargo uses
                    direct_dep_names.insert(dep.name.replace('-', "_"));
                }
            }
        }
    }

    // Build and collect rlib paths for direct deps only
    let deps_dir = meta.target_directory.join("debug/deps");
    let direct_dep_rlibs = build_and_collect_direct_deps(manifest_dir, &direct_dep_names);

    WorkspaceInfo {
        selected,
        deps_dir,
        direct_dep_rlibs,
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

fn build_and_collect_direct_deps(
    manifest_dir: &Path,
    direct_dep_names: &HashSet<String>,
) -> HashMap<String, PathBuf> {
    eprintln!("sage: building dependencies...");

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

    let mut rlibs: HashMap<String, PathBuf> = HashMap::new();

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
        let crate_name = target.name.replace('-', "_");

        if !direct_dep_names.contains(&crate_name) {
            continue;
        }

        let is_lib = target.kind.iter().any(|k| k == "lib");
        let is_proc_macro = target.kind.iter().any(|k| k == "proc-macro");
        if !is_lib && !is_proc_macro {
            continue;
        }

        let Some(filenames) = &msg.filenames else {
            continue;
        };
        let artifact = if is_lib {
            filenames
                .iter()
                .find(|f| f.extension().is_some_and(|e| e == "rlib"))
        } else {
            filenames
                .iter()
                .find(|f| f.extension().is_some_and(|e| e == "dylib" || e == "so"))
        };
        if let Some(artifact) = artifact {
            rlibs.insert(crate_name, artifact.clone());
        }
    }

    rlibs
}
