//! Shared fixture-based test harness for MEM-map and resolution tests.
//!
//! Tests declare one or more files with `//- /path` marker lines; each
//! assertion method creates a fresh database, builds the fixture, runs the
//! query, and compares the rendered output to an `expect_test::Expect`.
//!
//! # Example
//!
//! ```ignore
//! t(r#"
//!     //- /lib.rs
//!     struct X;
//! "#)
//! .resolve("X", Namespace::Type, expect!["<local Struct X>"])
//! .errors(expect![""]);
//! ```
//!
//! Because each method re-builds the database, calls are independent — tests
//! don't have to worry about query-log interference between assertions. This
//! is cheap in practice: fixtures are small and setup is microseconds.

#![allow(dead_code)]

use expect_test::Expect;
use sage_ir::db::Database;
use sage_ir::memmap::{MacroUseState, MemmapEntry, memmap_errors, module_memmap};
use sage_ir::module::{Module, ModuleSource};
use sage_ir::name::Name;
use sage_ir::resolve::{
    MacroKind, Namespace, ResolutionError, SourceRoot, resolve_module_path, resolve_name,
};
use sage_ir::source::SourceFile;
use sage_ir::symbol::{Symbol, SymbolSource};
use salsa::Database as _;

// ---------------------------------------------------------------------------
// Fixture parsing
// ---------------------------------------------------------------------------

/// Parse a fixture string into `(path, text)` pairs.
///
/// Files are separated by `//- /path` marker lines. Everything before the
/// first marker is discarded. Common leading indentation is stripped from
/// each file body so fixtures can be written indented inside raw strings.
///
/// Single-file fixtures without any marker are treated as `/lib.rs`.
pub fn parse_fixture(src: &str) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = Vec::new();
    let mut current: Option<(String, Vec<String>)> = None;

    for line in src.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("//- /") {
            if let Some((path, body)) = current.take() {
                files.push((path, dedent(&body)));
            }
            let path = rest.trim().to_owned();
            current = Some((path, Vec::new()));
        } else if let Some((_, body)) = &mut current {
            body.push(line.to_owned());
        }
    }
    if let Some((path, body)) = current {
        files.push((path, dedent(&body)));
    }

    // No marker at all → treat whole thing as lib.rs.
    if files.is_empty() {
        files.push(("lib.rs".to_owned(), src.to_owned()));
    }

    files
}

