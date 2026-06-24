# RFD: Diagnostics Rendering

**Status:** Proposed

**Depends on:**
- [Type Inference](./type-inference.md) — diagnostic collection during type checking

**Related:**
- [Error Sentinel Type](./error-sentinel-type.md) — `ErrorReported` witness that a diagnostic was emitted

## Problem

The current diagnostic rendering is a quick `fmt_ty` function inside `check/body.rs` that produces strings like `` type mismatch: expected `u32`, found `bool` ``. This is adequate for tests but insufficient for a real compiler:

- No span information (which expression caused the error?)
- No context (what was the expected type and why?)
- Type display is ad-hoc and not reusable outside the checker
- No structured output for IDE integration
- Diagnostics are flattened to `Vec<String>` in `CheckedBody`, losing all structure

## Goals

1. **`Ty::display(db, stash)` method** — a reusable `Display` impl for types, usable anywhere (diagnostics, hover info, debugging). Should handle inference variables (show resolved type after `find`), generic params (show name), and ADTs (show name + type args).

2. **Structured diagnostics** — diagnostics carry spans, severity, structured data (expected/actual types as typed values, not pre-rendered strings). Rendering to text is a separate final step.

3. **Rendering** — use an existing crate (`annotate_snippets` or `codespan-reporting`) for pretty terminal output with labeled source snippets, rather than rolling our own.

## Non-goals (for now)

- Colored terminal output (comes free from the rendering crate, but not a driver of design decisions)
- LSP diagnostic protocol integration
- Fix suggestions / quick-fix actions

## Current State

### Diagnostic struct (`sage-ir/src/check/body.rs`)

```rust
pub struct Diagnostic<'db> {
    pub kind: DiagnosticKind<'db>,
}

pub enum DiagnosticKind<'db> {
    TypeMismatch { expected: Ptr<Ty<'db>>, actual: Ptr<Ty<'db>> },
    UnresolvedInferVar { var: InferVarIndex },
    AmbiguousName { count: usize },
}
```

No spans. Types are stash pointers — only meaningfully readable during the checker lifetime.

### Collection

Diagnostics are pushed to a `Vec<Diagnostic>` on `BodyCheck`. On `finish()`, they're rendered to `Vec<String>` and stored in `CheckedBody`. The structural information is lost at that point.

### Rendering (`fmt_ty`)

A standalone function that pattern-matches on `Ty` variants. Works, but:
- Not reusable outside body checking (requires a `&Stash` reference)
- Doesn't resolve inference variables through the egraph
- Can't be used for hover info, signature display, etc.

### Unused scaffold (`sage-ir/src/diagnostics.rs`)

```rust
pub struct SageDiagnostic {
    pub severity: Severity,
    pub message: String,
    pub offset: u32,
}
```

Defined but never used. Too flat — single offset, no labels or sub-spans.

## Design: Diagnostic Struct

Replace the current internal-only `Diagnostic` with a structured type that carries labeled context and sub-diagnostics:

