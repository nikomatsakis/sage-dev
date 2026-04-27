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
struct BuildMessage {
    reason: String,
    package_id: Option<String>,
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
    /// crate_name -> rlib path, for all external (non-workspace) deps
    pub extern_rlibs: HashMap<String, PathBuf>,
    /// crate_name -> set of direct dep crate names (underscored)
    pub dep_graph: HashMap<String, Vec<String>>,
}

pub struct SelectedCrate {
    pub name: String,
    pub manifest_dir: PathBuf,
}

pub fn load_workspace(manifest_dir: &Path, selected_packages: &[String]) -> WorkspaceInfo {
    let meta = run_cargo_metadata(manifest_dir);
    let ws_member_ids: HashSet<&str> = meta.workspace_members.iter().map(|s| s.as_str()).collect();

    // Determine selected crates
    let selected: Vec<SelectedCrate> = meta
        .packages
        .iter()
        .filter(|p| ws_member_ids.contains(p.id.as_str()))
        .filter(|p| selected_packages.is_empty() || selected_packages.iter().any(|s| s == &p.name))
        .filter_map(|p| {
            // Need at least one target
            p.targets.first()?;
            Some(SelectedCrate {
                name: p.name.clone(),
                manifest_dir: p.manifest_path.parent().unwrap().to_path_buf(),
            })
        })
        .collect();

    // Workspace package names (to exclude from --extern)
    let ws_pkg_names: HashSet<&str> = meta
        .packages
        .iter()
        .filter(|p| ws_member_ids.contains(p.id.as_str()))
        .map(|p| p.name.as_str())
        .collect();

    // Build everything and collect ALL non-workspace lib rlibs
    let extern_rlibs = build_and_collect_rlibs(manifest_dir, &ws_pkg_names);

    // Build dep graph: crate_name -> [dep crate names]
    // Map package IDs to underscored crate names
    let pkg_id_to_name: HashMap<&str, String> = meta
        .packages
        .iter()
        .map(|p| (p.id.as_str(), p.name.replace('-', "_")))
        .collect();

    let mut dep_graph: HashMap<String, Vec<String>> = HashMap::new();
    for node in &meta.resolve.nodes {
        let Some(name) = pkg_id_to_name.get(node.id.as_str()) else {
            continue;
        };
        let deps: Vec<String> = node
            .deps
            .iter()
            .filter(|d| d.dep_kinds.iter().any(|dk| dk.kind.is_none()))
            .filter_map(|d| pkg_id_to_name.get(d.pkg.as_str()).cloned())
            .collect();
        dep_graph.insert(name.clone(), deps);
    }

    WorkspaceInfo {
        selected,
        extern_rlibs,
        dep_graph,
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
    ws_pkg_names: &HashSet<&str>,
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

    // Collect every non-workspace artifact from the build output.
    // This gives us the complete transitive closure with consistent versions.
    // We collect both lib rlibs and proc-macro dylibs.
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

        // Skip workspace crates — sage handles those
        if ws_pkg_names.contains(target.name.as_str()) {
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

        // For libs, prefer .rlib; for proc-macros, take the dylib
        let artifact = if is_lib {
            filenames
                .iter()
                .find(|f| f.extension().is_some_and(|e| e == "rlib"))
        } else {
            filenames
                .iter()
                .find(|f| f.extension().is_some_and(|e| e == "dylib" || e == "so"))
        };
        let Some(artifact) = artifact else { continue };

        let crate_name = target.name.replace('-', "_");
        rlibs.insert(crate_name, artifact.clone());
    }

    rlibs
}
