# WIP: MEM-map code reorganization

## Goal

Reorganize the `memmap.rs` implementation based on PR review feedback. Clean up the code structure, eliminate the double tree-sitter parse, split into submodules, and add proper documentation with Rust examples.

## Background

The MEM-map (Minimal Expanded Members map) implementation landed as a single 900-line `memmap.rs` file. PR review identified several structural issues:

1. `seed_from_cst` re-parses tree-sitter to find macro nodes, then also calls `file_item_tree` (which parses the same file). This should be a single pass through `file_item_tree`.
2. The file uses `// ----` section dividers instead of submodules.
3. `resolve_and_expand_macros` and `resolve_and_expand_inner` are near-duplicates.
4. Documentation is sparse — no Rust examples showing what source code produces which data structures.
5. External modules return an empty memmap, but shouldn't have one at all.

### Current data flow (problematic)

```
Source text
  → tree-sitter [in seed_from_cst] → macro nodes → MacroDef/MacroUse entries
  → tree-sitter [in file_item_tree, memoized] → Vec<Item> → Named/Glob/Anon entries
```

### Target data flow

```
Source text
  → tree-sitter [in file_item_tree only] → Vec<Item>  (now includes MacroDef, MacroInvocation)
                                               ↓
                                       module_memmap (Item → MemmapEntry, resolve, expand)
                                               ↓
                                       resolve_name / memmap_errors
```

## Architecture

After reorganization:

```
crates/sage-ir/src/
  item.rs              — Item enum (extended with MacroDef, MacroInvocation variants)
  lower.rs             — file_item_tree (handles macro nodes now)
  memmap/
    mod.rs             — re-exports, module_memmap query, ModuleMemmap struct
    data.rs            — MemmapEntry, NamedMember, MacroUse, MacroUseState, etc.
    seed.rs            — Item → MemmapEntry transformation (no tree-sitter)
    expand.rs          — resolve_and_expand (single implementation), expand_macro
    resolve_path.rs    — resolve_memmap_path, walk_path_to_macro, find_macro_in_module
    validate.rs        — memmap_errors, collect_all_names, time-travel detection
```

Key principle: `memmap/` never touches tree-sitter. It operates purely on `Vec<Item>` from `file_item_tree`.

## Implementation plan

### Testing strategy

We use three kinds of tests throughout:

1. **Correctness tests** — existing behavior still works (`cargo test` all green).
2. **Behavior tests** — new `Item` variants produce the expected output.
3. **Incremental tests** — use the salsa event log (`db.take_query_log()` captures `EventKind::WillExecute`) to verify which queries re-execute after a change. Key pattern:

   ```rust
   // Initial computation
   let _ = module_memmap(db, module, source_root, root);
   db.take_query_log();  // drain

   // Mutate an input
   source_file.set_text(db).to(new_text.to_owned());

   // Re-query and inspect the log
   let _ = module_memmap(db, module, source_root, root);
   let log = db.take_query_log();
   assert!(!log.contains("module_memmap"), "expected memmap to be cached");
   ```

   The log contains `salsa: WillExecute(QueryName(Id(N)))` lines — one per actual query re-execution. Cached (reused) queries do not appear.

### Phase 0: Baseline incremental tests (regression guard)

**Goal**: Lock in current incremental behavior as tests *before* refactoring, so regressions are immediately visible. These tests document what works today and what Phase 2 will improve.

**Files**: `crates/sage-ir/tests/memmap_incremental_tests.rs` (new)

**Tests** (all use the salsa event log via `db.take_query_log()`):

- **`baseline_initial_memmap_computation`**: Compute `module_memmap` on a fresh db for a module with a function and a macro invocation. Snapshot the log with `expect_test`. This captures the current set of queries invoked. Any reorganization that changes this set must update the snapshot — making regressions visible.

