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

Diagnostics are pushed to a `Vec<Diagnostic<'db>>` on `BodyCheck`. On `finish()`, they're rendered to `Vec<String>` and stored in `CheckedBody`. The structural information is lost at that point.

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

Replace the current internal-only `Diagnostic` with a structured type that carries full source location, labeled context, and sub-diagnostics:

```rust
/// A single diagnostic emitted by sage.
pub struct Diagnostic<'db> {
    pub severity: Severity,
    pub span: AbsoluteSpan<'db>,        // primary location
    pub message: String,                 // headline, e.g. "type mismatch"
    pub labels: Vec<Label<'db>>,         // labeled sub-spans for context
    pub notes: Vec<Diagnostic<'db>>,     // sub-diagnostics (wrapped originals, explanatory notes)
}

pub struct Label<'db> {
    pub span: AbsoluteSpan<'db>,
    pub message: String,                 // e.g. "expected `u32`" or "found `bool`"
    pub style: LabelStyle,
}

pub enum LabelStyle { Primary, Secondary }

pub enum Severity { Error, Warning }
```

The `notes` field holds sub-diagnostics — these are originals that were "wrapped" during propagation, or explanatory context added along the way. See [Error Propagation and Reporting](#design-error-propagation-and-reporting) for how these get built up.

### `ErrorReported` — the return value of reporting

Reporting a diagnostic returns an `ErrorReported` sentinel (see [error-sentinel-type RFD](./error-sentinel-type.md)). This is the *only* way to construct `Ty::Error`, `Res::Err`, and similar error variants — making it a compile error to introduce an error type without first emitting a user-visible message.

```rust
/// Witness that at least one diagnostic has been emitted.
/// Only constructible by the diagnostic-reporting machinery.
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
    span: span_of_expr_x,           // primary: the expression `x`
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
    pub fn error(span: AbsoluteSpan<'db>, message: impl Into<String>) -> Self { ... }
    pub fn warning(span: AbsoluteSpan<'db>, message: impl Into<String>) -> Self { ... }

    pub fn label(mut self, span: AbsoluteSpan<'db>, message: impl Into<String>) -> Self { ... }
    pub fn secondary(mut self, span: AbsoluteSpan<'db>, message: impl Into<String>) -> Self { ... }
}
```

Diagnostics are constructed at the point of failure, propagated up as `Result::Err(Diagnostic)` (accumulating context along the way), and *reported* at catch points where recovery is possible. Reporting stores the diagnostic and mints `ErrorReported`. See [Error Propagation and Reporting](#design-error-propagation-and-reporting) below for the full model.

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

### Core model

Diagnostics flow through two distinct mechanisms:

1. **Propagation** — low-level operations (unification, subtyping, resolution) return `Result<T, Diagnostic<'db>>`. On failure they produce a diagnostic describing *what* went wrong. Callers can add context about *why* the operation was attempted (via `.map_err()`), then either propagate further or catch.

2. **Reporting** — at "catch points" where the code can produce a fallback (e.g. `Ty::Error`), the diagnostic is reported. This mints `ErrorReported` and stores the diagnostic for later rendering.

This separates concerns cleanly: **low-level code doesn't decide recovery strategy**, and **context accumulates naturally on the way up**.

### `Result`-based propagation

Low-level operations produce a "raw" diagnostic describing the mechanical failure. As it propagates up through `?`, each layer adds context about *why* that operation was requested, until it reaches a catch point.

```rust
// Lowest level: "X is not a subtype of Y" — a mechanical fact
fn subtype(&mut self, sub: Ptr<Ty<'db>>, sup: Ptr<Ty<'db>>) -> Result<(), Diagnostic<'db>> {
    // ...
    Err(Diagnostic::error(span_sub, "mismatched types")
        .label(span_sub, format!("`{sub_ty}` is not a subtype of `{sup_ty}`")))
}
```

```rust
// Mid-level: "...because this is a return expression"
fn check_return(&mut self, expr: &Expr, ret_ty: Ptr<Ty<'db>>) -> Result<(), Diagnostic<'db>> {
    let actual = self.infer_expr(expr)?;
    self.subtype(actual, ret_ty)
        .map_err(|d| d.secondary(ret_ty_span, "expected because of return type"))?;
    Ok(())
}
```

```rust
// Catch point: reports and soldiers on
fn check_expr(&mut self, expr: &Expr, expected: Ptr<Ty<'db>>) -> Ptr<TyExpr<'db>> {
    match self.check_expr_inner(expr, expected) {
        Ok(te) => te,
        Err(diag) => {
            let e = self.report(diag);
            self.alloc_error_expr(e, expr.span)
        }
    }
}
```

### What happens at each `?` — a taxonomy

Most `?` points will fall into one of these operations. The principle is: **never discard context**. Even when you "replace" the message, the original remains as a sub-diagnostic.

#### 1. Pass through unchanged

The most common case — the error is already well-described and this layer has nothing useful to add:

```rust
let ty = self.resolve_path(path)?;
```

#### 2. Add labels/context, keep message

The error's headline is fine, but the caller knows *why* this operation was attempted:

```rust
self.subtype(actual, expected)
    .map_err(|d| d.secondary(call_span, "required by argument 2 of `foo`"))?;
```

#### 3. Replace the headline, keep details

The low-level error ("X is not a subtype of Y") is accurate but the user wants to see the higher-level framing. The original description becomes a sub-diagnostic note:

```rust
self.subtype(actual, field_ty)
    .map_err(|d| d.wrap("invalid struct field initializer"))?;
```

Where `wrap` replaces the top-level message but preserves the original as a child note:

```
error: invalid struct field initializer
  --> src/main.sage:5:12
   |
 5 |     Point { x: "hello" }
   |             ^^^^^^^^^^^ 
   |
   = note: mismatched types: `&str` is not a subtype of `u32`
```

#### 4. Wrap as sub-diagnostic of a new error

When the original error is a low-level detail that should be visible but subordinate:

```rust
self.check_where_clause(bound)
    .map_err(|d| {
        Diagnostic::error(call_span, "trait bound not satisfied")
            .label(call_span, format!("`{ty}` does not implement `{trait_}`"))
            .note(d)  // original error becomes a sub-diagnostic
    })?;
```

### Never discard

We never throw away a propagated diagnostic. Even if the final user-facing message is completely rewritten at a higher layer, the original mechanical error is preserved as a sub-diagnostic (note). This is invaluable for:

- **Debugging the compiler** — you can always trace *why* a particular error was reported
- **Power users** — verbose mode can show the full chain
- **IDE integration** — related information can be shown on hover or in a details pane

The rendering layer decides visibility: by default, sub-diagnostics marked as "internal" can be hidden from normal output but shown in verbose mode or test dumps.

### Builder methods for propagation

```rust
impl<'db> Diagnostic<'db> {
    /// Wrap: replace headline, demote self to a note on the new diagnostic.
    pub fn wrap(self, message: impl Into<String>) -> Self {
        Diagnostic {
            severity: self.severity,
            span: self.span,
            message: message.into(),
            labels: vec![],
            notes: vec![self],
        }
    }

    /// Attach a sub-diagnostic note.
    pub fn note(mut self, sub: Diagnostic<'db>) -> Self {
        self.notes.push(sub);
        self
    }

    /// Add a secondary label (common in .map_err closures).
    pub fn secondary(mut self, span: AbsoluteSpan<'db>, message: impl Into<String>) -> Self {
        self.labels.push(Label { span, message: message.into(), style: LabelStyle::Secondary });
        self
    }
}
```

### Context helpers — applying a transform to a block

In practice, you often want the same `.map_err` applied to *every* `?` within a region of code, not repeated on each line. A helper function wraps a closure and transforms whatever error escapes:

```rust
fn with_context<T>(
    transform: impl FnOnce(Diagnostic<'db>) -> Diagnostic<'db>,
    body: impl FnOnce() -> Result<T, Diagnostic<'db>>,
) -> Result<T, Diagnostic<'db>> {
    body().map_err(transform)
}
```

Usage:

```rust
with_context(
    |d| d.wrap("invalid struct field initializer"),
    || {
        let field_ty = self.resolve_field(name)?;
        let init_ty = self.infer_expr(init)?;
        self.subtype(init_ty, field_ty)?;
        Ok(...)
    },
)
```

Any of the three inner `?` points that fail will have their diagnostic wrapped with the same higher-level message. The caller doesn't care *which* step failed — they all mean "this field initializer is bad" — but the original error is preserved as a note explaining the mechanical reason.

This composes naturally — nested `with_context` calls build up layers:

```rust
with_context(
    |d| d.secondary(call_span, "in this function call"),
    || {
        for (i, (param, arg)) in params.iter().zip(args).enumerate() {
            with_context(
                |d| d.wrap(format!("argument {i} has wrong type")),
                || {
                    let arg_ty = self.infer_expr(arg)?;
                    self.subtype(arg_ty, param.ty)?;
                    Ok(())
                },
            )?;
        }
        Ok(())
    },
)
```

The exact signature and naming (`with_context`, `contextualize`, a method on `BodyCheck`, etc.) is an implementation detail — the key insight is that this is just `Result::map_err` applied to a block rather than to a single operation.

### Catch points — where `ErrorReported` is minted

A catch point is anywhere the code can manufacture a meaningful fallback value and continue checking. Examples:

| Level | Catches | Fallback |
|-------|---------|----------|
| Expression | type mismatch, unresolved name | `Ty::Error(e)` — continue checking siblings |
| Statement | invalid assignment target | skip statement, continue block |
| Item | signature error | skip body checking entirely |
| Module | parse error | skip item |

At each catch point, `report()` stores the (fully-contextualized) diagnostic and returns `ErrorReported`:

```rust
impl BodyCheck<'_, 'db> {
    pub fn report(&mut self, diag: Diagnostic<'db>) -> ErrorReported {
        self.diagnostics.push(diag);
        ErrorReported(())
    }
}
```

### Multiple independent errors

When checking N independent things (e.g., all arguments to a function call), each is its own catch point — you try each independently, report each failure, and keep going:

```rust
for (param, arg) in params.iter().zip(args) {
    if let Err(diag) = self.check_expr_against(arg, param.ty) {
        self.report(diag);
        // don't short-circuit — check remaining args too
    }
}
```

No need for a "set" type — each error is reported at its own catch site. The diagnostics vec accumulates them naturally.

### Propagation through already-errored values

When code encounters a `Ty::Error(e)` from upstream, it propagates without re-reporting — the witness already exists:

```rust
match res {
    Res::Err(e) => return self.alloc_ty(Ty::Error(e)),
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
Diagnostic<'db>  →  resolve spans to (file, line, col)  →  annotate_snippets::Message  →  String
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

## Design: Span Propagation

Currently `TyExpr` nodes carry `RelativeSpan` (byte offsets relative to the item's start). To produce an `AbsoluteSpan` for a diagnostic, we need:

1. The `AbsoluteSpan` of the containing item (function/impl) — available from `LocalFnSym`'s CST node.
2. The `RelativeSpan` from the `TyExpr` or CST node where the error occurs.
3. Combine via `absolute.resolve(relative)`.

The checker already has access to the item's source span (passed into `BodyCheck` setup). We just need to thread it so that diagnostic-emitting methods can resolve relative → absolute.

### Approach

Add to `BodyCheck`:
```rust
item_span: AbsoluteSpan<'db>,  // the containing item's absolute position
```

When emitting a diagnostic, convert and report:
```rust
fn report_at(&mut self, relative: RelativeSpan, diag: Diagnostic<'db>) -> ErrorReported {
    let span = self.item_span.resolve(relative);
    self.report(diag.with_span(span))
}
```

Or when building inline:
```rust
let e = self.report(
    Diagnostic::error(self.item_span.resolve(expr.span), "type mismatch")
        .label(self.item_span.resolve(expr.span), format!("found `{actual}`"))
);
self.alloc_ty(Ty::Error(e))
```

## Implementation Plan

### Phase 1: Type display

- Extract `fmt_ty` into a standalone `TyDisplay` struct with `Display` impl
- Move to its own module (`sage-ir/src/ty/display.rs` or similar)
- Keep existing behavior, just make it reusable

### Phase 2: Structured diagnostics + `ErrorReported` + `Result` propagation

- Introduce `ErrorReported` sentinel (per [error-sentinel-type RFD](./error-sentinel-type.md))
- Replace the internal `Diagnostic`/`DiagnosticKind` with the new span-carrying struct
- Add `report()` method on `BodyCheck` that stores a diagnostic and returns `ErrorReported`
- Thread `item_span` into `BodyCheck`
- Change `Ty::Error` to `Ty::Error(ErrorReported)`, `Res::Err` to `Res::Err(ErrorReported)`
- Refactor checker internals to return `Result<T, Diagnostic<'db>>` from low-level operations (unification, subtyping)
- Identify catch points (expression boundaries, item boundaries) where errors are reported and fallback values produced
- Change `CheckedBody.diagnostics` from `Vec<String>` to `Vec<Diagnostic<'db>>`

### Phase 3: Rendering

- Add `annotate_snippets` dependency
- Write a thin `render` module that converts `Diagnostic` → snippet output
- Update test harness to render diagnostics for assertions
- Remove old `render_diagnostic` / `SageDiagnostic` scaffold

### Future iterations

- Salsa accumulator for cross-phase diagnostic collection
- LSP diagnostic bridge
- Quick-fix suggestions

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
- Instead of `OrElse`/`Because` callbacks threaded *down* into low-level operations, we use `Result<T, Diagnostic>` and accumulate context on the way *up* via `.map_err()`. This is more idiomatic Rust and doesn't require passing closures through every layer.

Deferred to future iterations:
- Salsa accumulators
