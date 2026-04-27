#![feature(rustc_private)]

extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;

use std::collections::HashSet;

use rustc_driver::{Callbacks, Compilation};
use rustc_hir::def::DefKind;
use rustc_hir::def_id::DefId;
use rustc_interface::interface;
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::CRATE_DEF_INDEX;

struct SageDriver;

impl Callbacks for SageDriver {
    fn after_expansion<'tcx>(
        &mut self,
        _compiler: &interface::Compiler,
        tcx: TyCtxt<'tcx>,
    ) -> Compilation {
        let mut visited = HashSet::new();
        for &cnum in tcx.crates(()) {
            let name = tcx.crate_name(cnum);
            println!("crate {name}");
            let root = DefId {
                krate: cnum,
                index: CRATE_DEF_INDEX,
            };
            print_module(tcx, root, 1, &mut visited);
        }
        Compilation::Stop
    }
}

fn print_module(tcx: TyCtxt<'_>, module: DefId, depth: usize, visited: &mut HashSet<DefId>) {
    let indent = "  ".repeat(depth);
    for child in tcx.module_children(module) {
        let Some(did) = child.res.opt_def_id() else {
            continue;
        };
        if did.krate != module.krate || !visited.insert(did) {
            continue;
        }
        let kind = tcx.def_kind(did);
        let name = child.ident.name;
        println!("{indent}{kind:?} {name}");
        if kind == DefKind::Mod {
            print_module(tcx, did, depth + 1, visited);
        }
    }
}

fn main() {
    let stub = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("stub/lib.rs");

    let sysroot = std::process::Command::new("rustc")
        .arg("--print=sysroot")
        .output()
        .expect("rustc not found")
        .stdout;
    let sysroot = String::from_utf8(sysroot).unwrap().trim().to_string();

    let args: Vec<String> = vec![
        "sage".into(),
        stub.to_string_lossy().into_owned(),
        "--edition=2021".into(),
        "--crate-type=lib".into(),
        format!("--sysroot={sysroot}"),
    ];

    let mut driver = SageDriver;
    rustc_driver::run_compiler(&args, &mut driver);
}
