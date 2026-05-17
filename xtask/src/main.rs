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
        Some("ci") => {
            match args.get(1).map(|s| s.as_str()) {
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
            }
        }
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
    cmd!(sh, "cargo test --all --workspace").run()?;
    println!("ci: all tests passed");
    Ok(())
}

/// Verify every `.md` file under `md/` is referenced in `SUMMARY.md`.
fn check_orphaned_chapters() -> Result<()> {
    let root = project_root();
    let md_dir = root.join("md");
    let summary_path = md_dir.join("SUMMARY.md");

    let mut md_files = BTreeSet::new();
    collect_md_files(&md_dir, &md_dir, &mut md_files)?;
    md_files.remove(Path::new("SUMMARY.md"));

    let summary = std::fs::read_to_string(&summary_path)?;
    let referenced = parse_summary_links(&summary);

    let orphans: Vec<_> = md_files.difference(&referenced).collect();
    if !orphans.is_empty() {
        let list = orphans
            .iter()
            .map(|p| format!("  md/{}", p.display()))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!("orphaned markdown files not listed in SUMMARY.md:\n{list}").into());
    }

    println!("ci: all markdown files are referenced in SUMMARY.md");
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

fn parse_summary_links(content: &str) -> BTreeSet<PathBuf> {
    let mut links = BTreeSet::new();
    for line in content.lines() {
        let mut rest = line;
        while let Some(start) = rest.find("](") {
            rest = &rest[start + 2..];
            if let Some(end) = rest.find(')') {
                let link = &rest[..end];
                let link = link.strip_prefix("./").unwrap_or(link);
                if link.ends_with(".md") {
                    links.insert(PathBuf::from(link));
                }
                rest = &rest[end + 1..];
            } else {
                break;
            }
        }
    }
    links
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