fn dedent(lines: &[String]) -> String {
    // Strip leading/trailing blank lines and compute common indent on
    // non-blank lines.
    let start = lines.iter().position(|l| !l.trim().is_empty()).unwrap_or(0);
    let end = lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map(|i| i + 1)
        .unwrap_or(0);
    let slice = &lines[start..end];

    let common_indent = slice
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.chars().take_while(|c| *c == ' ').count())
        .min()
        .unwrap_or(0);

    let mut out = String::new();
    for line in slice {
        if line.len() >= common_indent {
            out.push_str(&line[common_indent..]);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// TestCrate — fluent entry point
// ---------------------------------------------------------------------------

/// Entry point — parses the fixture and returns a builder.
pub fn t(fixture: &str) -> TestCrate {
    TestCrate {
        files: parse_fixture(fixture),
    }
}

/// Fluent test builder.
///
/// Each assertion method creates a fresh database and re-builds the fixture,
/// so calls on the same `TestCrate` are independent.
pub struct TestCrate {
    files: Vec<(String, String)>,
}

impl TestCrate {
    /// Assert that `resolve_name(root, name, ns)` produces the expected
    /// rendered symbol (or error string).
    pub fn resolve(&self, name: &str, ns: Namespace, expect: Expect) -> &Self {
        self.resolve_in(&[], name, ns, expect)
    }

    /// Like `resolve`, but resolves against a submodule identified by a
    /// path like `["inner", "deeper"]` from the crate root.
    pub fn resolve_in(
        &self,
        module_path: &[&str],
        name: &str,
        ns: Namespace,
        expect: Expect,
    ) -> &Self {
        let db = Database::default();
        db.attach(|db| {
            let (source_root, root) = self.setup(db);
            let module = if module_path.is_empty() {
                root
            } else {
                match resolve_module_path(db, root, source_root, module_path) {
                    Some(m) => m,
                    None => panic!("module path {module_path:?} did not resolve"),
                }
            };

            let name_interned = Name::new(db, name.to_owned());
            let result = resolve_name(db, module, source_root, root, name_interned, ns);
            let rendered = fmt_resolve_result(db, &result);
            expect.assert_eq(&rendered);
        });
        self
    }

    /// Assert on `memmap_errors` aggregated across every module reachable
    /// from the crate root. An empty string means "no errors".
    pub fn errors(&self, expect: Expect) -> &Self {
        let db = Database::default();
        db.attach(|db| {
            let (source_root, root) = self.setup(db);
            let mut errs: Vec<String> = Vec::new();
            let mut visited: Vec<Module<'_>> = Vec::new();
            self.collect_errors(db, root, source_root, root, &mut errs, &mut visited);
            errs.sort();
            let rendered = errs.join("\n");
            expect.assert_eq(&rendered);
        });
        self
    }

    /// Pretty-print the memmap of a module identified by a path like
    /// `[]` (root) or `["inner"]`.
    pub fn memmap(&self, module_path: &[&str], expect: Expect) -> &Self {
        let db = Database::default();
        db.attach(|db| {
            let (source_root, root) = self.setup(db);
            let module = if module_path.is_empty() {
                root
            } else {
                match resolve_module_path(db, root, source_root, module_path) {
                    Some(m) => m,
                    None => panic!("module path {module_path:?} did not resolve"),
                }
            };
            let memmap = module_memmap(db, module, source_root, root);
            let rendered = fmt_memmap_entries(db, memmap.entries(db), 0);
            expect.assert_eq(&rendered);
        });
        self
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn setup<'db>(&self, db: &'db Database) -> (SourceRoot, Module<'db>) {
        let source_files: Vec<SourceFile> = self
            .files
            .iter()
            .map(|(path, text)| SourceFile::new(db, path.clone(), text.clone()))
            .collect();
        let source_root = SourceRoot::new(db, source_files.clone());

        // Crate root: prefer lib.rs, fall back to main.rs.
        let lib_file = source_files
            .iter()
            .find(|f| {
                let p = f.path(db);
                p == "lib.rs" || p == "main.rs"
            })
            .copied()
            .unwrap_or_else(|| {
                panic!(
                    "fixture has no lib.rs or main.rs; files = {:?}",
                    self.files.iter().map(|(p, _)| p).collect::<Vec<_>>()
                )
            });

        let root = Module::new(
            db,
            ModuleSource::Local {
                file: lib_file,
                parent: None,
            },
        );
        (source_root, root)
    }

    fn collect_errors<'db>(
        &self,
        db: &'db Database,
        module: Module<'db>,
        source_root: SourceRoot,
        crate_root: Module<'db>,
        out: &mut Vec<String>,
        visited: &mut Vec<Module<'db>>,
    ) {
        if visited.contains(&module) {
            return;
        }
        visited.push(module);

        if matches!(module.source(db), ModuleSource::External(..)) {
            return;
        }

        let errs = memmap_errors(db, module, source_root, crate_root);
        for err in &errs {
            out.push(fmt_memmap_error(db, err));
        }

        // Recurse into child modules declared by this module.
        let memmap = module_memmap(db, module, source_root, crate_root);
        for entry in memmap.entries(db) {
            if let MemmapEntry::Item(sage_ir::item::Item::Mod(mod_item)) = entry {
                if let Some(child) =
                    sage_ir::resolve::resolve_mod(db, module, *mod_item, source_root)
                {
                    self.collect_errors(db, child, source_root, crate_root, out, visited);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Pretty-printers
// ---------------------------------------------------------------------------

fn fmt_resolve_result(db: &dyn sage_ir::Db, result: &Result<Symbol, ResolutionError>) -> String {
    match result {
        Ok(sym) => fmt_symbol(db, *sym),
        Err(ResolutionError::Unresolved) => "<unresolved>".to_owned(),
        Err(ResolutionError::Ambiguous) => "<ambiguous>".to_owned(),
    }
}

pub fn fmt_symbol(db: &dyn sage_ir::Db, sym: Symbol) -> String {
    match sym.source(db) {
        SymbolSource::Local(item) => {
            let (kind, name) = item_kind_and_name(db, item);
            match name {
                Some(n) => format!("<local {kind} {n}>"),
                None => format!("<local {kind}>"),
            }
        }
        SymbolSource::External(cn, di) => match db.tcx().def_path(cn, di) {
            Some(path) => format!("<ext {path}>"),
            None => format!("<ext {}:{}>", cn.0, di.0),
        },
    }
}

fn item_kind_and_name(
    db: &dyn sage_ir::Db,
    item: sage_ir::item::Item<'_>,
) -> (&'static str, Option<String>) {
    use sage_ir::item::Item;
    match item {
        Item::Function(f) => ("Function", Some(f.name(db).text(db).clone())),
        Item::Struct(s) => ("Struct", Some(s.name(db).text(db).clone())),
        Item::Enum(e) => ("Enum", Some(e.name(db).text(db).clone())),
        Item::Trait(t) => ("Trait", Some(t.name(db).text(db).clone())),
        Item::TypeAlias(t) => ("TypeAlias", Some(t.name(db).text(db).clone())),
        Item::Const(c) => ("Const", Some(c.name(db).text(db).clone())),
        Item::Static(s) => ("Static", Some(s.name(db).text(db).clone())),
        Item::Mod(m) => ("Mod", Some(m.name(db).text(db).clone())),
        Item::Impl(_) => ("Impl", None),
        Item::Use(_) => ("Use", None),
        Item::MacroDef(d) => ("MacroDef", Some(d.name(db).text(db).clone())),
        Item::MacroInvocation(_) => ("MacroInvocation", None),
        Item::Error(_) => ("Error", None),
    }
}

pub fn fmt_namespace(ns: Namespace) -> &'static str {
    match ns {
        Namespace::Type => "Type",
        Namespace::Value => "Value",
        Namespace::Macro(MacroKind::Bang) => "Macro(Bang)",
        Namespace::Macro(MacroKind::Attr) => "Macro(Attr)",
        Namespace::Macro(MacroKind::Derive) => "Macro(Derive)",
    }
}

pub fn fmt_memmap_entries(db: &dyn sage_ir::Db, entries: &[MemmapEntry], indent: usize) -> String {
    let mut out = String::new();
    for entry in entries {
        fmt_entry(db, entry, indent, &mut out);
    }
    if !out.is_empty() && out.ends_with('\n') {
        out.pop();
    }
    out
}

fn fmt_entry(db: &dyn sage_ir::Db, entry: &MemmapEntry, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    match entry {
        MemmapEntry::Item(item) => {
            let (kind, name) = item_kind_and_name(db, *item);
            out.push_str(&pad);
            match name {
                Some(n) => out.push_str(&format!("Item {n} kind={kind}\n")),
                None => out.push_str(&format!("Item kind={kind}\n")),
            }
        }
        MemmapEntry::MacroDef(def) => {
            out.push_str(&pad);
            out.push_str(&format!("MacroDef {}\n", def.name(db).text(db)));
        }
        MemmapEntry::Redirect { name, target } => {
            out.push_str(&pad);
            out.push_str(&format!(
                "Redirect {} target={}\n",
                name.text(db),
                fmt_path(db, *target)
            ));
        }
        MemmapEntry::Glob { path } => {
            out.push_str(&pad);
            out.push_str(&format!("Glob path={}\n", fmt_path(db, *path)));
        }
        MemmapEntry::MacroUse(mu) => {
            out.push_str(&pad);
            out.push_str(&format!(
                "MacroUse path={} state={}\n",
                fmt_path(db, mu.path),
                fmt_macro_use_state(db, &mu.state, indent + 1)
            ));
        }
    }
}

fn fmt_macro_use_state(db: &dyn sage_ir::Db, state: &MacroUseState, indent: usize) -> String {
    match state {
        MacroUseState::Unresolved => "Unresolved".to_owned(),
        MacroUseState::Resolved(callees) => {
            let cs: Vec<String> = callees.iter().map(|c| fmt_callee(db, c)).collect();
            format!("Resolved [{}]", cs.join(", "))
        }
        MacroUseState::Expanded(exps) => {
            let mut s = String::from("Expanded [\n");
            for exp in exps {
                s.push_str(&"  ".repeat(indent));
                s.push_str(&format!(
                    "branch callee={} {{\n",
                    fmt_callee(db, &exp.callee)
                ));
                s.push_str(&fmt_memmap_entries(db, &exp.entries, indent + 1));
                s.push('\n');
                s.push_str(&"  ".repeat(indent));
                s.push_str("}\n");
            }
            s.push_str(&"  ".repeat(indent.saturating_sub(1)));
            s.push(']');
            s
        }
    }
}

fn fmt_callee(db: &dyn sage_ir::Db, callee: &sage_ir::memmap::MacroCallee) -> String {
    use sage_ir::memmap::MacroCallee;
    match callee {
        MacroCallee::Rules(def) => format!("Rules({})", def.name(db).text(db)),
        MacroCallee::Builtin(kind) => format!("Builtin({kind:?})"),
        MacroCallee::Proc {
            crate_num,
            def_index,
        } => {
            format!("Proc({},{})", crate_num.0, def_index.0)
        }
    }
}

fn fmt_path(db: &dyn sage_ir::Db, path: sage_ir::types::Path) -> String {
    path.segments(db)
        .iter()
        .map(|s| {
            let text = s.text(db);
            if text.is_empty() {
                "::".to_owned()
            } else {
                text.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("::")
}

fn fmt_module(db: &dyn sage_ir::Db, module: Module) -> String {
    match module.source(db) {
        ModuleSource::Local { file, .. } => format!("\"{}\"", file.path(db)),
        ModuleSource::External(cn, di) => format!("extern({},{})", cn.0, di.0),
    }
}

pub fn fmt_memmap_error(db: &dyn sage_ir::Db, err: &sage_ir::memmap::MemmapError) -> String {
    use sage_ir::memmap::MemmapError::*;
    match err {
        DuplicateName { name, ns } => {
            format!(
                "DuplicateName name={} ns={}",
                name.text(db),
                fmt_namespace(*ns)
            )
        }
        UnresolvedMacro { path } => format!("UnresolvedMacro path={}", fmt_path(db, *path)),
        AmbiguousMacro { path } => format!("AmbiguousMacro path={}", fmt_path(db, *path)),
        TimeTravelViolation { name, ns } => format!(
            "TimeTravelViolation name={} ns={}",
            name.text(db),
            fmt_namespace(*ns)
        ),
    }
}
