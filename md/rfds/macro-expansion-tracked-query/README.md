# RFD: Macro Expansion as a Tracked Query

![Status: Implemented](https://img.shields.io/badge/status-implemented-brightgreen)

## Goal

Refactor macro expansion so that (a) expanded text is properly linked back to the invocation site, and (b) expansion is a memoized salsa query rather than inlined data in the memmap. This improves incrementality, enables hover-through-macro in the future, and replaces the ad-hoc synthetic `SourceFile` pattern.

## Problem

Today, macro expansion has three issues:

1. **Synthetic `SourceFile`s lose provenance.** Expanded text is stored in ad-hoc `SourceFile` inputs (`<macro:NAME>`, `<proc-macro-expansion>`, `<generated>`). There's no way to trace an expanded item back to the macro invocation that produced it.

2. **Expansion is inlined in the memmap.** The `MacroUseState::Expanded(Vec<Expansion<'db>>)` variant stores expansion results directly inside `MemmapEntry::MacroUse`. This means re-running the memmap fixpoint loop also re-runs all expansions, even if the inputs haven't changed.

3. **No memoization boundary.** Two invocations of the same macro with the same input will expand independently — there's no salsa cache for expansion results.

## Design

Three new types and one new query:

### `MacroInput<'db>`: tracked struct from the parser

```rust
/// A macro invocation's input tokens, created during parsing/lowering.
/// Has stable salsa identity from the parse site — never mutated.
#[salsa::tracked]
pub struct MacroInput<'db> {
    #[returns(ref)]
    pub tokens: String,
    pub span: AbsoluteSpan<'db>,
}
```

Created alongside `MacroInvocationAst` during lowering. Captures the input tokens and the invocation's source location. Its salsa identity is stable (from the parser), making it a good key for memoized expansion.

### `ParseSource<'db>`: enum for parseable text

```rust
pub enum ParseSource<'db> {
    /// A real source file on disk.
    SourceFile(SourceFile),

    /// Output of a macro expansion, linked back to the invocation site.
    MacroExpansion(MacroExpansion<'db>),
}

#[salsa::tracked]
pub struct MacroExpansion<'db> {
    pub input: MacroInput<'db>,
    #[returns(ref)]
    pub text: String,
}

impl ParseSource<'db> {
    pub fn text(&self, db: &'db dyn Db) -> &'db str {
        match self {
            ParseSource::SourceFile(f) => f.text(db),
            ParseSource::MacroExpansion(exp) => exp.text(db),
        }
    }
}
```

`SourceFile` stays as the `#[salsa::input]` for real files on disk. `ParseSource` is a plain enum — no tracked wrapper needed. The tracked identity lives on `MacroExpansion`, where salsa memoization matters.

### `AbsoluteSpan<'db>`: gains provenance

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct AbsoluteSpan<'db> {
    pub source: ParseSource<'db>,
    pub start: u32,
    pub end: u32,
}
```

Replaces the current `AbsoluteSpan` which has `file: SourceFile`. Items carry provenance through their span — no separate `source_file` or `parse_source` field needed.

### Parsing: untracked core + tracked wrappers

```rust
/// Untracked: parse Rust source text into items.
/// Takes a ParseSource for stamping onto AbsoluteSpans during lowering.
fn parse_source_text<'db>(
    db: &'db dyn Db,
    source: ParseSource<'db>,
    text: &str,
) -> Vec<ItemAst<'db>>

/// Tracked wrapper: parse a real source file.
#[salsa::tracked(returns(ref))]
fn parse_source_file<'db>(db: &'db dyn Db, file: SourceFile) -> Vec<ItemAst<'db>> {
    parse_source_text(db, ParseSource::SourceFile(file), file.text(db))
}

/// Tracked wrapper: parse macro expansion output.
#[salsa::tracked(returns(ref))]
fn parse_macro_expansion<'db>(db: &'db dyn Db, exp: MacroExpansion<'db>) -> Vec<ItemAst<'db>> {
    parse_source_text(db, ParseSource::MacroExpansion(exp), exp.text(db))
}
```

Each tracked wrapper takes a proper salsa struct as its argument. `ParseSource` gets an untracked convenience method:

```rust
impl ParseSource<'db> {
    pub fn parse(&self, db: &'db dyn Db) -> &'db [ItemAst<'db>] {
        match self {
            ParseSource::SourceFile(f) => parse_source_file(db, *f),
            ParseSource::MacroExpansion(exp) => parse_macro_expansion(db, *exp),
        }
    }
}
```

### `MacroUse`: stays a plain struct

```rust
pub struct MacroUse<'db> {
    pub path: Path<'db>,
    pub input: MacroInput<'db>,
    pub expansions: Vec<Expansion<'db>>,
}

