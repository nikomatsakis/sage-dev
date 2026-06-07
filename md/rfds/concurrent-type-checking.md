# RFD: Concurrent Type Checking Execution Model

**Status:** Proposed

**Depends on:**
- [Type Inference](./type-inference.md) — the async executor sketch, structured concurrency model

## Problem

The type inference RFD describes an async execution model where independent sub-expressions are checked concurrently (via `futures::join`) and statements in a block introduce bindings sequentially while spawning initializer checks concurrently. The `sage-infer` runtime scaffold exists but the type checker currently walks the body synchronously.

## Goal

Design the concrete execution model for concurrent type checking within a function body. Key decisions:

1. **What is the concurrency granularity?** Per-expression? Per-statement? Per-block?
2. **How do scoping and binding visibility interact with concurrency?** A `let` binding must be visible to subsequent statements, but its initializer check can run concurrently with later statements.
3. **How does the executor schedule work?** Priority based on information unlocked? FIFO?
4. **How does structured concurrency (moro-style scopes) integrate?** All spawned tasks within a block must complete before the block's type is finalized.
5. **What are the await points?** Method resolution on inference variables, trait solving queries — where does a task suspend?

## Current state

- `check_expr` is synchronous. Converting to `async fn check_expr(...)` is mechanical.
- The runtime (`runtime.rs`) has `spawn`, `drain`, `block_on`, and per-variable waker lists.
- The RFD sketches `futures::future::join` for independent sub-expressions and a moro-style scope for blocks.

## Open questions

- Should we use real `async`/`await` (Rust futures) or a manual continuation-passing style? Real futures compose better but have lifetime constraints with `&mut InferCtx`.
- The `&mut InferCtx` problem: concurrent tasks need shared access to the inference state. The RFD proposes `&self` + `RefCell` internals. Is that the right tradeoff?
- How does cancellation work when a speculative branch is discarded? Tasks spawned in that branch should be dropped.
- Is there measurable benefit to concurrency for typical function bodies, or is this primarily an architectural investment for trait solving latency hiding?
