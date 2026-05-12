# WIP: MEM-map data model simplification and deferred resolution

## Goal

Simplify the MEM-map data model and split path resolution into two
distinct operations with different semantic guarantees:

- A **construction-time resolver** used only to resolve macro
  invocation paths during MEM-map construction, which discovers *all*
  candidate definitions (0, 1, or many) without ever erroring.
- A **post-construction resolver** used by the rest of the compiler,
  which sees a flattened MEM-map and returns a single answer or an
  error.

Along the way, fix the correctness bug where globs and redirects
whose targets are created by macro expansion are silently dropped,
and remove the data model redundancy that stores namespace
information on every entry.

## Motivation

PR review feedback identified three interconnected problems:

### 1. Globs can't be resolved during seeding

```rust
macro_rules! m { () => { mod foo { pub struct Bar; } } }
m!();
use foo::*;  // `foo` doesn't exist until m!() expands
```

Currently `seed_from_items` calls `resolve_use_path_to_module` for
every glob. If the target module doesn't exist yet (because a macro
hasn't expanded), the glob is silently dropped. Correctness bug.

### 2. Redirect namespaces can't be known during seeding

```rust
macro_rules! m { () => { mod things { pub struct Foo; } } }
m!();
use things::Foo;  // Is Foo a type? value? both? Can't know until `things` exists.
```

Currently we store `ns: Namespace::Type` on redirects as a
placeholder. The namespace should be determined at lookup time by
resolving the target.

### 3. The data model is over-structured

`NamedMember { name, ns, kind: NamedMemberKind }` stores redundant
information:
- For items, namespace is derivable from the item kind
- For redirects, namespace must be resolved dynamically
- For macros, namespace is always `Macro(Bang)`

The `Named`/`Anon` distinction is also redundant — it's just whether
`item_name()` returns `Some`.

## Background: why two resolvers

The current design has a single `resolve_memmap_path` used during
construction and a single `resolve_name` used after. Both try to
answer "what does this name resolve to in module M?" but with very
different semantics:

- During **MEM-map construction**, we're walking a
  still-being-built tree. Macro invocations haven't all expanded
  yet, so the set of visible names is growing monotonically across
  fixpoint iterations. The resolver for macro paths must therefore
  report *every* candidate def it can currently see — picking one
  prematurely would commit us to an answer that might change in a
  later iteration. And the caller (`resolve_and_expand_macros`)
  doesn't want an error when there are 0 candidates yet (stay
  `Unresolved`, try again next fixpoint iteration) or 2+ candidates
  (expand them all, record both branches; see Option 3 below).

- **After convergence**, the tree is fixed. Every reference in real
  code (`resolve_body`, display, IDE-style queries) wants exactly
  one answer. The level structure of the tree is an implementation
  detail of how names got there, not something callers should have
  to reason about. Priority is the classic rustc rule: named beats
  glob, *globally*, regardless of how deep in a macro expansion the
  named item was introduced.

Conflating the two forces compromises: either the post-construction
resolver over-flags cross-level situations as ambiguous (which the
current WIP algorithm did), or the construction-time resolver has
to return errors it shouldn't. Splitting them removes the conflict.

## New data model

```rust
enum MemmapEntry<'db> {
    /// A declared item (struct, fn, impl, mod, …).
    /// Name via `item_name()`. Namespace via `item_in_namespace()`.
    /// Anonymous items (impls) are also Item; walkers skip them via item_name.
    Item(Item<'db>),

    /// macro_rules! definition. Name via `def.name()`. Always Macro(Bang).
    MacroDef(MacroDefItem<'db>),

    /// `use foo::bar [as baz]` — name is the alias, namespace resolved
    /// dynamically by resolving `target` at lookup time.
    Redirect { name: Name<'db>, target: Path<'db> },

    /// `use foo::*` — `path` is resolved to a module dynamically at
    /// lookup time, not during seeding.
    Glob { path: Path<'db> },

    /// Macro invocation with its resolution/expansion state.
    MacroUse(MacroUse<'db>),
}

struct MacroUse<'db> {
    /// The invocation's path (e.g. `foo::bar::m`).
    path: Path<'db>,
    /// The token stream passed to the macro at the invocation site
    /// (the contents of `m!(...)`). Empty for no-arg invocations.
    /// This is a property of the invocation, not of any candidate
    /// def, so it lives here, not on Expansion.
    input_tokens: String,
    /// Resolution and expansion state. See `MacroUseState`.
    state: MacroUseState<'db>,
}

enum MacroUseState<'db> {
    /// Path hasn't resolved yet (still iterating), or converged
    /// without any candidate def (validator reports UnresolvedMacro).
    Unresolved,

    /// Resolved to one or more candidate callees, but no MEM-map
    /// entries have been produced. Two legitimate reasons to be here:
    ///
    ///   - The macro's output doesn't contribute names to the enclosing
    ///     module, so running the expansion is unnecessary work for
    ///     name resolution. Example: `#[derive(Debug)]` generates
    ///     `impl Debug for T`, which is anonymous and doesn't enter the
    ///     MEM-map. We know the callee (the builtin Debug derive) but
    ///     don't need to expand it to populate entries.
    ///
    ///   - Expansion is deferred to a later phase (e.g. body-time
    ///     expansion, or expansion requested only by specific
    ///     downstream queries).
    ///
    /// `len() > 1` is ambiguous resolution, same E0659 semantics as
    /// the Expanded case.
    Resolved(Vec<MacroCallee<'db>>),

    /// Resolved and expanded. Each Expansion pairs the chosen
    /// callee with the entries that expansion produced. `len() > 1`
    /// is fan-out (time-travel / cross-level ambiguity); each branch
    /// is independently usable, validator reports AmbiguousMacro.
    Expanded(Vec<Expansion<'db>>),
}

struct Expansion<'db> {
    /// The callee that produced this branch.
    callee: MacroCallee<'db>,
    /// The MemmapEntry values produced by expanding `callee`
    /// against the enclosing MacroUse's `input_tokens`.
    entries: Vec<MemmapEntry<'db>>,
}

/// Anything that can appear as the "target" of a macro invocation.
/// Broader than `MacroDefItem` because derives and proc-macros
/// aren't `macro_rules!` definitions.
///
/// Classification happens at `Symbol → MacroCallee` conversion time
/// (i.e., when `resolve_path_ctime`'s `Vec<Symbol>` is filtered to
/// macro callees). Knowing the variant up-front lets the caller
/// decide whether the MacroUse needs to enter `Expanded` state or
/// can stop at `Resolved`:
///
/// | Variant   | Can introduce names? | MacroUseState path |
/// |-----------|----------------------|--------------------|
/// | `Rules`   | Yes                  | Expanded           |
/// | `Builtin(DeriveKind)` | No (anonymous impls) | Resolved |
/// | `Builtin(BangKind)`   | Depends on kind (see table) | Expanded when yes, Resolved when no |
/// | `Proc`    | Yes (can emit any item) | Expanded        |
enum MacroCallee<'db> {
    /// Local `macro_rules!` definition.
    Rules(MacroDefItem<'db>),

    /// Builtin macro. Identified by resolving the name through the
    /// std prelude to `Symbol::External(cn, di)` and asking
    /// `tcx.classify_builtin_macro(cn, di)`. Covers:
    ///   - Derives: Debug, Clone, Copy, PartialEq, Eq, Hash,
    ///     PartialOrd, Ord, Default
    ///   - Bangs: println, print, eprint(ln), vec, format(_args),
    ///     dbg, assert, debug_assert, matches, todo, unreachable,
    ///     unimplemented, stringify, concat, env, option_env,
    ///     file, line, column, module_path, include, include_str,
    ///     include_bytes, compile_error, thread_local, ...
    ///
    /// Split into a separate variant from `Proc` because builtins
    /// have known, compiler-implemented behaviour — the kind tells
    /// us immediately whether expansion can contribute names.
    Builtin(BuiltinMacroKind),

    /// External proc-macro (bang, derive, or attribute) that is not
    /// a builtin. Can emit arbitrary items, so expansion must run
    /// for name resolution.
    Proc { crate_num: CrateNum, def_index: DefIndex },
}

