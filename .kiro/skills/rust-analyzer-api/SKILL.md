---
name: rust-analyzer-api
description: "Guide to rust-analyzer's public API crates (ra_ap_*). Use this skill when building Rust analysis tools, code intelligence, linters, refactoring tools, or any programmatic consumer of rust-analyzer's libraries. Covers the full crate hierarchy: ide (Analysis/AnalysisHost), hir (Semantics, semantic model), syntax (CST/AST/parsing), ide_db (RootDatabase/Salsa), vfs (virtual filesystem), load-cargo (bootstrapping), project-model (Cargo discovery), and secondary crates (completion, SSR, hir_def, hir_ty, cfg, parser, paths, proc_macro_api). All crates at version 0.0.330. Do NOT use this skill for contributing to rust-analyzer itself or for rustc_driver-based tools (see the rustc-driver-frontend skill instead)."
---

# rust-analyzer Public API Guide (`ra_ap_*` crates, v0.0.330)

## When this applies

You're building a tool that analyzes Rust code at a semantic level — types, traits, name resolution, completions, diagnostics — and you want to reuse rust-analyzer's analysis engine as a library rather than talking to it over LSP. This is the right choice when:

- You need IDE-grade analysis (go-to-definition, find-references, type inference) in a batch tool
- You want to build a custom linter, refactoring tool, or code generator on top of rust-analyzer
- You need to parse Rust source into a full-fidelity syntax tree with error recovery
- You want SCIP/LSIF index generation, structural search-replace, or custom diagnostics

This is NOT the right choice when:
- You need rustc's exact type system, MIR, or monomorphization — use `rustc_driver` (see rustc-driver-frontend skill)
- You just need to talk to rust-analyzer over LSP — use `lsp-types` directly
- You need proc-macro expansion in isolation — use `proc-macro2`/`syn`/`quote`

## Architecture overview

```
┌─────────────────────────────────────────────────────────┐
│  ra_ap_rust-analyzer  (LSP server binary — not the API) │
└────────────────────────┬────────────────────────────────┘
                         │ uses
┌────────────────────────▼────────────────────────────────┐
│  ra_ap_ide          — Top-level IDE API                 │
│    Analysis / AnalysisHost                              │
├─────────────────────────────────────────────────────────┤
│  ra_ap_hir          — High-level semantic model         │
│    Semantics, Module, Function, Struct, Type, Trait     │
├─────────────────────────────────────────────────────────┤
│  ra_ap_ide_db       — Salsa database + symbol index     │
│    RootDatabase, Definition, Search, LineIndex          │
├──────────────────────┬──────────────────────────────────┤
│  ra_ap_hir_def       │  ra_ap_hir_ty                    │
│  (definitions, nameres)  (type inference, trait solving) │
├──────────────────────┴──────────────────────────────────┤
│  ra_ap_syntax       — CST/AST, parsing                  │
│    SyntaxNode, SourceFile, ast::*, AstNode              │
├─────────────────────────────────────────────────────────┤
│  ra_ap_parser       — Token → syntax tree               │
├─────────────────────────────────────────────────────────┤
│  Bootstrapping layer:                                   │
│    ra_ap_load_cargo    — Cargo project → RootDatabase   │
│    ra_ap_project_model — Project/workspace discovery    │
│    ra_ap_vfs           — Virtual file system            │
│    ra_ap_vfs_notify    — File-watching VFS loader       │
├─────────────────────────────────────────────────────────┤
│  Utilities:                                             │
│    ra_ap_cfg, ra_ap_paths, ra_ap_proc_macro_api,        │
│    ra_ap_ide_completion, ra_ap_ide_ssr                   │
└─────────────────────────────────────────────────────────┘
```

## Bootstrapping: loading a Cargo project

The canonical way to get a working `Analysis` from a Cargo project on disk:

