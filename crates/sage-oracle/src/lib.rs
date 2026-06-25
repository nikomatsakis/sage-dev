#![feature(rustc_private)]

extern crate rustc_ast;
extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

mod emit;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rust_ref::{Crate, NormalizedDef};

pub use emit::{OracleError, emit_crate};

pub fn analyze_file(path: &Path) -> Result<Crate<NormalizedDef>, OracleError> {
    analyze_crate(path, &[path.to_path_buf()])
}

pub fn analyze_crate(
    entry: &Path,
    _source_files: &[PathBuf],
) -> Result<Crate<NormalizedDef>, OracleError> {
    let sysroot = find_sysroot()?;
    let entry = entry
        .canonicalize()
        .map_err(|e| OracleError::Io(format!("cannot canonicalize {}: {}", entry.display(), e)))?;

    let result: Arc<Mutex<Option<Result<Crate<NormalizedDef>, OracleError>>>> =
        Arc::new(Mutex::new(None));
    let result_clone = result.clone();

    let args = vec![
        "sage-oracle".to_string(),
        entry.to_string_lossy().to_string(),
        "--edition".to_string(),
        "2021".to_string(),
        "--sysroot".to_string(),
        sysroot,
        "--crate-type".to_string(),
        "lib".to_string(),
    ];

    let mut callbacks = OracleCallbacks {
        result: result_clone,
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        rustc_driver::run_compiler(&args, &mut callbacks);
    }));

    let lock = result.lock().unwrap();
    lock.clone().unwrap_or(Err(OracleError::NoOutput))
}

fn find_sysroot() -> Result<String, OracleError> {
    let output = std::process::Command::new("rustc")
        .arg("--print=sysroot")
        .output()
        .map_err(|e| OracleError::Io(format!("cannot run rustc: {}", e)))?;
    if !output.status.success() {
        return Err(OracleError::Io("rustc --print=sysroot failed".to_string()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

struct OracleCallbacks {
    result: Arc<Mutex<Option<Result<Crate<NormalizedDef>, OracleError>>>>,
}

impl rustc_driver::Callbacks for OracleCallbacks {
    fn after_analysis(
        &mut self,
        _compiler: &rustc_interface::interface::Compiler,
        tcx: rustc_middle::ty::TyCtxt<'_>,
    ) -> rustc_driver::Compilation {
        let result = emit::emit_crate(tcx);
        *self.result.lock().unwrap() = Some(result);
        rustc_driver::Compilation::Stop
    }
}
