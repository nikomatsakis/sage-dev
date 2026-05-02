# WIP: Proc-macro derive expansion

## Goal

Actually invoke proc-macro derive dylibs (e.g. clap's `Parser`,
`Subcommand`) and feed the expanded source back into sage's IR.
Currently, proc-macro derives are resolved to their `Symbol` but
never called — `DeriveResult::ProcMacro { symbol }` is a stub.

Target: expand `#[derive(Parser)]` and `#[derive(Subcommand)]` on
mini-redis's CLI structs, lower the output into `Vec<Item>`, and
snapshot the result.

## Background: how rustc invokes proc macros

Proc-macro crates are compiled to dylibs. At load time, rustc reads
a static array of `proc_macro::bridge::client::ProcMacro` entries
from the dylib. Each entry contains a `Client` — essentially a
function pointer into the dylib:

```rust
// proc_macro::bridge::client (in the standard library)
#[repr(C)]
pub struct Client<I, O> {
    handle_counters: &'static HandleCounters,
    run: extern "C" fn(BridgeConfig<'_>) -> Buffer,
    _marker: PhantomData<fn(I) -> O>,
}
// Client is Copy, Send, Sync, 'static.
```

To invoke a derive, you call `client.run(strategy, server, input, false)`
where:
- `strategy` controls threading (`SAME_THREAD` or `CROSS_THREAD`)
- `server` implements `proc_macro::bridge::server::Server` — the
  bridge callbacks for token/span/symbol operations
- `input` is the item's token stream (the struct/enum source)

The `Server` trait has ~30 methods but most are trivial for our use
case. The proc-macro dylib calls back through the bridge to create
tokens, look up spans, and emit diagnostics. It never touches the
resolver or type system.

## Architecture

```
Salsa thread                          Rustc thread
───────────                           ───────────
expand_derives()
  → tcx.expand_proc_macro_derive(     TcxRequest::ExpandDerive
       crate_num, def_index,            → CStore::load_macro_untracked(def_id)
       item_source_text)                → unsafe: extract Client from DeriveProcMacro
                                        → client.run(&SAME_THREAD, SageServer, input)
                                        → output.to_string()
  ← expanded source text              ← reply.send(expanded_text)
  → tree-sitter lower → Vec<Item>
```

Expansion happens on the rustc thread because the `Client` is buried
inside `Arc<dyn MultiItemModifier>` with no `Any` downcast support.
We use one small `unsafe` pointer cast to extract it.

Future optimization: if we add a public accessor to `CStore` for the
raw `Client`, expansion can move to the salsa side (or a thread pool)
since `Client` is `Copy + Send + 'static`.

## The unsafe

`CStore::load_macro_untracked` returns:
```
LoadedMacro::ProcMacro(SyntaxExtension {
    kind: SyntaxExtensionKind::Derive(Arc<dyn MultiItemModifier>),
    ...
})
```

The concrete type behind the `Arc` is `DeriveProcMacro`:
```rust
// rustc_expand::proc_macro (public struct, public field)
pub struct DeriveProcMacro {
    pub client: Client<TokenStream, TokenStream>,
}
```

`MultiItemModifier` doesn't extend `Any`, so we can't downcast
through the trait. Instead:

```rust
let ptr = Arc::as_ref(&arc) as *const dyn MultiItemModifier
                              as *const DeriveProcMacro;
let client = unsafe { (*ptr).client }; // Client is Copy
```

This is sound because:
1. We only do this when `SyntaxExtensionKind::Derive` came from
   `load_macro_untracked` on a proc-macro crate
2. `load_proc_macro` in `rustc_metadata` always wraps
   `ProcMacro::CustomDerive { client }` in
   `Arc::new(DeriveProcMacro { client })`
3. `DeriveProcMacro` is a single-field struct, `Client` is `Copy`

## SageServer — our `Server` implementation

Lives in `src/proc_macro_srv.rs`. Uses `proc_macro2` for token stream
manipulation but wraps `Span` because `proc_macro2::Span` doesn't
implement `Eq + Hash` (required by the `Server` trait).

```rust
struct SageServer;

/// Dummy span — we don't track span info through proc-macro expansion.
/// Must be Copy + Eq + Hash as required by Server trait.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct SageSpan;

impl Server for SageServer {
    type TokenStream = proc_macro2::TokenStream;
    type Span = SageSpan;
    type Symbol = String;
    // ...
}
```