```rust
use load_cargo::{LoadCargoConfig, ProcMacroServerChoice, load_workspace_at};
use project_model::CargoConfig;
use ide::AnalysisHost;

let cargo_config = CargoConfig::default();
let load_config = LoadCargoConfig {
    load_out_dirs_from_check: true,   // run `cargo check` for OUT_DIR
    with_proc_macro_server: ProcMacroServerChoice::Sysroot,
    prefill_caches: true,
    num_worker_threads: 4,
    proc_macro_processes: 1,
};

let (db, vfs, _proc_macro_client) = load_workspace_at(
    std::path::Path::new("/path/to/project"),
    &cargo_config,
    &load_config,
    &|msg| eprintln!("{msg}"),  // progress callback
)?;

// `db` is a RootDatabase — use it directly or wrap in AnalysisHost
let analysis = db.analysis();  // not quite — see below
```

For more control, load in stages:

```rust
use project_model::{ProjectManifest, ProjectWorkspace};
use load_cargo::{load_workspace, LoadCargoConfig, ProcMacroServerChoice};

// 1. Discover the project
let manifest = ProjectManifest::discover_single(&abs_path)?;

// 2. Load workspace metadata (runs cargo metadata)
let mut workspace = ProjectWorkspace::load(manifest, &cargo_config, &|_| {})?;

// 3. Optionally run build scripts
if want_build_scripts {
    let build_scripts = workspace.run_build_scripts(&cargo_config, &|_| {})?;
    workspace.set_build_scripts(build_scripts);
}

// 4. Load into database
let (db, vfs, proc_macro_client) = load_workspace(
    workspace,
    &cargo_config.extra_env,
    &load_config,
)?;
```

**Key types in the bootstrap chain:**
- `ProjectManifest` — discovered `Cargo.toml` or `rust-project.json`
- `ProjectWorkspace` — loaded project with three variants: `Cargo`, `Json`, `DetachedFile`
- `CargoConfig` — controls cargo metadata invocation, target, features, env
- `LoadCargoConfig` — controls proc-macro loading, cache priming, worker threads
- `RootDatabase` — the Salsa incremental database with all analysis data
- `Vfs` — virtual filesystem mapping `FileId` ↔ `VfsPath`

## `ra_ap_ide` — Top-level IDE API

The main entry point for consumers. Provides `AnalysisHost` (mutable) and `Analysis` (immutable snapshot) with all IDE features.

### AnalysisHost / Analysis

```rust
use ide::{AnalysisHost, Analysis, FilePosition, FileRange, FileId};
use hir::ChangeWithProcMacros;

// AnalysisHost owns the database, applies changes
let mut host = AnalysisHost::new(/* lru_cap */ None);
let mut change = ChangeWithProcMacros::default();
// ... populate change with files, crate graph ...
host.apply_change(change);

// Analysis is an immutable snapshot for querying
let analysis: Analysis = host.analysis();
```

### Navigation

```rust
// Go to definition — returns Vec<NavigationTarget>
analysis.goto_definition(FilePosition { file_id, offset })?;

// Go to implementation (find impl blocks for a trait/type)
analysis.goto_implementation(FilePosition { file_id, offset })?;

// Go to type definition
analysis.goto_type_definition(FilePosition { file_id, offset })?;

// Find all references
analysis.find_all_refs(FilePosition { file_id, offset }, search_scope)?;
```

### Hover & signature help

```rust
let hover = analysis.hover(&hover_config, FileRange { file_id, range })?;
// hover.info.markup — rendered documentation
// hover.info.actions — go-to-definition links, etc.

let sig = analysis.signature_help(FilePosition { file_id, offset })?;
// sig.signatures, sig.active_parameter
```

### Completions

```rust
let items: Vec<CompletionItem> = analysis.completions(
    &completion_config,
    FilePosition { file_id, offset },
    trigger_char,
)?;
// Each item: label, kind, text_edit, detail, relevance, docs
```

### Diagnostics