/// Compile-time-known builtin macros. Enumeration mirrors rustc's
/// `#[rustc_builtin_macro]` set. See sage's `derive/builtins.rs` for
/// the derive portion; bang-macro entries are added alongside.
enum BuiltinMacroKind {
    // -- Derives (produce anonymous impls; never introduce names) --
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default,
    // -- Bangs (vary by kind; see builtin_introduces_names) --
    Println, Print, Eprintln, Eprint,
    Vec, Format, FormatArgs,
    Dbg, Assert, DebugAssert, Matches,
    Todo, Unreachable, Unimplemented,
    Stringify, Concat, Env, OptionEnv,
    File, Line, Column, ModulePath,
    Include, IncludeStr, IncludeBytes,
    CompileError, ThreadLocal,
    // (extend as encountered; exhaustive match keeps this honest)
}
```

Key changes from today:
- No `Namespace` stored on entries — always computed dynamically.
- `Glob` stores raw `Path`, not a resolved `Module`.
- `Redirect` is a flat variant with `name` and `target` (no more
  `NamedMember { name, ns, kind: Redirect { target } }`).
- `Item` variant covers both named items and anonymous items (impls).
- `MacroUse` moves from a 5-state enum inside a struct to a struct
  with an explicit 3-variant `MacroUseState` (`Unresolved` /
  `Resolved(callees)` / `Expanded(expansions)`). The new `Resolved`
  state captures "we know the callee(s) but haven't expanded" —
  needed for derives and attribute macros whose output doesn't
  contribute names.
- New `MacroCallee` type replaces `MacroDefItem` anywhere a macro
  invocation's target is stored. Covers local `macro_rules!`,
  external proc-macros, and builtin derives uniformly.
- `MacroUse` also gains `input_tokens`, which today gets dropped on
  the floor at the `file_item_tree` boundary.
- `NamedMember` / `NamedMemberKind` / `GlobStem` are eliminated.

### Companion IR change: MacroInvocationItem gains input tokens

Today `MacroInvocationItem` only carries `path` and `span`. The
invocation's argument tokens never make it into the IR, so even if
the MEM-map wanted them they'd be unavailable. We add:

```rust
#[salsa::tracked(debug)]
pub struct MacroInvocationItem<'db> {
    pub path: Path<'db>,
    #[tracked]
    #[returns(ref)]
    pub input_tokens: String,  // NEW — contents of m!(...)
    #[tracked]
    pub span: SpanIndices,
}
```

`lower.rs` already has the tokens in hand when it builds the
`macro_invocation` node; we just stop discarding them.
`seed_from_items` then wires the tokens into
`MacroUse { path, input_tokens, state: MacroUseState::Unresolved }`.

For the same reason, `MacroDefItem::body_tokens` stays as-is — the
*definition's* body is what a `macro_rules!` matches against, and
that's already plumbed. The missing piece was the *invocation's*
input, which combines with `body_tokens` during expansion.

### Companion IR change: `ModuleSource` gains `LocalInline`

`Module` today is either `ModuleSource::Local { file, parent }` or
`ModuleSource::External(cn, di)`. Inline `mod foo { ... }` has no
`SourceFile` of its own, so there's no way to build a `Module` for
it; `sym_to_module` currently returns `None` for inline mods,
which means any path like `foo::Bar` where `foo` is inline breaks
at the first intermediate segment.

We add a third variant, plus a new `declaration` field on the
existing `Local`:

```rust
pub enum ModuleSource<'db> {
    /// Workspace module backed by a source file (e.g. `mod foo;` →
    /// `foo.rs`).
    Local {
        file: SourceFile,
        parent: Option<Module<'db>>,
        /// The `mod foo;` item in the parent that declared this
        /// module. None only for the crate root (lib.rs/main.rs has
        /// no declaring item). Needed so that `module_as_symbol`
        /// can produce `Symbol::Local(Item::Mod(m))` in O(1) when a
        /// path resolves to a module itself — otherwise we'd have
        /// to search the parent's items.
        declaration: Option<ModItem<'db>>,
    },
    /// Workspace module declared inline in its parent's file
    /// (e.g. `mod foo { ... }`).
    LocalInline {
        parent: Module<'db>,
        mod_item: ModItem<'db>,
    },
    /// External crate module, queryable via TcxDb.
    External(CrateNum, DefIndex),
}
```

Inline modules become first-class `Module`s. `Module` stays a
`salsa::interned` keyed on `ModuleSource`, so
`LocalInline { parent, mod_item }` produces a stable ID as long
as `ModItem`'s salsa identity is stable — which it is, since
`ModItem.items` is a tracked field.

Consequences for the existing code:

| Site | Change |
|---|---|
| `module_items(module)` | New arm: `LocalInline { mod_item, .. } → mod_item.items(db).unwrap_or_default()` |
| `module_memmap(module)` | New arm: seed directly from `mod_item.items(db)` instead of calling `file_item_tree` |
| `sym_to_module(Item::Mod(m), parent)` | When `m.items(db).is_some()`, return `Module::new(db, LocalInline { parent, mod_item: m })` instead of `None` |
| `module_as_symbol(module)` | New helper (used by Layer 1). `Local { declaration: Some(m), .. } → Some(Symbol::Local(Item::Mod(m)))`; `Local { declaration: None, .. } → None` (crate root has no backing symbol); `LocalInline { mod_item, .. } → Some(Symbol::Local(Item::Mod(mod_item)))`; `External(cn, di) → Some(Symbol::External(cn, di))` |
| `module_label(module)` (logging) | New arm: `LocalInline { mod_item, .. } → "inline {name}"` or similar |
| All other `match module.source(db)` sites | New arm for `LocalInline`; `Local` gets an extra field (destructuring with `..`); `LocalInline` generally behaves like `Local` for memmap purposes |
| Helper `Module::containing_file(db) → Option<SourceFile>` | Walks `LocalInline.parent` up to the first `Local { file }` — needed for spans and for `SourceRoot` lookups that are keyed by file |
| Construction sites for `ModuleSource::Local` (driver, `resolve_mod`, tests) | Pass `declaration`: `None` for the crate root, `Some(mod_item)` elsewhere. `resolve_mod` already has `mod_item` in scope — just stops discarding it |

Cycle recovery: `LocalInline` participates in the same salsa
cycle-initial mechanism as `Local` — cycles that involve inline
modules bottom out at empty memmaps the same way cross-file
cycles do.

Identity under incremental edits: if the user edits the body of
an inline `mod foo`, its `ModItem`'s `items` tracked field
changes, so the tracked struct gets a new salsa ID, so the
interned `Module(LocalInline { parent, mod_item })` is also new.
Every downstream query keyed on that Module re-runs — correct, no
extra work.

## The two resolvers

### Construction-time: `resolve_path_ctime`

Called by `resolve_and_expand_macros` (and recursively, by itself,
for redirect and glob targets) to resolve a general path to a set
of candidate `Symbol`s. Despite the name "macro-map", it's a
general path resolver; the macro-invocation caller just filters
the returned symbols for "is this a macro callee?" at the end.

```text
resolve_path_ctime(module, entries_snapshot, path, final_ns) -> Vec<Symbol>
```

Semantics: "all candidates across time, with named > glob acting
only as a within-tree-node override." Never errors — 0 candidates
means the caller decides what to do (for macros: leave Unresolved
and try again next fixpoint iteration).

The algorithm has three layers: a path walker that dispatches
segments and fans out, a per-module lookup that handles local vs
external sources, and a per-tree-node collector that implements
the named > glob priority.

#### Layer 1: path walker

```text
resolve_path_ctime(module, entries_snapshot, path, final_ns) -> Vec<Symbol>:
  segments = path.segments()
  if segments.is_empty(): return []

  # Dispatch the leading segment(s). Returns a set of "starting points";
  # each start pairs a module + an entries source with the segments
  # still remaining to resolve from that module.
  starts: Vec<(Module, EntriesSource, &[Name])> =
      dispatch_first_segment(module, entries_snapshot, segments)
  # EntriesSource = Snapshot(&[MemmapEntry]) | ViaModuleMemmap(Module) | External

  # Unified work queue. Each tuple represents "continue resolving these
  # segments starting from this module". Terminal results accumulate
  # into `results`.
  results: Vec<Symbol> = []
  work: Vec<(Module, EntriesSource, &[Name])> = starts

  while !work.is_empty():
    next_work: Vec<(Module, EntriesSource, &[Name])> = []

    for (m, src, remaining) in work:
      if remaining.is_empty():
        # A start whose leading dispatch consumed every segment
        # (e.g. pure `crate`, `self`, `super`, `::extern_crate`).
        if let Some(sym) = module_as_symbol(m):
          results.push(sym)
        continue

      seg      = remaining[0]
      rest     = &remaining[1..]
      is_last  = rest.is_empty()
      seg_ns   = if is_last { final_ns } else { Namespace::Type }

      syms = lookup_in_module_ctime(m, src, seg, seg_ns)
      for sym in syms:
        if is_last:
          results.push(sym)
        else:
          if let Some(next_mod) = sym_to_module(sym, m):
            next_work.push((
                next_mod,
                entries_source_for(next_mod, module, entries_snapshot),
                rest,
            ))

    work = next_work

  results

