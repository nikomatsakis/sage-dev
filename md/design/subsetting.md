# Language Subsetting

Sage intentionally supports a subset of Rust. This is a pragmatic choice — each
restriction eliminates significant implementation complexity while affecting
little real-world code. Restrictions are documented here with rationale and may
be lifted as sage matures.

<a id="sub-a1"></a>
> **SUB-A1 — A language restriction is explicit at its semantic boundary.** An
> intentionally unsupported Rust construct states what is excluded, why Sage
> excludes it, its user-visible impact, and the response at the responsible
> phase. That response is normally a diagnostic or conservative incomplete
> result; any temporary semantic approximation, such as `Lifetime::Dummy`, is
> identified explicitly as a soundness exception rather than silently
> presented as full Rust semantics.
>
> **Required verification:** Every listed restriction has focused acceptance
> and rejection fixtures at the phase which encounters it, and the observable
> diagnostic, incomplete result, or documented approximation agrees with the
> stated impact.

<a id="sub-a2"></a>
> **SUB-A2 — Destination restrictions and implementation gaps have different
> owners.** This page lists deliberate limits on Sage's language contract.
> Temporary gaps in otherwise supported Rust belong in the relevant phase or
> subsystem's Current Status and in cross-cutting roadmap slices, not in the
> destination restriction list.
>
> **Required verification:** Documentation review traces every unsupported
> fixture either to a deliberate restriction here or to a capability gap and
> roadmap owner, and rejects restrictions justified only by the implementation
> not having been written yet.

## Restrictions

### No proc-macro crates defined in the workspace

**What:** Sage does not support workspace crates with `proc-macro = true` in
their `[lib]` target. Proc-macro crates from external dependencies (e.g.,
`serde_derive`, `tokio_macros`, `clap_derive`) can be loaded by the rustc bridge,
but their name resolution and output are not yet fully integrated into Sage's
module expansion pipeline.

**Why:** A workspace-defined proc-macro requires compiling it to a host-side
dylib before it can be expanded. This means sage would need to invoke rustc (or
itself) to produce a working dylib, manage host vs target compilation, and
handle the case where the proc-macro crate is being edited (invalidating the
dylib). This is a significant amount of machinery for a feature that most
application-level workspaces don't use — proc-macros are typically published as
separate crates.

**Impact:** Low. Application workspaces (web services, CLI tools, libraries)
almost never define proc-macros inline. Projects that do (e.g., a proc-macro
crate + its consumer in one workspace) can still use sage for the consumer
crate — just not for the proc-macro crate itself.

### No glob imports targeting inline modules

**What:** `use some_inline_module::*` (where the target is `mod foo { ... }`,
not `mod foo;`) is not yet supported. Glob imports from file-based workspace
modules and from external dependencies work fine. Inline modules can still
be navigated as path targets (`mod foo { ... }; foo::Bar` resolves), and the
items inside an inline module participate in normal name resolution; only
glob-importing *into* a parent's scope from an inline module is broken.

**Why:** Glob target resolution feeds back into the construction-time path
walker. The walker takes a fast items-based path that doesn't traverse
inline-module bodies; until it does, globs that resolve through an inline
intermediate produce no entries.

**Impact:** Low. Inline modules with glob imports are uncommon in
application code, and the cross-module glob case (the primary fixpoint
trigger) goes through file-based modules in practice.

### No `#[path = "..."]` on modules

**What:** `#[path = "custom.rs"] mod foo;` is not supported. Module file
resolution assumes conventional paths (`foo.rs` or `foo/mod.rs`).

**Why:** Supporting `#[path]` requires parsing attributes on `mod` items during
module resolution, before the module's contents are known. The current
`resolve_mod` function only looks at the module name.

**Impact:** Very low. `#[path]` is rare in practice — almost all Rust code uses
conventional module paths.

### No derive helper attributes

**What:** Derive helper attributes introduced by proc-macro derives are not
resolved. For example, `#[derive(Serialize)]` introduces `#[serde(...)]`
helper attributes — sage does not recognize these.

**Why:** Derive helper attributes require knowing which attributes a proc-macro
derive registers, which means reading the proc-macro's registration metadata.
This is a separate mechanism from derive expansion itself.

**Impact:** Low for type checking — helper attributes affect the derive
expansion output but don't change the type structure. Sage will report unknown
attribute warnings on helper attributes.

### `macro_rules!` scoping is module-scoped

**What:** `macro_rules!` definitions are visible throughout their containing
module, not just after the definition point. In real Rust, `macro_rules!` uses
textual scoping — a macro is only visible to code that appears after it in the
source file.

**Why:** Textual scoping requires tracking source position during name
resolution, which adds complexity to the resolution algorithm. Module-scoped
visibility is simpler and correct for the vast majority of code.