```rust
// Syntax-level diagnostics (parse errors)
analysis.syntax_diagnostics(&diagnostics_config, file_id)?;

// Semantic diagnostics (type errors, unresolved names, etc.)
analysis.semantic_diagnostics(&diagnostics_config, file_id)?;

// Full diagnostics (both + assists-as-diagnostics)
analysis.full_diagnostics(&diagnostics_config, resolve, file_id)?;
```

### Code actions / assists

```rust
let assists: Vec<Assist> = analysis.assists_with_fixes(
    &assist_config,
    &diagnostics_config,
    resolve,
    FileRange { file_id, range },
)?;
// Each Assist has: id, label, group, source_change (edits to apply)
```

### Other features

```rust
// Syntax highlighting (semantic tokens)
analysis.highlight(highlight_config, file_id)?;  // Vec<HlRange>

// Inlay hints (type annotations, parameter names)
analysis.inlay_hints(&inlay_hints_config, file_id, range)?;

// File structure (outline)
analysis.file_structure(file_id)?;  // Vec<StructureNode>

// Symbol search across workspace
analysis.symbol_search(query)?;  // Vec<NavigationTarget>

// Runnables (test/bench/bin targets)
analysis.runnables(file_id)?;  // Vec<Runnable>

// Rename
analysis.rename(FilePosition { file_id, offset }, "new_name")?;

// Structural search-replace
analysis.structural_search_replace(query, parse_only, resolve_context, selections)?;

// Call hierarchy
analysis.incoming_calls(FilePosition { file_id, offset })?;
analysis.outgoing_calls(FilePosition { file_id, offset })?;

// Folding ranges
analysis.folding_ranges(file_id)?;
```

### Key types

| Type | Purpose |
|------|---------|
| `FileId` | Opaque file identifier (u32 newtype) |
| `FilePosition` | `{ file_id: FileId, offset: TextSize }` |
| `FileRange` | `{ file_id: FileId, range: TextRange }` |
| `NavigationTarget` | Location + metadata for go-to results |
| `SymbolKind` | Function, Struct, Trait, Enum, Module, etc. |
| `CompletionItem` | Completion with label, edit, kind, relevance |
| `Diagnostic` | Error/warning with range, message, fixes |
| `Assist` | Code action with label and `SourceChange` |
| `SourceChange` | Set of file edits to apply atomically |
| `HlRange` | Highlighted range with semantic tag |
| `Runnable` | Test/bench/binary with cargo invocation info |
| `RangeInfo<T>` | Result `T` with the source range it applies to |

## `ra_ap_hir` — High-level semantic model

Object-oriented API over Rust's semantic structure. This is the crate you use when you need to navigate the module tree, inspect types, resolve traits, or map between syntax and semantics.

### Semantics — the bridge between syntax and semantics

```rust
use hir::Semantics;
use ide_db::RootDatabase;

let sema = Semantics::new(&db);

// Map syntax → semantic elements
let func: hir::Function = sema.to_def(&fn_syntax_node)?;
let module: hir::Module = sema.to_def(&module_syntax_node)?;

// Type of an expression
let ty: hir::Type = sema.type_of_expr(&expr)?.original;

// Resolve a path to its definition
let resolution = sema.resolve_path(&path_expr)?;

// Descend into macro expansions
let expanded_tokens = sema.descend_into_macros(token);

// Find node at offset with type
let node: ast::Expr = sema.find_node_at_offset_with_descend(syntax, offset)?;
```

### Module tree traversal

```rust
let krate: hir::Crate = /* from db */;
let root: hir::Module = krate.root_module(&db);

// Iterate children
for child in root.children(&db) {
    let name = child.name(&db);
    // ...
}

// Path to root (for fully-qualified names)
let path: Vec<hir::Module> = module.path_to_root(&db);

// All declarations in a module
let decls: Vec<hir::ModuleDef> = module.declarations(&db);

// Module scope (names visible in the module)
let scope = module.scope(&db, /* visible_from */ None);
```

