//! Best-effort source navigation over parsed workspace modules, without expansion.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use syn::spanned::Spanned;

#[derive(Clone, Debug, serde::Serialize)]
pub struct RustLocation {
    pub file: String,
    pub line: usize,
    pub end_line: usize,
    pub macro_input: bool,
}

#[derive(Default)]
pub(crate) struct SourceIndex {
    locations: BTreeMap<String, Vec<RustLocation>>,
    aliases: BTreeMap<String, Vec<String>>,
    pub files: BTreeMap<String, String>,
    pub warnings: Vec<String>,
    visited: BTreeSet<(PathBuf, String)>,
    workspace: PathBuf,
}

#[derive(serde::Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}
#[derive(serde::Deserialize)]
struct Package {
    targets: Vec<Target>,
}
#[derive(serde::Deserialize)]
struct Target {
    name: String,
    kind: Vec<String>,
    src_path: PathBuf,
}

impl SourceIndex {
    pub fn load(workspace: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let workspace = workspace.canonicalize()?;
        let output = std::process::Command::new("cargo")
            .args([
                "metadata",
                "--no-deps",
                "--offline",
                "--format-version",
                "1",
                "--manifest-path",
            ])
            .arg(workspace.join("Cargo.toml"))
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "cannot locate Rust crates: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        let metadata: Metadata = serde_json::from_slice(&output.stdout)?;
        let mut index = Self {
            workspace,
            ..Self::default()
        };
        for package in metadata.packages {
            let library = package.targets.iter().find(|t| {
                t.kind
                    .iter()
                    .any(|k| matches!(k.as_str(), "lib" | "proc-macro" | "rlib"))
            });
            let target = library.or_else(|| {
                package
                    .targets
                    .iter()
                    .find(|t| t.kind.iter().any(|k| k == "bin"))
            });
            if let Some(target) = target {
                index.file(&target.src_path, &target.name.replace('-', "_"));
            }
        }
        Ok(index)
    }

    fn file(&mut self, file: &Path, module: &str) {
        let Ok(file) = file.canonicalize() else {
            self.warnings
                .push(format!("Rust module not found: {}", file.display()));
            return;
        };
        if !file.starts_with(&self.workspace) || !self.visited.insert((file.clone(), module.into()))
        {
            return;
        }
        let relative = file
            .strip_prefix(&self.workspace)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let source = match std::fs::read_to_string(&file) {
            Ok(source) => source,
            Err(error) => {
                self.warnings.push(format!("{relative}: {error}"));
                return;
            }
        };
        let syntax = match syn::parse_file(&source) {
            Ok(syntax) => syntax,
            Err(error) => {
                self.warnings.push(format!("{relative}: {error}"));
                return;
            }
        };
        self.files.insert(relative.clone(), source);
        let parent = file.parent().unwrap();
        let directory = match file.file_name().and_then(|s| s.to_str()) {
            Some("lib.rs" | "main.rs" | "mod.rs") => parent.to_path_buf(),
            _ => parent.join(file.file_stem().unwrap()),
        };
        self.items(&syntax.items, module, &directory, parent, &relative);
    }

    fn items(
        &mut self,
        items: &[syn::Item],
        module: &str,
        directory: &Path,
        physical_parent: &Path,
        file: &str,
    ) {
        for item in items {
            let ident = match item {
                syn::Item::Struct(i) => Some(&i.ident),
                syn::Item::Enum(i) => Some(&i.ident),
                syn::Item::Trait(i) => Some(&i.ident),
                syn::Item::Type(i) => Some(&i.ident),
                // Other Rust items do not introduce the type declarations annotated by .sage.
                _ => None,
            };
            if let Some(ident) = ident {
                self.locations
                    .entry(format!("{module}::{ident}"))
                    .or_default()
                    .push(RustLocation {
                        file: file.into(),
                        line: ident.span().start().line,
                        end_line: item.span().end().line,
                        macro_input: false,
                    });
            }
            match item {
                syn::Item::Mod(child) => {
                    let namespace = format!("{module}::{}", child.ident);
                    if let Some((_, children)) = &child.content {
                        self.items(
                            children,
                            &namespace,
                            &directory.join(child.ident.to_string()),
                            &directory.join(child.ident.to_string()),
                            file,
                        );
                    } else {
                        let explicit = child.attrs.iter().find_map(|attr| {
                            if !attr.path().is_ident("path") {
                                return None;
                            }
                            match &attr.meta {
                                syn::Meta::NameValue(value) => match &value.value {
                                    syn::Expr::Lit(lit) => match &lit.lit {
                                        syn::Lit::Str(path) => Some(path.value()),
                                        _ => None,
                                    },
                                    _ => None,
                                },
                                _ => None,
                            }
                        });
                        let path = if let Some(path) = explicit {
                            physical_parent.join(path)
                        } else {
                            let flat = directory.join(format!("{}.rs", child.ident));
                            if flat.exists() {
                                flat
                            } else {
                                directory.join(child.ident.to_string()).join("mod.rs")
                            }
                        };
                        self.file(&path, &namespace);
                    }
                }
                syn::Item::Use(import) => self.import(&import.tree, module, &[]),
                syn::Item::Macro(mac) if mac.ident.is_none() => {
                    self.macro_input(mac.mac.tokens.clone(), module, file)
                }
                _ => (),
            }
        }
    }