We use a unit struct `SageSpan` instead of `proc_macro2::Span`
because the `Server` trait requires `Span: Copy + Eq + Hash` and
`proc_macro2::Span` only implements `Copy` (no `Eq` or `Hash`).
This is fine — we don't need real span tracking through proc-macro
expansion. All spans in the expanded output will be dummy values
anyway.

This means the `TokenTree` conversion between bridge types and
`proc_macro2` types must strip/inject spans at the boundary:
- Bridge → `proc_macro2`: ignore the `SageSpan`, use
  `proc_macro2::Span::call_site()` when constructing tokens
- `proc_macro2` → bridge: use `SageSpan` for all span fields

### Method implementation strategy

The `Server` trait has 3 inherent methods plus ~30 from `with_api!`:

| Category | Methods | Implementation |
|---|---|---|
| Globals | `globals` | Return `SageSpan` for def_site, call_site, mixed_site |
| Symbols (inherent) | `intern_symbol`, `with_symbol_string` | `String` — intern is `.to_owned()`, lookup is `f(s)` |
| Token stream ops | `ts_from_str`, `ts_to_string`, `ts_clone`, `ts_is_empty`, `ts_from_token_tree`, `ts_concat_trees`, `ts_concat_streams`, `ts_into_trees`, `ts_drop` | Delegate to `proc_macro2` with span conversion at boundary |
| Literals | `literal_from_str` | Parse with `proc_macro2`, convert to bridge `Literal` |
| Spans | `span_debug`, `span_parent`, `span_source`, `span_byte_range`, `span_start`, `span_end`, `span_line`, `span_column`, `span_file`, `span_local_file`, `span_join`, `span_subspan`, `span_resolved_at`, `span_source_text`, `span_save_span`, `span_recover_proc_macro_span` | Dummy values — `SageSpan`, `None`, `0`, `""` |
| Symbols (from api) | `symbol_normalize_and_validate_ident` | Check `syn::parse_str::<syn::Ident>(s)` or basic ident validation |
| Env/tracking | `injected_env_var`, `track_env_var`, `track_path` | No-op / `None` |
| Diagnostics | `emit_diagnostic` | Ignore (eprintln in debug builds) |
| Expansion | `ts_expand_expr` | `Err(())` — not supported |

The core work is the `TokenTree` conversion between bridge types and
`proc_macro2` types. Both have the same four variants (Group, Ident,
Punct, Literal), so the mapping is mechanical. The bridge `TokenTree`
is generic over `(TokenStream, Span, Symbol)`:

```rust
// proc_macro::bridge (generic over Server types)
enum TokenTree<TokenStream, Span, Symbol> {
    Group { delimiter: Delimiter, stream: Option<TokenStream>, span: DelimSpan<Span> },
    Ident { sym: Symbol, is_raw: bool, span: Span },
    Punct { ch: u8, joint: bool, span: Span },
    Literal { kind: LitKind, symbol: Symbol, suffix: Option<Symbol>, span: Span },
}
```

Note: the bridge `Literal` uses `LitKind` + `symbol` (the text) +
optional `suffix`, not a parsed value. This differs from
`proc_macro2::Literal` which is opaque. For bridge→proc_macro2
conversion, we reconstruct the literal text from kind+symbol+suffix
and parse it. For proc_macro2→bridge, we format the literal to string
and decompose it.

## TcxDb changes

### New trait method

```rust
// crates/sage-ir/src/tcx/mod.rs
pub trait TcxDb: Send + Sync {
    // ... existing methods ...

    /// Expand a proc-macro derive. Returns the expanded source text.
    fn expand_proc_macro_derive(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
        item_source: &str,
    ) -> Option<String>;
}
```

### New request variant

```rust
// crates/sage-ir/src/tcx/proxy.rs
enum TcxRequest {
    // ... existing variants ...
    ExpandDerive {
        crate_num: CrateNum,
        def_index: DefIndex,
        item_source: String,
        reply: mpsc::Sender<Option<String>>,
    },
}
```

### RustcTcxDb implementation