### Type inspection

```rust
let ty: hir::Type = /* from sema.type_of_expr, field.ty, etc. */;

// Check what the type is
ty.as_adt()          // → Option<Adt> (struct/enum/union)
ty.as_callable(&db)  // → Option<Callable> (function/closure)
ty.as_reference()    // → Option<(Type, Mutability)>

// Fields (for structs/enums)
ty.fields(&db)       // → Vec<(Field, Type)>

// Autoderef chain
ty.autoderef(&db)    // → impl Iterator<Item = Type>

// Trait checking
ty.impls_trait(&db, trait_, &[])  // → bool

// Method resolution
ty.iterate_method_candidates(&db, scope, &visible_traits, None, |method| {
    // method: Function
    Some(method)
});
```

### Key semantic types

| Type | Represents |
|------|-----------|
| `Crate` | A crate in the dependency graph |
| `Module` | A module (file or inline `mod {}`) |
| `Function` | `fn` item |
| `Struct`, `Enum`, `Union` | ADT definitions |
| `Trait` | Trait definition |
| `Impl` | `impl` block |
| `TypeAlias` | `type Foo = ...` |
| `Const`, `Static` | `const`/`static` items |
| `Field` | Struct/enum variant field |
| `EnumVariant` | Enum variant |
| `Local` | Local variable binding |
| `Type<'db>` | A resolved type with environment context |
| `GenericParam` | Type/const/lifetime parameter |
| `AssocItem` | Associated function/type/const in a trait/impl |

### Finding implementations

```rust
// All impls for a type
let impls: Vec<hir::Impl> = hir::Impl::all_for_type(&db, ty);

// All impls of a trait
let impls: Vec<hir::Impl> = hir::Impl::all_for_trait(&db, trait_);

// Items in an impl block
for item in impl_.items(&db) {
    match item {
        AssocItem::Function(f) => { /* ... */ }
        AssocItem::TypeAlias(t) => { /* ... */ }
        AssocItem::Const(c) => { /* ... */ }
    }
}
```

### Source mapping (semantic → syntax)

```rust
use hir::HasSource;

// Get the syntax node for a semantic element
let source = func.source(&db)?;
// source.file_id — which file
// source.value   — the ast::Fn node
```

## `ra_ap_syntax` — Concrete syntax tree and parsing

Full-fidelity, error-resilient Rust parser. Built on the `rowan` library. Preserves all source text including whitespace and comments.

### Parsing

```rust
use syntax::{SourceFile, Edition, AstNode};

// Parse a complete file
let parse = SourceFile::parse(source_text, Edition::Edition2024);
let tree: SourceFile = parse.tree();
let errors: Vec<SyntaxError> = parse.errors();

// Parse a fragment (expression, type, pattern, etc.)
let expr_parse = ast::Expr::parse(expr_text, Edition::Edition2024);
```

### Untyped tree (SyntaxNode / SyntaxToken)

```rust
use syntax::{SyntaxNode, SyntaxToken, SyntaxKind, SyntaxElement};

let root: SyntaxNode = parse.syntax_node();

// Every node has a kind
root.kind()  // → SyntaxKind::SOURCE_FILE

// Traversal
root.children()                  // child nodes
root.children_with_tokens()      // child nodes + tokens
root.descendants()               // recursive DFS
root.parent()                    // parent node
root.ancestors()                 // walk up to root

// Preorder traversal with enter/leave events
for event in root.preorder_with_tokens() {
    match event {
        WalkEvent::Enter(element) => { /* ... */ }
        WalkEvent::Leave(element) => { /* ... */ }
    }
}
```

### Typed AST wrappers

Zero-cost wrappers over `SyntaxNode`. Cast with `AstNode::cast()`:

