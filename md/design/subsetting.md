# Language Subsetting

Sage intentionally supports a subset of Rust. This is a pragmatic choice — each
restriction eliminates significant implementation complexity while affecting
little real-world code. Restrictions are documented here with rationale and may
be lifted as sage matures.

## Restrictions

### No proc-macro crates defined in the workspace

**What:** Sage does not support workspace crates with `proc-macro = true` in
their `[lib]` target. Proc-macro crates from external dependencies (e.g.,
`serde_derive`, `tokio_macros`, `clap_derive`) are fully supported — sage calls
them via `proc_macro::bridge` like any other consumer.

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

### No glob imports from workspace modules

**What:** `use some_workspace_module::*` is not supported for inline modules
(`mod foo { ... }`). Glob imports from file-based workspace modules
(`mod foo;` → `foo.rs`) and external dependencies work fine.

**Why:** Glob imports between workspace modules are the primary source of
fixed-point iteration in name resolution. Without them, every `use` import
names exactly what it brings in, and name resolution becomes a single
deterministic pass. With them, you need to iterate: resolve module A's exports
to know what B's glob imports, but A might glob from C, which might glob from
B...

Glob imports from external deps don't have this problem — the dep snapshot
already has complete, fully-resolved module contents. Looking up `std::io::*`
is a single query into the `TyCtxt` snapshot. File-based workspace modules
are similarly straightforward since their items are discovered via
`file_item_tree`.

**Impact:** Low. Inline modules with glob imports are uncommon. The most common
glob pattern is `use MyEnum::*` inside a function body (enum variant shorthand)
— this is a *local* glob, not a cross-module one, and sage can support it
trivially.

**Future:** This restriction can be lifted by implementing `symbol_to_module`
for inline modules — creating a `Module` backed by the inline module's item
list rather than a `SourceFile`.

### No inline module resolution

**What:** `mod foo { ... }` (inline modules) cannot be resolved as path
targets. `mod foo;` (file-based modules) work fine.

**Why:** `symbol_to_module` currently only handles file-based modules (looks up
`foo.rs` or `foo/mod.rs`) and external modules. Inline modules would need a
`ModuleSource` variant backed by the inline item list rather than a
`SourceFile`.

**Impact:** Low. Inline modules are uncommon in application code. Most modules
use `mod foo;` with a separate file.

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

## Supported features

Everything not listed above is intended to be supported, including:

- async/await
- Trait definitions and implementations
- Generics, lifetimes, where clauses
- Pattern matching
- Closures and `impl Fn` / `dyn Fn`
- Derive macros (from external crates)
- Proc-macro attributes (from external crates), e.g. `#[tokio::test]`
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

**Exception:** Derive macros from external crates ARE expanded. Builtin
derives (`Debug`, `Clone`, etc.) produce synthetic IR. Proc-macro
derives (e.g., `Parser`, `Subcommand`) are invoked via
`proc_macro::bridge` and the expanded source is lowered through
tree-sitter into `Vec<Item>`. See `derive.rs` and `proc_macro_srv.rs`.

### Type references in bodies pass through

`TypeRef` in let-bindings and casts passes through unchanged. Type path
resolution is deferred to type checking.

### Closure captures not tracked

The resolver doesn't track which variables a closure captures.
