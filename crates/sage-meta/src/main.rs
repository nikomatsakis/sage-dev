#![feature(register_tool)]
#![register_tool(sage)]
mod server;

use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args
        .first()
        .is_some_and(|arg| arg == "--help" || arg == "-h")
    {
        println!(
            "sage-meta [sage-src directory]\nsage-meta check [--correspondence] [--json] [sage-src directory]\nsage-meta browse [sage-src directory] [output.html]\nsage-meta serve <port> [sage-src directory]\n\nValidate the model, or export a standalone interactive HTML explorer."
        );
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "check") {
        return check(&args[1..]);
    }
    let serve = args.first().is_some_and(|arg| arg == "serve");
    let browse = args.first().is_some_and(|arg| arg == "browse");
    let args = if browse || serve {
        &args[1..]
    } else {
        &args[..]
    };
    let port = if serve {
        let port: u16 = args
            .first()
            .and_then(|arg| arg.to_str())
            .ok_or("usage: sage-meta serve <port> [sage-src directory]")?
            .parse()
            .map_err(|_| "port must be an integer between 1 and 65535")?;
        if port == 0 {
            return Err("port must be between 1 and 65535".into());
        }
        Some(port)
    } else {
        None
    };
    if args.len() > if browse || serve { 2 } else { 1 } {
        return Err("usage: sage-meta [sage-src directory] | sage-meta browse [sage-src directory] [output.html] | sage-meta serve <port> [sage-src directory]".into());
    }
    let root = args
        .get(usize::from(serve))
        .map_or(Path::new("sage-src"), Path::new);
    let documents = sage_meta::workspace::load_documents(root)?;
    let errors = sage_meta::validate(&documents);
    if !errors.is_empty() {
        for error in &errors {
            eprintln!("{error}");
        }
        return Err(format!("{} architecture errors", errors.len()).into());
    }
    if browse || serve {
        let output = args
            .get(1)
            .map_or(Path::new("target/sage-browser.html"), Path::new);
        if browse && output.extension().is_none_or(|ext| ext != "html") {
            return Err("browser output must have an .html extension".into());
        }
        let absolute_root = root.canonicalize()?;
        let workspace = absolute_root
            .ancestors()
            .find(|path| path.join("Cargo.toml").is_file());
        let html = if let Some(workspace) = workspace {
            sage_meta::render_browser_with_workspace(&documents, workspace)?
        } else {
            sage_meta::render_browser(&documents)?
        };
        if let Some(port) = port {
            return server::serve(port, html);
        }
        if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(output, html)?;
        println!(
            "Open {} in a browser.",
            std::fs::canonicalize(output)?.display()
        );
        return Ok(());
    }
    println!(
        "Validated {} .sage documents (declared model only).",
        documents.len()
    );
    Ok(())
}

fn check(args: &[std::ffi::OsString]) -> Result<(), Box<dyn std::error::Error>> {
    let mut correspondence = false;
    let mut json = false;
    let mut root = None;
    for argument in args {
        if argument == "--correspondence" {
            correspondence = true;
        } else if argument == "--json" {
            json = true;
        } else if argument.to_str().is_some_and(|arg| arg.starts_with('-')) || root.is_some() {
            return Err(
                "usage: sage-meta check [--correspondence] [--json] [sage-src directory]".into(),
            );
        } else {
            root = Some(Path::new(argument));
        }
    }
    let root = root.unwrap_or(Path::new("sage-src"));
    let documents = match sage_meta::workspace::load_documents(root) {
        Ok(documents) => documents,
        Err(error) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"model": {"valid": false, "load_error": error.to_string()}, "correspondence": null})
                );
            }
            return Err(error);
        }
    };
    let model_errors = sage_meta::validate(&documents);
    let mut correspondence_report = None;
    let mut source_errors = Vec::new();
    if correspondence && model_errors.is_empty() {
        let result = (|| -> Result<_, Box<dyn std::error::Error>> {
            let absolute_root = root.canonicalize()?;
            let rust_root = absolute_root
                .ancestors()
                .find(|path| path.join("Cargo.toml").is_file())
                .ok_or("correspondence checking requires a Cargo workspace containing the model")?;
            sage_meta::correspondence::CorrespondenceIndex::load(rust_root)
        })();
        let index = match result {
            Ok(index) => index,
            Err(error) => {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "model": {"valid": true, "documents": documents.len(), "diagnostics": []},
                            "correspondence": {"valid": false, "load_error": error.to_string()}
                        })
                    );
                }
                return Err(error);
            }
        };
        source_errors = index.validate(&documents);
        let coverage: Vec<_> = index
            .entries
            .iter()
            .map(|(name, sites)| {
                serde_json::json!({
                    "sage_item": name,
                    "source_sites": sites.len(),
                    "executable_declarations": sites.iter().filter(|site| site.executable).count(),
                })
            })
            .collect();
        let sites = index.entries.values().flatten().count();
        let executable = index
            .entries
            .values()
            .flatten()
            .filter(|site| site.executable)
            .count();
        correspondence_report = Some(serde_json::json!({
            "valid": source_errors.is_empty(), "mapped_items": index.entries.len(),
            "source_sites": sites, "executable_declarations": executable,
            "source_only_sites": sites - executable, "diagnostics": source_errors,
            "coverage": coverage,
            "scope": "crate/module headers, named declarations, and explicitly declared methods; all source cfg alternatives",
            "runtime_conformance_checked": false,
        }));
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "model": {"valid": model_errors.is_empty(), "documents": documents.len(), "diagnostics": model_errors},
                "correspondence": correspondence_report,
            }))?
        );
    } else {
        if model_errors.is_empty() {
            println!(
                "Validated {} .sage documents (declared model).",
                documents.len()
            );
        }
        for error in model_errors.iter().chain(&source_errors) {
            eprintln!("{error}");
        }
        if let Some(report) = correspondence_report {
            println!(
                "Mapped {} Sage items at {} Rust sites ({} executable declarations; {} source-only). Runtime conformance was not checked.",
                report["mapped_items"],
                report["source_sites"],
                report["executable_declarations"],
                report["source_only_sites"]
            );
        }
    }
    let errors = model_errors.len() + source_errors.len();
    if errors == 0 {
        Ok(())
    } else {
        Err(format!("{errors} architecture/correspondence errors").into())
    }
}