```rust
use syntax::ast::{self, AstNode, HasName, HasAttrs, HasVisibility};

// Cast untyped → typed
let fn_node: ast::Fn = ast::Fn::cast(syntax_node)?;

// Access typed children
fn_node.name()           // → Option<ast::Name>
fn_node.param_list()     // → Option<ast::ParamList>
fn_node.ret_type()       // → Option<ast::RetType>
fn_node.body()           // → Option<ast::BlockExpr>
fn_node.visibility()     // → Option<ast::Visibility>
fn_node.attrs()          // → impl Iterator<Item = ast::Attr>

// Back to untyped
fn_node.syntax()         // → &SyntaxNode
```

**Major AST enums:**
- `ast::Item` — top-level items (`Fn`, `Struct`, `Impl`, `Use`, `Mod`, ...)
- `ast::Expr` — expressions (`BinExpr`, `CallExpr`, `IfExpr`, `MatchExpr`, ...)
- `ast::Type` — type expressions (`PathType`, `RefType`, `TupleType`, ...)
- `ast::Pat` — patterns (`IdentPat`, `TuplePat`, `WildcardPat`, ...)
- `ast::Stmt` — statements (`LetStmt`, `ExprStmt`, `Item`)

**Common traits on AST nodes:**
- `HasName` — `.name()` → `Option<ast::Name>`
- `HasAttrs` — `.attrs()` → iterator of `ast::Attr`
- `HasVisibility` — `.visibility()` → `Option<ast::Visibility>`
- `HasGenericParams` — `.generic_param_list()`, `.where_clause()`
- `HasDocComments` — `.doc_comments()`

### Pattern matching with `match_ast!`

```rust
use syntax::match_ast;

match_ast! {
    match node {
        ast::Fn(it) => { /* handle function */ }
        ast::Struct(it) => { /* handle struct */ }
        ast::Impl(it) => { /* handle impl */ }
        _ => { /* other */ }
    }
}
```

### Finding nodes at positions

```rust
use syntax::algo;

// Find typed node at byte offset
let expr: ast::Expr = algo::find_node_at_offset(root.syntax(), offset)?;

// All ancestors at offset (for nested nodes)
let ancestors = algo::ancestors_at_offset(root.syntax(), offset);
```

### Pointers (stable references across reparses)

```rust
use syntax::{SyntaxNodePtr, AstPtr};

// Untyped pointer — survives tree rebuilds if structure unchanged
let ptr = SyntaxNodePtr::new(&node);
let recovered: SyntaxNode = ptr.to_node(&new_root);

// Typed pointer
let fn_ptr = AstPtr::new(&fn_node);
let recovered: ast::Fn = fn_ptr.to_node(&new_root);
```

### Text types

- `TextSize` — byte offset (`u32` newtype)
- `TextRange` — `start..end` range of `TextSize`
- `node.text_range()` — range in source
- `token.text()` — the actual source text of a token

## `ra_ap_ide_db` — Database and symbol infrastructure

The Salsa incremental database that underpins everything. You rarely construct this directly (use `load_cargo` instead), but you interact with its types constantly.

### RootDatabase

```rust
use ide_db::RootDatabase;

// Created by load_cargo, or manually:
let mut db = RootDatabase::new(/* lru_cap */ None);
db.enable_proc_attr_macros();

// Apply changes (files, crate graph)
db.apply_change(change);
```

Implements these Salsa database traits:
- `SourceDatabase` — file text, crate graph
- `DefDatabase` — definitions, name resolution
- `ExpandDatabase` — macro expansion
- `HirDatabase` — type inference, trait solving

### Definition — unified representation of any Rust definition

```rust
use ide_db::defs::Definition;

match definition {
    Definition::Function(f) => { /* ... */ }
    Definition::Adt(adt) => { /* struct/enum/union */ }
    Definition::Trait(t) => { /* ... */ }
    Definition::Module(m) => { /* ... */ }
    Definition::Const(c) => { /* ... */ }
    // ... many more variants
}
```

### Symbol search

