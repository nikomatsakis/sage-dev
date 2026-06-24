#![feature(rustc_private)]

extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;

use clap::Parser;
use sage_ir::Db;
use sage_ir::symbol::ModSymbol;

use sage::driver::run_sage_with;

#[derive(clap::Parser)]
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

        /// Expand a specific module and print the result.
        #[arg(long, value_name = "PATH")]
        module: Option<String>,
    },
}

fn main() {
    let Cargo {
        cmd: CargoCmd::Sage { p, module },
    } = Cargo::parse();
    let cwd = std::env::current_dir().expect("no cwd");

    run_sage_with(&cwd, &p, |sage| {
        if let Some(module_path) = &module {
            let segments: Vec<&str> = module_path.split("::").collect();
            match resolve_module_path(sage.db, sage.root, &segments) {
                Some(target) => {
                    let items = target.expanded_module_items(sage.db);
                    println!("=== ModSymbol: {} ({} items) ===", module_path, items.len());
                    for item in items {
                        println!("  {:?}", item.data(sage.db));
                    }
                }
                None => {
                    eprintln!("sage: could not resolve module path: {module_path}");
                }
            }
        } else {
            let items = sage.root.expanded_module_items(sage.db);
            println!("=== Root module ({} items) ===", items.len());
            for item in items {
                println!("  {:?}", item.data(sage.db));
            }
        }
    });
}

fn resolve_module_path<'db>(
    db: &'db dyn Db,
    root: ModSymbol<'db>,
    segments: &[&str],
) -> Option<ModSymbol<'db>> {
    let mut current = root;
    for &seg in segments {
        let items = current.expanded_module_items(db);
        let found = items.iter().find(|item| {
            item.name(db)
                .map(|(name, _)| name.text(db) == seg)
                .unwrap_or(false)
        });
        match found {
            Some(item) => match item.module(db) {
                Some(m) => current = m,
                None => return None,
            },
            None => return None,
        }
    }
    Some(current)
}
