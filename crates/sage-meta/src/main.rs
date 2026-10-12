mod server;

use std::path::Path;

fn load(
    path: &Path,
    documents: &mut Vec<sage_meta::Document>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut entries = std::fs::read_dir(path)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    for path in entries {
        if path.is_symlink() {
            continue;
        }
        if path.is_dir() {
            load(&path, documents)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "sage")
        {
            documents.push(sage_meta::parse(
                &path.to_string_lossy(),
                &std::fs::read_to_string(&path)?,
            )?);
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args
        .first()
        .is_some_and(|arg| arg == "--help" || arg == "-h")
    {
        println!(
            "sage-meta [sage-src directory]\nsage-meta browse [sage-src directory] [output.html]\nsage-meta serve <port> [sage-src directory]\n\nValidate the model, or export a standalone interactive HTML explorer."
        );
        return Ok(());
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
    let mut documents = Vec::new();
    load(root, &mut documents)?;
    if documents.is_empty() {
        return Err("no .sage documents found".into());
    }
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