```rust
/// A single diagnostic emitted by sage.
/// Each span is paired with the symbol it's relative to — resolution to
/// absolute file positions happens only at the rendering boundary.
pub struct Diagnostic<'db> {
    pub severity: Severity,
    pub span: Span<'db>,                // primary location
    pub message: String,                // headline, e.g. "type mismatch"
    pub labels: Vec<Label<'db>>,        // labeled sub-spans for context
    pub notes: Vec<Diagnostic<'db>>,    // sub-diagnostics (wrapped originals, explanatory notes)
}

/// A source location, resolvable to an absolute file position at rendering time.
pub enum Span<'db> {
    /// A byte range within a symbol's body (e.g., an expression inside a function).
    /// `LocalSymbol` is a placeholder — concretely `LocalModItemSym<'db>` in the codebase.
    Relative(LocalSymbol<'db>, RelativeSpan),

    /// The entire span of a symbol (e.g., "this function" or "this struct").
    Symbol(LocalSymbol<'db>),

    /// An already-absolute span. Use sparingly — only outside tracked functions
    /// or when the span doesn't belong to any symbol (e.g., top-level parse errors).
    Absolute(AbsoluteSpan<'db>),
}

pub struct Label<'db> {
    pub span: Span<'db>,
    pub message: String,                // e.g. "expected `u32`" or "found `bool`"
    pub style: LabelStyle,
}

pub enum LabelStyle { Primary, Secondary }

pub enum Severity { Error, Warning }
```

### Why an enum instead of just `AbsoluteSpan`

Diagnostics are produced inside Salsa tracked functions (`LocalFnSym::body()`). If they stored `AbsoluteSpan` directly, the result would depend on the item's file position — add a blank line above the function and the entire body re-checks. The `Relative` and `Symbol` variants reference interned Salsa identities, which are position-independent.

The `Absolute` variant exists for edge cases outside tracked functions (e.g., top-level parse errors, or diagnostics emitted at the rendering boundary itself). Inside tracked functions, prefer `Relative` or `Symbol`.

The enum is extensible — future variants might include things like `MacroExpansion(...)` for pointing into generated code.

Resolution to absolute happens at the rendering boundary:

```rust
impl<'db> Span<'db> {
    pub fn resolve(&self, db: &'db dyn Db) -> AbsoluteSpan<'db> {
        match self {
            Span::Relative(sym, rel) => sym.absolute_span(db).resolve(*rel),
            Span::Symbol(sym) => sym.absolute_span(db),
            Span::Absolute(abs) => *abs,
        }
    }
}
```

The `notes` field holds sub-diagnostics — these are originals that were "wrapped" during propagation, or explanatory context added along the way. See [Error Propagation and Reporting](#design-error-propagation-and-reporting) for how these get built up.

**Key invariant:** `Diagnostic` contains only rendered `String` messages and `Span<'db>` locations — never `Ptr<Ty>` or other stash pointers. Type names are rendered to strings at `to_diagnostic()` time (while the stash is live). This makes `Diagnostic` self-contained and safe to store in `CheckedBody` after the stash moves.

`Diagnostic<'db>` must be `Clone + PartialEq + Eq + Hash` (required by `CheckedBody`'s `salsa::Update` impl).

### `ErrorReported` — the return value of reporting

Reporting a diagnostic returns an `ErrorReported` sentinel (see [error-sentinel-type RFD](./error-sentinel-type.md)). This is the *only* way to construct `Ty::Error`, `Res::Error`, and similar error variants — making it a compile error to introduce an error type without first emitting a user-visible message.

```rust
/// Witness that at least one diagnostic has been emitted.
/// Only constructible by the diagnostic-reporting machinery.
/// Must remain zero-sized — Ty::Error(ErrorReported) and Res::Error(ErrorReported)
/// are stash-allocated, so this cannot grow without changing type layout.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ErrorReported(());
```

The diagnostic infrastructure is the sole mint for this token. Propagation (matching an existing `Ty::Error(e)` and threading `e` forward) doesn't require re-reporting — you already hold the witness.

### Example diagnostic

For `fn bad(x: u32) -> bool { x }`:

```
error: type mismatch
  --> src/main.sage:1:27
   |
 1 | fn bad(x: u32) -> bool { x }
   |                    ----   ^ found `u32`
   |                    |
   |                    expected `bool` because of return type
```

Represented as:
```rust
Diagnostic {
    severity: Error,
    span: span_of_expr_x,           // primary: the expression `x` (relative)
    message: "type mismatch",
    labels: vec![
        Label { span: span_of_expr_x, message: "found `u32`", style: Primary },
        Label { span: span_of_return_ty, message: "expected `bool` because of return type", style: Secondary },
    ],
}
```

### Builder API

```rust
impl<'db> Diagnostic<'db> {
    pub fn error(span: Span<'db>, message: impl Into<String>) -> Self { ... }
    pub fn warning(span: Span<'db>, message: impl Into<String>) -> Self { ... }

    pub fn label(mut self, span: Span<'db>, message: impl Into<String>) -> Self { ... }
    pub fn secondary(mut self, span: Span<'db>, message: impl Into<String>) -> Self { ... }
}
```

`Diagnostic` is constructed at **catch points** by calling `to_diagnostic()` on the structured error value (which carries raw `Ptr<Ty>` etc.). It is NOT the type that flows through `Result` — that role belongs to the structured error types (`TypeError`). See [Error Propagation and Reporting](#design-error-propagation-and-reporting) below for the full two-level model.

## Design: Type Display

Extract a reusable type-display utility that works outside the checker context.

### Interface

```rust
/// Wrapper that implements `Display` for a type.
pub struct TyDisplay<'a, 'db> {
    db: &'db dyn crate::Db,
    stash: &'a Stash,
    ty: Ptr<Ty<'db>>,
}

impl fmt::Display for TyDisplay<'_, '_> { ... }
```

Usage: `format!("{}", ty.display(db, stash))` anywhere — diagnostics, hover info, debug logging.

### Behavior

- Primitives: `u32`, `bool`, `str`, etc.
- Generic params: show the declared name from `Param::name(db)`
- ADTs: `Vec<u32>`, `Option<T>` (qualified path only when ambiguous — future)
- Tuples: `(u32, bool)`
- References: `&u32`, `&mut String`
- Fn pointers: `fn(u32, u32) -> bool`
- Inference variables: `_` (user-facing) or `?0` (internal/debug mode)
- Error: `{error}` (internal sentinel, shouldn't normally surface)

### Resolution of inference variables

The display layer should operate on *resolved* types — after the egraph's `find()` has been applied. Two options:

1. **Resolve before display** — walk the type and substitute each `InferVar` with its resolved type before displaying. This is what `finish()` already does when building the final `TyBody`.
2. **Resolve during display** — pass the egraph/version into `TyDisplay` and call `find()` on the fly.

Option 1 is simpler and aligns with the current flow: diagnostics are rendered after finalization, at which point inference variables have been resolved. We should go with (1) initially and support (2) only if needed for mid-inference debugging.

## Design: Error Propagation and Reporting

### Two-level model

There are two distinct types in play:

1. **Error types** — structured Rust error values that flow through `Result` during checking. They carry raw data (`Ptr<Ty>`, spans, inner errors) and can be composed/wrapped like normal Rust errors. They hold references into the stash (which is still live during checking).

2. **`Diagnostic`** — the rendered, self-contained output type stored in `CheckedBody`. Created by calling `.to_diagnostic(db, stash)` on an error value at a catch point. Contains pre-rendered `String` messages and anchored `Span<'db>` locations. Does NOT hold `Ptr<Ty>` or any stash references.

This separation means:
- Error values are cheap to construct and compose (no string formatting during propagation)
- Type display happens once, at the catch point, while the stash is still live
- `Diagnostic` stored in `CheckedBody` is fully self-contained — no dangling stash pointers after `finish()` moves the stash

### Error types

Error types are plain Rust enums/structs — potentially using `thiserror` — that describe what went wrong in structured terms:

```rust
#[derive(Debug)]
pub enum TypeError<'db> {
    Mismatch {
        expected: Ptr<Ty<'db>>,
        actual: Ptr<Ty<'db>>,
        span: RelativeSpan,
    },
    UnresolvedName {
        span: RelativeSpan,
    },
    // ... other variants
}
```

Error types can carry inner errors for composition:

```rust
#[derive(Debug)]
pub struct ContextualError<'db> {
    pub inner: Box<TypeError<'db>>,
    pub context: ErrorContext<'db>,
}

pub enum ErrorContext<'db> {
    ReturnType { ret_span: RelativeSpan },
    Argument { index: usize, call_span: RelativeSpan },
    FieldInit { field_name: Name<'db>, struct_span: RelativeSpan },
    // ...
}
```

Or more simply, using a single error type with an optional `source` (tree-shaped, like standard Rust error chains):

```rust
#[derive(Debug)]
pub struct TypeError<'db> {
    pub kind: TypeErrorKind<'db>,
    pub span: RelativeSpan,
    pub source: Option<Box<TypeError<'db>>>,  // inner/wrapped error
}

pub enum TypeErrorKind<'db> {
    Mismatch { expected: Ptr<Ty<'db>>, actual: Ptr<Ty<'db>> },
    UnresolvedName,
    InvalidFieldInit { field: Name<'db> },
    ArgTypeMismatch { index: usize },
    // ...
}
```

The exact shape is an implementation detail — the key properties are:
- Carries `Ptr<Ty>` (not rendered strings)
- Has a tree structure (source/inner errors)
- Lightweight to construct during checking

### `to_diagnostic` — the rendering boundary

At catch points, error values are converted to `Diagnostic` while the stash is still accessible:

```rust
impl<'db> TypeError<'db> {
    pub fn to_diagnostic(&self, cx: &BodyCheck<'_, 'db>) -> Diagnostic<'db> {
        let db = cx.db;
        let stash = cx.stash();
        match &self.kind {
            TypeErrorKind::Mismatch { expected, actual } => {
                let mut diag = Diagnostic::error(
                    cx.span(self.span),
                    "type mismatch",
                )
                .label(cx.span(self.span), format!(
                    "found `{}`", actual.display(db, stash)
                ));
                // If there's context (source error), add it
                if let Some(source) = &self.source {
                    diag = diag.note(source.to_diagnostic(cx));
                }
                diag
            }
            // ... other variants
        }
    }
}
```

### `Result`-based propagation

Low-level operations return `Result<T, TypeError<'db>>`. Context accumulates on the way up via wrapping:

```rust
// Lowest level: returns a raw TypeError with the types involved
fn require_eq(&mut self, a: Ptr<Ty<'db>>, b: Ptr<Ty<'db>>, span: RelativeSpan)
    -> Result<(), TypeError<'db>>
{
    // ... decompose, compare skeletons ...
    Err(TypeError { kind: TypeErrorKind::Mismatch { expected: b, actual: a }, span, source: None })
}
```

```rust
// Mid-level: wraps with context
fn check_return(&mut self, expr: &Expr, ret_ty: Ptr<Ty<'db>>) -> Result<(), TypeError<'db>> {
    let actual = self.infer_expr(expr)?;
    self.require_eq(actual, ret_ty, expr.span)
        .map_err(|e| e.with_context(ErrorContext::ReturnType { ret_span }))?;
    Ok(())
}
```

```rust
// Catch point: converts to Diagnostic, reports, soldiers on
fn check_expr(&mut self, expr: &Expr, expected: Ptr<Ty<'db>>) -> Ptr<TyExpr<'db>> {
    match self.check_expr_inner(expr, expected) {
        Ok(te) => te,
        Err(type_err) => {
            let diag = type_err.to_diagnostic(self);
            let e = self.report(diag);
            self.alloc_error_expr(e, expr.span)
        }
    }
}
```

### What happens at each `?` — a taxonomy

The principle is: **never discard context**. Wrapping preserves the original as a `source`.

#### 1. Pass through unchanged

```rust
let ty = self.resolve_path(path)?;
```

#### 2. Add context (wrapping with source)

```rust
self.require_eq(actual, expected, span)
    .map_err(|e| e.with_context(ErrorContext::Argument { index: 2, call_span }))?;
```

The `with_context` method wraps the error, keeping the original accessible via `.source`.

#### 3. Replace the top-level kind

```rust
self.require_eq(actual, field_ty, init_span)
    .map_err(|e| TypeError {
        kind: TypeErrorKind::InvalidFieldInit { field: name },
        span: init_span,
        source: Some(Box::new(e)),
    })?;
```

The `to_diagnostic` impl for `InvalidFieldInit` renders the high-level message ("invalid struct field initializer") and includes the source error as a note:

```
error: invalid struct field initializer
  --> src/main.sage:5:12
   |
 5 |     Point { x: "hello" }
   |             ^^^^^^^^^^^ 
   |
   = note: mismatched types: expected `u32`, found `&str`
```

### Recursive `require_eq` and multiple child failures

`require_eq` recurses into children when skeletons match. Multiple children could fail. We propagate the **first** failure and suppress the rest:

```rust
for (ca, cb) in da.children.iter().zip(db_decomposed.children.iter()) {
    self.require_eq(*ca, *cb, span)?;  // first failure short-circuits
}
```

This is fine because the top-level type mismatch (e.g., `Foo<u32, bool>` vs `Foo<u32, u32>`) is typically more useful to the user than listing each differing child. The catch point sees the innermost structural mismatch and the context chain explains why it matters.

### Never discard

We never throw away a propagated error. Even when the top-level kind is replaced, the original lives on as `.source`. The `to_diagnostic` implementation recursively renders the source chain into sub-diagnostic notes. The rendering layer decides visibility: verbose mode can show the full chain, normal mode may elide internal-only notes.

### Context helpers — applying a transform to a block

```rust
fn with_context<'db, T>(
    context: ErrorContext<'db>,
    body: impl FnOnce() -> Result<T, TypeError<'db>>,
) -> Result<T, TypeError<'db>> {
    body().map_err(|e| e.with_context(context))
}
```

Usage:

```rust
with_context(
    ErrorContext::FieldInit { field: name, struct_span },
    || {
        let field_ty = self.resolve_field(name)?;
        let init_ty = self.infer_expr(init)?;
        self.require_eq(init_ty, field_ty, init_span)?;
        Ok(...)
    },
)
```

The exact signature and naming are implementation details — the key insight is that this is just `Result::map_err` applied to a block.

### Catch points — where `ErrorReported` is minted

A catch point is anywhere the code can manufacture a meaningful fallback value and continue checking. At a catch point, the error is converted to a `Diagnostic` (rendering types to strings while the stash is live), then reported.

| Level | Catches | Fallback |
|-------|---------|----------|
| Expression | type mismatch, unresolved name | `Ty::Error(e)` — continue checking siblings |
| Statement | invalid assignment target | skip statement, continue block |
| Item | signature error | skip body checking entirely |
| Module | parse error | skip item |

```rust
impl<'a, 'db> BodyCheck<'a, 'db> {
    pub fn report(&mut self, diag: Diagnostic<'db>) -> ErrorReported {
        self.diagnostics.push(diag);
        ErrorReported(())
    }

    /// Catch a TypeError: convert to Diagnostic (rendering types now) and report.
    pub fn catch(&mut self, err: TypeError<'db>) -> ErrorReported {
        let diag = err.to_diagnostic(self);
        self.report(diag)
    }
}
```

### Multiple independent errors

When checking N independent things (e.g., all arguments to a function call), each is its own catch point — you try each independently, report each failure, and keep going:

```rust
for (param, arg) in params.iter().zip(args) {
    if let Err(type_err) = self.check_expr_against(arg, param.ty) {
        self.catch(type_err);
        // don't short-circuit — check remaining args too
    }
}
```

No need for a "set" type — each error is reported at its own catch site. The diagnostics vec accumulates them naturally.

### Propagation through already-errored values

When code encounters a `Ty::Error(e)` from upstream, it propagates without re-reporting — the witness already exists:

```rust
match res {
    Res::Error(e) => return self.alloc_ty(Ty::Error(e)),
    // ...
}
```

No new diagnostic is emitted. The original error was already reported at whatever catch point created it.

### Storage

Two viable approaches for where reported diagnostics end up:

**A. `Vec<Diagnostic>` on `BodyCheck`** (initial)

Diagnostics are collected during the body walk via `report()`. Extracted on `finish()` and stored in `CheckedBody`. Simple, no Salsa machinery needed.

**B. Salsa accumulator** (future)

`report()` calls `diag.accumulate(db)` instead of pushing to a vec. Better when diagnostics are emitted across multiple tracked functions (resolution, macro expansion, type checking). Either way, `report()` still returns `ErrorReported`.

```rust
pub struct CheckedBody<'db> {
    pub body: TyBody<'db>,
    pub diagnostics: Vec<Diagnostic<'db>>,
}
```

Rendering to human-readable text happens at the call site (test harness, CLI driver), not inside the IR.

### Diagnostics live on `Check`, not just `BodyCheck`

The diagnostic infrastructure (diagnostics vec, `current_sym`, `report()`, `catch()`) lives on `Check` — the base struct that both signature checking and body checking use. `BodyCheck` derefs to `Check` and inherits these methods.

This means signature-level errors (e.g., unresolved type in a function parameter) can also report diagnostics and mint `ErrorReported` — no special-casing needed. The `LocalFnSym::sig()` tracked function returns diagnostics alongside the signature, same as `body()` does for the body.

## Design: Rendering

### Crate choice: `annotate_snippets`

[`annotate_snippets`](https://docs.rs/annotate_snippets) is the same crate `rustc` uses for error rendering. It's maintained, handles multi-line spans, multiple labels per snippet, and produces output familiar to Rust developers:

```
error: type mismatch
  --> src/main.sage:1:27
   |
 1 | fn bad(x: u32) -> bool { x }
   |                    ----   ^ found `u32`
   |                    |
   |                    expected `bool` because of return type
```

### Rendering pipeline

```
Diagnostic<'db>  →  resolve anchored spans via db  →  (file, line, col)  →  annotate_snippets::Message  →  String
```

The conversion from `Diagnostic` to `annotate_snippets` types lives in a thin rendering module (e.g. `sage-diagnostics` crate or a `render` module in `sage-ir`). The test harness can bypass this and just inspect the structured diagnostics directly.

### Test expectations

Tests should continue to assert on rendered strings for readability, but the rendering step is now explicit:

```rust
TestCrate::in_memory("fn bad(x: u32) -> bool { x }")
    .check_errors(expect![[r#"
        error: type mismatch
          --> input.sage:1:27
           |
         1 | fn bad(x: u32) -> bool { x }
           |                           ^ found `u32`
           |
    "#]]);
```

Or for simpler assertions during early development, a one-line mode that skips the snippet:

```rust
    .check_errors_short(expect![[r#"type mismatch: expected `bool`, found `u32`"#]]);
```

## Design: Span Handling and Rendering Boundary

### Span variants in diagnostics

`Span<'db>` is an enum — each variant represents a different way to identify a source location:

- `Relative(LocalSymbol, RelativeSpan)` — a byte range within a symbol's body. Most common during body checking.
- `Symbol(LocalSymbol)` — the full span of a symbol. Useful for "this function" or "this struct field" labels.
- `Absolute(AbsoluteSpan)` — a pre-resolved position. Only for use outside tracked functions.

Since `LocalSymbol` is a Salsa-tracked struct (interned identity), the `Relative` and `Symbol` variants don't depend on file position. The diagnostic result is stable under edits elsewhere in the file — incrementality is preserved.

During body checking, `BodyCheck` knows which symbol it's checking. A convenience method builds spans anchored to the current item:

```rust
impl<'a, 'db> BodyCheck<'a, 'db> {
    pub fn span(&self, relative: RelativeSpan) -> Span<'db> {
        Span::Relative(self.current_sym, relative)
    }
}
```

For cross-item labels (e.g., pointing at a return type annotation, a field definition, or a trait bound), use `Span::Relative(other_sym, rel)` or `Span::Symbol(other_sym)` to point at the relevant item.

### Resolution to absolute — only at the rendering boundary

The rendering boundary is outside the Salsa tracked function. Resolution calls `absolute_span(db)` on the anchor:

```rust
// Outside the tracked function — e.g., test harness or CLI driver
let checked = fn_sym.body(db);

for diag in &checked.diagnostics {
    let rendered = render(db, diag);  // each span resolves itself via anchor
    println!("{rendered}");
}
```

This means `BodyCheck` does NOT need an `AbsoluteSpan` field. It stores the `LocalSymbol` for anchoring, which is already known.

## Implementation Plan

Each phase is driven by **snapshot tests** (`expect_test`). The workflow:

1. Write the test with `expect![[""]]` — it fails (red).
2. Implement the feature.
3. Run with `UPDATE_EXPECT=1` — the snapshot fills in with the actual output (green).
4. Review the git diff of the snapshot — that IS the test.
5. Commit — the snapshot becomes the regression guard.

Subsequent phases refine the rendering. Existing snapshots from earlier phases get updated via `UPDATE_EXPECT=1` as the format improves — the diff shows exactly what changed.

### Phase 1: Structured diagnostics with spans

**Status: DONE**

**Snapshot test:**
```rust
#[test]
fn error_has_span() {
    TestCrate::in_memory("fn bad(x: u32) -> bool { x }")
        .check_errors(expect![[""]]);  // starts empty → red
}
```

After implementing, `UPDATE_EXPECT=1` fills in:
```rust
        .check_errors(expect![[r#"
            error at 23..28: type mismatch: expected `bool`, found `u32`
        "#]]);
```

Today this same test would produce `"type mismatch: expected `bool`, found `u32`"` with no location. The existing `return_type_mismatch` test's snapshot also updates — it gains a span prefix. The diff shows the improvement.

**Work done:**
- Defined new `Diagnostic<'db>`, `Span<'db>` enum, `Label<'db>`, `Severity`, `ErrorReported` in `sage-ir/src/diagnostic.rs`
- Defined `TypeError<'db>` error type with `to_diagnostic(&self, cx: &BodyCheck) -> Diagnostic<'db>` (lives in `check/body.rs` for now)
- `LocalModItemSym::absolute_span(db)` already existed — used as the anchor type in `Span`
- Stored `current_sym: LocalModItemSym<'db>` on `BodyCheck`; added `report()`, `catch()`, and `span()` methods
- Converted `require_eq`/`require_sub`/`require_coerce` to return `Result<(), TypeError<'db>>` with a `span: RelativeSpan` parameter
- Migrated all constraint call sites in `cst/expr.rs` (if, while, binary, assign, call, match arm, let, array, struct fields) to catch and report
- Changed `CheckedBody.diagnostics` from `Vec<String>` to `Vec<Diagnostic<'db>>`
- Updated test harness `collect_errors()` to render diagnostics with spans (simple format: `"error at {start}..{end}: {message}"`)
- All 10 existing error tests updated with span prefixes

**Deviations from design:**
- `Ty::Error` and `Res::Err` remain bare variants (not `Ty::Error(ErrorReported)` / `Res::Error(ErrorReported)`). The `ErrorReported` witness is used at catch points but not threaded through the type system yet. This avoids a large multi-file refactor that would touch `skeleton.rs`, `ty_fold.rs`, and all pattern-match sites. Deferred to a future iteration.
- `resolve_path` catches errors internally and returns `Res::Err` (fire-and-forget style) rather than returning `Result<Res, TypeError>`. This is simpler for the many call sites in pattern checking that just need the resolution value.
- The span on the `return_type_mismatch` test is `23..28` (the body block expression) rather than just the inner `x` expression (`25..26`). This is because `require_coerce` gets the body expression's span. Phase 2 will add secondary labels pointing at the return type annotation.

### Phase 2: Secondary labels / context

**Snapshot test:**
```rust
#[test]
fn error_labels_expected_and_found() {
    TestCrate::in_memory("fn bad(x: u32) -> bool { x }")
        .check_errors(expect![[""]]);
}
```

**Status: DONE**

After implementing, `UPDATE_EXPECT=1` fills in:
```rust
        .check_errors(expect![[r#"
            error at 23..28: type mismatch: expected `bool`, found `u32`
              at 23..28: found `u32`
              at 18..22: expected `bool` because of return type"#]]);
```

**Work done:**
- Added `ErrorContext` enum (`ReturnType`, `Argument`, `FieldInit`) and `context: Option<ErrorContext>` field on `TypeError`
- Added `.with_context()` method on `TypeError`
- At return-type checking in `fns.rs`, errors are wrapped with `ErrorContext::ReturnType { ret_span }`
- Enriched `to_diagnostic()` to render context as secondary labels (primary label shows "found `X`", secondary shows "expected `Y` because of return type")
- `render_short()` now emits labels as indented sub-lines

**Deviations from design:**
- Did not convert `resolve_path()` to return `Result` — it remains fire-and-forget with internal `catch()`. This matches the Phase 1 deviation.
- Did not add a `with_context` block helper — simple `.with_context(...)` on the error value at catch sites is sufficient for now.
- The diagnostic message still includes both types in the headline (`"type mismatch: expected \`bool\`, found \`u32\`"`) rather than splitting them purely into labels. This avoids losing information when rendered without labels.

### Phase 3: `TyDisplay` — reusable type formatting

**Status: DONE**

**Snapshot tests:**
```rust
#[test]
fn ty_display_unit_return() {
    TestCrate::in_memory("fn f() -> u32 { }").check_errors(expect![[r#"
        error at 14..17: type mismatch: expected `u32`, found `()`
          at 14..17: found `()`
          at 10..13: expected `u32` because of return type"#]]);
}

#[test]
fn ty_display_fn_pointer() {
    TestCrate::in_memory("fn f(g: fn(u32) -> bool) -> u32 { g }").check_errors(expect![[r#"
        error at 32..37: type mismatch: expected `u32`, found `fn(u32) -> bool`
          at 32..37: found `fn(u32) -> bool`
          at 28..31: expected `u32` because of return type"#]]);
}
```

**Work done:**
- Extracted `fmt_ty` into `TyDisplay<'a, 'db>` struct with `impl Display` in `sage-ir/src/display.rs`
- `TyDisplay::new(db, stash, ty)` is usable anywhere — diagnostics, hover info, debugging
- Used `TyDisplay` in `TypeError::to_diagnostic()` via `.to_string()`
- Deleted old `fmt_ty` function from `check/body.rs`

**Deviations from design:**
- `TyDisplay` lives in `sage-ir/src/display.rs` (existing placeholder file) rather than `sage-ir/src/ty/display.rs` — simpler module layout, `ty.rs` remains a single file.

### Phase 4: Rich rendering with `annotate_snippets`

**Status: DONE**

After `UPDATE_EXPECT=1`, all snapshots now show full rustc-style source snippets:
```rust
        .check_errors(expect![[r#"
            error: type mismatch: expected `bool`, found `u32`
             --> lib.rs:1:24
              |
            1 | fn bad(x: u32) -> bool { x }
              |                   ---- ^^^^^ found `u32`
              |                   |
              |                   expected `bool` because of return type"#]]);
```

**Work done:**
- Added `annotate_snippets` v0.12 dependency to `sage-ir`
- Added `Diagnostic::render()` method that resolves spans → source text → `annotate_snippets::Snippet` with annotations
- Test harness now uses `render()` (rich) instead of `render_short()` (simple)
- All 30 test snapshots updated to show source snippets with underlines and labels
- Removed old `SageDiagnostic` scaffold (`diagnostics.rs` deleted)

**Deviations from design:**
- Rendering lives as a method on `Diagnostic` in `diagnostic.rs` rather than a separate `diagnostic/render.rs` module — simpler for the current scope.
- `notes` (sub-diagnostics) are not yet rendered via annotate_snippets (no need yet since no test exercises them).

### Phase 5: Sub-diagnostics / wrap

**Snapshot test:**
```rust
#[test]
fn wrapped_error_shows_note() {
    TestCrate::in_memory(
        "struct Point { x: u32 }
         fn f() -> Point { Point { x: true } }"
    )
    .check_errors(expect![[""]]);
}
```

After Phase 4, this test's snapshot shows the raw `"type mismatch"`. After Phase 5, running `UPDATE_EXPECT=1` changes it to show the wrapped form with a note. The snapshot diff is the proof:

```diff
- error: type mismatch
+ error: invalid struct field initializer
    ...
+   = note: type mismatch: expected `u32`, found `bool`
```

**Work:**
- At struct-lit checking code, use `.wrap("invalid struct field initializer")` so the low-level subtyping error becomes a note
- Ensure `annotate_snippets` rendering handles `notes` field
- Apply similar wrapping patterns to other catch points where context is valuable

### Phase 6: Oracle harness integration

**Snapshot test:** The oracle harness itself uses snapshot comparison. Extend the `//# ERROR` annotations to optionally include span info:

```rust
// test-fixtures/oracle/errors/type_mismatch.rs
fn returns_wrong_type() -> u32 {
    "hello"
    //# ERROR 2:5..2:12 type mismatch
}
```

The oracle test that runs this fixture will fail (snapshot mismatch) until the harness is updated to check spans. Then `UPDATE_EXPECT=1` (or oracle `.json` regeneration) captures the new expected output.

**Work:**
- Extend `annotations.rs` parsing to support optional span ranges in `//# ERROR` comments
- Wire the oracle comparison to check that sage's diagnostic spans match the annotated positions
- Regenerate oracle snapshots to include span expectations

### Future iterations

- Salsa accumulator for cross-phase diagnostic collection
- LSP diagnostic bridge
- Quick-fix suggestions
- Verbose mode that surfaces internal sub-diagnostics

## Inspiration: Dada

Key patterns from [Dada](https://github.com/dada-lang/dada) worth noting:

- **Salsa accumulator** for diagnostics — `Diagnostic` is `#[salsa::accumulator]`, collected via `.accumulate(db)` during any tracked function, retrieved with `fn::accumulated::<Diagnostic>(db)`.
- **`Reported` sentinel** — `type Errors<T> = Result<T, Reported>` prevents cascading errors. Once reported, the span is captured in `Reported(span)` and further operations can early-return without re-reporting.
- **`OrElse` trait + `Because` enum** — low-level operations (subtyping, unification) receive an `OrElse` callback. On failure, it's invoked with a `Because` variant that explains *why* the operation was attempted. This produces diagnostics like "expected T because of return type annotation" vs "expected T because of assignment target".
- **Builder API with labels** — `.label(db, Level::Error, span, msg)` for multi-span errors, `.child(diagnostic)` for nested explanatory notes.
- **`annotate_snippets`** for rendering to terminal.

We adopt immediately:
- Structural shape (span + labels + builder)
- `ErrorReported` sentinel (our version of Dada's `Reported` / rustc's `ErrorGuarantee`)
- `annotate_snippets` for rendering

Different approach from Dada:
- Instead of `OrElse`/`Because` callbacks threaded *down* into low-level operations, we use `Result<T, Diagnostic<'db>>` and accumulate context on the way *up* via `.map_err()`. This is more idiomatic Rust and doesn't require passing closures through every layer.

Deferred to future iterations:
- Salsa accumulators