```rust
use ide_db::symbol_index::Query;

let mut query = Query::new("MyStruct".to_owned());
query.limit(10);
query.exact();  // or query.fuzzy() for fuzzy matching
// Execute against the database
```

### Find references / usage search

```rust
use ide_db::search::{UsageSearchResult, FileReference, SearchScope};

// UsageSearchResult maps FileId → Vec<FileReference>
// Each FileReference has: range, name (for rename), category (read/write/import)
```

### LineIndex — efficient line/column mapping

```rust
use ide_db::line_index::LineIndex;

let line_index = LineIndex::new(source_text);
let line_col = line_index.line_col(offset);  // TextSize → LineCol
let offset = line_index.offset(line_col);     // LineCol → TextSize
```

## `ra_ap_vfs` — Virtual file system

Manages the mapping between file paths and `FileId`s. Tracks file changes for incremental updates.

```rust
use vfs::{Vfs, VfsPath, FileId};

let mut vfs = Vfs::default();

// Add/update a file
vfs.set_file_contents(VfsPath::from(abs_path), Some(contents));

// Look up FileId for a path
let (file_id, excluded) = vfs.file_id(&path)?;

// Look up path for a FileId
let path: &VfsPath = vfs.file_path(file_id);

// Drain pending changes (for incremental updates)
let changes: IndexMap<FileId, ChangedFile> = vfs.take_changes();
for (_, changed) in changes {
    match changed.change {
        Change::Create(bytes, _) => { /* new file */ }
        Change::Modify(bytes, _) => { /* modified */ }
        Change::Delete => { /* removed */ }
    }
}
```

**VfsPath** supports both real filesystem paths and virtual paths (for in-memory files):
```rust
let real = VfsPath::from(AbsPathBuf::assert_utf8(path));
let virtual_ = VfsPath::new_virtual_path("/virtual/file.rs".into());
```

### File loader interface

The `vfs::loader` module defines the `Handle` trait for file loading/watching:
- `vfs_notify::NotifyHandle` — real filesystem loader using `notify` crate
- `Entry::Directories { include, exclude, extensions }` — what to load
- `Message::Loaded { files }` / `Message::Changed { files }` — change notifications

## `ra_ap_project_model` — Project discovery and modeling

Discovers and models Cargo workspaces, `rust-project.json` files, and sysroots.

### Project discovery

```rust
use project_model::{ProjectManifest, ProjectWorkspace, CargoConfig};

// Find a single project from a path
let manifest = ProjectManifest::discover_single(&abs_path)?;

// Find all projects in a directory tree
let manifests: Vec<ProjectManifest> = ProjectManifest::discover(&abs_path)?;

// Supported manifest types: Cargo.toml, rust-project.json, .rust-project.json, .rs scripts
```

### Loading a workspace

```rust
let cargo_config = CargoConfig {
    set_test: true,           // set #[cfg(test)]
    // target, features, extra_env, etc.
    ..CargoConfig::default()
};

let mut workspace = ProjectWorkspace::load(manifest, &cargo_config, &|msg| {})?;

// ProjectWorkspace variants:
// - Cargo { cargo, sysroot, rustc_cfg, ... }
// - Json { project, sysroot, ... }
// - DetachedFile { file, cargo_script, sysroot, ... }
```

### Build scripts and proc macros

```rust
// Run build scripts to get OUT_DIR, cfg flags, env vars
let build_scripts = workspace.run_build_scripts(&cargo_config, &|_| {})?;
workspace.set_build_scripts(build_scripts);

// Convert to crate graph (the core operation)
let (crate_graph, proc_macros) = workspace.to_crate_graph(&mut load_file, &extra_env);
```

### CargoWorkspace — Cargo-specific model

```rust
use project_model::CargoWorkspace;

// Arena-indexed access to packages and targets
for pkg in cargo_workspace.packages() {
    let name = cargo_workspace[pkg].name.as_str();
    for target in cargo_workspace[pkg].targets.iter() {
        let kind = &cargo_workspace[*target].kind;  // Lib, Bin, Test, etc.
    }
}
```

