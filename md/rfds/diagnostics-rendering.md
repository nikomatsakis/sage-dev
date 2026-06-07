# RFD: Diagnostics Rendering

**Status:** Proposed

**Depends on:**
- [Type Inference](./type-inference.md) — diagnostic collection during type checking

## Problem

The current diagnostic rendering is a quick `fmt_ty` function inside `check.rs` that produces strings like `` type mismatch: expected `u32`, found `bool` ``. This is adequate for tests but insufficient for a real compiler:

- No span information (which expression caused the error?)
- No context (what was the expected type and why?)
- Type display is ad-hoc and not reusable outside the checker
- No structured output for IDE integration

## Goals

1. **`Ty::display(db, stash)` method** — a reusable `Display` impl for types, usable anywhere (diagnostics, hover info, debugging). Should handle inference variables (show resolved type after `find`), generic params (show name), and ADTs (show name + type args).

2. **Structured diagnostics** — diagnostics carry spans, severity, structured data (expected/actual types as typed values, not pre-rendered strings). Rendering to text is a separate final step.

3. **Look at Dada's diagnostics model** for inspiration on how to structure multi-part error messages with labeled spans.

## Non-goals (for now)

- Colored terminal output
- LSP diagnostic protocol integration
- Fix suggestions / quick-fix actions