entries_source_for(target, starting_module, snapshot):
  if target == starting_module  { Snapshot(snapshot) }
  else if target is External    { External }
  else                          { ViaModuleMemmap(target) }

module_as_symbol(module) -> Option<Symbol>:
  # Emitted when the walker reaches a module with no remaining
  # segments — i.e., the path resolved to the module itself.
  # Rare in practice; only valid for paths like `foo` where `foo`
  # is a single-segment extern crate. Returns None for the crate
  # root (no declaring Item::Mod exists).
  match module.source():
    External(cn, di)                      => Some(Symbol::External(cn, di))
    LocalInline { mod_item, .. }          => Some(Symbol::Local(Item::Mod(mod_item)))
    Local { declaration: Some(m), .. }    => Some(Symbol::Local(Item::Mod(m)))
    Local { declaration: None, .. }       => None  # crate root

dispatch_first_segment(module, snapshot, segments)
    -> Vec<(Module, EntriesSource, &[Name])>:
  # Interprets the leading segment(s) to produce starting points. Most
  # cases return a single start; the "bare ident that could also be an
  # extern crate" case returns two with different remaining-lengths.
  match segments[0].text:
    "" =>
      # Leading `::` — next segment must be an extern crate name.
      if segments.len() < 2: return []
      match tcx.extern_crate(segments[1].text):
        Some(cn) =>
          ext = Module::External(cn, DefIndex(0))
          [(ext, External, &segments[2..])]
        None => []

    "crate" =>
      [(crate_root, ViaModuleMemmap(crate_root), &segments[1..])]

    "self" =>
      [(module, Snapshot(snapshot), &segments[1..])]

    "super" =>
      match module.parent():
        Some(p) => [(p, ViaModuleMemmap(p), &segments[1..])]
        None    => []

    _ =>
      # Bare identifier. Up to three parallel interpretations; each
      # becomes a separate entry in the work queue.
      let mut out = vec![];

      # 1. Local interpretation: the name refers to something inside
      #    the current module.
      #    Start = current module with its snapshot, remaining =
      #    all segments (lookup will consume segments[0]).
      out.push((module, Snapshot(snapshot), segments));

      # 2. Extern-crate interpretation: the name refers to a crate
      #    in the extern prelude.
      #    Start = that external crate's root, remaining =
      #    segments[1..] (the name has already been consumed as
      #    the crate).
      if let Some(cn) = tcx.extern_crate(segments[0].text):
        let ext = Module::External(cn, DefIndex(0));
        out.push((ext, External, &segments[1..]));

      # 3. Std-prelude interpretation: the name could be re-exported
      #    by `std::prelude::v1` (i.e. the implicit `use
      #    std::prelude::v1::*` at the crate root). Starting from
      #    that prelude module with the FULL segments handles
      #    single-segment cases (`println`, `Debug`, `Option`, ...)
      #    and multi-segment cases (`Option::Some`, `Vec::new`, ...)
      #    uniformly — lookup consumes segments[0] inside the
      #    prelude module.
      if let Some(prelude_mod) = std_prelude_module(db):
        out.push((prelude_mod, External, segments));

      out