```rust
// src/tcx_impl.rs
fn expand_proc_macro_derive(&self, cn, di, item_source) -> Option<String> {
    let def_id = make_def_id(cn, di);
    let cstore = CStore::from_tcx(self.tcx);
    let loaded = cstore.load_macro_untracked(self.tcx, def_id);
    let LoadedMacro::ProcMacro(ext) = loaded else { return None };
    let SyntaxExtensionKind::Derive(ref arc) = ext.kind else { return None };

    // SAFETY: see "The unsafe" section above
    let client = unsafe {
        let ptr = Arc::as_ref(arc) as *const dyn MultiItemModifier
                                    as *const DeriveProcMacro;
        (*ptr).client
    };

    let input: proc_macro2::TokenStream = item_source.parse().ok()?;
    let server = SageServer::new();
    match client.run(&SAME_THREAD, server, input, false) {
        Ok(output) => Some(output.to_string()),
        Err(_panic) => None,
    }
}
```

## Integration with derive.rs

Currently `expand_derives` returns `DeriveResult::ProcMacro { symbol }`
for non-builtin derives. The change:

```rust
DeriveResult::ProcMacro { symbol } => {
    let (cn, di) = match symbol.source(db) {
        SymbolSource::External(cn, di) => (cn, di),
        _ => continue,
    };
    // Get the item's source text from the SourceFile
    let item_source = extract_item_source(db, item);
    if let Some(expanded) = db.tcx().expand_proc_macro_derive(cn, di, &item_source) {
        // Lower expanded text through tree-sitter
        let expanded_items = lower_expanded_source(db, &expanded);
        results.push(DeriveResult::Expanded { items: expanded_items });
    }
}
```

`extract_item_source` gets the source text for the struct/enum from
its `SpanTable` and `SpanIndices`:

```rust
fn extract_item_source(db: &dyn Db, item: Item<'_>) -> Option<String> {
    let (span_table, span) = match item {
        Item::Struct(s) => (s.span_table(db), s.span(db)),
        Item::Enum(e) => (e.span_table(db), e.span(db)),
        _ => return None,
    };
    let file = span_table.file(db);
    let text = file.text(db);
    Some(text[span.start as usize..span.end as usize].to_owned())
}
```

Note: rustc strips the `#[derive(...)]` attribute before passing the
item to the proc-macro. Our `span` covers the full item including
attributes. For v1, we pass the full text — most proc-macros ignore
the derive attribute. If a proc-macro misbehaves, we can strip
`#[derive(...)]` from the text before passing it.

`lower_expanded_source` takes the expanded text (which is valid Rust
source — impl blocks, function definitions, etc.), creates a
temporary `SourceFile`, and runs it through `file_item_tree` to get
`Vec<Item>`. The expanded items are synthetic (generated spans, no
real file), similar to how builtin derive expansion already works in
`derive/builtins.rs`.

## mini-redis proc-macro derives

| Derive | Crate | File | Struct |
|---|---|---|---|
| `Parser` | clap | `src/bin/server.rs` | `Cli` |
| `Parser` | clap | `src/bin/cli.rs` | `Cli` |
| `Subcommand` | clap | `src/bin/cli.rs` | `Command` |

All other derives in mini-redis are builtins (`Debug`, `Clone`,
`Default`) — already handled.

## Implementation plan

Each phase follows TDD: write failing tests first, then implement
until they pass. Tests are listed with their exact assertions.

### Process

1. **Write tests first.** Copy the test code from the phase into the
   appropriate test file. Run them — they must fail (compile errors
   count as failure).
2. **Implement.** Write the minimum code to make the tests pass.
3. **Verify.** All tests pass (new and existing).
4. **Update WIP.md.** After each phase:
   - Mark the phase as complete in the "Implementation status" section
     at the bottom.
   - If the implementation deviated from the plan (different function
     signatures, extra types needed, tests changed, etc.), document
     the deviation under "Deviations from plan" with a brief
     explanation of what changed and why.
   - If new questions or issues surfaced, add them to the FAQ or
     note them under "Open issues".
5. **Commit.** One commit per phase with the message shown.

### Phase 1: SageServer — bridge TokenTree conversion

**Goal:** Implement the `Server` trait with `proc_macro2` types.
The hardest part is converting between bridge `TokenTree` and
`proc_macro2::TokenTree`. Get this right first in isolation.

**Files:** new `src/proc_macro_srv.rs`, `src/lib.rs` (add module)

