# RFD: Async Type Checker

**Status:** Active — Phases A, B, C, D complete

**Depends on:**
- [Type Inference](./type-inference.md) (completed) — egraph, bounds, runtime, skeleton

**Subsumes:**
- [Concurrent Type Checking](./concurrent-type-checking.md) — the open questions there are resolved by this design

## Problem

The type inference infrastructure is built (egraph, bounds, versioning, runtime) but the body checker (`cst/expr.rs`) walks the CST synchronously, taking `&mut BodyCheck`. This means:

1. **No suspension on unknowns.** Method resolution, trait queries, and downstream type propagation cannot wait for an inference variable to resolve — they must produce a result (or an error) immediately.
2. **No concurrency within a body.** Independent sub-expressions, match arms, and function arguments are checked sequentially even though they could run interleaved, feeding information to each other through inference variable wakeups.
3. **Eager errors.** When a type isn't yet known (e.g., awaiting return type from a call whose arguments haven't been checked), the checker must pessimistically error or leave a variable unresolved rather than suspending and retrying when information arrives.

The [Type Inference RFD](./type-inference.md) designed an async execution model (§ "Execution model: async type checking") that solves all three. The runtime scaffold (`runtime.rs`) implements the executor. This RFD covers the remaining work to wire the async model into the actual expression checker.

## What's already built

| Component | File | Status |
|-----------|------|--------|
| Custom executor (spawn, drain, block_on, wake) | `check/infer/runtime.rs` | Done |
| Versioned egraph + union-find | `check/infer/egraph.rs` | Done |
| Monotonic bounds (None → AtLeast → Exactly) | `check/infer/bound.rs` | Done |
| Version tree (branch/discard) | `check/infer/version.rs` | Done |
| Skeleton decompose/recompose | `check/infer/skeleton.rs` | Done |
| Shared InferCtx (RefCell, &self) | `check/infer_ctx.rs` | Done |
| Task-local Scope (resolver, locals) | `check/infer_ctx.rs` | Done |
| Constraint ops (require_eq, require_sub, require_coerce) | `check/infer_ctx.rs` | Done |
| Expression checker with Result error model | `check/expr.rs` | Done |
| CheckError + RecordErr | `check/infer_ctx.rs` | Done |
| ExprSlot, TyExprData::Error/Unresolved | `tytree/mod.rs` | Done |

## Design

### Split context into shared state + scoped context

`BodyCheck` currently bundles everything behind `&mut self`. The async model requires separating:

1. **Shared inference state (`InferCtx`)** — the egraph, runtime, stash, variable tracking, and diagnostic accumulator. Truly shared across all concurrent tasks; protected by `RefCell` since the executor is single-threaded and cooperative.

2. **Scoped context (`Scope`)** — the resolver ribs, visible locals, current universe. These are *task-local*: a task checking one match arm shouldn't see bindings introduced in a sibling arm. Passed by shared reference (`&Scope`) to expression checkers; only mutated locally within block-checking code.

```rust
/// Shared inference state — one per function body, shared by all tasks.
pub struct InferCtx<'check, 'db> {
    pub db: &'db dyn crate::Db,
    source_stash: &'check Stash,

    // Shared mutable state (single-threaded cooperative → RefCell is safe)
    egraph: RefCell<VersionedEGraph<'db>>,
    runtime: RefCell<Runtime<'check>>,
    target_stash: RefCell<Stash>,
    infer_var_ptrs: RefCell<Vec<Ptr<Ty<'db>>>>,
    local_vars: RefCell<Vec<LocalVar<'db>>>,
    expr_slots: RefCell<Vec<Option<Ptr<TyExpr<'db>>>>>,

    // Shared diagnostic accumulator — all errors go here
    diagnostics: RefCell<Vec<Diagnostic<'db>>>,
}

/// Task-local scope — passed by &Scope to expression checks.
/// Only mutated locally in block-level code between statements.
struct Scope<'db> {
    resolver: Resolver<'db>,
    locals: Vec<Ptr<Ty<'db>>>,
    universe: Universe,
}
```

The `'check` lifetime covers the entire body-checking operation. All spawned futures live within `'check` — the scoped spawn API guarantees all tasks are joined before `'check` ends. Internally this requires unsafe lifetime erasure in `spawn` (accepting `'check` futures, storing them as `'static`), which is sound because the scope guarantees join-before-return.

