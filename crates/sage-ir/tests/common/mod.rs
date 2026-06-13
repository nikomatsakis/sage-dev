//! Shared fixture-based test harness for MEM-map and resolution tests.

#![allow(dead_code)]

use expect_test::Expect;
use sage_ir::db::Database;
use sage_ir::item::ModAst;
use sage_ir::memmap::{MemmapEntry, memmap_errors, module_memmap};
use sage_ir::module::{ModSymbol, ModSymbolData};
use sage_ir::name::Name;
use sage_ir::resolve::{
    MacroKind, Namespace, ResolutionError, SourceRoot, resolve_module_path, resolve_name,
};
use sage_ir::source::SourceFile;
use sage_ir::symbol::{Symbol, SymbolData};
use sage_stash::{Slice, Stash};
use salsa::Database as _;

// ---------------------------------------------------------------------------
// Fixture parsing
// ---------------------------------------------------------------------------

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

    if files.is_empty() {
        files.push(("lib.rs".to_owned(), src.to_owned()));
    }

    files
}

fn dedent(lines: &[String]) -> String {
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

pub fn t(fixture: &str) -> TestCrate {
    TestCrate {
        files: parse_fixture(fixture),
    }
}

pub struct TestCrate {
    files: Vec<(String, String)>,
}

impl TestCrate {
    pub fn resolve(&self, name: &str, ns: Namespace, expect: Expect) -> &Self {
        self.resolve_in(&[], name, ns, expect)
    }

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
            let result = resolve_name(db, module, source_root, name_interned, ns);
            let rendered = fmt_resolve_result(db, &result);
            expect.assert_eq(&rendered);
        });
        self
    }

    pub fn errors(&self, expect: Expect) -> &Self {
        let db = Database::default();
        db.attach(|db| {
            let (source_root, root) = self.setup(db);
            let mut errs: Vec<String> = Vec::new();
            let mut visited: Vec<ModSymbol<'_>> = Vec::new();
            self.collect_errors(db, root, source_root, root, &mut errs, &mut visited);
            errs.sort();
            let rendered = errs.join("\n");
            expect.assert_eq(&rendered);
        });
        self
    }

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
            let memmap = module_memmap(db, module, source_root);
            let stash = memmap.stash(db);
            let entries = memmap.entries(db);
            let rendered = fmt_memmap_entries(db, stash, entries, 0);
            expect.assert_eq(&rendered);
        });
        self
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn setup<'db>(&self, db: &'db Database) -> (SourceRoot, ModSymbol<'db>) {
        let source_files: Vec<SourceFile> = self
            .files
            .iter()
            .map(|(path, text)| SourceFile::new(db, path.clone(), text.clone()))
            .collect();
        let source_root = SourceRoot::new(db, source_files.clone());

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

        let root = ModSymbol::ast(ModAst::crate_root(db, lib_file));
        (source_root, root)
    }

    fn collect_errors<'db>(
        &self,
        db: &'db Database,
        module: ModSymbol<'db>,
        source_root: SourceRoot,
        crate_root: ModSymbol<'db>,
        out: &mut Vec<String>,
        visited: &mut Vec<ModSymbol<'db>>,
    ) {
        if visited.contains(&module) {
            return;
        }
        visited.push(module);

        if matches!(module.data(), ModSymbol::Ext(_)) {
            return;
        }

        let errs = memmap_errors(db, module, source_root);
        for err in &errs {
            out.push(fmt_memmap_error(db, err));
        }

        // Recurse into child modules declared by this module.
        let memmap = module_memmap(db, module, source_root);
        let stash = memmap.stash(db);
        let entries = memmap.entries(db);
        for entry in &stash[entries] {
            if let MemmapEntry::Item(sage_ir::item::LocalModItemSym::Mod(mod_item)) = entry {
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
    if let Some(ext) = sym.as_ext() {
        return match db.tcx().def_path(ext.crate_num, ext.def_index) {
            Some(path) => format!("<ext {path}>"),
            None => format!("<ext {}:{}>", ext.crate_num.0, ext.def_index.0),
        };
    }
    match sym {
        SymbolData::Fn(s) => format!("<local Function {}>", s.as_ast().unwrap().name(db).text(db)),
        SymbolData::Struct(s) => {
            format!("<local Struct {}>", s.as_ast().unwrap().name(db).text(db))
        }
        SymbolData::TupleStructCtor(s) => {
            format!("<ctor {}>", s.as_ast().unwrap().name(db).text(db))
        }
        SymbolData::Enum(s) => format!("<local Enum {}>", s.as_ast().unwrap().name(db).text(db)),
        SymbolData::Trait(s) => {
            format!("<local Trait {}>", s.as_ast().unwrap().name(db).text(db))
        }
        SymbolData::Impl(_) => "<local Impl>".to_owned(),
        SymbolData::Mod(m) => match m {
            sage_ir::module::ModSymbol::Ast(a) => {
                format!("<local Mod {}>", a.name(db).text(db))
            }
            sage_ir::module::ModSymbol::Ext(_) => unreachable!(),
        },
        SymbolData::TypeAlias(s) => {
            format!(
                "<local TypeAlias {}>",
                s.as_ast().unwrap().name(db).text(db)
            )
        }
        SymbolData::Const(s) => {
            format!("<local Const {}>", s.as_ast().unwrap().name(db).text(db))
        }
        SymbolData::Static(s) => {
            format!("<local Static {}>", s.as_ast().unwrap().name(db).text(db))
        }
        SymbolData::MacroDef(_) => "<local MacroDef>".to_owned(),
        SymbolData::Use(_) => "<local Use>".to_owned(),
        SymbolData::MacroInvocation(_) => "<local MacroInvocation>".to_owned(),
        SymbolData::GenericParam(p) => match p.name(db) {
            Some(n) => format!("<param {}>", n.text(db)),
            None => "<param ?>".to_owned(),
        },
        SymbolData::Intrinsic(i) => format!("<intrinsic {i:?}>"),
        SymbolData::Error(_) => "<local Error>".to_owned(),
        SymbolData::Unknown(_) => unreachable!(),
    }
}

fn item_kind_and_name(
    db: &dyn sage_ir::Db,
    item: sage_ir::item::LocalModItemSym<'_>,
) -> (&'static str, Option<String>) {
    use sage_ir::item::LocalModItemSym;
    match item {
        LocalModItemSym::Function(f) => ("Function", Some(f.name(db).text(db).clone())),
        LocalModItemSym::Struct(s) => ("Struct", Some(s.name(db).text(db).clone())),
        LocalModItemSym::Enum(e) => ("Enum", Some(e.name(db).text(db).clone())),
        LocalModItemSym::Trait(t) => ("Trait", Some(t.name(db).text(db).clone())),
        LocalModItemSym::TypeAlias(t) => ("TypeAlias", Some(t.name(db).text(db).clone())),
        LocalModItemSym::Const(c) => ("Const", Some(c.name(db).text(db).clone())),
        LocalModItemSym::Static(s) => ("Static", Some(s.name(db).text(db).clone())),
        LocalModItemSym::Mod(m) => ("Mod", Some(m.name(db).text(db).clone())),
        LocalModItemSym::Impl(_) => ("Impl", None),
        LocalModItemSym::Use(_) => ("Use", None),
        LocalModItemSym::MacroDef(d) => ("MacroDef", Some(d.name(db).text(db).clone())),
        LocalModItemSym::MacroInvocation(_) => ("MacroInvocation", None),
        LocalModItemSym::Error(..) => ("Error", None),
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

pub fn fmt_memmap_entries(
    db: &dyn sage_ir::Db,
    stash: &Stash,
    entries: Slice<MemmapEntry>,
    indent: usize,
) -> String {
    let mut out = String::new();
    for entry in &stash[entries] {
        fmt_entry(db, stash, entry, indent, &mut out);
    }
    if !out.is_empty() && out.ends_with('\n') {
        out.pop();
    }
    out
}

fn fmt_entry(
    db: &dyn sage_ir::Db,
    stash: &Stash,
    entry: &MemmapEntry,
    indent: usize,
    out: &mut String,
) {
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
        MemmapEntry::TupleStructCtor(s) => {
            out.push_str(&pad);
            out.push_str(&format!("TupleStructCtor {}\n", s.name(db).text(db)));
        }
        MemmapEntry::MacroDef(def) => {
            out.push_str(&pad);
            out.push_str(&format!("MacroDef {}\n", def.name(db).text(db)));
        }
        MemmapEntry::Redirect { name, target } => {
            out.push_str(&pad);
            let target_slice = &stash[*target];
            out.push_str(&format!(
                "Redirect {} target={}\n",
                name.text(db),
                fmt_name_path(db, target_slice)
            ));
        }
        MemmapEntry::Glob { path } => {
            out.push_str(&pad);
            let path_slice = &stash[*path];
            out.push_str(&format!("Glob path={}\n", fmt_name_path(db, path_slice)));
        }
        MemmapEntry::MacroUse(mu) => {
            out.push_str(&pad);
            let path_slice = &stash[mu.path];
            out.push_str(&format!(
                "MacroUse path={} state={}\n",
                fmt_name_path(db, path_slice),
                fmt_macro_use_state(db, stash, &mu.state(stash), indent + 1)
            ));
        }
    }
}

fn fmt_macro_use_state(
    db: &dyn sage_ir::Db,
    stash: &Stash,
    state: &sage_ir::memmap::MacroUseState,
    indent: usize,
) -> String {
    use sage_ir::memmap::MacroUseState;
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
                s.push_str(&fmt_memmap_entries(db, stash, exp.entries, indent + 1));
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

fn fmt_name_path(db: &dyn sage_ir::Db, path: &[sage_ir::name::Name]) -> String {
    path.iter()
        .map(|n| {
            let text = n.text(db);
            if text.is_empty() {
                "::".to_owned()
            } else {
                text.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("::")
}

fn fmt_module(db: &dyn sage_ir::Db, module: ModSymbol) -> String {
    match module {
        ModSymbol::Ast(ast) => match (ast.file(db), ast.inline_unexpanded_items(db).is_some()) {
            (Some(f), _) => format!("\"{}\"", f.path(db)),
            (None, true) => format!("inline \"{}\"", ast.name(db).text(db)),
            (None, false) => format!("decl \"{}\"", ast.name(db).text(db)),
        },
        ModSymbol::Ext(ext) => format!("extern({},{})", ext.crate_num.0, ext.def_index.0),
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
        UnresolvedMacro { path } => {
            format!("UnresolvedMacro path={}", fmt_name_path(db, path))
        }
        AmbiguousMacro { path } => {
            format!("AmbiguousMacro path={}", fmt_name_path(db, path))
        }
        TimeTravelViolation { name, ns } => format!(
            "TimeTravelViolation name={} ns={}",
            name.text(db),
            fmt_namespace(*ns)
        ),
        UnresolvedRedirect { name } => {
            format!("UnresolvedRedirect name={}", name.text(db))
        }
        UnresolvedGlob { path } => {
            format!("UnresolvedGlob path={}", fmt_name_path(db, path))
        }
    }
}