- **`baseline_body_change_behavior`**: Current behavior. Compute memmap, change a body, re-compute. Snapshot the log. (Today: `module_memmap` re-executes because it reads `file.text` directly via tree-sitter. After Phase 2: it should NOT re-execute — this test's snapshot will need updating in Phase 2 to reflect the improvement.)

- **`baseline_sibling_module_isolation`**: Compute `module_memmap(a)`. Change `b`. Re-compute `module_memmap(a)`. Snapshot log.

These tests use `expect_test` snapshots so they update easily when behavior changes intentionally. The commit that changes the snapshot should explain *why* in the commit message.

**Steps**:
1. Create the new test file.
2. Write the three tests with `expect![[r#""#]].assert_eq(&log)` initially empty.
3. Run tests with `UPDATE_EXPECT=1` to populate the snapshots.
4. Commit.

**Commit message**: `add baseline incremental tests for module_memmap`

### Phase 1: Extend `Item` with macro variants

**Goal**: `file_item_tree` handles `macro_definition` and `macro_invocation` nodes, eliminating the need for `seed_from_cst` to parse tree-sitter directly.

**Files**: `item.rs`, `lower.rs`, `resolve.rs` (item_name update)

**Tests**:

*Behavior:*
- `file_item_tree_produces_macro_def`: parse `macro_rules! m { () => {} }`, assert the returned `Vec<Item>` contains `Item::MacroDef(...)`; extract the name via `def.name(db)` and assert it equals `m`.
- `file_item_tree_produces_macro_invocation`: parse `m!();` at item level, assert the returned items contain `Item::MacroInvocation(...)` with path segments `["m"]`.
- `file_item_tree_multi_segment_macro_path`: parse `foo::bar::m!();`, assert the invocation's path has three segments.
- `item_name_returns_none_for_macro_def`: `item_name(db, Item::MacroDef(...))` returns `None` (preserves `definition()` semantics — macros are in their own namespace).
- `item_name_returns_none_for_macro_invocation`: returns `None` (invocations don't introduce a name).
- `display_item_macro_def`: `format!("{}", Item::MacroDef(...))` renders as `macro_rules! m { () => { ... } }`.

*Regression:*
- All existing tests pass.

**Steps**:
1. Add `Item::MacroDef(MacroDefItem)` variant with tracked struct holding `name: Name` (identity field), `body_tokens: String` (tracked), `span: SpanIndices` (tracked). Match the memmap's existing `MacroDef` fields exactly so Phase 2 can swap them.
2. Add `Item::MacroInvocation(MacroInvocationItem)` variant with tracked struct holding `path: Path` (identity field), `span: SpanIndices` (tracked).
3. In `lower.rs`, handle `"macro_definition"` → `Item::MacroDef(...)` instead of `Item::Error`.
4. In `lower.rs`, handle `"expression_statement"` containing `"macro_invocation"` → `Item::MacroInvocation(...)` instead of `Item::Error`.
5. Update `display.rs`: add `Item::MacroDef(v) => fmt::Display::fmt(v, f)` and `Item::MacroInvocation(v) => ...` arms to the existing exhaustive `Item` match. Add `Display` impls for `MacroDefItem` (prints `macro_rules! $name { () => { $body } }`) and `MacroInvocationItem` (prints `$path!()`).
6. **Do NOT update `item_name`** to return a name for `Item::MacroDef`. Leaving `item_name` returning `None` for macros preserves current `definition()` behavior — macros aren't reachable via non-namespaced name lookup, which matches rustc (macros are in their own namespace). The memmap's Phase 2 seeding code accesses the macro name directly via `def.name(db)` on the tracked struct.
7. Audit `module_items` callers for fallout from the new variants:
   - `resolve.rs::definition` — unchanged behavior (macros still invisible via `item_name`).
   - `main.rs` — iterates items for display. The new `Display` impls (step 5) make this work. Verify.
   - Tests in `expand_tests.rs`, `body_resolve_tests.rs` — may need assertion updates if they count items. Search for assertion patterns like `items.len()`.
   - `item_in_namespace` — uses a catch-all `_ => false`. `Item::MacroDef` and `Item::MacroInvocation` fall through to `false`, which is correct: they're not in Type or Value namespace.
8. Verify all existing tests pass.

**Commit message**: `extend file_item_tree with MacroDef and MacroInvocation item variants`

### Phase 2: Rewrite `seed_from_cst` to use `file_item_tree` only

**Goal**: `module_memmap` no longer touches tree-sitter. Seeding reads `Vec<Item>` exclusively.

**Files**: `memmap.rs` (soon to be `memmap/seed.rs`)

**Tests**:

*Regression:*
- All existing memmap tests pass unchanged.

*Incremental (the main value-add of this phase):*

- **`body_change_does_not_invalidate_memmap`**: Create a module with `fn foo() { 1 }`. Compute `module_memmap`. Drain query log. Update the file text to `fn foo() { 2 }` (body changed, signature unchanged). Re-compute `module_memmap`. Expected log:
  - `file_item_tree` DOES re-execute (source text changed)
  - `module_memmap` does NOT re-execute (served from cache — `item_name` on the same-identity `FunctionItem` returns the same `Name`, and `module_memmap` only reads names)

- **`signature_change_invalidates_memmap`**: Same setup, but rename `foo` → `bar`. Expected log: both `file_item_tree` AND `module_memmap` re-execute (name changed, identity of `FunctionItem` differs).

- **`adding_use_statement_invalidates_memmap`**: Initial: `struct S;`. Change to: `use foo::Bar; struct S;`. Expected: `module_memmap` re-executes (new entry added).

- **`module_memmap_calls_file_item_tree_only_once`**: Compute `module_memmap` on a fresh db. Count `file_item_tree` events in the log for the target file: should be exactly 1, proving no double-parse.

- **`body_change_in_sibling_module_does_not_invalidate_memmap`**: Two modules `a` and `b` in the same crate. Compute `module_memmap(a)`. Change body of a function in `b`. Expected: `module_memmap(a)` does NOT re-execute (independence across modules).

**Steps**:
1. Rewrite seeding to iterate `file_item_tree` items:
   - `Item::MacroDef(def)` → `MemmapEntry::Named(NamedMember { name: def.name(db), ns: Namespace::Macro(Bang), kind: MacroDef(def) })` — note `NamedMemberKind::MacroDef` now directly wraps the `MacroDefItem` from lowering.
   - `Item::MacroInvocation(inv)` → `MemmapEntry::MacroUse(MacroUse { path: inv.path(db), state: Unresolved })`
   - `Item::Use(group)` → iterate imports, emit `Named { kind: Redirect }` for named, `Glob` for glob (unchanged).
   - Other items → same as current `seed_regular_items`.
2. Preserve current redirect namespace behavior: `NamedMember { ns: Namespace::Type }` for redirects (they match any namespace during resolution via `resolve_name`'s namespace handling; see memmap.rs deviations #3).
3. Remove the old memmap-local `MacroDef` tracked struct. Replace all uses with `MacroDefItem` (the one from `item.rs`). Fields match exactly (`name`, `body_tokens`, `span`), so this is mechanical.
4. Remove `seed_from_cst`, `lower_macro_definition`, `lower_macro_invocation`, `collect_macro_path_segments`, `collect_segments_recursive`.
5. Remove `tree_sitter` and `tree_sitter_rust` imports from memmap.
6. Verify all tests pass.

**Commit message**: `rewrite memmap seeding to use file_item_tree (no direct tree-sitter)`

### Phase 3: Split into submodules

**Goal**: Replace the single `memmap.rs` with a `memmap/` directory of focused submodules.

**Files**: `memmap.rs` → `memmap/{mod,data,seed,expand,resolve_path,validate}.rs`

**Tests**: All tests pass unchanged (pure refactor).

**Pre-step: map function dependencies.** Before splitting, list every private function and which future submodule it belongs in. Functions used across multiple submodules become `pub(super)` in their home module. Examples to check:
- `find_macro_in_module` — used by `resolve_path` and `validate` (for glob lookup in time-travel detection). Lives in `resolve_path`, exported as `pub(super)`.
- `collect_all_names` — used by `validate`. Lives in `validate`.
- `expand_macro` — called from `expand`. Lives in `expand`.
- `resolve_and_expand` (unified) — called from `mod.rs` (the query). Lives in `expand`, exported as `pub(super)`.

**Steps**:
1. Create `memmap/data.rs` — data model types (`MemmapEntry`, `NamedMember`, `MacroUse`, `MacroUseState`, `GlobStem`, `MacroDef`, `ModuleMemmap`). Public re-exports from `mod.rs`.
2. Create `memmap/seed.rs` — `seed_from_items` function. Private `pub(super)`.
3. Create `memmap/expand.rs` — **unified** `resolve_and_expand` (deduplicate the two existing functions — the only difference is root_entries is the snapshot in the first call and passed through recursion in the second; unify by always passing a snapshot reference), `expand_macro`, `MAX_EXPANSION_DEPTH`.
4. Create `memmap/resolve_path.rs` — `resolve_memmap_path`, `MacroResolution`, `walk_path_to_macro`, `find_macro_in_module`, `resolve_redirect_to_macro`.
5. Create `memmap/validate.rs` — `memmap_errors`, `MemmapError`, all `collect_*` helpers, `name_available_via_glob`.
6. Create `memmap/mod.rs` — `module_memmap` query, `module_memmap_initial`, public re-exports (`ModuleMemmap`, `MemmapEntry`, etc., `MemmapError`, `memmap_errors`).
7. Delete `memmap.rs`.
8. Verify all tests pass.

**Commit message**: `split memmap.rs into focused submodules`

### Phase 4: Documentation

**Goal**: Every public type and function has doc comments with Rust examples showing what source code produces which values.

**Files**: All `memmap/*.rs` files

**Tests**: N/A (docs only)

**Steps**:
1. `mod.rs` — expand module doc: explain "MEM" = Minimal Expanded Members, the purpose (minimal work to resolve names, unexpanded macros are fine if we know they produce no names)
2. `data.rs` — each struct/enum gets a Rust example:
   ```rust
   /// A named member in the module's MEM-map.
   ///
   /// Given this Rust source:
   /// ```rust
   /// struct Foo;
   /// use bar::Baz;
   /// macro_rules! m { () => { struct X; } }
   /// ```
   ///
   /// The module's MEM-map contains:
   /// - `NamedMember { name: "Foo", ns: Type, kind: Item(...) }`
   /// - `NamedMember { name: "Foo", ns: Value, kind: Item(...) }` (unit struct constructor)
   /// - `NamedMember { name: "Baz", ns: Type, kind: Redirect { target: "bar::Baz" } }`
   /// - `NamedMember { name: "m", ns: Macro, kind: MacroDef(...) }`
   ```
3. `MacroUseState` — document the state machine transitions and why `Unexpanded` exists (no input needed because only no-arg macros supported; future: macros that declare their output names)
4. `resolve_path.rs` — document each path kind with examples:
   - `self::m!()` — resolves in current module
   - `crate::inner::m!()` — absolute from crate root
   - `super::m!()` — parent module
   - `bar::m!()` — bare identifier, checks local then extern prelude
5. `MAX_EXPANSION_DEPTH` — note that rustc makes this configurable via `#![recursion_limit = "N"]`; we hardcode 128 as a simplification
6. `expand.rs` — document why there's a snapshot-based approach (need to read entries while mutating them)

**Commit message**: `add comprehensive documentation with Rust examples to memmap module`

### Phase 5: Remove external module memmap

**Goal**: `module_memmap` is not called for external modules. Callers that need external module contents go through TcxDb directly.

**Files**: `memmap/mod.rs`, `memmap/resolve_path.rs`, `resolve.rs`

**Tests**:

*Regression:*
- Existing external-crate tests (e.g., extern prelude resolution) continue to pass.

*Behavior:*
- **`external_module_memmap_panics_or_unreachable`**: Calling `module_memmap` on a `ModuleSource::External(..)` panics (debug assertion) or is statically unreachable (after refactoring callers).

*Incremental:*
- **`resolving_extern_crate_does_not_invoke_memmap`**: Setup a test with extern prelude resolution (e.g., `use serde::*`). Compute `resolve_name` for a name from the external crate. Inspect the log: `module_memmap` is invoked for the current crate's module (to read the glob stem), but NOT for the external module.

**Steps**:
1. Add a debug assertion or early-return in `module_memmap` for external modules (they should never be queried)
2. In `resolve_name`, when resolving in an external module, use TcxDb directly (already the case for `definition()`)
3. In `find_macro_in_module` and glob resolution, skip external modules (already done with `ModuleSource::External` guards)
4. Verify all tests pass

**Commit message**: `remove memmap computation for external modules`

## Documentation updates

| Phase | Doc | Section to update |
|---|---|---|
| Phase 1 | `WIP.md` | Deviations (Item enum extended) |
| Phase 3 | Each `memmap/*.rs` | `//!` module docs |
| Phase 4 | All `memmap/*.rs` | Doc comments |

## FAQ

**Q: Can a salsa tracked struct field reference another tracked struct?**
Yes. `ImplItem` has `items: Vec<Item<'db>>` where `Item` is an enum of tracked struct variants. `FunctionItem` has `body: FunctionBody` (a different pattern using `Stashed<Ptr>`). Both work. So in Phase 2, `MacroDefItem` can be a field of a memmap entry with no issue.

**Q: What happens to the memmap's existing `MacroDef` tracked struct after Phase 2?**
Replaced by `MacroDefItem` (the one added in Phase 1 to `item.rs`). Fields match exactly — `name`, `body_tokens`, `span` — so the swap is mechanical. If cross-crate macro support ever needs a different representation, we can reintroduce a wrapper then.

**Q: Should `resolve_and_expand` be iterative (outer loop) or recursive?**
The review suggested an outer loop walking the tree one level at a time. The current recursive approach works but duplicates code. In Phase 3, we'll unify into a single function that processes entries iteratively — walk all entries, expand what we can, recurse into expanded subtrees. The depth counter prevents infinite recursion.

**Q: Does the submodule split affect public API?**
No. `memmap/mod.rs` re-exports everything that's currently `pub` in `memmap.rs`. Test imports don't change.

**Q: What about `module_items`? Is it still needed?**
Yes. Used by `definition()` in resolve.rs, the CLI in `main.rs`, and tests. It's a different query from `module_memmap` — `module_items` returns raw `Vec<Item>` (pre-macro-expansion), while `module_memmap` returns the resolved + expanded view. We keep both. After Phase 1, `module_items` will include `Item::MacroDef` and `Item::MacroInvocation` variants; callers need to handle them.

## What's NOT in scope

- Changing the expansion algorithm (snapshot-based approach stays, just deduplicated)
- Adding new macro features (multi-arm, metavariables, etc.)
- Changing the salsa query structure (`module_memmap` signature stays the same)
- Removing `file_item_tree` or `module_items` (they're still used by other queries)

## Implementation status

- [x] Phase 0: Baseline incremental tests (regression guard)
- [x] Phase 1: Extend `Item` with macro variants
- [x] Phase 2: Rewrite seeding to use `file_item_tree` only
- [x] Phase 3: Split into submodules
- [x] Phase 4: Documentation
- [x] Phase 5: Remove external module memmap

### Deviations from plan

- Phase 0: Used `db.attach` pattern with separate mutation between attach calls (salsa 0.26 requires `&mut db` for input setters, which conflicts with `attach`'s `&db`). Interned structs (Module) are re-created in the second attach call (same data → same ID).
- Phase 1: Behavior tests were added in a follow-up pass (in `item_macro_tests.rs`) rather than during the initial Phase 1 implementation. Added 7 tests (6 from plan + `display_item_macro_invocation`).
- Phase 2: The `baseline_body_change_behavior` snapshot updated to reflect the improvement. Phase 2 incremental tests were added in a follow-up pass (appended to `memmap_incremental_tests.rs`) using content assertions rather than `expect_test` snapshots for semantic clarity.
- Phase 3: The plan called for deduplicating `resolve_and_expand_macros` / `resolve_and_expand_inner` during the split. This was done in a follow-up pass — unified into a single `resolve_with_snapshot` function called by a thin `resolve_and_expand_macros` entry point. Additionally, shared tree-sitter helpers (`collect_macro_path_segments`, `extract_macro_body_tokens`) were extracted to `src/ts_helpers.rs` to eliminate duplication between `lower.rs` and `memmap/expand.rs`.
- Phase 5: No code changes needed beyond the debug assertion — all callers already guard against external modules. Test (`external_module_memmap_panics`) added in a follow-up pass.

### Open issues

(None.)