**Impact:** Very low. Code that depends on textual scoping (e.g., defining a
macro between two items where only the second should see it) is extremely rare
in practice.

### Lifetime semantics and borrow checking are deferred

**What:** Sage preserves lifetime syntax in the CST but maps every lifetime to
the dedicated checked value `Lifetime::Dummy`. All lifetime relations hold
trivially, and Sage does not perform borrow checking.

**Why:** The intended lifetime and type inference design is more unified than
rustc's separate region subsystem. Introducing a temporary region solver would
prematurely constrain that design. `Dummy` keeps reference structure available
to ordinary type elaboration without pretending to be `'static` or a solved
region.

**Impact:** Sage currently accepts some programs which rustc rejects for
lifetime or borrow errors. This is a documented temporary soundness hole, not
an ambiguous type-checking result.

Outside that explicit lifetime exception, unsupported or unrepresented source
is governed by
[D16](./decisions.md#d16-incompleteness-is-an-explicit-terminal-outcome).

<a id="sub-a3"></a>
> **SUB-A3 — Unrepresented source never becomes negative evidence.** Published
> symbols from an incomplete phase may support their own positive facts, but
> omitted or unsupported source cannot justify a false proof, failed lookup,
> or exhaustive ground `No` in a downstream consumer.
>
> **Required verification:** Malformed, unresolved, unsupported-attribute, and
> resource-limited fixtures retain usable represented symbols while each
> potentially affected resolution or solver-negative query remains explicitly
> incomplete or ambiguous.

## Supported features

The destination language includes the following. This list is not a statement
that every path is implemented today; current implementation limitations are
recorded below and in the build-out roadmap.

- async/await
- Trait definitions and implementations
- Generics, lifetime syntax, where clauses (with lifetime semantics deferred)
- Pattern matching
- Closures and `impl Fn` / `dyn Fn`
- Derive macros (external integration in progress)
- Proc-macro attributes, e.g. `#[tokio::test]` (external integration in progress)
- `macro_rules!` definitions and invocations within the workspace
  (module-scoped — see restriction above)
- Module tree (`mod`, `pub use`, `pub(crate)`)
- Type aliases, constants, statics
- `cfg` attributes

## Current status

### Current frontier and evidence

The destination feature list above is not an implementation-coverage list.
Current coverage is driven by reviewed vertical slices and recorded locally in
the architecture guide. These links establish [SUB-A2](#sub-a2)'s current ownership
split for the named phases:

- [Module and Macro Expansion](./pipeline/module-expansion.md#current-status)
  records the empty-input local `macro_rules!` subset, built-in derives,
  external bang expansion, active-attribute incompleteness, and depth limit.
- [Name Resolution](./subsystems/name-resolution.md#current-status) records
  represented path/import/prelude behavior and associated lookup limitations.
- [Signature Checking](./pipeline/signature-checking.md#current-status) records
  supported interface forms and the `Dummy` lifetime boundary.
- [Body Checking](./pipeline/body-checking.md#current-status) records the
  implemented expression, method, inference, and obligation frontier.
- [Typed IR](./typed-ir.md#current-status) records which source constructs are
  fully elaborated rather than merely parsed or retained for recovery.

Unsupported or malformed items remain explicit error items. Incomplete item
or attribute expansion is conservatively visible to name resolution and trait
candidate discovery, so unrepresented code cannot justify a false proof or
ground negative result. Lint-only inner attributes are treated as inert; other
unrepresented active attributes remain incomplete. The focused tests
`unresolved_item_macro_prevents_ground_no`,
`malformed_item_macro_output_prevents_ground_no_without_panicking`, and
`recursive_item_macro_hits_expansion_limit_and_returns_maybe` establish the
unresolved, malformed, and resource-limited expansion portion of
[SUB-A3](#sub-a3).

### Current limitations

- **Known deviation [KD-3](../implementation/known-deviations.md#kd-3-derive-helper-attributes-do-not-produce-the-promised-warning):**
  registered derive helper attributes are not distinguished from arbitrary
  potentially active attributes, so Sage omits the affected item
  conservatively instead of retaining it and reporting the promised warning.
- The restriction list does not yet link focused acceptance, rejection, or
  approximation fixtures for every entry, so SUB-A1 is not established as a
  complete matrix.
- Several entries are phrased as “not yet supported.” They still need an audit
  to determine whether they are deliberate destination restrictions or
  temporary phase gaps before SUB-A2 is fully established.
- SUB-A3 has focused expansion-to-solver evidence, but not yet the complete
  downstream resolution and solver matrix for every incomplete source class.

### Related roadmap slices

The [Build-Out Roadmap](../implementation/roadmap.md) orders the next
cross-cutting coverage slices. This page owns only intentional language
restrictions; phase Current Status sections own temporary implementation gaps.
