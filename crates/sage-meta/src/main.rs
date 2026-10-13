#![feature(register_tool)]
#![register_tool(sage)]
mod book;
mod server;

use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args
        .first()
        .is_some_and(|arg| arg == "--help" || arg == "-h")
    {
        println!(
            "sage-meta [sage-src directory]\nsage-meta check [--json] [sage-src directory]\nsage-meta review prepare\nsage-meta review record <file>\nsage-meta browse [sage-src directory] [output.html]\nsage-meta serve <port> [sage-src directory]\n\nCheck the model, Rust correspondence, specification tests, and review snapshots, or export an interactive explorer."
        );
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "preprocess") {
        return book::preprocess(&args[1..]);
    }
    if args.first().is_some_and(|arg| arg == "check") {
        return check(&args[1..]);
    }
    if args.first().is_some_and(|arg| arg == "review") {
        return review(&args[1..]);
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

fn review(args: &[std::ffi::OsString]) -> Result<(), Box<dyn std::error::Error>> {
    match args {
        [command] if command == "prepare" => {
            let pending = sage_meta::review::prepare(Path::new("."))?;
            println!("{}", serde_json::to_string_pretty(&pending)?);
        }
        [command, path] if command == "record" => {
            let reviewed = sage_meta::review::load(Path::new(path))?;
            sage_meta::review::record(Path::new("."), &reviewed)?;
            println!(
                "Recorded reviewed snapshots in {}.",
                sage_meta::review::CHECKLIST_FILE
            );
        }
        _ => return Err("usage: sage-meta review prepare | sage-meta review record <file>".into()),
    }
    Ok(())
}

fn check(args: &[std::ffi::OsString]) -> Result<(), Box<dyn std::error::Error>> {
    let mut json = false;
    let mut root = None;
    for argument in args {
        if argument == "--correspondence" {
            // Kept for existing scripts; correspondence is now always checked.
        } else if argument == "--json" {
            json = true;
        } else if argument.to_str().is_some_and(|arg| arg.starts_with('-')) || root.is_some() {
            return Err("usage: sage-meta check [--json] [sage-src directory]".into());
        } else {
            root = Some(Path::new(argument));
        }
    }
    let report = match root {
        Some(root) => sage_meta::check::check_model(root),
        None => sage_meta::check(Path::new(".")),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    }
    report.ensure_valid()?;
    if !json {
        print!("{report}");
    }
    Ok(())
}
