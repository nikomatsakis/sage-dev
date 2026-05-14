#![feature(rustc_private)]

extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;

use clap::Parser;
use sage_ir::resolve::{module_items, resolve_module_path};

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
            match resolve_module_path(sage.db, sage.root, sage.source_root, &segments) {
                Some(module) => {
                    let items = module_items(sage.db, module);
                    println!("=== ModSymbol: {} ({} items) ===", module_path, items.len());
                    for item in items {
                        println!("  {item}");
                    }
                }
                None => {
                    eprintln!("sage: could not resolve module path: {module_path}");
                }
            }
        } else {
            // Default: print all items in the root module
            let items = module_items(sage.db, sage.root);
            println!("=== Root module ({} items) ===", items.len());
            for item in items {
                println!("  {item}");
            }
        }
    });
}