**Test 1.1 — token stream round-trip** (unit test in `proc_macro_srv.rs`):
```rust
#[test]
fn ts_round_trip() {
    let mut srv = SageServer::new();
    let ts = srv.ts_from_str("struct Foo { x: i32 }").unwrap();
    let s = srv.ts_to_string(&ts);
    assert!(!s.is_empty());
    // Parse back — should not error
    let ts2 = srv.ts_from_str(&s).unwrap();
    assert_eq!(srv.ts_to_string(&ts), srv.ts_to_string(&ts2));
}
```

**Test 1.2 — tree conversion round-trip** (unit test):
```rust
#[test]
fn tree_round_trip() {
    let mut srv = SageServer::new();
    let ts = srv.ts_from_str("fn foo(x: u32) -> bool { true }").unwrap();
    let trees = srv.ts_into_trees(ts.clone());
    assert!(!trees.is_empty());
    let ts2 = srv.ts_concat_trees(None, trees);
    assert_eq!(srv.ts_to_string(&ts), srv.ts_to_string(&ts2));
}
```

**Test 1.3 — empty stream** (unit test):
```rust
#[test]
fn empty_stream() {
    let mut srv = SageServer::new();
    let ts: proc_macro2::TokenStream = Default::default();
    assert!(srv.ts_is_empty(&ts));
    assert!(srv.ts_into_trees(ts).is_empty());
}
```

**Test 1.4 — symbol interning** (unit test):
```rust
#[test]
fn symbols() {
    let s = SageServer::intern_symbol("foo");
    assert_eq!(s, "foo");
    SageServer::with_symbol_string(&s, |text| assert_eq!(text, "foo"));
}
```

**Test 1.5 — concat streams** (unit test):
```rust
#[test]
fn concat_streams() {
    let mut srv = SageServer::new();
    let a = srv.ts_from_str("struct A;").unwrap();
    let b = srv.ts_from_str("struct B;").unwrap();
    let combined = srv.ts_concat_streams(Some(a), vec![b]);
    let text = srv.ts_to_string(&combined);
    assert!(text.contains("A"));
    assert!(text.contains("B"));
}
```

**Implement:**
1. Define `SageSpan` (unit struct, derive `Copy, Clone, PartialEq, Eq, Hash`)
2. Define `SageServer` struct (empty — no state needed)
3. Implement bridge `TokenTree<TS, SageSpan, String>` → `proc_macro2::TokenTree`
4. Implement `proc_macro2::TokenTree` → bridge `TokenTree<TS, SageSpan, String>`
5. Implement all `Server` trait methods (see method table above)

**Verify:** All 5 tests pass.

**Commit:** `phase 1: SageServer proc-macro bridge implementation`

---

### Phase 2: TcxDb wiring

**Goal:** Add `expand_proc_macro_derive` to the `TcxDb` trait and
wire it through proxy/noop/driver. No actual expansion yet — just
the plumbing. Everything compiles, existing tests pass.

**Files:** `crates/sage-ir/src/tcx/mod.rs`, `proxy.rs`, `noop.rs`,
`src/driver.rs`

**Test 2.1 — noop returns None** (unit test in sage-ir):
```rust
#[test]
fn noop_expand_returns_none() {
    let tcx = NoopTcxDb;
    let result = tcx.expand_proc_macro_derive(
        CrateNum(1), DefIndex(0), "struct Foo;",
    );
    assert!(result.is_none());
}
```

**Implement:**
1. Add `fn expand_proc_macro_derive(&self, crate_num: CrateNum, def_index: DefIndex, item_source: &str) -> Option<String>` to `TcxDb` trait
2. Add `ExpandDerive { crate_num, def_index, item_source: String, reply: Sender<Option<String>> }` to `TcxRequest`
3. Implement in `ProxyTcxDb` (send/recv pattern, matching existing methods)
4. Implement in `NoopTcxDb` (return `None`)
5. Handle `ExpandDerive` in `driver.rs` dispatch loop — call `tcx_db.expand_proc_macro_derive(...)` and send reply
6. Add stub `expand_proc_macro_derive` to `RustcTcxDb` that returns `None` (real impl in phase 3)

**Verify:** Test 2.1 passes. All existing tests pass. `cargo build` succeeds.

**Commit:** `phase 2: TcxDb expand_proc_macro_derive wiring`

---