pub struct Expansion<'db> {
    pub callee: MacroCallee<'db>,
    pub entries: Vec<MemmapEntry<'db>>,
}
```

`MacroUse` stays as local data inside the memmap fixpoint loop. It holds `MacroInput<'db>` (stable, tracked) plus a vec of expansions that grows as the fixpoint resolves and expands. No state machine — resolution and expansion happen in the same step. `expansions` starts empty and gains entries as the loop discovers callees.

This works because `MacroUse` is never a salsa-tracked struct — it's a plain struct stored in a `Vec<MemmapEntry>` built locally within `expanded_module`. The `Expansion` entries come from the memoized `expand_macro` query. Entries can include nested `MacroUse`s with empty `expansions`, which get populated on subsequent fixpoint iterations.

### `expand_macro`: tracked query

```rust
/// Expand a macro invocation with a specific callee.
/// Returns a MacroExpansion with provenance linking back to the invocation.
/// Keyed on stable parser-created values — memoized by salsa.
#[salsa::tracked]
fn expand_macro<'db>(
    db: &'db dyn Db,
    callee: MacroCallee<'db>,
    input: MacroInput<'db>,
) -> MacroExpansion<'db>
```

`expand_macro` produces the expanded text and wraps it in a `MacroExpansion`, providing provenance back to the invocation site. It does **not** parse or seed entries — the fixpoint loop handles that.

Inside `expand_macro`:
1. Read `input.tokens(db)` for the macro input.
2. Expand using the `callee`.
3. Return `MacroExpansion::new(db, input, text)`.

### Updated data flow

The fixpoint loop inside `expanded_module` iterates until stable:

Each iteration, for each `MacroUse` in the entry tree (including nested entries from prior expansions):
1. Resolve the macro path to get callees.
2. For any **new** callees (not already in `expansions`), call `expand_macro(db, callee, input)` → get a `MacroExpansion`. Parse via `ParseSource::MacroExpansion(exp).parse(db)` (which calls the memoized `parse_macro_expansion`), seed into `MemmapEntry`s, and push an `Expansion` onto `macro_use.expansions`.
3. Recursively process the new entries (which may contain their own `MacroUse`s with empty `expansions`).
4. Stop when no new callees are discovered across the entire tree.

**What changed from today:** Expansion calls the memoized `expand_macro` query (which returns `MacroExpansion` with provenance) instead of producing ad-hoc synthetic `SourceFile`s. The fixpoint changes from a recursive single-pass with depth tracking to an iterating loop.

## Migration

1. ~~Add `MacroInput<'db>` tracked struct. Create it during lowering alongside `MacroInvocationAst`. Update `MacroUse` to hold `MacroInput<'db>` instead of a raw `String` for input tokens.~~ **Done.**

2. ~~Add `ParseSource<'db>` enum, `MacroExpansion<'db>` tracked struct, and `ParseSource::text()` / `ParseSource::parse()` methods. Change `AbsoluteSpan` to `AbsoluteSpan<'db>` with `source: ParseSource<'db>` replacing `file: SourceFile`. Refactor `file_item_tree` into untracked `parse_source_text` + tracked wrappers `parse_source_file(db, SourceFile)` and `parse_macro_expansion(db, MacroExpansion)`. Update `LowerCtx` to hold `ParseSource<'db>` instead of `SourceFile`. The driver calls `parse_source_file` for real files.~~ **Done.** `file_item_tree` removed; all callers migrated to `parse_source_file`.

3. ~~Add `expand_macro` tracked query returning `MacroExpansion`. Update the fixpoint loop to call `expand_macro` for resolved macros, wrap as `ParseSource::MacroExpansion`, parse and seed the result. Change the fixpoint from recursive single-pass to an iterating loop — nested macros in expansion output get resolved on subsequent iterations.~~ **Done.**

4. ~~Remove the ad-hoc synthetic `SourceFile` creation in `expand.rs`.~~ **Done.** The `derive.rs` and `builtins.rs` synthetic `SourceFile` sites are **out of scope** — a future "fully integrated attribute macros" RFD will unify derive/attribute expansion into the memmap fixpoint. `builtins.rs` updated mechanically to use `ParseSource::SourceFile` in `AbsoluteSpan`.

## Dependents

- **RFD: Relative span model** — implemented; items carry `AbsoluteSpan` with `file: SourceFile`. This RFD changes `AbsoluteSpan` to `AbsoluteSpan<'db>` with `source: ParseSource<'db>`.
- **RFD: Resolve at position** — depends on `ParseSource` (layer 2 filters items by `ParseSource` variant to distinguish real files from macro expansions).
