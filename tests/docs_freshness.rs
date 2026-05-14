//! Drift guard for `md/design/` design docs.
//!
//! The mdbook under `md/design/` is the *evergreen* description of
//! sage's architecture. Each entry below names an identifier that's
//! been retired from the codebase: if any of these names appears in
//! a design page, the page is stale and needs updating.
//!
//! When a real refactor renames or removes an identifier, add the
//! retired name to `RETIRED` so future drift gets caught.
//!
//! Matching is word-boundary aware (so `Item` rules don't trigger
//! on `ItemAst`), but it doesn't understand markdown structure or
//! code fences. The retired list should avoid names that legitimately
//! appear in *historical* prose (e.g. "we used to call this
//! `Foo`"). RFDs under `md/rfds/` are deliberately excluded — those
//! are journey docs and may freely reference retired names.
//!
//! If a design page genuinely needs to mention a retired name (rare),
//! either reword to use the current name, or move the historical
//! note to an RFD.

use std::fs;
use std::path::{Path, PathBuf};

/// Names of types, functions, and modules that have been removed or
/// renamed. Each entry is the *retired* name; the second field is a
/// short hint pointing at the current spelling.
///
/// Matching is word-boundary aware: `Item` won't trigger on
/// `ItemAst`, and `UseGroup` won't trigger on `UseGroupAst`.
const RETIRED: &[(&str, &str)] = &[
    // Item-kind renames: *Item → *Ast
    ("FunctionItem", "FnAst"),
    ("StructItem", "StructAst"),
    ("EnumItem", "EnumAst"),
    ("TraitItem", "TraitAst"),
    ("ImplItem", "ImplAst"),
    ("TypeAliasItem", "TypeAliasAst"),
    ("ConstItem", "ConstAst"),
    ("StaticItem", "StaticAst"),
    ("ModItem", "ModAst"),
    ("UseGroup", "UseGroupAst"),
    ("MacroDefItem", "MacroDefAst"),
    ("MacroInvocationItem", "MacroInvocationAst"),
    // Cross-source enum: Item<'db> → ItemAst<'db>. Bare `Item` is
    // too noisy to match generically (appears in plain English),
    // so check the lifetime-parameterized forms only.
    ("Item<'db>", "ItemAst<'db>"),
    // Module wrapper: Module → ModSymbol; source enum collapsed.
    ("ModuleSource", "ModSymbolData (or ModAst fields)"),
    ("ModSymbolKind", "ModSymbolData"),
    ("Module<'db>", "ModSymbol<'db>"),
    // Symbol wrapper: interned struct → Copy wrapper-of-enum.
    ("SymbolSource", "SymbolData"),
    // Memmap rename.
    ("ModuleMemmap", "ExpandedModule"),
];

/// Files under `md/design/` are scanned. RFDs are explicitly skipped
/// — they're allowed to use retired names because they document the
/// history of how we got to the current shape.
fn design_files() -> Vec<PathBuf> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let design_dir = Path::new(manifest_dir).join("md").join("design");
    let mut out = Vec::new();
    collect_md(&design_dir, &mut out);
    out.sort();
    out
}

/// Does `line` contain `needle` as a token, with word boundaries on
/// the alphanumeric edges? Allows retired names to be embedded in
/// punctuation (`<`, `(`, `,`, …) but not as a substring of a longer
/// identifier — so the rule for `UseGroup` doesn't match `UseGroupAst`.
fn line_contains_token(line: &str, needle: &str) -> bool {
    let bytes = line.as_bytes();
    let needle_bytes = needle.as_bytes();
    if needle_bytes.is_empty() || bytes.len() < needle_bytes.len() {
        return false;
    }
    let last_idx = bytes.len() - needle_bytes.len();
    for start in 0..=last_idx {
        if &bytes[start..start + needle_bytes.len()] != needle_bytes {
            continue;
        }
        let before_ok = start == 0 || !is_ident_char(bytes[start - 1]);
        let end = start + needle_bytes.len();
        // Check trailing edge only against the last char of `needle`.
        // If the needle itself ends with a non-identifier char (e.g.
        // `Item<'db>` ends with `>`), don't require a trailing
        // boundary.
        let needle_ends_in_ident = is_ident_char(needle_bytes[needle_bytes.len() - 1]);
        let after_ok = !needle_ends_in_ident || end == bytes.len() || !is_ident_char(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

#[test]
fn design_docs_use_current_names() {
    let files = design_files();
    assert!(
        !files.is_empty(),
        "expected to find design docs under md/design/"
    );

    let mut violations: Vec<String> = Vec::new();

    for file in &files {
        let text = fs::read_to_string(file).expect("read design doc");
        for (retired, hint) in RETIRED {
            for (line_idx, line) in text.lines().enumerate() {
                if line_contains_token(line, retired) {
                    violations.push(format!(
                        "{}:{}: retired name `{}` (use `{}`)\n    {}",
                        file.display(),
                        line_idx + 1,
                        retired,
                        hint,
                        line.trim_end()
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "stale design docs reference retired identifiers:\n\n{}\n\n\
         If the refactor is complete, update the design page.\n\
         If the reference is genuinely historical, move it to an RFD.\n\
         (See `tests/docs_freshness.rs` for the retired-name list.)",
        violations.join("\n")
    );
}