Safety: the executor is single-threaded and cooperative — no two tasks are polled simultaneously, so `RefCell` borrows on `InferCtx` never overlap. The types are `!Send + !Sync` which is correct.

### Error model

All diagnostics are reported to the shared accumulator on `InferCtx`:

```rust
impl<'check, 'db> InferCtx<'check, 'db> {
    fn record(&self, diag: Diagnostic<'db>) {
        self.diagnostics.borrow_mut().push(diag);
    }
}
```

**Two error paths:**

1. **Non-fatal** (type mismatch, coercion failure): report directly to `cx` and keep building the structural node:

   ```rust
   cx.require_eq(lhs_ty, rhs_ty, span).record_err(cx);
   // keep going — node is still built
   ```

   `.record_err(cx)` is a method on `Result<(), Diagnostic>` that calls `cx.record(diag)` on `Err` and returns `()`.

2. **Fatal** (can't construct the node at all — e.g., unresolved name): return `Err(CheckError)`. Propagates via `?` until caught at a scope boundary (`.spawned()`), which records the diagnostic and substitutes `TyExprData::Error(ErrorReported)`.

**Background obligations** (spawned by constraint ops like `require_sub` or `coerce` that can't decide immediately) also report to `cx.diagnostics` when they eventually fail.

**The error variant is minimal:**

```rust
TyExprData::Error(ErrorReported)
```

No embedded children, no diagnostic payload — just a sentinel. The diagnostic was already reported to `cx` when the error was produced.

### Expression slots (unresolved expression placeholders)

Analogous to inference variables for types, expression slots are placeholders for expressions that haven't been checked yet:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct ExprSlot(u32);

TyExprData::Unresolved(ExprSlot)
```

When a block spawns an initializer check in the background, it needs a `Ptr<TyExpr>` to put in the statement's slot *now*. It allocates an `ExprSlot`, puts `TyExprData::Unresolved(slot)` in the tree, and the background task fills the slot when it completes.

The expression slot has an inference variable as its type — downstream code interacts with the *type*, never awaits the expression itself. At finalization, all slots must be resolved; unfilled ones become error nodes.

```rust
impl<'check, 'db> InferCtx<'check, 'db> {
    fn alloc_expr_slot(&self) -> (ExprSlot, Ptr<Ty<'db>>) {
        let ty = self.fresh_ty_var();
        let slot = ExprSlot(self.expr_slots.borrow().len() as u32);
        self.expr_slots.borrow_mut().push(None);
        (slot, ty)
    }

    fn fill_expr_slot(&self, slot: ExprSlot, expr: Ptr<TyExpr<'db>>) {
        self.expr_slots.borrow_mut()[slot.0 as usize] = Some(expr);
    }
}
```

### Coercion as expression transformation

`coerce` is an expression-level wrapper around the type-level `require_sub`:

```rust
fn coerce(
    &self,
    expr: Ptr<TyExpr<'db>>,
    target: Ptr<Ty<'db>>,
    span: RelativeSpan,
) -> Result<Ptr<TyExpr<'db>>, Diagnostic<'db>>
```

Three possible outcomes:
1. Types already equal → return original expr unchanged
2. Coercion needed → return new expr wrapping the original (e.g., deref, unsizing)
3. Failure → return `Err(Diagnostic)`

The caller uses `.record_err(cx)` or `?` as appropriate.

### Constraint operations as obligations

Constraint operations (`require_eq`, `require_sub`, `coerce`) are potentially long-lived. They may:
- **Resolve immediately** — skeleton mismatch → error, obviously equal → success
- **Spawn a background obligation** — one or both sides are inference variables; the obligation monitors the bound stream and resolves when it can decide

```rust
impl<'check, 'db> InferCtx<'check, 'db> {
    /// May resolve immediately or spawn a background obligation.
    fn require_sub(
        &self,
        a: Ptr<Ty<'db>>,
        b: Ptr<Ty<'db>>,
        span: RelativeSpan,
    ) -> Result<(), Diagnostic<'db>> {
        // Fast path: immediate resolution
        // Slow path: spawn obligation that monitors both vars
    }
}
```

Obligation errors are reported to `cx.diagnostics` when they fire.

### Two-tier concurrency API

#### Tier 1: Builder (static arity)

For expressions with a compile-time-known number of sub-expressions (binary, unary, if/else, ref, assign, index, cast — the majority):

```rust
CheckReturn::build()
    .subexpr(async { lhs_expr.check(cx, scope) })
    .subexpr(async { rhs_expr.check(cx, scope) })
    .check(async |(lhs, (rhs, ()))| {
        cx.require_eq(cx.ty_of(lhs), cx.ty_of(rhs), span)
    })
    .create(|(lhs, (rhs, ()))| {
        cx.alloc_expr(TyExprData::Binary(lhs, op, rhs), cx.ty_of(lhs), span)
    })
```

**The three steps:**

1. **`.subexpr(async { ... })`** — adds a concurrent async future. Each subexpr runs concurrently with the others. Subexprs take `&Scope` (read-only — they don't introduce bindings). If a subexpr's task returns `Err`, it becomes `TyExprData::Error(ErrorReported)` automatically (diagnostic reported to `cx`); the pipeline continues with the error node in its slot.

2. **`.check(async |subexprs| { ... })`** — spawned as a background obligation into `cx`. Returns `Result<(), Error>`. Never blocks `.create()`. If it eventually fails, the error is reported to `cx.diagnostics`. This is where constraint operations are invoked — they may resolve immediately or themselves spawn further obligations.

3. **`.create(|subexprs| { ... })`** — synchronous, infallible. Builds the structural `TyExpr` node from the subexprs. The node's type may still be an inference variable at this point — that's fine, it resolves as obligations complete.

`CheckReturn::build()` returns a `CheckReturnBuilder`. Initially implemented in terms of `CheckReturn::scoped()` internally — same semantics. Later, the builder can be optimized to compile down to a stack-allocated `join` over `[Future; N]` with no `Box`, no channel, no dynamic dispatch — since the arity is statically known at each call site. The optimization is invisible to callers.

#### Tier 2: Scoped spawn (dynamic arity)

For expressions with a runtime-determined number of children (call args, method call args, tuple elements, array elements, struct field initializers, match arms):

```rust
CheckReturn::scoped(async |scope| {
    let handles: Vec<_> = args.iter()
        .map(|a| async { a.check(cx, name_scope) }.spawned(scope))
        .collect();
    let results = join_all(handles).await;
    // ...
    Ok(expr)
}).await  // → Ptr<TyExpr<'db>>
```

**Key properties:**

- `.spawned(scope)` takes a future returning `Result<Ptr<TyExpr>, CheckError>` and gives back a handle that resolves to `Ptr<TyExpr<'db>>`. If the inner task returns `Err`, the scope catches it, reports the diagnostic to `cx`, and substitutes `TyExprData::Error(ErrorReported)`.
- The body closure returns `Result<Ptr<TyExpr<'db>>, CheckError>` — if it returns `Err`, that too becomes an error node.
- Spawned tasks can themselves call `.spawned()` (nested spawn).

**Implementation sketch** (inspired by `run_until` in agent-client-protocol):

Internally the scope maintains a `FuturesUnordered` and a channel sender. `.spawned()` sends the future through the channel; a driver task polls the `FuturesUnordered` and feeds in newly submitted futures. When the body closure completes, the driver drains remaining tasks before returning.

#### Composing the tiers

```rust
// If/else: static arity builder
let result_ty = cx.fresh_ty_var();
CheckReturn::build()
    .subexpr(async { cond.check(cx, scope) })
    .subexpr(async { then_branch.check(cx, scope) })
    .subexpr(async { else_branch.check(cx, scope) })
    .check(async |(cond, (then, (else_, ())))| {
        cx.require_eq(cx.ty_of(cond), bool_ty, cond_span)?;
        cx.coerce(then, result_ty, then_span)?;
        cx.coerce(else_, result_ty, else_span)?;
        Ok(())
    })
    .create(|(cond, (then, (else_, ())))| {
        cx.alloc_expr(TyExprData::If(cond, then, Some(else_)), result_ty, span)
    })

// Call: scoped for dynamic args, with await_concrete for callee resolution
CheckReturn::scoped(async |s| {
    let callee = async { func.check(cx, scope) }.spawned(s);
    let arg_handles: Vec<_> = args.iter()
        .map(|a| async { a.check(cx, scope) }.spawned(s))
        .collect();

    let callee = callee.await;
    let args = join_all(arg_handles).await;

    let callee_ty = cx.await_concrete(cx.ty_of(callee)).await;
    // ... constrain args against params ...
    Ok(cx.alloc_expr(TyExprData::Call(callee, args_slice), ret_ty, span))
}).await
```

### The `await_concrete` primitive

The core suspension point — a task awaiting a type to become fully resolved. Uses the inference variable's *bound stream*: each call to `next_bound` yields the next bound transition. The task only returns when it sees `Exactly` or `find()` resolves to a concrete type.

```rust
impl<'check, 'db> InferCtx<'check, 'db> {
    /// Suspend until a type is concrete (not an unresolved infer var).
    /// After finalization, returns Ty::Error for unresolvable vars.
    async fn await_concrete(&self, ty: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        loop {
            let canon = self.find(ty);
            let data = self.target_stash.borrow()[canon];
            match data {
                Ty::InferVar(idx) => {
                    // Wait for the next bound transition on this variable.
                    // AtLeast means more info may come — keep waiting.
                    // Exactly means resolved — loop will find concrete type via find().
                    match self.next_bound(idx).await {
                        Some(Bound::Exactly(_)) => continue, // re-find to get concrete type
                        Some(Bound::AtLeast(_)) => continue, // more info may come
                        None => return canon, // finalized, no more updates
                    }
                }
                _ => return canon,
            }
        }
    }

    /// Yield the next bound transition for a variable.
    /// Returns None when the variable is finalized (no more updates possible).
    async fn next_bound(&self, var: InferVarIndex) -> Option<Bound<'db>> {
        poll_fn(|_cx| {
            // Check if bound has changed since last observation.
            // If not, register waker and suspend.
            // Woken when set_bound/union is called on this variable.
            ...
        }).await
    }
}
```

After finalization, unresolved variables are promoted to `Ty::Error(...)`, so `await_concrete` always terminates — it either gets a concrete type or an error sentinel.

### Async expression checker

The synchronous `ExprCst::check(&self, cx: &mut BodyCheck) -> Ptr<TyExpr>` becomes:

```rust
impl<'db> ExprCst<'db> {
    async fn check(
        &self,
        cx: &InferCtx<'_, 'db>,
        scope: &Scope<'db>,
    ) -> Result<Ptr<TyExpr<'db>>, CheckError<'db>>
}
```

Note: `scope` is `&Scope` (shared reference). Expression checking never introduces bindings — only block-level code mutates scope (locally, between statements).

Example arms showing both tiers:

**Binary op — static arity builder:**
```rust
ExprCstKind::Binary(lhs, op, rhs) => {
    CheckReturn::build()
        .subexpr(async { src[*lhs].check(cx, scope) })
        .subexpr(async { src[*rhs].check(cx, scope) })
        .check(async |(lhs, (rhs, ()))| {
            cx.require_eq(cx.ty_of(lhs), cx.ty_of(rhs), span)
        })
        .create(|(lhs, (rhs, ()))| {
            cx.alloc_expr(TyExprData::Binary(lhs, *op, rhs), cx.ty_of(lhs), span)
        })
}
```

**Field access — scoped (needs await_concrete):**
```rust
ExprCstKind::Field(obj, name) => {
    CheckReturn::scoped(async |s| {
        let obj = async { src[*obj].check(cx, scope) }.spawned(s);
        let obj = obj.await;
        let obj_ty = cx.await_concrete(cx.ty_of(obj)).await;
        match cx.stash_ref(obj_ty) {
            Ty::Adt(sym, type_args) => {
                let field_ty = cx.lookup_field(sym, type_args, *name, span)?;
                Ok(cx.alloc_expr(TyExprData::Field(obj, *name), field_ty, span))
            }
            Ty::Error(_) => {
                Ok(cx.alloc_expr(TyExprData::Field(obj, *name), obj_ty, span))
            }
            _ => Err(CheckError::no_field(obj_ty, *name, span)),
        }
    }).await
}
```

**Function call — scoped spawn for dynamic args:**
```rust
ExprCstKind::Call(func, args) => {
    CheckReturn::scoped(async |s| {
        let callee = async { src[*func].check(cx, scope) }.spawned(s);
        let arg_handles: Vec<_> = src[*args].iter()
            .map(|a| async { a.check(cx, scope) }.spawned(s))
            .collect();

        let callee = callee.await;
        let args = join_all(arg_handles).await;
        let args_slice = cx.alloc_slice(&args);

        // Suspend until callee type is known
        let callee_ty = cx.await_concrete(cx.ty_of(callee)).await;
        let ret_ty = match cx.stash_ref(callee_ty) {
            Ty::FnPtr(params, ret) => {
                for (param_ty, arg) in params.iter().zip(args.iter()) {
                    cx.require_eq(cx.ty_of(*arg), *param_ty, span)
                        .record_err(cx);
                }
                ret
            }
            Ty::Error(_) => callee_ty,
            _ => return Err(CheckError::not_callable(callee_ty, span)),
        };

        Ok(cx.alloc_expr(TyExprData::Call(callee, args_slice), ret_ty, span))
    }).await
}
```

**Block — sequential bindings, concurrent initializer checks:**
```rust
ExprCstKind::Block(stmts, tail) => {
    CheckReturn::scoped(async |s| {
        let mut scope = scope.clone(); // local mutable copy
        scope.push_rib();

        for stmt in src[*stmts].iter() {
            match &stmt.kind {
                StmtCstKind::Let(pat, ty_ann, init) => {
                    // Introduce binding immediately (visible to later stmts)
                    let (slot, slot_ty) = cx.alloc_expr_slot();
                    let local_id = scope.add_binding(cx, pat, ty_ann, slot_ty);

                    // Spawn initializer check concurrently
                    if let Some(init_expr) = init {
                        let local_ty = scope.local_type(local_id);
                        let check_scope = scope.clone();
                        async move {
                            let checked = init_expr.check(cx, &check_scope).await?;
                            let coerced = cx.coerce(checked, local_ty, init_expr.span)
                                .record_err_or(cx, checked);
                            cx.fill_expr_slot(slot, coerced);
                            Ok(coerced)
                        }.spawned(s);
                    }
                }
                StmtCstKind::Expr(expr) => {
                    expr.check(cx, &scope).await?;
                }
            }
        }

        // Tail expression (determines block type)
        let (tail_ptr, ty) = match tail {
            Some(t) => {
                let te = src[*t].check(cx, &scope).await?;
                (Some(te), cx.ty_of(te))
            }
            None => (None, cx.unit_ty()),
        };

        scope.pop_rib();
        Ok(cx.alloc_expr(TyExprData::Block(stmts_slice, tail_ptr), ty, span))
    }).await
}
```

### Entry point (`LocalFnSym::body`)

```rust
pub fn body(self, db: &'db dyn crate::Db) -> CheckedBody<'db> {
    let cx = InferCtx::new(db, src);
    let scope = Scope::new(Resolver::new(db, self.scope(db)));
    // ... setup (import sig, bind params into scope) ...

    let body_expr = cx.block_on(async {
        let expr = cst.body.check(&cx, &scope).await?;
        let coerced = cx.coerce(expr, imported.ret, body_span)
            .record_err_or(cx, expr);
        Ok(coerced)
    });

    cx.finalize();
    cx.resolve_expr_slots(); // replace Unresolved(slot) with filled exprs
    cx.resolve_types();
    cx.finish(body_expr)
}
```

### Tracking the current task

For `next_bound` to register a waker, the task needs to know its own ID. The runtime already has `CURRENT_TASK` as a thread-local. The executor sets it before each `poll`:

```rust
fn drain(&mut self) {
    while let Some(mut task) = self.ready.pop_front() {
        CURRENT_TASK.with(|t| *t.borrow_mut() = Some(task.id));
        let waker = Waker::noop();
        let mut cx = Context::from_waker(&waker);
        match task.future.as_mut().poll(&mut cx) {
            Poll::Ready(()) => {}
            Poll::Pending => {
                self.suspended.insert(task.id, task);
            }
        }
        CURRENT_TASK.with(|t| *t.borrow_mut() = None);
    }
}
```

This is already partially implemented but needs to be wired into `block_on` as well.

## Migration strategy

The transition can be incremental:

1. **Phase A:** ✅ Split `BodyCheck` into `InferCtx` (shared, `&self` + `RefCell`, with diagnostic accumulator) and `Scope` (task-local, `&mut Scope` for now). Introduce `TyExprData::Error(ErrorReported)` and `TyExprData::Unresolved(ExprSlot)` variants. Keep the walker synchronous — just change the context threading. All existing tests pass unchanged.

2. **Phase B:** ✅ Introduce `CheckError` type and the `Result`-returning `check_expr` inner method. The public `check_with` catches errors at scope boundaries and substitutes `TyExprData::Error` nodes. Non-fatal constraint errors use `.record_err(cx)`. The function is still synchronous (not yet `async`); the `async` keyword and `block_on` wrapper arrive in Phase C when suspension is actually needed.

3. **Phase C:** ✅ Converted `check_expr` to `async fn` via `#[boxed_async_fn]` proc macro (recursive async with `Box::pin`). `Scope` is now `Clone` and passed by `&Scope` (shared reference); block-level code clones locally for binding mutations. `check_with` is `async`, recursive calls use `.await`. Entry point wraps everything in `block_on`. `await_concrete` is implemented and unit-tested; call/field arms use `find_mut` for now (switching to `await_concrete` requires concurrent background resolution, which arrives with trait solving). Added `pending_wakes` queue to decouple variable wakeups from runtime borrows.

4. **Phase D:** ✅ Removed the old synchronous walker (`cst/expr.rs` body-checking section) and the `BodyCheck` struct entirely.

Each phase is independently testable and shippable. Phase A was the largest mechanical diff; Phase C is the interesting semantic change remaining.

### Implementation notes

- **Scope is `Clone`, passed by `&Scope`.** Expression checking takes `&Scope` (shared reference). Block-level code clones locally for binding mutations. `Resolver` and `Ribs` also derive `Clone`.
- **`#[boxed_async_fn]` macro** transforms `async fn check_expr(...)` into `fn check_expr(...) { Box::pin(async move { ... }).await }`, enabling recursive async without infinite type sizes. Added to `sage-macros-from-impls` crate.
- **`await_concrete` not yet wired into call/field.** Currently uses `find_mut` (eager). Switching to `await_concrete` requires concurrent resolution from a background task (arrives with trait solving). The primitive is tested via unit tests.
- **Pending wake queue:** `InferCtx::pending_wakes` collects variable indices whose bounds changed during a poll. `flush_and_drain` feeds them into the runtime between polls, avoiding RefCell double-borrow.
- **Resolver cloning in `resolve_path` / `check_ty`.** The resolver needs `&mut self` for cycle detection (`in_flight`). Since we pass `&Scope`, we clone the resolver for each resolution. This is not on the hot path (per-name, not per-node).

### Files

| Component | File | Status |
|-----------|------|--------|
| Shared inference context (InferCtx) | `check/infer_ctx.rs` | Done |
| Task-local scope (Scope, Clone) | `check/infer_ctx.rs` | Done |
| Async expression checker (check_with/check_expr) | `check/expr.rs` | Done |
| `#[boxed_async_fn]` proc macro | `sage-macros-from-impls/src/lib.rs` | Done |
| Entry point with block_on | `local_syms/fns.rs` | Done |
| CST expression nodes (data only) | `cst/expr.rs` | Done |
| TyExprData::Error, Unresolved, ExprSlot | `tytree/mod.rs` | Done |
| CheckError, RecordErr | `check/infer_ctx.rs` | Done |
| block_on, flush_and_drain, await_concrete | `check/infer_ctx.rs` | Done |
| pending_wakes queue | `check/infer_ctx.rs` | Done |
| Unit tests (block_on, await_concrete) | `check/infer_ctx.rs` | Done |

## Open questions

1. **Scope snapshot cost.** `Scope::clone()` copies the resolver ribs and local type vec. For deeply nested blocks this could add up. In practice ribs are small (each scope adds a few entries) and locals are just `Vec<Ptr<Ty>>`. If this becomes a problem, a persistent data structure (im-rs or a rib linked list) can replace the vec.

2. **Stash allocation from async tasks.** Multiple tasks may need to allocate into `target_stash` concurrently. With `RefCell`, this is fine as long as no task holds a `Ref`/`RefMut` across a yield point. We should lint for this.

3. **How much concurrency helps.** For most function bodies, the benefit is suspension/retry on unknowns (not parallelism). The real win is latency hiding for trait solver queries. We should benchmark after Phase C.

4. **`next_bound` state tracking.** The stream needs to know what the caller last observed to avoid redundant wakeups. Likely a per-registration "last seen version" counter on the variable.

## Relation to other RFDs

- **Trait System** — trait solving is the primary consumer of `await_concrete`. Method resolution suspends until the receiver type resolves, then queries the trait solver. That work is orthogonal and can proceed in parallel with this RFD.
- **Numeric Inference Variables** — trivial addon once the async model is in place; numeric vars just have additional constraints on what `next_bound` accepts.
- **Concurrent Type Checking (old RFD)** — fully subsumed. The questions it raised are answered here.
