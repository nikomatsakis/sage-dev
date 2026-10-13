#![feature(register_tool)]
#![register_tool(sage)]
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use xshell::{Shell, cmd};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("codegen") => {
            let check = args.iter().any(|a| a == "--check");
            codegen(check)
        }
        Some("tidy") => tidy(),
        Some("ci") => match args.get(1).map(|s| s.as_str()) {
            Some("lint") => ci_lint(),
            Some("test") => ci_test(),
            None => {
                ci_lint()?;
                ci_test()
            }
            Some(sub) => {
                eprintln!("unknown ci subcommand: {sub}");
                usage();
            }
        },
        Some(cmd) => {
            eprintln!("unknown command: {cmd}");
            usage();
        }
        None => usage(),
    }
}

fn usage() -> ! {
    eprintln!("usage: cargo xtask <codegen [--check] | tidy | ci [lint|test]>");
    std::process::exit(1);
}

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask should be in a subdirectory")
        .to_path_buf()
}

/// TODO: Replace with your codegen logic.
fn codegen(check: bool) -> Result<()> {
    let _sh = Shell::new()?;

    // TODO: Add your codegen logic here.

    if check {
        println!("codegen --check: all generated files are up to date");
    } else {
        println!("codegen: nothing to generate (add your logic here)");
    }
    Ok(())
}

/// Check for trailing whitespace across the repo.
fn tidy() -> Result<()> {
    let sh = Shell::new()?;
    let root = project_root();
    let _dir = sh.push_dir(&root);
    let output = cmd!(sh, "grep -rn --include=*.rs [[:space:]]$$ src/")
        .ignore_status()
        .read()?;
    if !output.is_empty() {
        return Err(format!("trailing whitespace found:\n{output}").into());
    }
    println!("tidy: no trailing whitespace found");
    Ok(())
}

fn ci_lint() -> Result<()> {
    codegen(true)?;
    tidy()?;
    check_orphaned_chapters()?;
    check_book_build()?;
    Ok(())
}

fn ci_test() -> Result<()> {
    let sh = Shell::new()?;
    let root = project_root();
    let _dir = sh.push_dir(&root);
    cmd!(sh, "cargo run -p sage-meta -- check").run()?;
    cmd!(sh, "cargo test --all --workspace").run()?;
    println!("ci: all tests passed");
    Ok(())
}

/// Verify every Markdown source belongs to the book's shared chapter inventory.
fn check_orphaned_chapters() -> Result<()> {
    check_orphaned_chapters_at(&project_root())?;
    println!("ci: all markdown files belong to the book chapter inventory");
    Ok(())
}

// {anchor}`meta.book.shared_model`
fn check_orphaned_chapters_at(root: &Path) -> Result<()> {
    let md_dir = sage_meta::workspace::book_source_root(root)?;
    let inventory = sage_meta::spec::SpecIndex::load_workspace(root)?;

    let mut md_files = BTreeSet::new();
    collect_md_files(&md_dir, &md_dir, &mut md_files)?;
    md_files.remove(Path::new("SUMMARY.md"));

    let published = inventory.chapters.keys().map(PathBuf::from).collect();

    let orphans: Vec<_> = md_files.difference(&published).collect();
    if !orphans.is_empty() {
        let list = orphans
            .iter()
            .map(|p| format!("  {}", md_dir.join(p).display()))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(
            format!("orphaned markdown files outside the book chapter inventory:\n{list}").into(),
        );
    }
    if !inventory.diagnostics.is_empty() {
        return Err(inventory
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            .into());
    }
    Ok(())
}

fn collect_md_files(base: &Path, dir: &Path, out: &mut BTreeSet<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_md_files(base, &path, out)?;
        } else if path.extension().is_some_and(|e| e == "md") {
            let rel = path.strip_prefix(base)?.to_path_buf();
            out.insert(rel);
        }
    }
    Ok(())
}

/// Run `mdbook build` to validate the book (including linkcheck if installed).
fn check_book_build() -> Result<()> {
    let sh = Shell::new()?;
    let root = project_root();
    let _dir = sh.push_dir(&root);
    cmd!(sh, "mdbook build").run()?;
    println!("ci: mdbook build succeeded");
    Ok(())
}

// Source anchors for architectural modules and distributed representations.
mod sage_model {
    /// Architectural crate anchor.
    #[sage::item(crate)]
    mod crate_model {}
}

#[cfg(test)]
mod tests {
    use super::check_orphaned_chapters_at;

    #[test]
    #[sage::test(meta.book.shared_model)]
    fn orphan_check_uses_generated_chapters_and_rejects_unpublished_sources() {
        let root = std::env::temp_dir().join(format!(
            "sage-book-lint-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let source = root.join("manual");
        std::fs::create_dir_all(source.join("design/crates/demo")).unwrap();
        std::fs::write(root.join("book.toml"), "[book]\nsrc = 'manual'\n").unwrap();
        for (path, text) in [
            (
                "SUMMARY.md",
                "- [Guide](./guide.md#usage)\n- [Crates](design/crates/README.md)\n",
            ),
            ("guide.md", "# Usage\n"),
            ("design/crates/README.md", "# Crates\n"),
            (
                "design/crates/demo/crate.sage",
                "crate demo { \"\"\"Demo model.\"\"\" }",
            ),
            ("design/crates/demo/guide.md", "# Generated crate guide\n"),
        ] {
            std::fs::write(source.join(path), text).unwrap();
        }
        // The generated guide is published without its own SUMMARY.md entry.
        check_orphaned_chapters_at(&root).unwrap();

        std::fs::write(source.join("orphan.md"), "# Unpublished\n").unwrap();
        let error = check_orphaned_chapters_at(&root).unwrap_err().to_string();
        assert!(error.contains("orphaned markdown files"), "{error}");
        assert!(error.contains("manual/orphan.md"), "{error}");
        assert!(!error.contains("demo/guide.md"), "{error}");
        std::fs::remove_file(source.join("orphan.md")).unwrap();

        // A generated companion needs an enclosing modeled crate.
        std::fs::write(source.join("design/crates/unowned.md"), "# Unowned\n").unwrap();
        let error = check_orphaned_chapters_at(&root).unwrap_err().to_string();
        assert!(error.contains("design/crates/unowned.md"), "{error}");
        assert!(!error.contains("demo/guide.md"), "{error}");
        std::fs::remove_file(source.join("design/crates/unowned.md")).unwrap();

        // Merely residing under design/crates does not publish a chapter.
        std::fs::write(source.join("SUMMARY.md"), "- [Guide](guide.md)\n").unwrap();
        let error = check_orphaned_chapters_at(&root).unwrap_err().to_string();
        assert!(error.contains("design/crates/demo/guide.md"), "{error}");
        std::fs::remove_dir_all(root).unwrap();
    }
}
