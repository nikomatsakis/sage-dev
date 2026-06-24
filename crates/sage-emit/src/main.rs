use std::path::Path;
use std::process;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: sage-emit <file.rs> [additional_file.rs ...]");
        eprintln!();
        eprintln!("For single-file crates, pass one .rs file.");
        eprintln!("For multi-file crates, pass the entry file (lib.rs) first,");
        eprintln!("then all additional source files with relative paths.");
        process::exit(1);
    }

    let entry_path = Path::new(&args[1]);
    let entry_content = std::fs::read_to_string(entry_path).unwrap_or_else(|e| {
        eprintln!("error reading {}: {e}", entry_path.display());
        process::exit(1);
    });

    let entry_name = entry_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    if args.len() == 2 {
        let krate = sage_test_harness::with_test_crate(&entry_content, |db, root| {
            sage_emit::emit_module(db, root)
        });
        let json = serde_json::to_string_pretty(&krate).unwrap();
        println!("{json}");
    } else {
        let mut files: Vec<(String, String)> = vec![(entry_name, entry_content)];
        for extra in &args[2..] {
            let path = Path::new(extra);
            let content = std::fs::read_to_string(path).unwrap_or_else(|e| {
                eprintln!("error reading {}: {e}", path.display());
                process::exit(1);
            });
            let rel = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            files.push((rel, content));
        }
        let refs: Vec<(&str, &str)> = files
            .iter()
            .map(|(p, c)| (p.as_str(), c.as_str()))
            .collect();
        let krate = sage_test_harness::with_test_crate_files(&refs, |db, root| {
            sage_emit::emit_module(db, root)
        });
        let json = serde_json::to_string_pretty(&krate).unwrap();
        println!("{json}");
    }
}