    fn import(&mut self, tree: &syn::UseTree, module: &str, prefix: &[String]) {
        match tree {
            syn::UseTree::Path(path) => {
                let mut prefix = prefix.to_vec();
                prefix.push(path.ident.to_string());
                self.import(&path.tree, module, &prefix);
            }
            syn::UseTree::Group(group) => {
                for tree in &group.items {
                    self.import(tree, module, prefix);
                }
            }
            syn::UseTree::Name(name) => {
                let mut path = prefix.to_vec();
                let local = if name.ident == "self" {
                    prefix.last().cloned().unwrap_or_default()
                } else {
                    path.push(name.ident.to_string());
                    name.ident.to_string()
                };
                self.alias(module, &local, &path);
            }
            syn::UseTree::Rename(rename) => {
                let mut path = prefix.to_vec();
                if rename.ident != "self" {
                    path.push(rename.ident.to_string());
                }
                self.alias(module, &rename.rename.to_string(), &path);
            }
            syn::UseTree::Glob(_) => (), // Wildcard imports are not sufficient evidence for a source link.
        }
    }
    fn alias(&mut self, module: &str, local: &str, path: &[String]) {
        if path.is_empty() {
            return;
        }
        let mut prefix: Vec<_> = module.split("::").map(str::to_owned).collect();
        let mut index = 0;
        if path[0] == "crate" {
            prefix.truncate(1);
            index = 1;
        } else if path[0] == "self" {
            index = 1;
        }
        while path.get(index).is_some_and(|part| part == "super") {
            prefix.pop();
            index += 1;
        }
        prefix.extend_from_slice(&path[index..]);
        self.aliases
            .entry(format!("{module}::{local}"))
            .or_default()
            .push(prefix.join("::"));
    }
    fn macro_input(&mut self, stream: proc_macro2::TokenStream, module: &str, file: &str) {
        let tokens: Vec<_> = stream.into_iter().collect();
        for pair in tokens.windows(2) {
            if let [
                proc_macro2::TokenTree::Ident(keyword),
                proc_macro2::TokenTree::Ident(name),
            ] = pair
                && matches!(
                    keyword.to_string().as_str(),
                    "struct" | "enum" | "trait" | "type"
                )
            {
                self.locations
                    .entry(format!("{module}::{name}"))
                    .or_default()
                    .push(RustLocation {
                        file: file.into(),
                        line: name.span().start().line,
                        end_line: name.span().end().line,
                        macro_input: true,
                    });
            }
        }
        for token in tokens {
            if let proc_macro2::TokenTree::Group(group) = token {
                self.macro_input(group.stream(), module, file);
            }
        }
    }

    pub fn resolve(&self, key: &str) -> Vec<RustLocation> {
        let mut queue = vec![key.to_owned()];
        let mut visited = BTreeSet::new();
        let mut found = BTreeMap::new();
        while let Some(key) = queue.pop() {
            if !visited.insert(key.clone()) || visited.len() > 256 {
                continue;
            }
            if let Some(locations) = self.locations.get(&key) {
                for location in locations {
                    found.insert((location.file.clone(), location.line), location.clone());
                }
            }
            // Also substitute module aliases, e.g. crate::resolve::Resolution.
            let parts: Vec<_> = key.split("::").collect();
            for length in 1..=parts.len() {
                if let Some(aliases) = self.aliases.get(&parts[..length].join("::")) {
                    for alias in aliases {
                        let suffix = parts[length..].join("::");
                        queue.push(if suffix.is_empty() {
                            alias.clone()
                        } else {
                            format!("{alias}::{suffix}")
                        });
                    }
                }
            }
        }
        found.into_values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_aliases_and_macro_inputs_without_matching_unrelated_names() {
        let syntax = syn::parse_file(
            "mod actual { pub struct Item; }
             use crate::actual as renamed;
             pub use renamed::Item as Exported;
             define_symbols! { enum Generated {} }
             use crate::CycleB as CycleA;
             use crate::CycleA as CycleB;",
        )
        .unwrap();
        let mut index = SourceIndex::default();
        index.items(
            &syntax.items,
            "demo",
            Path::new("."),
            Path::new("."),
            "lib.rs",
        );
        let found = index.resolve("demo::Exported");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 1);
        assert!(!found[0].macro_input);
        assert!(index.resolve("demo::Generated")[0].macro_input);
        assert!(index.resolve("other::Item").is_empty());
        assert!(index.resolve("demo::CycleA").is_empty());
    }

    #[test]
    fn follows_explicit_paths_inside_inline_modules() {
        let root = std::env::temp_dir().join(format!("sage-source-path-{}", std::process::id()));
        std::fs::create_dir_all(root.join("inline")).unwrap();
        std::fs::write(
            root.join("lib.rs"),
            r#"mod inline { #[path = "custom.rs"] mod nested; }"#,
        )
        .unwrap();
        std::fs::write(root.join("inline/custom.rs"), "pub struct Found;").unwrap();
        let mut index = SourceIndex {
            workspace: root.canonicalize().unwrap(),
            ..SourceIndex::default()
        };
        index.file(&root.join("lib.rs"), "demo");
        assert_eq!(
            index.resolve("demo::inline::nested::Found")[0].file,
            "inline/custom.rs"
        );
        assert!(index.warnings.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
