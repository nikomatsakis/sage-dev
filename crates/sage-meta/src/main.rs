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
            "sage-meta [model directory]\nsage-meta check [--json] [model directory]\nsage-meta review start <anchor> [--instructions <file>]\nsage-meta review read <anchor> <path> [selector]\nsage-meta review record <anchor> <report-file>\nsage-meta browse [model directory] [output.html]\nsage-meta serve <port> [model directory]\n\nCheck the model, Rust correspondence, specification tests, and review snapshots, or export an interactive explorer."
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
            .ok_or("usage: sage-meta serve <port> [model directory]")?
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
        return Err("usage: sage-meta [model directory] | sage-meta browse [model directory] [output.html] | sage-meta serve <port> [model directory]".into());
    }
    let root = match args.get(usize::from(serve)) {
        Some(root) => Path::new(root).to_path_buf(),
        None => sage_meta::workspace::default_model_root(Path::new("."))?,
    };
    let documents = sage_meta::workspace::load_documents(&root)?;
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

// {anchor}`meta.reviews.preparation`
// {anchor}`meta.reviews.recorded_reads`
// {anchor}`meta.reviews.recording`
// {anchor}`meta.reviews.review_instructions`
fn review(args: &[std::ffi::OsString]) -> Result<(), Box<dyn std::error::Error>> {
    use sage_meta::review::{self, Selector, ValidationOutput};
    let args = args
        .iter()
        .map(|arg| arg.to_str().ok_or("arguments must be UTF-8"))
        .collect::<Result<Vec<_>, _>>()?;
    match args.as_slice() {
        ["start", anchor] => {
            println!("{}",serde_json::to_string_pretty(&review::start(Path::new("."),anchor,Vec::new())?)?);
        }
        ["start", anchor, "--instructions", file] => {
            let instructions=std::fs::read_to_string(file)?;
            println!("{}",serde_json::to_string_pretty(&review::start(Path::new("."),anchor,vec![instructions])?)?);
        }
        ["read", anchor, path, tail @ ..] => {
            let selector=match tail {
                [] => Selector::WholeFile,
                ["--lines",start,end] => Selector::LineRange {start:start.parse()?,end:end.parse()?},
                ["--claim",anchor_id] => Selector::Claim {anchor_id:(*anchor_id).into()},
                ["--test",crate_name,target,rust_path] => Selector::Test {crate_name:(*crate_name).into(),target:(*target).into(),rust_path:(*rust_path).into()},
                _ => return Err("read selector: --lines START END | --claim ANCHOR | --test CRATE TARGET RUST_PATH".into()),
            };
            print!("{}",review::capture_read(Path::new("."),anchor,path,selector)?);
        }
        ["record", anchor, file] => {
            let output:ValidationOutput=serde_json::from_slice(&std::fs::read(file)?)?;
            review::record(Path::new("."),anchor,&output)?;
            println!("Recorded validation in validations/{anchor}/.");
        }
        _ => return Err("usage: cargo meta review start ANCHOR [--instructions FILE] | cargo meta review read ANCHOR PATH [--lines START END | --claim ANCHOR | --test CRATE TARGET RUST_PATH] | cargo meta review record ANCHOR REPORT_FILE".into()),
    }
    Ok(())
}

// {anchor}`meta.reviews.gate`
// {anchor}`meta.reviews.record_required`
// {anchor}`meta.reviews.review_diagnostics`
fn check(args: &[std::ffi::OsString]) -> Result<(), Box<dyn std::error::Error>> {
    let mut json = false;
    let mut root = None;
    for argument in args {
        if argument == "--correspondence" {
            // Kept for existing scripts; correspondence is now always checked.
        } else if argument == "--json" {
            json = true;
        } else if argument.to_str().is_some_and(|arg| arg.starts_with('-')) || root.is_some() {
            return Err("usage: sage-meta check [--json] [model directory]".into());
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
