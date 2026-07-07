# RFD: Make Error Types Take a Reported-Error Sentinel Value

**Status:** Proposed

## Problem

Throughout the codebase, `Ty::Error`, `Res::Err`, and similar error variants can be constructed freely — there is no structural guarantee that a diagnostic has actually been reported before the error type is created. This leads to a class of bugs where error types are introduced without any user-visible error message, causing silent failures or confusing downstream behavior.

The invariant we want: **every `Ty::Error` (or `Res::Err`, etc.) in the IR implies that at least one diagnostic has been emitted.** Today this is a convention enforced only by code review.

### Current state

- **Correct (propagation):** When code matches an existing `Res::Err` and propagates it as `Ty::Error`, no new error needs to be reported — the original error was already reported upstream.
- **Incorrect (fresh construction without reporting):** Code that encounters unexpected state and constructs `Ty::Error` without first emitting a diagnostic violates the invariant.

## Proposal

Introduce a sentinel type (e.g., `ErrorReported`) that witnesses the fact that a diagnostic has been emitted. Error variants must require this sentinel to be constructed:

```rust
/// Witness that at least one error has been reported.
/// Only constructible by the diagnostics infrastructure.
#[derive(Copy, Clone)]
pub struct ErrorReported(());

impl ErrorReported {
    /// Only the diagnostic-reporting machinery calls this.
    pub(crate) fn new() -> Self {
        ErrorReported(())
    }
}
```

Error types then carry or require this sentinel:

```rust
enum Ty<'db> {
    // ...
    Error(ErrorReported),
}

enum Res<'db> {
    // ...
    Err(ErrorReported),
}
```

### Propagation

Propagating an existing error is still zero-cost — you already have an `ErrorReported` from the value you're matching on:

```rust
match res {
    Res::Err(e) => return cx.alloc_ty(Ty::Error(e)),
    // ...
}
```

### Fresh error construction

When code detects a new error condition, it must report a diagnostic first:

```rust
let e = cx.report(diagnostic);  // returns ErrorReported
cx.alloc_ty(Ty::Error(e))
```

This makes it a compile error to construct `Ty::Error` without either (a) propagating an existing sentinel or (b) going through the diagnostic reporting API.

## Considerations

- **Stash representation:** If `Ty` is stored in a stash, the `ErrorReported` sentinel is zero-sized and should not affect layout. We may need to confirm this doesn't interact poorly with stash allocation.
- **Multiple error variants:** All error-like variants (`Ty::Error`, `Res::Err`, future ones) should adopt this pattern.
- **Gradual migration:** Can be done incrementally — change the variant, fix compile errors one by one.

## Open questions

- Should `ErrorReported` carry an error index for tracing which diagnostic it corresponds to, or is a zero-sized witness sufficient?
- Naming: `ErrorReported`, `ErrorGuarantee` (rustc's name), `DiagEmitted`?
