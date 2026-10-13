# Language Subsetting

Sage intentionally supports a subset of Rust. This is a pragmatic choice — each
restriction eliminates significant implementation complexity while affecting
little real-world code. Restrictions are documented here with rationale and may
be lifted as sage matures.

{spec}`crates.sage_ir.crate.explicit_restrictions`

Verification scenarios: Every listed restriction has focused acceptance
and rejection fixtures at the phase which encounters it, and the observable
diagnostic, incomplete result, or documented approximation agrees with the
stated impact.

{spec}`crates.sage_ir.crate.restrictions_and_gaps`

Verification scenarios: Documentation review traces every unsupported
fixture either to a deliberate restriction here or to a capability gap and
roadmap owner, and rejects restrictions justified only by the implementation
not having been written yet.

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
[Incompleteness is an explicit terminal outcome](../rfds/architectural-foundations/README.md#incompleteness-is-an-explicit-terminal-outcome).

{spec}`crates.sage_ir.crate.represented_completeness`

Verification scenarios: Malformed, unresolved, unsupported-attribute, and
resource-limited fixtures retain usable represented symbols while each
potentially affected resolution or solver-negative query remains explicitly
incomplete or ambiguous.

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

## Implementation and test evidence

The generated [anchor pages](anchor/crates/sage_ir/crate.html) report implementation entries,
annotated tests, explicit TODO/N/A gaps, and saved claim/test reviews. These
associations do not themselves establish that a test ran.

The [earlier coverage snapshot](../rfds/implementation-sequencing/coverage-history.md#language-subsetting)
preserves the former capability descriptions, edit experiments, and limitations
as historical evidence. Current coverage comes from the generated entries;
implementation plans and decision rationale belong in the relevant RFDs.