### Phase 3: RustcTcxDb — actual proc-macro invocation

**Goal:** Implement the unsafe `Client` extraction and actual
proc-macro invocation on the rustc thread. After this phase, calling
`tcx.expand_proc_macro_derive(cn, di, source)` with a valid
proc-macro DefId actually runs the dylib and returns expanded code.

**Files:** `src/tcx_impl.rs`

**Test 3.1 — expand clap Parser derive** (integration test in `tests/expand_tests.rs`):
```rust
#[test]
fn expand_proc_macro_clap_parser() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        // Resolve clap crate
        let clap_cn = sage.db.tcx().extern_crate("clap").expect("clap not found");

        // Find the Parser derive's DefIndex by walking clap's children
        // looking for a Derive-namespace item named "Parser"
        let clap_root_children = sage.db.tcx().module_children(clap_cn, DefIndex(0));
        let parser_child = clap_root_children.iter()
            .find(|c| c.name == "Parser" && matches!(c.namespace, Namespace::Macro(MacroKind::Derive)))
            .expect("Parser derive not found in clap");

        let source = r#"#[command(name = "test")]
struct Cli {
    #[arg(long)]
    port: Option<u16>,
}"#;

        let expanded = sage.db.tcx().expand_proc_macro_derive(
            parser_child.crate_num, parser_child.def_index, source,
        );
        assert!(expanded.is_some(), "Parser expansion should succeed");
        let text = expanded.unwrap();
        assert!(text.contains("impl"), "expanded output should contain impl");
    });
}
```

**Test 3.2 — expansion returns valid Rust source** (integration test):
```rust
#[test]
fn expanded_output_is_parseable() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        // Same setup as 3.1 to get clap Parser
        let clap_cn = sage.db.tcx().extern_crate("clap").unwrap();
        let children = sage.db.tcx().module_children(clap_cn, DefIndex(0));
        let parser = children.iter()
            .find(|c| c.name == "Parser" && matches!(c.namespace, Namespace::Macro(MacroKind::Derive)))
            .unwrap();

        let source = "struct Simple { x: i32 }";
        let expanded = sage.db.tcx().expand_proc_macro_derive(
            parser.crate_num, parser.def_index, source,
        ).unwrap();

        // The expanded text should parse as valid Rust via tree-sitter
        let mut parser_ts = tree_sitter::Parser::new();
        parser_ts.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
        let tree = parser_ts.parse(&expanded, None).unwrap();
        assert!(!tree.root_node().has_error(), "expanded output has parse errors:\n{expanded}");
    });
}
```

**Implement:**
1. Replace the stub `expand_proc_macro_derive` in `RustcTcxDb` with real implementation
2. `CStore::from_tcx(self.tcx)` → `cstore.load_macro_untracked(self.tcx, def_id)`
3. Match `LoadedMacro::ProcMacro(ext)` → `SyntaxExtensionKind::Derive(ref arc)`
4. Unsafe: extract `Client` via pointer cast from `Arc<dyn MultiItemModifier>`
5. Parse `item_source` into `proc_macro2::TokenStream`
6. Call `client.run(&SAME_THREAD, SageServer::new(), input, false)`
7. Return `Ok(output) → Some(output.to_string())`, `Err(_) → None`

**Verify:** Tests 3.1 and 3.2 pass.

**Commit:** `phase 3: proc-macro derive invocation via Client bridge`

---

### Phase 4: Integration with derive.rs

**Goal:** Wire proc-macro expansion into the existing `expand_derives`
flow. Expanded text gets lowered through tree-sitter into `Vec<Item>`.
The `DeriveResult::ProcMacro` stub becomes `DeriveResult::Expanded`.

**Files:** `crates/sage-ir/src/derive.rs`, `tests/expand_tests.rs`