```

Notes on prelude handling:

- Extern prelude and std prelude are both "ambient sources of
  first-segment names", mirroring how rustc's resolver treats
  them. The construction-time resolver lists them as parallel
  starting points; the post-construction resolver treats them as
  low-priority fallbacks (named → glob → extern → std).
- `std_prelude_module(db)` walks `std → prelude → v1` via tcx and
  memoises the `v1` module as a salsa query. The existing
  `resolve_in_std_prelude` helper in `resolve.rs` already does
  this walk inline; extracting and caching the intermediate
  module is a small refactor.
- Leading `::` still goes exclusively through extern prelude —
  `::foo` means "the foo extern crate, not a local foo". The
  `""` arm doesn't add a std-prelude start.

#### Worked example: bare-plus-extern dispatch

Path `foo::Bar`, resolved in a module where `foo` is both an inline
module with a `Bar` inside *and* an extern crate that also happens
to contain a `Bar`. `dispatch_first_segment` returns two starts:

```text
[
  (current_module, Snapshot(snap), &[foo, Bar]),    # local interpretation
  (Module::Extern(crate_foo), External, &[Bar]),    # extern interpretation
]
```

First iteration of the walker:

- Local start: `remaining = [foo, Bar]`, `seg = foo`, not last.
  `lookup_in_module_ctime` finds `Item::Mod(foo)`. `sym_to_module`
  walks into it. Enqueues `(foo_module, ViaModuleMemmap(foo_module), &[Bar])`.

- Extern start: `remaining = [Bar]`, `seg = Bar`, *is* last.
  `lookup_in_module_ctime` on the external `foo` crate yields
  `Symbol::External(foo, Bar's DefIndex)`. Pushed to `results`.

Second iteration processes the local-descended entry:

- `(foo_module, ..., [Bar])`: `seg = Bar`, last. Lookup finds
  `Item::Struct(Bar)`. Pushed to `results`.

Final result: two symbols, one for the local `foo::Bar`, one for
the extern `foo::Bar`. Caller decides what to do with the ambiguity.

#### Worked example: `println!()` via std prelude

Path `println`, resolved in `Namespace::Macro(Bang)` with no local
binding and no extern crate named `println`:

```text
dispatch_first_segment(...) → [
  (current_module, Snapshot(snap), &[println]),    # local
  # No extern `println` crate — no extern start.
  (std_prelude_v1,  External,      &[println]),    # std prelude
]
```

First iteration:

- Local: `lookup_in_module_ctime(current, snap, "println", Macro(Bang))`
  → `resolve_name_ctime` walks tree nodes, finds nothing for
  `println` → empty.
- Std prelude: `lookup_in_module_ctime(std_prelude_v1, External,
  "println", Macro(Bang))` → routes to
  `tcx.definition_in_ns(std_cn, prelude_v1_di, "println", Macro(Bang))`
  → finds the `println!` macro → `Symbol::External(std_cn, println_di)`.
  Since `remaining = [println]` has length 1 it's the last segment,
  pushed to `results`.

Caller calls `symbol_to_macro_callee(db, Symbol::External(std_cn,
println_di))`:
- `classify_builtin_macro(std_cn, println_di)` →
  `Some(BuiltinMacroKind::Println)`.
- Return `MacroCallee::Builtin(Println)`.

Caller then checks `needs_expansion_for_memmap([Builtin(Println)])`:
- `builtin_introduces_names(Println)` is `true` (conservative for
  expression-level bangs).
- So `MacroUse.state = Expanded(vec![Expansion{ callee: Builtin(Println),
  entries: expand_macro(...) }])`.

In practice `println!()` at item position would fail later (it's
an expression-level macro, `expand_macro` produces no items), but
the MEM-map records the resolution correctly and no
`UnresolvedMacro` diagnostic fires.

#### Layer 2: per-module lookup

```text
lookup_in_module_ctime(module, src, name, ns) -> Vec<Symbol>:
  match (module.source(), src):
    (External(cn, di), _) =>
      # Externals have no entries — go through tcx.
      tcx.definition_in_ns(cn, di, name, ns)
         .into_iter().map(Symbol::External).collect()

    (Local, Snapshot(entries)) =>
      resolve_name_ctime(entries, name, ns)

    (Local, ViaModuleMemmap(m)) =>
      resolve_name_ctime(module_memmap(db, m, ...).entries(db), name, ns)

    (Local, External) =>
      unreachable!()
```

#### Layer 3: per-tree-node collection with per-node named > glob

This is where the construction-time shadowing rule lives. Each call
to `collect_at_node` on a particular entry list is one "tree node"
for the shadowing rule. We walk the top-level entry list as one
node, and every `Expansion::entries` inside a `MacroUse::Expanded`
as its own separate node.

```text
resolve_name_ctime(entries, name, ns) -> Vec<Symbol>:
  results: Vec<Symbol> = []
  for node in walk_tree_nodes(entries):
    named = named_candidates_at_node(node, name, ns)
    if named.is_empty():
      results.extend(glob_candidates_at_node(node, name, ns))
    else:
      results.extend(named)
  results

walk_tree_nodes(entries):
  yield entries
  for entry in entries:
    if let MacroUse { state: Expanded(exps), .. } = entry:
      for exp in exps:
        yield_from walk_tree_nodes(exp.entries)
```

#### Per-entry handling table

For a given segment name and namespace at the current position, each
`MemmapEntry` variant contributes (or doesn't) as follows. Columns
distinguish intermediate-segment behaviour (we need a module to walk
into — so non-module candidates are dropped after collection) from
terminal-segment behaviour (any symbol in `final_ns` is fine).

| Entry | Matching condition | As named/glob | At intermediate | At terminal (in `final_ns`) |
|---|---|---|---|---|
| `Item(Item::Mod(m))` | `m.name == seg` | named | walk into: inline or file-based module | returns `Symbol::Local(Item::Mod(m))` if `Type` |
| `Item(Item::Struct/Enum/Trait/TypeAlias)` | name matches | named | dropped (not a module) | returns the symbol if `Type` |
| `Item(Item::Function/Const/Static)` | name matches | named | dropped (not a module) | returns the symbol if `Value` (also `Value` for struct constructor) |
| `Item(Item::Impl(_))` | anonymous — never matches | — | — | — |
| `Item(Item::Use/MacroInvocation/MacroDef/Error)` | not reached — seeding already re-categorised these as `Redirect` / `MacroUse` / dropped | — | — | — |
| `MacroDef(def)` | `def.name == seg` | named | dropped | returns `MacroCallee::Rules(def)` via symbol if `Macro(Bang)` |
| `Redirect { name: n, target }` | `n == seg` | named | recurse: `resolve_path_ctime(module, snapshot, target, Type)`; filter to modules | recurse: `resolve_path_ctime(module, snapshot, target, final_ns)` |
| `Glob { path }` | always (for this node) | glob | resolve `path` via `resolve_path_ctime(module, snapshot, path, Type)` → target modules; `lookup_in_module_ctime(target, ..., seg, Type)` per target | same, but with `final_ns` |
| `MacroUse { state: Unresolved, .. }` | — | — | contributes nothing | contributes nothing |
| `MacroUse { state: Resolved(callees), .. }` | `final_ns == Macro(Bang)` and callee name matches | named | dropped (callees aren't modules) | returns each matching callee's symbol |
| `MacroUse { state: Expanded(exps), .. }` | — | — | each `exp.entries` is a separate tree node — contributes via the tree walk, not the entry itself | same |

Redirects and globs both recurse via `resolve_path_ctime`. Cycle
handling prevents infinite recursion — see next section.

#### Cycle detection

`resolve_path_ctime` maintains a visited set plus a depth cap:

```rust
struct Visited<'db> {
    in_flight: HashSet<(Module<'db>, Path<'db>)>,
    depth: usize,
}

const MAX_PATH_DEPTH: usize = 128;
```

Entering `resolve_path_ctime` with `(module, path)` already in
`in_flight` returns `[]` immediately. Entering beyond
`MAX_PATH_DEPTH` also returns `[]`. This turns:

```rust
use B as A;
use A as B;
```

into "resolving `A` in-flight → see redirect `A → B` → recurse on
`B` → see redirect `B → A` → (A, _) already in flight → return
`[]`". The caller observes 0 candidates for the redirect target,
and the validator reports `UnresolvedRedirect` after convergence.

Globs through a cycle terminate the same way: `use a::*` whose
target resolves via `use b::*` whose target resolves through `a`
again returns no additional candidates on the second visit.

### Caller dispatch

After `resolve_path_ctime` returns, the caller filters symbols to
macro callees and decides whether to expand:

```rust
/// Classifies a resolved symbol as a MacroCallee, consulting tcx
/// for external defs to separate builtin macros from proc-macros.
/// Returns None if the symbol isn't a macro-callable entity.
fn symbol_to_macro_callee<'db>(
    db: &'db dyn Db,
    sym: Symbol<'db>,
) -> Option<MacroCallee<'db>> {
    match sym.source(db) {
        SymbolSource::Local(Item::MacroDef(def)) => {
            Some(MacroCallee::Rules(def))
        }
        SymbolSource::External(cn, di) => {
            // Ask tcx first whether this is a builtin macro.
            if let Some(kind) = db.tcx().classify_builtin_macro(cn, di) {
                return Some(MacroCallee::Builtin(kind));
            }
            // Otherwise, a proc-macro if tcx says so.
            if db.tcx().is_proc_macro(cn, di) {
                return Some(MacroCallee::Proc {
                    crate_num: cn, def_index: di,
                });
            }
            None  // External def that isn't a macro at all.
        }
        _ => None,
    }
}

/// Whether a specific builtin macro can introduce names into the
/// enclosing module. Derives never can; bangs vary by kind.
fn builtin_introduces_names(kind: BuiltinMacroKind) -> bool {
    use BuiltinMacroKind::*;
    match kind {
        // Derives: always anonymous impls.
        Debug | Clone | Copy | PartialEq | Eq | Hash
        | PartialOrd | Ord | Default => false,

        // Bangs that produce items at item position.
        Include | ThreadLocal => true,

        // Bangs that are expression-level (shouldn't appear at item
        // position in normal code). Conservatively `true` so that if
        // one does show up, we try to expand rather than silently
        // dropping it.
        Println | Print | Eprintln | Eprint
        | Vec | Format | FormatArgs
        | Dbg | Assert | DebugAssert | Matches
        | Todo | Unreachable | Unimplemented
        | Stringify | Concat | Env | OptionEnv
        | File | Line | Column | ModulePath
        | IncludeStr | IncludeBytes
        | CompileError => true,
    }
}

/// Whether ANY callee could introduce names into the enclosing
/// module. If yes, the MacroUse takes the Expanded path (all
/// branches expanded, even ones that contribute nothing). If no,
/// Resolved suffices.
fn needs_expansion_for_memmap(callees: &[MacroCallee]) -> bool {
    callees.iter().any(|c| match c {
        MacroCallee::Rules(_)       => true,
        MacroCallee::Proc { .. }    => true,
        MacroCallee::Builtin(kind)  => builtin_introduces_names(*kind),
    })
}
```

Caller logic in `resolve_and_expand_macros`:

```rust
let callees: Vec<MacroCallee<'db>> = resolve_path_ctime(
        db, module, snapshot, macro_use.path, Namespace::Macro(Bang))
    .into_iter()
    .filter_map(|sym| symbol_to_macro_callee(db, sym))
    .collect();

match callees.as_slice() {
    [] => {
        // No callees — leave state = Unresolved and try again next iteration.
    }
    callees => {
        if needs_expansion_for_memmap(callees) {
            let expansions = callees.iter().map(|callee| Expansion {
                callee: *callee,
                entries: expand_macro(db, *callee, macro_use.input_tokens),
            }).collect();
            macro_use.state = MacroUseState::Expanded(expansions);
        } else {
            // All callees are builtin derives — expansion output is
            // anonymous impls only, which don't enter the MEM-map.
            macro_use.state = MacroUseState::Resolved(callees.to_vec());
        }
    }
}
```

Note: `MacroUseState::Resolved` being reached in Phase 3 — even
without derives support in-memory — requires Phase 3 to also wire
`#[derive(...)]` attributes into `seed_from_items` as `MacroUse`
entries with `Namespace::Macro(Derive)` paths. If that's out of
scope for Phase 3, the `Builtin` variant and `Resolved` path
exist as structural placeholders; the caller will reach
`Resolved` only once derive-seeding lands. We flag this explicitly
in the phase plan.

If `callees.len() > 1`, all branches are expanded (or all recorded
in `Resolved`). Each branch's entries are independently subject to
the fixpoint.

### Post-construction: `resolve_name`

Called by `resolve_body`, display, and everything downstream.

```text
resolve_name(module, name, ns) -> Result<Symbol, ResolutionError>
```

Semantics: the tree is flattened — entries inside `Expansion::entries`
at any depth are treated equivalently to entries at the top level.
Named beats glob globally.

```text
resolve_name(module, name, ns):
  memmap = module_memmap(db, module, ...).entries

  named, globs = [], []
  flatten_collect(memmap, name, ns, &mut named, &mut globs)
  # flatten_collect descends through every MacroUse::Expanded branch,
  # treating all Item/Redirect/MacroDef as named, all Glob as glob.
  # For Redirect, resolve target and check namespace dynamically.
  # For Glob, resolve path to module and search children for (name, ns).

  match named.len():
    1 => return Ok(named[0])
    n if n > 1 => return Err(Ambiguous)

  match globs.len():
    1 => return Ok(globs[0])
    n if n > 1 => return Err(Ambiguous)

  # Extern prelude, std prelude (unchanged from today)
  …
```

Note: "flattening" the tree means `resolve_name` happily returns
answers even when a `MacroUse` is in `Expanded(vec)` state with
`vec.len() > 1`. A name that appears identically in all branches
resolves fine. A name that differs across branches becomes
`Err(Ambiguous)` (or the named candidate wins over a glob
candidate, same as any other case). `MacroUse` in `Resolved(callees)`
state contributes nothing to `resolve_name` — its entries don't
exist because they were never produced. The validator separately
reports structural problems (unresolved, ambiguous-resolution,
E0659).

`resolve_name` uses the same cycle-detection machinery as
`resolve_path_ctime` — a redirect chain that loops yields
`ResolutionError::Unresolved` rather than stack-overflowing.

### Path resolution for non-macro paths

`resolve_path_to_symbol` (currently in `resolve.rs`, used by use
redirects) becomes a thin wrapper around `resolve_name` applied
segment by segment — same pattern as today, but the first-segment
dispatch now delegates to `resolve_name` instead of `definition` so
that macro-introduced names are visible.

## Termination and overflow

The fixpoint around `module_memmap` terminates because the
per-module state grows monotonically through a finite partial
order:

- Every `MacroUse` starts in `MacroUseState::Unresolved`. The only
  transitions are `Unresolved → Resolved(callees)` (all callees
  are `Builtin(kind)` with `builtin_introduces_names(kind) ==
  false`) or `Unresolved → Expanded(branches)`. Once in `Resolved`
  or `Expanded`, a `MacroUse` never leaves those states.
- Within `Resolved(callees)` and `Expanded(branches)`, the inner
  `Vec` can grow (more callees become reachable as more macros
  expand and more names resolve) but never shrinks. Growth is
  bounded by the finite universe of `MacroCallee` values available
  in the workspace plus its dependencies.
- Expansion is a pure function of `(callee, input_tokens)`. Salsa
  caches it, so a given branch's `entries` is fixed once computed.
  Branches can be added to `Expanded(branches)` in later
  iterations, but no branch's entries change.

Salsa's fixpoint stops when two consecutive iterations produce
byte-equal output. Since state is monotonically growing and the
state space is finite, equality eventually holds — termination.

In practice real programs converge in 2–4 iterations. The caps
below exist for malformed or pathological inputs, not normal
code.

### Caps and overflow behaviour

| Cap | Default | Meaning |
|---|---|---|
| `MAX_EXPANSION_DEPTH` | 128 | Max chain of macro-expansion-inside-macro-expansion |
| `MAX_PATH_DEPTH` | 128 | Max recursion depth of `resolve_path_ctime` (redirects/globs) |
| `MAX_BRANCHES_PER_USE` | 16 | Max branches in a single `MacroUseState::Expanded` |
| `MAX_EXPANSION_ITEMS` | 10_000 | Max items produced by a single expansion |

When a cap is hit we keep partial results so downstream queries
that don't depend on the cut-off region still work. Validator
errors flag the overflow:

| Overflow | Behaviour | Validator error |
|---|---|---|
| Expansion depth exceeds `MAX_EXPANSION_DEPTH` at a given MacroUse | State stays `Unresolved` | `ExpansionDepthExceeded { path }` |
| Path-resolution depth exceeds `MAX_PATH_DEPTH` | `resolve_path_ctime` returns `[]` at the overflowing frame | `ResolutionDepthExceeded { path }` |
| Branch count exceeds `MAX_BRANCHES_PER_USE` | Keep first N branches in `Expanded`, drop the rest | `BranchOverflow { path, dropped: usize }` |
| Expansion output exceeds `MAX_EXPANSION_ITEMS` | Keep first N items in `Expansion.entries`, drop the rest | `ExpansionItemsOverflow { callee, dropped: usize }` |

The design principle: never silently truncate. Partial results
are fine, but an overflow must always produce a diagnostic so
the user knows their analysis is incomplete.

## Validation

`memmap_errors` runs after convergence and reports:

| Condition | Error |
|---|---|
| `state = Unresolved` after convergence | `UnresolvedMacro { path }` |
| `state = Resolved(callees)` with `callees.len() > 1` | `AmbiguousMacro { path }` |
| `state = Expanded(vec)` with `vec.len() > 1` | `AmbiguousMacro { path }` (may be classified as E0659 time-travel — see below) |
| `resolve_name(name, ns)` over the flattened tree returns `Err(Ambiguous)` with candidates originating from different branches/levels | `TimeTravelViolation { name, ns }` |
| Redirect whose target, resolved post-convergence, yields no symbols (or fails due to cycle) | `UnresolvedRedirect { name }` |
| Glob whose `path`, resolved post-convergence, yields no module | `UnresolvedGlob { path }` |
| Two items/redirects with same `(name, ns)` at the top level (or within the same branch) | `DuplicateName { name, ns }` |
| Any overflow cap hit | Per the overflow table above (`ExpansionDepthExceeded`, `ResolutionDepthExceeded`, `BranchOverflow`, `ExpansionItemsOverflow`) |

Time-travel vs plain ambiguity is purely a diagnostic
classification — the tree structure is the same either way.

## expand_macro via file_item_tree (open design)

Currently `expand_macro(def)` does its own tree-sitter parsing and
produces `MemmapEntry::Named { kind: Item(Item::Error(…)) }`
placeholders — the items inside expansions are not real `Item`s,
which means display, body resolution, and signatures all break for
anything inside a macro expansion. It also ignores invocation-site
input entirely.

**Goal**: `expand_macro(callee, input_tokens)` produces real `Item`
values through `file_item_tree`, same as source-level items.

For the noop macro cases we currently support, `input_tokens` can
be empty and `expand_macro` just parses `callee`'s body (for
`MacroCallee::Rules`, that's `def.body_tokens()`; for proc-macros
and builtin derives, the expansion path differs — see
`derive/builtins.rs` and `expand_proc_macro_derive` for the
existing plumbing). The signature is forward-compatible with real
`macro_rules!` matching once we add it.

**Open question**: salsa identity. `file_item_tree` is keyed on
`SourceFile` (a salsa input). For expansion to be deterministic
across fixpoint iterations, we need a stable way to turn
`(callee, input_tokens) → expanded text → items` without creating
fresh salsa IDs on every call.

Candidate approaches:

1. **Make `expand_macro` salsa-tracked on `(MacroCallee,
   input_tokens)`.** The function interns a synthetic `SourceFile`
   (via `SourceFile::new(db, path, text)` where `path` encodes the
   invocation identity, e.g. `"<macro:{kind}:{id}:{hash(input)}>"`)
   and calls `file_item_tree` on it. Salsa caches the result under
   the `(callee, input_tokens)` key.

2. **Refactor `file_item_tree` to have a pure core.** Extract a
   `parse_items_from_text(db, text: &str) -> Vec<Item<'_>>` that
   doesn't take a `SourceFile`, and call that directly. Items
   still get salsa IDs, but they're based on their tracked-struct
   fields, so identical bodies produce identical IDs.

Decision deferred to Phase 4; both are viable. Approach 2 is
cleaner architecturally; approach 1 is a smaller diff.

## Tests

All tests use the `//-` fixture format introduced in Phase 0. Each
fixture declares one or more files separated by `//- /path` markers.
Existing tests should continue to pass — behaviour is unchanged for
cases that don't involve macro-created paths or cross-level
interactions.

### New: deferred-resolution cases

**Glob whose target is created by macro expansion**

```rust
t(r#"
    //- /lib.rs
    macro_rules! m { () => { mod foo { pub struct Bar; } } }
    m!();
    use foo::*;
"#)
.resolve("Bar", Type, expect!["<local Struct Bar>"])
.errors(expect![""]);
```

**Redirect whose target is created by macro expansion**

```rust
t(r#"
    //- /lib.rs
    macro_rules! m { () => { mod things { pub struct Foo; } } }
    m!();
    use things::Foo;
"#)
.resolve("Foo", Type, expect!["<local Struct Foo>"])
.errors(expect![""]);
```

**Glob import statement itself comes from macro expansion**

```rust
t(r#"
    //- /lib.rs
    mod foo { pub struct X; }
    macro_rules! m { () => { use foo::*; } }
    m!();
"#)
.resolve("X", Type, expect!["<local Struct X>"]);
```

**Redirect import statement itself comes from macro expansion**

```rust
t(r#"
    //- /lib.rs
    mod foo { pub struct X; }
    macro_rules! m { () => { use foo::X; } }
    m!();
"#)
.resolve("X", Type, expect!["<local Struct X>"]);
```

**Glob target created by nested macro (two expansion levels)**

```rust
t(r#"
    //- /lib.rs
    macro_rules! inner { () => { pub struct Deep; } }
    macro_rules! outer { () => { mod nested { inner!(); } } }
    outer!();
    use nested::*;
"#)
.resolve("Deep", Type, expect!["<local Struct Deep>"]);
```

**Unresolvable glob path is an error (not silently dropped)**

```rust
t(r#"
    //- /lib.rs
    use nonexistent::*;
"#)
.errors(expect![["UnresolvedGlob path=nonexistent::*"]]);
```

### New: global named > glob (fixes post-construction over-flagging)

**Named at root beats glob from expansion**

```rust
t(r#"
    //- /lib.rs
    mod other { pub struct X; }
    struct X;
    macro_rules! m { () => { use other::*; } }
    m!();
"#)
.resolve("X", Type, expect!["<local Struct X @ root>"])
.errors(expect![""]);
// No ambiguity, no E0659. This case was over-flagged by the earlier
// "collect-across-levels" algorithm — the whole reason we split
// into two resolvers.
```

**Same-level shadowing (regression guard)**

```rust
t(r#"
    //- /lib.rs
    mod a { pub struct Foo; }
    mod b { pub struct Foo; }
    use a::*;
    use b::Foo;
"#)
.resolve("Foo", Type, expect!["<local Struct Foo @ b>"]);
```

### New: construction-time fan-out (Option 3 branches)

**Cross-level macro resolution produces multiple branches**

```rust
t(r#"
    //- /lib.rs
    mod other {
        pub mod foo {
            pub macro_rules! m { () => { struct A; } }
        }
    }
    use other::*;
    macro_rules! ex {
        () => {
            mod foo {
                pub macro_rules! m { () => { struct B; } }
            }
        };
    }
    ex!();
    foo::m!();
"#)
.memmap("root", expect![[r#"
    ...
    MacroUse path=foo::m!() state=Expanded [
      branch callee=Rules(other::foo::m) {
        Item Struct("A")
      }
      branch callee=Rules(expanded::foo::m) {
        Item Struct("B")
      }
    ]
"#]])
.resolve("A", Type, expect!["<local Struct A>"])
.resolve("B", Type, expect!["<local Struct B>"])
.errors(expect![["AmbiguousMacro path=foo::m"]]);
```

**Cross-level conflict on a name introduced by expansion (E0659)**

```rust
t(r#"
    //- /lib.rs
    mod other { pub mod foo { pub struct X; } }
    use other::*;
    macro_rules! m { () => { mod foo { pub struct Y; } } }
    m!();
"#)
// `foo` has two candidates (glob from other, named from m!'s expansion).
// Post-construction named wins globally → `foo` resolves to the expansion
// one. But resolution changed before vs after expansion, so:
.resolve("foo", Type, expect!["<local Mod foo @ m!-expansion>"])
.errors(expect![["TimeTravelViolation name=foo ns=Type"]]);
```

### New: cycle handling

**Mutual globs between modules terminate**

```rust
t(r#"
    //- /lib.rs
    mod a;
    mod b;
    use a::*;

    //- /a.rs
    pub use crate::b::*;

    //- /b.rs
    pub use crate::a::*;
    pub struct X;
"#)
.resolve("X", Type, expect!["<local Struct X @ b>"])
.errors(expect![""]);
```

**Cyclic redirects are unresolved, not a stack overflow**

```rust
t(r#"
    //- /lib.rs
    use B as A;
    use A as B;
"#)
.resolve("A", Type, expect!["<unresolved>"])
.resolve("B", Type, expect!["<unresolved>"])
.errors(expect![[r#"
    UnresolvedRedirect name=A
    UnresolvedRedirect name=B
"#]]);
```

**Self-glob at crate root terminates**

```rust
t(r#"
    //- /lib.rs
    use crate::*;
    pub struct X;
"#)
.resolve("X", Type, expect!["<local Struct X>"])
.errors(expect![""]);
// The glob's target is the module it lives in; cycle detector prevents
// infinite recursion. `X` is still findable via the direct named entry.
```

**Redirect through glob cycle**

```rust
t(r#"
    //- /lib.rs
    mod a;
    use a as b;
    use b::*;

    //- /a.rs
    pub use crate::*;   // cycles back through b which is `a`
    pub struct Y;
"#)
.resolve("Y", Type, expect!["<local Struct Y @ a>"])
.errors(expect![""]);
```

**Macro path traversing a cycle terminates at 0 candidates**

```rust
t(r#"
    //- /lib.rs
    mod a;
    use a::*;
    thing!();   // `thing` is only reachable via the cycle

    //- /a.rs
    pub use crate::*;
"#)
// `thing!()` can't be resolved — the cycle truncates, no macro def
// anywhere. Validator reports it; no stack overflow.
.errors(expect![["UnresolvedMacro path=thing"]]);
```

**Macro-created glob target depends on the macro itself**

```rust
t(r#"
    //- /lib.rs
    use foo::*;
    macro_rules! m { () => { mod foo { pub struct Bar; } } }
    m!();
"#)
.resolve("Bar", Type, expect!["<local Struct Bar>"])
.errors(expect![""]);
// Fixpoint: iteration 1 — glob's target `foo` doesn't exist, m!()
// resolves and expands, producing mod foo. Iteration 2 — glob's
// target `foo` now resolves, provides `Bar`. Converges with both
// working.
```

## Architecture

```text
         ┌────────────────────┐
Source → │   file_item_tree   │ → Vec<Item>
         └────────────────────┘          │
                                         ▼
         ┌──────────────────────────────────────────────┐
         │ module_memmap (salsa, cycle recovery)        │
         │                                              │
         │   seed_from_items  → initial entries         │
         │   resolve_and_expand_macros:                 │
         │     for each MacroUse with state=Unresolved: │
         │       syms    = resolve_path_ctime(...)      │
         │       callees = filter_macros(syms)          │
         │       if !callees.empty():                   │
         │         if needs_expansion_for_memmap(...):  │
         │           state = Expanded(vec![Expansion {  │
         │             callee, entries: expand_macro(..)│
         │           } for callee in callees])          │
         │         else:                                │
         │           state = Resolved(callees)          │
         └──────────────────────────────────────────────┘
                                         │
                                         ▼
                              ┌────────────────────┐
                              │  resolve_name      │ ← resolve_body, display, …
                              │  (flattened view)  │
                              └────────────────────┘
                              ┌────────────────────┐
                              │  memmap_errors     │ ← diagnostics
                              │  (validation)      │
                              └────────────────────┘
```

Two consumer faces on the same `ModuleMemmap`: a level-aware
resolver used internally during construction, and a flattened
resolver used by everything downstream. Same tree, different views.

## Implementation plan

Six phases, each independently committable with TDD. Phase 0 lands
first so every subsequent phase has a concise place to put its
tests.

### Phase 0 — Test harness

**Goal**: build a `//-`-fixture-based test harness that makes each
test case a small readable snippet, with pretty-printed snapshot
assertions via `expect_test`.

**Files**:
- `crates/sage-ir/tests/common/mod.rs` — fixture parser,
  `TestCrate` builder, pretty-printers
- `crates/sage-ir/Cargo.toml` — already has `expect-test` as a
  dev-dependency; nothing new

**Tests**: rewrite a handful of existing tests in
`memmap_phase2_tests.rs` / `memmap_phase3_tests.rs` using the new
harness, as a smoke test. Don't migrate everything at once.

**Implement**:
1. `parse_fixture(src: &str) -> Vec<(String, String)>` — splits on
   `//-\s+/(\S+)` marker lines. Strips common leading indent from
   file bodies.
2. `TestCrate` struct holding `Database`, `SourceRoot`, and a
   cached root module. Constructed via `TestCrate::new(fixture)`.
3. Fluent assertion methods (each returns `&self`):
   - `resolve(name: &str, ns: Namespace, expect: Expect)` — runs
     `resolve_name`, formats the result, compares against the
     expected snapshot.
   - `memmap(module_path: &str, expect: Expect)` — resolves the
     module by path, calls `module_memmap`, pretty-prints the
     entries tree, compares.
   - `errors(expect: Expect)` — calls `memmap_errors` over every
     module, sorts, formats, compares.
   - `resolve_ctime(path: &str, ns: Namespace, expect: Expect)` —
     whitebox hook for directly testing `resolve_path_ctime`.
4. Pretty-printers (module-private to the harness — not
   production `Display`):
   - `fmt_memmap_entries(entries: &[MemmapEntry], indent: usize) -> String`
   - `fmt_macro_use_state(state: &MacroUseState) -> String`
   - `fmt_macro_callee(callee: &MacroCallee) -> String`
   - `fmt_symbol_for_test(sym: Symbol) -> String` — compact form
     like `<local Struct X @ b.rs>` rather than the full TcxDb
     display pipeline.
   - `fmt_memmap_error(err: &MemmapError) -> String`
5. Default `TcxDb` is noop. A `TestCrate::with_tcx(mock)` method
   lets individual tests bolt on a mock TcxDb when extern-prelude
   or std-prelude lookup matters.

**Commit message**: `tests: add //- fixture-based test harness for memmap/resolve tests`

### Phase 1 — Data model

**Goal**: swap `MemmapEntry` and `MacroUse` to the new shapes without
changing resolution semantics.

**Files**: `memmap/data.rs`, `memmap/seed.rs`, `memmap/expand.rs`,
`memmap/resolve_path.rs`, `memmap/validate.rs`, `resolve.rs`.

**Tests**: all existing tests pass. No new tests in this phase.

**Implement**:
1. Add `input_tokens: String` to `MacroInvocationItem`; update
   `lower.rs` to populate it from the `macro_invocation` node's
   arguments (stop throwing them away).
2. Replace `MemmapEntry` enum with the 5-variant version.
3. Replace `MacroUse` with
   `{ path, input_tokens, state: MacroUseState }` and introduce the
   `MacroUseState` enum (`Unresolved` /
   `Resolved(Vec<MacroCallee>)` / `Expanded(Vec<Expansion>)`).
   Also add the `MacroCallee` enum (`Rules(MacroDefItem) /
   Builtin(BuiltinMacroKind) / Proc{cn,di}`) and the
   `BuiltinMacroKind` enum. Only `Rules` is populated in Phase 1 —
   other variants are carrying infrastructure.
4. Translate the current algorithm 1:1 onto the new shapes:
   - Current `Unresolved` → new `Unresolved`
   - Current `Expanded(entries)` → new
     `Expanded(vec![Expansion { callee, entries }])` where `callee`
     is `MacroCallee::Rules(def)` for the resolved candidate (we
     currently know it at expansion time but throw it away —
     preserve it now as a callee).
   - Current `Ambiguous`/`Error` → new `Unresolved` for now (validator
     will report).
   - `Resolved(callees)` is new; no current algorithm path produces
     it in Phase 1, but downstream match sites must handle it
     (treat as "contributes no entries" — same as Unresolved for
     name-resolution purposes, but reported as ambiguous-if-len>1
     by the validator).
5. Update `seed_from_items`: no namespace computation on items;
   store `Redirect { name, target }` without ns; store
   `Glob { path }` with raw path; copy `input_tokens` from
   `MacroInvocationItem` into `MacroUse`; remove
   `resolve_use_path_to_module` call from seeding.
6. Update `expand_macro` signature to accept `input_tokens` (still
   ignore it for noop expansion — Phase 4 will consume it
   properly).
7. Update every match site throughout the codebase.

**Commit message**: `memmap: flatten MemmapEntry and introduce MacroUseState enum`

### Phase 2 — Post-construction resolver: flatten + global named > glob

**Goal**: fix the correctness bug where globs/redirects depending on
expansion are dropped, and fix the over-flagging when named at root
coexists with a glob deep in an expansion.

**Files**: `resolve.rs` (`resolve_name`,
`resolve_path_to_symbol`), new helpers in `memmap/`.

**Tests** (new):
- "Glob whose target is created by macro expansion"
- "Redirect whose target is created by macro expansion"
- "Glob import statement itself comes from macro expansion"
- "Redirect import statement itself comes from macro expansion"
- "Glob target created by nested macro"
- "Unresolvable glob path is an error"
- "Named at root beats glob from expansion" (the over-flagging guard)
- "Same-level shadowing" (regression guard)

**Implement**:
1. Implement `flatten_collect` that walks every
   `Expansion::entries` recursively, collecting named vs glob.
2. For `Redirect`: resolve `target` at lookup time, check namespace
   of the resulting symbol dynamically.
3. For `Glob`: resolve `path` at lookup time to a module, then
   search children for `(name, ns)`. Unresolvable glob path →
   recorded as a validation error (but doesn't contribute a
   resolution candidate).
4. `resolve_name` implements the priority: named (global) → glob
   (global) → extern prelude → std prelude.

**Commit message**: `memmap: flatten tree in resolve_name, defer glob/redirect resolution to lookup time`

### Phase 3 — Construction-time resolver: fan-out and branches

**Goal**: when a macro invocation has multiple candidate defs, expand
all of them and record each as a branch.

**Files**: `memmap/resolve_path.rs`, `memmap/expand.rs`,
`memmap/validate.rs`.

**Tests** (new):
- "Cross-level macro resolution produces multiple branches"
- "Cross-level conflict on a name introduced by expansion (E0659)"

**Implement**:
1. Rename `resolve_memmap_path` → `resolve_path_ctime`; change
   return type to `Vec<Symbol>` (the general path resolver).
2. Extend `ModuleSource` with the `LocalInline { parent, mod_item }`
   variant. Update `module_items`, `module_memmap`,
   `sym_to_module`, and the ~30 other match sites to handle it
   (most follow the same treatment as `Local`). Add
   `Module::containing_file(db)` helper.
3. Implement the three-layer algorithm described above:
   `resolve_path_ctime` (Layer 1), `lookup_in_module_ctime`
   (Layer 2), `resolve_name_ctime` with per-node named > glob
   (Layer 3), plus `dispatch_first_segment`.
4. Add the `MacroCallee` enum with variants
   `Rules` / `Builtin(BuiltinMacroKind)` / `Proc`. Add the
   `BuiltinMacroKind` enum (derives + bang builtins). Implement
   `symbol_to_macro_callee(db, sym)` with tcx-backed
   classification and `builtin_introduces_names(kind)` driving
   `needs_expansion_for_memmap`.
5. Extend `TcxDb` with the queries needed by
   `symbol_to_macro_callee`:
   - `classify_builtin_macro(cn, di) -> Option<BuiltinMacroKind>`
     (replacing the current `is_builtin_derive` bool; broader
     since it also classifies bang builtins)
   - `is_proc_macro(cn, di) -> bool`
6. Extract `std_prelude_module(db) -> Option<Module<'db>>` as a
   salsa-memoised helper from the existing inline walk in
   `resolve_in_std_prelude`. Used both by the ctime dispatch and
   by the post-construction resolver's fallback.
7. Implement cycle detection: `Visited` set of
   `(Module, Path)` in-flight, plus `MAX_PATH_DEPTH = 128`.
8. Update `resolve_and_expand_macros` to: filter
   `resolve_path_ctime` results through `symbol_to_macro_callee`,
   use `needs_expansion_for_memmap` to pick between `Resolved` and
   `Expanded`, and populate the chosen variant.
9. Cap branch count, expansion depth, resolution depth, and
   per-expansion item count. On overflow, keep partial results and
   emit the corresponding validator error (`BranchOverflow`,
   `ExpansionDepthExceeded`, `ResolutionDepthExceeded`,
   `ExpansionItemsOverflow`). Never silently truncate.
10. `memmap_errors` reports `AmbiguousMacro` when `Expanded(vec)` has
    `vec.len() > 1` or `Resolved(callees)` has `callees.len() > 1`.
    Reports `UnresolvedRedirect` / `UnresolvedGlob` when a redirect
    or glob target yields no symbols after convergence.
11. `memmap_errors` classifies time-travel vs plain ambiguity for
    diagnostic quality.

Note: actually reaching `MacroUseState::Resolved` requires wiring
`#[derive(...)]` attributes through `seed_from_items` as
`MacroUse` entries in `Namespace::Macro(Derive)`. If that
seed-time plumbing is too big for this phase, split it out:
Phase 3 lands the classification + dispatch machinery, a
follow-up phase lands `#[derive]` seeding. The structural
placeholder still holds.

**Commit message**: `memmap: fan out macro resolution into Expansion branches`

### Phase 4 — expand_macro via file_item_tree

**Goal**: expanded items become real `Item` values, not `Item::Error`
placeholders. Pick one of the two candidate approaches from the
"open design" section above and implement it.

**Files**: `memmap/expand.rs`, `lower.rs`.

**Tests**: existing expansion tests produce real item types for
anything inside a macro expansion (signatures, display, etc. work).

**Implement**: decided in Phase 4 itself — try the pure-core
refactor of `file_item_tree` first; fall back to
salsa-tracked-on-`MacroDefItem` if that hits snags.

**Commit message**: `memmap: expand_macro reuses file_item_tree for real items`

### Phase 5 — Cleanup

**Goal**: remove dead types, tighten documentation.

**Files**: everywhere.

**Implement**:
1. Delete `NamedMember`, `NamedMemberKind`, `GlobStem` (should be
   unreferenced after Phase 1).
2. Remove `resolve_use_path_to_module` if no longer called.
3. Update `md/design/ir.md` module resolution section.
4. Update `md/design/arch.md` if the MEM-map gets a mention.
5. Delete WIP.md.

**Commit message**: `memmap: cleanup dead types and update design docs`

## Documentation updates

| Phase | Doc | Section |
|---|---|---|
| Phase 2 | `md/design/ir.md` | Module resolution (resolve_name semantics) |
| Phase 3 | `md/design/ir.md` | Module resolution (MacroUse branches, validation) |
| Phase 3 | `md/design/ir.md` | Modules (new `LocalInline` variant, containing_file helper) |
| Phase 3 | `md/design/arch.md` | TcxDb trait listing (`classify_builtin_macro`, `is_proc_macro`) |
| Phase 4 | `md/design/ir.md` | IR lowering (expand path shares file_item_tree) |
| Phase 5 | `md/design/ir.md` | Final pass; remove stale references |

## FAQ

**Why two resolvers instead of one?**
Because they answer different questions. Construction-time wants
every candidate because the tree is still growing and committing
would be premature. Post-construction wants a single answer because
the tree is done and callers downstream don't want to reason about
expansion order. Conflating them forces one of the two to accept
wrong semantics.

**Why Option 3 (branches) over Option 1 (flat siblings) for
ambiguous macro expansion?**
Option 1 loses the correlation between a name and the branch it
came from. If branch 0 produces `{X, Y}` and branch 1 produces
`{X, Z}`, `Y` and `Z` are conditional on which branch was taken,
but as flat siblings they look unconditional. Option 3 preserves
the grouping, which matters for diagnostics and for tools that
want to partially analyze around an ambiguous region.

**Does the post-construction resolver really flatten across all
branches?**
Yes. Named anywhere beats glob anywhere. A name introduced three
macro expansions deep wins over a root-level glob. If multiple
branches produce conflicting named bindings for the same name, that
*is* a real post-construction ambiguity — the program's meaning
genuinely depends on which branch was taken.

**What about cycle recovery in the fan-out case?**
`module_memmap` still uses salsa's cycle-recovery with
`cycle_initial = empty memmap`. Fan-out happens within a single
module's resolution; cross-module cycles are handled the same way
as today. Branch count is capped to prevent pathological blowup
from deeply nested ambiguous macros.

**Why is `MacroUseState` a three-variant enum instead of
`Option<Vec<Expansion>>`?**
Because there's a real third state: *resolved but not expanded*.
The canonical example is `#[derive(Debug)]` — we know which callee
`Debug` refers to (the builtin Debug derive), but the expansion
only produces anonymous impls, which don't contribute to the
MEM-map. With `Option<Vec<Expansion>>` you'd have to either run
expansion unnecessarily (creating empty entries) or abuse `None`
to mean both "not resolved yet" and "resolved but skipped". The
enum makes the distinction explicit and gives future work
(attribute macros, deferred bang-macro expansion, macros that
produce only impls) somewhere clean to live.

**Why `MacroCallee` with three variants instead of `MacroDefItem`?**
`MacroDefItem` is specifically a local `macro_rules!`. Derives
(builtin or proc-macro), bang builtins (like `println!`), and
attribute macros aren't `MacroDefItem`s but they are legitimate
macro targets that the MEM-map should be able to name.
`MacroCallee` covers all three cases (`Rules` / `Builtin` / `Proc`).

The split between `Builtin` and `Proc` matters because `Builtin`
carries a compile-time-known `BuiltinMacroKind`. That kind
determines name-introduction behaviour up-front:
- **Builtin derives** (Debug, Clone, Copy, etc.) always generate
  anonymous `impl` blocks; they can't introduce names. Go to
  `Resolved`.
- **Builtin bangs** vary. Some (`include!`, `thread_local!`)
  produce items at item position; most (`println!`, `vec!`,
  `dbg!`, …) are expression-level and shouldn't appear at item
  position anyway. Go to `Expanded` (expand and let the
  expansion either succeed or produce nothing).
- **Proc-macros** can emit arbitrary items and always go to
  `Expanded`.
- **Rules** likewise always go to `Expanded`.

Classifying at `Symbol → MacroCallee` conversion time (rather than
deferring to `expand_macro`) means we can pick the right
`MacroUseState` up-front.

Keeping `MacroCallee` separate from `Symbol` means `Symbol` stays
focused on "thing you can reference from code" while `MacroCallee`
narrows to "thing you can invoke as a macro".

**Are builtin macros resolved through the std prelude?**
Yes — the same way extern prelude names are. In standard Rust,
builtin macros (derives *and* bangs) live in `libcore` with
`#[rustc_builtin_macro]` attributes and are re-exported through
`core::prelude::v1` / `std::prelude::v1`. When we see `println!()`
or `#[derive(Debug)]`, construction-time resolution walks the std
prelude as a candidate starting module (in parallel with the
current module and extern prelude), finds the name there, and
then `tcx.classify_builtin_macro(cn, di)` identifies it as a
builtin with a specific `BuiltinMacroKind`. No special-case
lookup path for builtins — they flow through the same
`resolve_path_ctime` machinery as any other external-defined
macro.

**Does expand_macro need to know which branch it's expanding
under?**
No. `expand_macro(callee, input_tokens)` depends only on those two
inputs. The branch identity lives in the parent `Expansion {
callee, entries }` wrapper, not inside the expansion logic. Input
tokens come from the enclosing `MacroUse`, shared across all
branches — they're a property of the invocation site, not of any
candidate callee.

## What's NOT in scope

- Changing the fixpoint/cycle-recovery mechanism.
- **Actually matching `macro_rules!` patterns against input.** The
  data model now *carries* invocation input tokens through to
  `expand_macro(def, input_tokens)`, but the expansion logic still
  treats `def.body_tokens` as the verbatim expansion (noop
  behaviour). Real pattern matching and token substitution is
  future work; the data model just stops throwing input away.
- Changing how `file_item_tree` or `lower.rs` parses source (Phase 4
  may refactor but not rewrite).
- External module handling (already correct — `debug_assert`
  prevents `module_memmap` on externals).

## Implementation status

- [x] Phase 0 — Test harness
- [x] Phase 1 — Data model
- [x] Phase 2 — Post-construction resolver
- [ ] Phase 3 — Construction-time fan-out (with cycle detection)
- [ ] Phase 4 — expand_macro via file_item_tree
- [ ] Phase 5 — Cleanup

### Deviations from plan

(none yet)

### Phase 2 deviations

- Phase 2's new test cases involving glob/redirect **targets** that
  are macro-created inline modules (`mod foo { .. }` produced by
  macro expansion) are deferred to Phase 3. They fundamentally
  require `ModuleSource::LocalInline` so that `symbol_to_module`
  can return a Module for an inline mod; without it,
  `resolve_use_path_to_module_from_path` has no module to walk
  into.

  Phase 2 still delivers the core fix: deferred resolution of
  `Redirect { name, target }` and `Glob { path }` at lookup time
  via the target module's MEM-map (rather than `module_items`).
  This means the case "redirect/glob points to a *file-based*
  module whose contents were macro-expanded" works today.
  Inline-module targets land with LocalInline in Phase 3.

- Over-flagging of same-named items across expansion levels is
  already correctly handled by the Phase 1 + Phase 2 flattening
  logic (collect_named_matches returns exactly 1 when only one
  matching entry exists anywhere in the tree). Additional
  regression guards are in place.

### Open issues

- Phase 4 sub-design (pure core of `file_item_tree` vs
  salsa-tracked `expand_macro`) deferred to Phase 4 execution.
- Overflow cap *values* may need tuning after real-world profiling
  (`MAX_EXPANSION_DEPTH`, `MAX_PATH_DEPTH`,
  `MAX_BRANCHES_PER_USE`, `MAX_EXPANSION_ITEMS`). Behaviour is
  fixed — keep partial results, emit overflow diagnostic — just
  the numeric thresholds may move.
