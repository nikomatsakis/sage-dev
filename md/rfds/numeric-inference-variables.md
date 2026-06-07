# RFD: Numeric Inference Variables

**Status:** Proposed

**Depends on:**
- [Type Inference](./type-inference.md) — inference variable infrastructure

## Problem

Integer and float literals currently get a plain `InferVar` — a fully unconstrained type variable. This means:

- `let x = 1;` with no further constraints resolves to `Error` (unresolved var) at finalization
- There's no way to express "this must be *some* integer type" without pinning it to a specific one
- The default fallback behavior (`1` → `i32`, `1.0` → `f64`) has no mechanism

## Goal

Introduce constrained inference variables for numeric literals that:

1. Unify only with integer types (or only with float types)
2. Default to `i32` / `f64` at finalization if unconstrained beyond "is numeric"
3. Use the same underlying `InferVarIndex` infrastructure (no separate tracking)

## Sketch

Add variants to `TyData`:

```rust
enum TyData<'db> {
    // ... existing ...
    IntVar(InferVarIndex),    // must resolve to an integer type
    FloatVar(InferVarIndex),  // must resolve to a float type
}
```

Or alternatively, store a "kind constraint" in `VarInfo`:

```rust
enum VarKind {
    General,   // any type
    Int,       // i8..i128, isize, u8..u128, usize
    Float,     // f32, f64
}

struct VarInfo {
    universe: Universe,
    kind: VarKind,
}
```

The second approach keeps `TyData` simpler (just `InferVar(idx)` for all) and puts the constraint in metadata. `require_eq` would check kind compatibility when unifying two vars or a var with a concrete type.

## Finalization behavior

At finalization, unconstrained numeric vars get promoted:
- `VarKind::Int` with `Bound::None` → `Exactly(i32)`
- `VarKind::Float` with `Bound::None` → `Exactly(f64)`
- `VarKind::General` with `Bound::None` → `Error` (as today)

## Open questions

- Should `as` casts from int-var to a specific int type pin the var? (Probably yes.)
- How do numeric vars interact with generic type parameters? (`fn foo<T>(x: T)` called with `foo(1)` — does `T` get constrained to int-kind?)
