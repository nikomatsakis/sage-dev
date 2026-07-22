# Language Subsetting

Sage intentionally supports a subset of Rust. This is a pragmatic choice — each
restriction eliminates significant implementation complexity while affecting
little real-world code. Restrictions are documented here with rationale and may
be lifted as sage matures.

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

## Body resolution restrictions

These are limitations of the current body resolver (`body_resolve.rs`),
not fundamental design choices. They'll be lifted as type inference and
impl resolution are added.

### Method calls stay unresolved

`receiver.method(args)` preserves the method `Name` but doesn't resolve
which impl provides it. Requires type inference to know the receiver type.

### Field access stays unresolved

`expr.field` preserves the field `Name`. Resolving to a specific struct
field requires knowing the expression's type.

### Enum variants need type-qualified paths

`Frame::Bulk` shows as `<unresolved>` because enum variants aren't
directly in the module's value namespace — they're children of the enum
type. Resolving `Type::Variant` requires looking up the type first, then
searching its variants. Not yet implemented.

### Associated functions need impl lookup

`Type::func()` — the type path resolves, but which `impl` block provides
`func` is unknown. No impl-block search infrastructure exists yet.

### Macro calls are not expanded

Macro paths are resolved to their definition (`<ext tracing::debug>`),
but the token tree is opaque. Paths inside macro arguments are not
resolved. `macro_rules!` expansion is the next major feature needed.

**Exception:** represented derive expansions produce ordinary sibling items
with generated-source provenance. The current builtin subset includes
`Clone` for non-generic named-field structs; other builtin input shapes remain
unsupported. The rustc bridge contains the proc-macro invocation mechanism,
but fully integrating proc-macro derive name resolution and output into the
module expansion pipeline remains work in progress. See `derive.rs`,
`local_syms/mods.rs`, and `proc_macro_srv.rs`.

Trait candidate discovery treats unresolved item macros and active attribute
macros conservatively as incomplete. An impl with an active attribute whose
transformation has not run is excluded from definite candidates, preventing a
disabled or replaced source impl, containing module, or macro expansion from
producing a false proof. Derives attached to an item are likewise withheld
while another active attribute on that item remains unexpanded. A uniquely
resolved item macro whose output parses
successfully remains complete. This is a soundness boundary, not a substitute
for completing macro expansion. Likewise, a `use` with an unrepresented active
attribute is excluded from name resolution; its pre-expansion target cannot
steer a definite impl candidate.

Unsupported item kinds (including unions) and malformed item syntax are
retained as explicit error items. Trait candidate discovery treats their
module as incomplete, so an attribute or derive attached to an unsupported
item cannot disappear and justify a ground negative result.

Lint-only inner attributes (`#![allow(...)]`, `warn`, `deny`, and `forbid`) are
known inert for this pipeline and do not make candidate discovery incomplete.
Other inner module attributes are conservatively incomplete until Sage
represents their effects.

### Type references in bodies pass through

`TypeRef` in let-bindings and casts passes through unchanged. Type path
resolution is deferred to type checking.

### Closure captures not tracked

The resolver doesn't track which variables a closure captures.