### Sysroot

```rust
use project_model::Sysroot;

// Auto-discovered via `rustc --print sysroot`
// Three loading modes:
// 1. Cargo workspace (builds stdlib from source)
// 2. JSON project (manual specification)
// 3. Stitched (fallback — reads .rlib files directly)
```

## Secondary crates

### `ra_ap_ide_completion` — Completion engine

Used internally by `ide::Analysis::completions()`. Direct use gives more control:

```rust
use ide_completion::{completions, CompletionConfig, CompletionItem};

// CompletionConfig controls:
// - enable_postfix_completions (`.if`, `.match`, `.dbg`)
// - enable_imports_on_the_fly (auto-import)
// - snippet_cap (whether snippets are supported)
// - callable — how to render function signatures (FillArguments, AddParentheses, None)
// - limit — max number of completions

// resolve_completion_edits() — lazily resolves import edits for auto-import items
```

### `ra_ap_ide_ssr` — Structural search and replace

Pattern-based code transformation with semantic awareness:

```rust
use ide_ssr::{MatchFinder, SsrRule};

// Parse a rule: pattern ==>> replacement
let rule: SsrRule = "$a.foo($b) ==>> bar($a, $b)".parse()?;

// Create finder with database context
let mut finder = MatchFinder::in_context(&db, &sema, resolve_context, selections);
finder.add_rule(rule)?;

// Find matches
let matches = finder.matches();

// Get edits to apply
let edits = finder.edits();
```

Wildcards: `$name` matches any expression/pattern/type and binds it for the replacement template.

### `ra_ap_hir_def` — Definition-level IR

Lower-level than `hir`. You rarely use this directly, but it's useful to understand:

- **ID types**: `FunctionId`, `StructId`, `EnumId`, `TraitId`, `ModuleDefId`, `AdtId`, `MacroId`
- **DefMap**: Module-level name resolution results
- **ItemTree**: Syntax-independent representation of items in a file (survives reparsing if structure unchanged)
- **GenericParams**: Generic parameter lists with bounds
- **Visibility**: Pub/restricted visibility

### `ra_ap_hir_ty` — Type inference and trait solving

The type system engine. Again, rarely used directly (go through `hir::Type`):

- **`Ty`** / **`TyKind`**: Core type representation (uses `rustc_type_ir` types)
- **`InferenceResult`**: Per-function type inference results (type of every expr/pat)
- **`TraitRef`**: A trait reference with substitutions
- **Method resolution**: `iterate_method_candidates()` with autoderef
- **Coercion/unification**: `could_coerce()`, `could_unify()`

### `ra_ap_cfg` — `#[cfg(...)]` evaluation

```rust
use cfg::{CfgExpr, CfgAtom, CfgOptions};

// Parse cfg expressions
let expr = CfgExpr::parse(&attr);  // e.g., cfg(all(unix, feature = "foo"))

// Evaluate against a set of enabled cfgs
let mut opts = CfgOptions::default();
opts.insert_atom("unix".into());
opts.insert_key_value("feature".into(), "foo".into());
let enabled: bool = opts.check(&expr) != Some(false);
```

### `ra_ap_parser` — Token-to-tree parser

The parser itself, separate from the syntax tree. Used by `ra_ap_syntax` internally:

```rust
use parser::{TopEntryPoint, PrefixEntryPoint, Edition};

// Parse a complete construct
let output = TopEntryPoint::SourceFile.parse(&input, Edition::Edition2024);

// Parse entry points: SourceFile, MacroStmts, MacroItems, Pattern, Type, Expr, MetaItem

// Prefix entry points (for macro-by-example): Item, Stmt, Expr, Pat, Ty, Block, Meta, Vis

// Incremental reparsing
let reparser = Reparser::for_node(syntax_kind, first_child, parent);
```

### `ra_ap_paths` — Type-safe path handling

