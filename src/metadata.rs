use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    workspace_root: PathBuf,
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
    src_path: PathBuf,
    edition: String,
}

#[derive(Deserialize)]
struct Resolve {
    nodes: Vec<ResolveNode>,
}

#[derive(Deserialize)]
struct ResolveNode {
    id: String,
    deps: Vec<ResolveDep>,
    #[serde(default)]
    features: Vec<String>,
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

#[derive(Deserialize)]
struct BuildMessage {
    reason: String,
    package_id: Option<String>,
    target: Option<BuildTarget>,
    filenames: Option<Vec<PathBuf>>,
}

#[derive(Deserialize)]
struct BuildTarget {
    kind: Vec<String>,
}

// --- public API ---

#[derive(Debug)]
pub struct WorkspaceInfo {
    pub selected: Vec<SelectedCrate>,
    pub workspace_root: PathBuf,
    pub deps_dir: PathBuf,
    pub direct_dep_rlibs: HashMap<String, PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedCrate {
    pub name: String,
    pub manifest_dir: PathBuf,
    pub target: SelectedTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedTarget {
    pub name: String,
    pub src_path: PathBuf,
    pub edition: sage_ir::scope::Edition,
    pub enabled_features: Vec<String>,
    pub cfgs: Vec<String>,
}

/// Get the sysroot for sage's own embedded rustc.
/// Embedded at compile time by build.rs — always matches the linked rustc.
pub fn our_sysroot() -> &'static str {
    env!("SAGE_SYSROOT")
}

/// Get the path to the rustc binary in sage's sysroot.
pub fn our_rustc() -> PathBuf {
    Path::new(our_sysroot()).join("bin/rustc")
}

pub fn load_workspace(
    manifest_dir: &Path,
    selected_packages: &[String],
) -> Result<WorkspaceInfo, String> {
    let meta = run_cargo_metadata(manifest_dir)?;
    let ws_member_ids: HashSet<&str> = meta.workspace_members.iter().map(|s| s.as_str()).collect();
    let node_by_id: HashMap<&str, &ResolveNode> = meta
        .resolve
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n))
        .collect();
    let target_cfgs = rustc_target_cfgs()?;

    let mut selected = Vec::new();
    for package in meta.packages.iter().filter(|package| {
        ws_member_ids.contains(package.id.as_str())
            && (selected_packages.is_empty()
                || selected_packages.iter().any(|name| name == &package.name))
    }) {
        let Some(target) = package.targets.iter().find(|target| {
            target.kind.iter().any(|kind| kind == "lib")
                && !target.kind.iter().any(|kind| kind == "proc-macro")
        }) else {
            continue;
        };
        let edition = sage_ir::scope::Edition::parse(&target.edition).ok_or_else(|| {
            format!(
                "unsupported Rust edition {:?} for {}",
                target.edition, package.name
            )
        })?;
        let mut enabled_features = node_by_id
            .get(package.id.as_str())
            .map_or_else(Vec::new, |node| node.features.clone());
        enabled_features.sort();
        let manifest_dir = package
            .manifest_path
            .parent()
            .ok_or_else(|| format!("package {} has no manifest directory", package.name))?
            .to_path_buf();
        selected.push(SelectedCrate {
            name: package.name.clone(),
            manifest_dir,
            target: SelectedTarget {
                name: target.name.clone(),
                src_path: target.src_path.clone(),
                edition,
                enabled_features,
                cfgs: target_cfgs.clone(),
            },
        });
    }

    if selected.len() != 1 {
        return Err(format!(
            "sage requires exactly one library target, but {} were selected; use --package to select one workspace crate",
            selected.len()
        ));
    }

    let selected_package = meta
        .packages
        .iter()
        .find(|package| package.name == selected[0].name)
        .expect("the selected package came from Cargo metadata");
    let mut direct_deps_by_package: HashMap<String, Vec<String>> = HashMap::new();
    if let Some(node) = node_by_id.get(selected_package.id.as_str()) {
        for dep in &node.deps {
            if dep.dep_kinds.iter().any(|kind| kind.kind.is_none()) {
                direct_deps_by_package
                    .entry(dep.pkg.clone())
                    .or_default()
                    .push(dep.name.replace('-', "_"));
            }
        }
    }
    for names in direct_deps_by_package.values_mut() {
        names.sort();
        names.dedup();
    }

    let deps_dir = meta.target_directory.join("debug/deps");
    let direct_dep_rlibs =
        build_and_collect_direct_deps(manifest_dir, &selected[0].name, &direct_deps_by_package)?;

    Ok(WorkspaceInfo {
        selected,
        workspace_root: meta.workspace_root,
        deps_dir,
        direct_dep_rlibs,
    })
}

fn rustc_target_cfgs() -> Result<Vec<String>, String> {
    let output = Command::new(our_rustc())
        .args(["--print", "cfg"])
        .output()
        .map_err(|error| format!("failed to ask rustc for target cfg values: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "rustc --print cfg failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let mut cfgs: Vec<_> = String::from_utf8(output.stdout)
        .map_err(|error| format!("rustc cfg output was not UTF-8: {error}"))?
        .lines()
        .map(str::to_owned)
        .collect();
    cfgs.sort();
    Ok(cfgs)
}

fn run_cargo_metadata(manifest_dir: &Path) -> Result<Metadata, String> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(manifest_dir)
        .output()
        .map_err(|error| format!("failed to run cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("failed to parse cargo metadata: {error}"))
}

fn build_and_collect_direct_deps(
    manifest_dir: &Path,
    selected_package: &str,
    direct_deps_by_package: &HashMap<String, Vec<String>>,
) -> Result<HashMap<String, PathBuf>, String> {
    eprintln!("sage: building dependencies...");

    // Use RUSTC to force cargo to use the exact same rustc that's linked into sage.
    // This guarantees rlib metadata version compatibility.
    let output = Command::new("cargo")
        .args([
            "build",
            "--package",
            selected_package,
            "--lib",
            "--message-format=json",
        ])
        .env("RUSTC", our_rustc())
        .current_dir(manifest_dir)
        .output()
        .map_err(|error| format!("failed to run cargo build: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "cargo build failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
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
        let Some(extern_names) = msg
            .package_id
            .as_ref()
            .and_then(|package_id| direct_deps_by_package.get(package_id))
        else {
            continue;
        };
        let Some(target) = &msg.target else { continue };

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
            for extern_name in extern_names {
                rlibs.insert(extern_name.clone(), artifact.clone());
            }
        }
    }

    Ok(rlibs)
}