**Test 4.1 — expand_derives returns expanded items for Parser** (integration test):
```rust
#[test]
fn expand_derives_clap_parser() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let module = resolve_module_path(
            sage.db, sage.root, sage.source_root, &["bin", "server"],
        ).unwrap();
        let items = module_items(sage.db, module);
        let cli_struct = items.iter()
            .find(|i| matches!(i, Item::Struct(s) if s.name(sage.db).text(sage.db) == "Cli"))
            .expect("Cli struct not found");

        let results = expand_derives(
            sage.db, module, sage.source_root, sage.root, *cli_struct,
        );

        // Should have Debug (builtin) + Parser (now expanded, not stub)
        let has_builtin = results.iter().any(|r| matches!(r, DeriveResult::Builtin { .. }));
        let has_expanded = results.iter().any(|r| matches!(r, DeriveResult::Expanded { .. }));
        assert!(has_builtin, "should have builtin Debug derive");
        assert!(has_expanded, "should have expanded Parser derive");

        // No more ProcMacro stubs
        let has_stub = results.iter().any(|r| matches!(r, DeriveResult::ProcMacro { .. }));
        assert!(!has_stub, "should not have unexpanded ProcMacro stubs");
    });
}
```

**Test 4.2 — expanded items lower to valid IR** (integration test):
```rust
#[test]
fn expanded_items_are_valid_ir() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let module = resolve_module_path(
            sage.db, sage.root, sage.source_root, &["bin", "server"],
        ).unwrap();
        let items = module_items(sage.db, module);
        let cli_struct = items.iter()
            .find(|i| matches!(i, Item::Struct(s) if s.name(sage.db).text(sage.db) == "Cli"))
            .expect("Cli struct not found");

        let results = expand_derives(
            sage.db, module, sage.source_root, sage.root, *cli_struct,
        );

        for result in &results {
            if let DeriveResult::Expanded { items } = result {
                assert!(!items.is_empty(), "expanded items should not be empty");
                for item in items {
                    // Expanded derive output should be impl blocks
                    assert!(
                        matches!(item, Item::Impl(_) | Item::Function(_) | Item::Const(_)),
                        "unexpected expanded item kind: {:?}", item,
                    );
                }
            }
        }
    });
}
```

**Test 4.3 — snapshot expanded output** (integration test):
```rust
#[test]
fn snapshot_expanded_clap_parser() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let module = resolve_module_path(
            sage.db, sage.root, sage.source_root, &["bin", "server"],
        ).unwrap();
        let items = module_items(sage.db, module);
        let cli_struct = items.iter()
            .find(|i| matches!(i, Item::Struct(s) if s.name(sage.db).text(sage.db) == "Cli"))
            .expect("Cli struct not found");

        let results = expand_derives(
            sage.db, module, sage.source_root, sage.root, *cli_struct,
        );

        let mut out = String::new();
        for result in &results {
            match result {
                DeriveResult::Builtin { impls } => {
                    for impl_item in impls {
                        out.push_str(&format!("builtin: {impl_item}\n"));
                    }
                }
                DeriveResult::Expanded { items } => {
                    for item in items {
                        out.push_str(&format!("expanded: {item}\n"));
                    }
                }
                DeriveResult::ProcMacro { symbol } => {
                    out.push_str(&format!("unexpanded: {symbol}\n"));
                }
            }
        }
        expect![[r#"
            ...snapshot filled on first run...
        "#]].assert_eq(&out);
    });
}
```

**Implement:**
1. Add `DeriveResult::Expanded { items: Vec<Item<'db>> }` variant
2. `extract_item_source(db, item) -> Option<String>` — get source text
   from `SpanTable` → `SourceFile` → byte slice
3. `lower_expanded_source(db, text) -> Vec<Item>` — create temporary
   `SourceFile`, run tree-sitter parse + `file_item_tree`-style lowering
4. In `expand_derives`: when a non-builtin external derive is found,
   call `db.tcx().expand_proc_macro_derive(cn, di, &source)`, lower
   the result, produce `DeriveResult::Expanded`
5. Update existing `expand_derives_cmd_get_full` test (it currently
   expects `DeriveResult::ProcMacro` — cmd/get only has builtins so
   it should be unaffected, but verify)

**Verify:** Tests 4.1, 4.2, 4.3 pass. All existing tests pass.

**Commit:** `phase 4: end-to-end proc-macro derive expansion`

## Dependencies

Add to root `sage` `Cargo.toml`:
```toml
proc-macro2 = "1"
```

## FAQ

**Q: Is `CStore::from_tcx` accessible from outside `rustc_metadata`?**
Yes. It's `pub` on `CStore` in `rustc_metadata::creader`. It
downcasts `tcx.untracked().cstore` via `as_any()`. Sage can call it
from `tcx_impl.rs` since we link `rustc_metadata` through
`rustc_private`.