```rust
use paths::{AbsPathBuf, AbsPath, RelPathBuf, RelPath};

// Absolute paths (guaranteed at construction time)
let abs = AbsPathBuf::assert_utf8(std::env::current_dir()?);
let joined: AbsPathBuf = abs.join("src/lib.rs");
let normalized = abs.normalize();

// Relative paths
let rel = RelPathBuf::try_from("src/lib.rs")?;

// All paths are UTF-8 (via camino crate)
// No IO methods — pure data structures
```

### `ra_ap_proc_macro_api` — Proc-macro client

Communicates with an external `proc-macro-srv` process to expand procedural macros:

```rust
use proc_macro_api::{ProcMacroClient, ProcMacroKind, MacroDylib};

// Spawn the proc-macro server
let client = ProcMacroClient::spawn(&server_path, &env, toolchain.as_ref(), num_processes)?;

// Load a dylib
let dylib = MacroDylib::new(path);
let macros = client.load_dylib(dylib)?;

// Each macro has: name, kind (CustomDerive/Attr/Bang), expand()
for mac in &macros {
    let name = mac.name();
    let kind = mac.kind();  // ProcMacroKind::{CustomDerive, Attr, Bang}
}
```

## Cargo dependency setup

All crates are published at the same version. Pin to an exact version — these are unstable internal APIs that break between releases:

```toml
[dependencies]
ra_ap_ide = "=0.0.330"
ra_ap_hir = "=0.0.330"
ra_ap_syntax = "=0.0.330"
ra_ap_ide_db = "=0.0.330"
ra_ap_vfs = "=0.0.330"
ra_ap_load-cargo = "=0.0.330"
ra_ap_project-model = "=0.0.330"
# Add others as needed:
# ra_ap_hir_def, ra_ap_hir_ty, ra_ap_cfg, ra_ap_parser,
# ra_ap_paths, ra_ap_proc_macro_api, ra_ap_ide_completion,
# ra_ap_ide_ssr, ra_ap_vfs-notify, ra_ap_stdx, ra_ap_profile
```

**Important**: All `ra_ap_*` crates in your dependency tree must be the same version. Mixing versions will cause trait mismatch errors at compile time.

## Practical notes

### Which crate to start with

- **Just parsing?** `ra_ap_syntax` alone. No database, no Cargo loading.
- **Semantic analysis of a Cargo project?** `ra_ap_load_cargo` + `ra_ap_ide` (or `ra_ap_hir` for lower-level access).
- **Custom IDE features?** `ra_ap_ide` for the full feature set.
- **Batch diagnostics/stats?** Look at `ra_ap_rust-analyzer`'s `cli` module for examples.

### The Salsa incremental computation model

All semantic queries go through Salsa. This means:
- Queries are memoized and invalidated incrementally when inputs change
- `Analysis` snapshots are cheap (just an `Arc` bump)
- Long-running queries can be cancelled via `Cancellable<T>` return types
- LRU caches control memory usage (`RootDatabase::new(lru_cap)`)

### Error resilience

- The parser always produces a tree, even for invalid code
- Semantic analysis gracefully handles missing/broken definitions
- Most API methods return `Option` or `Cancellable<Option<T>>`

### The `ra_ap_rust-analyzer` crate itself

This is the LSP server binary, **not** the programmatic API. Its public surface is minimal:
- `main_loop()` — runs the LSP server
- `server_capabilities()` — LSP capability advertisement
- `cli` module — batch-processing subcommands (analysis-stats, diagnostics, SCIP/LSIF generation, SSR)
- `config` module — server configuration
- `tracing` module — logging setup

The `cli` module is useful as reference code for how to use the API crates in batch mode. Key examples:
- `cli::analysis_stats` — loads a project and runs type inference on every function
- `cli::diagnostics` — runs all diagnostics on a project
- `cli::scip` / `cli::lsif` — generates code intelligence indexes
- `cli::ssr` — runs structural search-replace from the command line
