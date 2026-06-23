#![feature(rustc_private)]

use std::path::Path;
use std::process;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: sage-oracle <file.rs>");
        process::exit(1);
    }

    let path = Path::new(&args[1]);
    match sage_oracle::analyze_file(path) {
        Ok(krate) => {
            let json = serde_json::to_string_pretty(&krate).unwrap();
            println!("{json}");
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}