**Q: Is the concrete type behind `SyntaxExtensionKind::Derive(Arc<...>)`
always `DeriveProcMacro`?**
Yes, for proc-macro crates. `load_proc_macro` in
`rustc_metadata::rmeta::decoder` matches `ProcMacro::CustomDerive`
and always wraps it in `Arc::new(DeriveProcMacro { client })`. There
is no other code path that produces `SyntaxExtensionKind::Derive` for
external proc-macro crates. (Builtin derives go through a different
path and are already handled by sage.)

**Q: What does the proc-macro receive as input?**
The item source text (struct/enum definition) as a `TokenStream`.
Rustc strips the `#[derive(...)]` attribute before passing it. Other
attributes (e.g. `#[clap(...)]`, `#[serde(...)]`) are preserved.

**Q: Does sage handle binary targets (`src/bin/*.rs`)?**
Yes. `collect_source_files` in `driver.rs` recursively collects all
`.rs` files under the crate's `src/` directory, including `src/bin/`.
The mini-redis clap derives are in `src/bin/server.rs` and
`src/bin/cli.rs`.

**Q: Why not use `proc_macro2::Span` as the `Server::Span` type?**
`proc_macro2::Span` implements `Copy` but not `Eq` or `Hash`. The
`Server` trait requires `Span: 'static + Copy + Eq + Hash`. We use a
unit struct `SageSpan` instead — we don't need real span tracking
through proc-macro expansion.

**Q: Why not extract the `Client` and send it to the salsa thread?**
`Client` is `Copy + Send + 'static` and could theoretically be sent
across threads. But it's buried inside `Arc<dyn MultiItemModifier>`
with no `Any` downcast support. The only way to extract it is the
unsafe pointer cast, which we do on the rustc thread. Moving
expansion to the salsa side (or a thread pool) is a future
optimization that would require either the same unsafe or a small
rustc patch to expose the `Client` publicly.

**Q: Can a proc-macro panic? What happens?**
`client.run()` returns `Result<TokenStream, PanicMessage>`. If the
proc-macro panics, we get `Err` and return `None` from
`expand_proc_macro_derive`. The derive is silently skipped. Error
reporting can be added later.

## What's NOT in scope

- **Attribute macros** (`#[tokio::main]`, `#[tokio::test]`) — different
  `Client` signature, transforms the item rather than appending
- **Bang proc-macros** — different `Client` signature
- **Parallelism** — `SAME_THREAD` on the rustc thread for now
- **Error reporting** — `emit_diagnostic` is no-op; panicking
  proc-macros return `None`
- **Incremental** — no caching of expansion results yet (salsa
  tracked function can be added later)
- **Helper attributes** — `#[serde(...)]`, `#[clap(...)]` are not
  resolved; they pass through as regular attributes

## Implementation status

- [x] Phase 1: SageServer
- [x] Phase 2: TcxDb wiring
- [x] Phase 3: RustcTcxDb proc-macro invocation
- [x] Phase 4: Integration with derive.rs

### Deviations from plan

(Phase 1: Added `#![feature(proc_macro_internals)]` to `src/lib.rs` — required to access `proc_macro::bridge` types. No other deviations.)
(Phase 3: SageServer implements `rustc_proc_macro::bridge::server::Server` instead of `proc_macro::bridge::server::Server`. The `Client` in `DeriveProcMacro` uses `rustc_proc_macro` (the compiler's internal copy), not the standard library's `proc_macro`. Both traits have the same shape but are different crate instances. Added `extern crate rustc_proc_macro`, `rustc_metadata`, and `rustc_expand` to `lib.rs`.)
(Phase 4: Major deviation — `definition()` for external modules returns the first child matching a name, without namespace filtering. When clap re-exports `Parser` as both a trait (Type ns, from `clap_builder`) and a derive macro (Macro ns, from `clap_derive`), the first match is the trait. Added `definition_in_ns()` for namespace-aware lookup. Also added `catch_unwind` guard in `expand_proc_macro_derive` to handle ICEs from `load_macro_untracked` when called with non-proc-macro DefIds. The `try_expand_proc_macro` function falls back to searching known crate names when the direct DefId doesn't work. Tests use `SourceRoot.files()` to find `bin/server.rs` directly since binary targets aren't in the module tree.)

### Open issues

(None yet — document any issues that surface during implementation.)
