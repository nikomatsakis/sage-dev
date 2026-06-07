# Rustc Subtyping and Coercion: Research Report

This report documents how the Rust compiler (rustc) handles subtyping and coercion
during type checking, based on reading the source at `/home/nikomat/dev/rust/compiler/`.

## 1. The Distinction Between Subtyping and Coercion

### Subtyping: Lifetimes and Variance

In Rust, subtyping is almost exclusively about **lifetimes**. If `'a: 'b` (i.e., `'a`
outlives `'b`), then `&'a T <: &'b T`. The structural types themselves must match --
you cannot subtype `i32` to `u32`, or `Vec<T>` to `Vec<U>` where `T != U`.

The `TypeRelating` struct in `rustc_infer/src/infer/relate/type_relating.rs` implements
subtyping. It carries an `ambient_variance` field that tracks the current variance
context. The key insight: **subtyping in rustc means "check structural equality of
types while allowing lifetime relationships to vary according to variance."**

From `at.rs`, the API is:
```rust
infcx.at(cause, param_env).sub(expected, actual)
// requires that `expected <: actual`

infcx.at(cause, param_env).eq(expected, actual)  
// requires that `expected == actual`
```

Under the hood, `sub` creates a `TypeRelating` with `ambient_variance = Covariant`,
while `eq` uses `Invariant`:

```rust
pub fn sub<T>(...) -> InferResult<'tcx, ()> {
    let mut op = TypeRelating::new(
        self.infcx,
        ToTrace::to_trace(self.cause, expected, actual),
        self.param_env,
        define_opaque_types,
        ty::Covariant,  // <-- subtyping = covariant ambient variance
    );
    op.relate(expected, actual)?;
    Ok(InferOk { value: (), obligations: op.into_obligations() })
}
```

### Coercion: Representation-Changing Transformations

Coercion goes beyond subtyping -- it can **change the representation** of a value.
From the module doc in `coercion.rs`:

> Under certain circumstances we will coerce from one type to another, for example by
> auto-borrowing. This occurs in situations where the compiler has a firm 'expected
> type' that was supplied from the user, and where the actual type is similar to that
> expected type in purpose but not in representation (so actual subtyping is
> inappropriate).

Coercions include:
- `!` (never) to any type (`NeverToAny`)
- `&mut T` to `&T` (reborrowing with mutability change)
- Deref coercions: `&T` to `&U` where `T: Deref<Target=U>`
- Unsizing: `&[T; N]` to `&[T]`, `&T` to `&dyn Trait`
- Function item to function pointer (`ReifyFnPointer`)
- Closure to function pointer (`ClosureFnPointer`)
- `*mut T` to `*const T` (`MutToConstPointer`)

The `Coerce` struct wraps an `FnCtxt` and drives the coercion logic:

```rust
struct Coerce<'a, 'tcx> {
    fcx: &'a FnCtxt<'a, 'tcx>,
    cause: ObligationCause<'tcx>,
    use_lub: bool,
    allow_two_phase: AllowTwoPhase,
    coerce_never: bool,
}
```

The main entry point is `Coerce::coerce(a, b)` which tries to coerce type `a` to type `b`.
The algorithm is:

1. If `a` is `!`, produce a `NeverToAny` adjustment
2. If `a` is an unresolved inference variable, fall back to subtyping
3. Try unsizing coercion (`CoerceUnsized` trait)
4. Match on target type `b` for reference/pointer-specific coercions
5. Match on source type `a` for function/closure coercions
6. Fall back to plain subtyping via `self.unify(a, b)`

## 2. How Match Arms Are Unified

Match arm unification is orchestrated by `CoerceMany` (defined at line 1566 of
`coercion.rs`). The algorithm finds a common type for all arms through an incremental
process.

### The CoerceMany Protocol

```rust
pub(crate) struct CoerceMany<'tcx> {
    expected_ty: Ty<'tcx>,     // initial expected type (from context or fresh var)
    final_ty: Option<Ty<'tcx>>, // running "merged" type
    expressions: Vec<&'tcx hir::Expr<'tcx>>, // previously processed expressions
}
```

From `_match.rs`, the match checking works as follows:

```rust
let mut coercion = {
    let coerce_first = match expected {
        Expectation::ExpectHasType(ety) if ety != tcx.types.unit => ety,
        _ => self.next_ty_var(expr.span),  // fresh inference variable
    };
    CoerceMany::with_capacity(coerce_first, arms.len())
};

for arm in arms {
    let arm_ty = self.check_expr_with_expectation(arm.body, expected);
    coercion.coerce_inner(self, &cause, Some(arm.body), arm_ty, ...);
}

coercion.complete(self)
```

### The Coercion Algorithm for Each Arm

Inside `coerce_inner`, the behavior differs for the first arm vs. subsequent arms:

**First arm**: Directly coerces the expression to the `expected_ty`:
```rust
fcx.coerce(expression, expression_ty, self.expected_ty, AllowTwoPhase::No, ...)
```

**Subsequent arms**: Uses `try_find_coercion_lub`:
```rust
fcx.try_find_coercion_lub(cause, &self.expressions, self.merged_ty(), expression, expression_ty)
```

### try_find_coercion_lub: The LUB Algorithm

This function (line 1330) implements a multi-step strategy:

1. **Fast path**: If `prev_ty == new_ty`, return immediately.

2. **Function/closure pairs**: If both types are `FnDef` or `Closure`, first try
   `at.lub(prev_ty, new_ty)`. If that fails, coerce both to a common function pointer.

3. **General case** (line 1441+): Creates a `Coerce` instance with `use_lub = true`:
   ```rust
   let mut coerce = Coerce::new(self, cause.clone(), AllowTwoPhase::No, true);
   coerce.use_lub = true;
   ```
   Then tries two strategies:
   - First, try coercing the **new** expression to the **previous** merged type
   - If that fails, try coercing **all previous** expressions to the **new** type

4. When `use_lub = true`, the `unify_raw` method inside `Coerce` uses `at.lub(b, a)`
   instead of `at.sup(b, a)`:
   ```rust
   let res = if self.use_lub {
       at.lub(b, a)
   } else {
       at.sup(DefineOpaqueTypes::Yes, b, a)
   };
   ```

### Handling Inference Variables in Arms

When both types are inference variables, `coerce_from_inference_variable` defers the
actual coercion by emitting `Coerce` predicate obligations:

```rust
if b.is_ty_var() {
    let target_ty = if self.use_lub {
        // Create a fresh target and coerce BOTH sides to it
        let target_ty = self.next_ty_var(self.cause.span);
        push_coerce_obligation(a, target_ty);
        push_coerce_obligation(b, target_ty);
        target_ty
    } else {
        push_coerce_obligation(a, b);
        b
    };
    success(vec![], target_ty, obligations)
}
```

## 3. The TypeRelation / Relate Machinery

### The TypeRelation Trait

Defined in `rustc_type_ir/src/relate.rs`:

```rust
pub trait TypeRelation<I: Interner>: Sized {
    fn cx(&self) -> I;
    fn relate<T: Relate<I>>(&mut self, a: T, b: T) -> RelateResult<I, T>;
    fn relate_ty_args(...) -> RelateResult<I, I::Ty>;
    fn relate_with_variance<T: Relate<I>>(
        &mut self, variance: ty::Variance, info: VarianceDiagInfo<I>, a: T, b: T,
    ) -> RelateResult<I, T>;
    fn tys(&mut self, a: I::Ty, b: I::Ty) -> RelateResult<I, I::Ty>;
    fn regions(&mut self, a: I::Region, b: I::Region) -> RelateResult<I, I::Region>;
    fn consts(&mut self, a: I::Const, b: I::Const) -> RelateResult<I, I::Const>;
    fn binders<T>(...) -> RelateResult<I, ty::Binder<I, T>> where T: Relate<I>;
}
```

The `Relate` trait provides the structural decomposition:
```rust
pub trait Relate<I: Interner>: TypeFoldable<I> + PartialEq + Copy {
    fn relate<R: TypeRelation<I>>(relation: &mut R, a: Self, b: Self) -> RelateResult<I, Self>;
}
```

### Structural Recursion: structurally_relate_tys

The `structurally_relate_tys` function (line 337 of `relate.rs`) handles the recursive
decomposition of types:

- For ADTs: calls `relation.relate_ty_args(...)` which dispatches to variance-aware
  argument comparison
- For references: relates the region with `Covariant` variance and the pointee with
  variance depending on mutability
- For closures/coroutines: relates all args **invariantly** (since they are anonymous
  types whose identity is fixed)
- For raw pointers: `*const T` is covariant in T, `*mut T` is invariant

### Three Key Implementations

1. **`TypeRelating`** (type_relating.rs): Implements `eq` and `sub`. Uses
   `ambient_variance` to track the current relationship direction. Handles inference
   variable instantiation directly.

2. **`LatticeOp`** (lattice.rs): Implements `lub` and `glb`. When encountering
   inference variables, creates a fresh variable and constrains both sides to be
   subtypes (for lub) or supertypes (for glb) of it.

3. **`super_combine_tys`** (combine.rs): A shared helper that handles inference
   variable unification (int/float vars), error types, and opaque type handling.
   Called by both `TypeRelating` and `LatticeOp` for the "structural" cases.

### The Difference Between eq, sub, and lub

| Operation | Variance | Meaning | Region handling |
|-----------|----------|---------|-----------------|
| `eq(a, b)` | Invariant | `a == b` | `'a == 'b` (both directions) |
| `sub(a, b)` | Covariant | `a <: b` | `'a: 'b` (a outlives b) |
| `sup(a, b)` | Contravariant | `b <: a` | `'b: 'a` (b outlives a) |
| `lub(a, b)` | N/A (lattice) | least upper bound | `glb('a, 'b)` (shortest-lived) |

The LUB for regions is counter-intuitive: `LUB(&'a T, &'b T)` needs the region that
is a **subtype** of both -- i.e., the GLB of the regions, because subtyping for
references is **covariant** in the lifetime (longer lifetime = subtype). From the
lattice code:
```rust
// LUB(&'static u8, &'a u8) == &RegionGLB('static, 'a) u8 == &'a u8
LatticeOpKind::Lub => constraints.glb_regions(self.cx(), origin, a, b),
```

## 4. How Coercions Interact with Inference Variables

### Source is an Inference Variable

When the source type `a` is an unresolved inference variable, coercion cannot determine
what representation changes might be needed. The `coerce` method handles this at line
265:

```rust
if a.is_ty_var() {
    return self.coerce_from_inference_variable(a, b);
}
```

This falls back to subtyping (or, in the LUB case, creates deferred `Coerce` predicate
obligations).

### Target is an Inference Variable

When the target type is an inference variable, unsizing coercion explicitly bails out:

```rust
fn coerce_unsized(&self, source: Ty<'tcx>, target: Ty<'tcx>) -> CoerceResult<'tcx> {
    if target.is_ty_var() {
        debug!("coerce_unsized: target is a TyVar, bailing out");
        return Err(TypeError::Mismatch);
    }
    // ...
}
```

But the general `coerce` method does NOT bail out -- it will fall through to the
type-specific cases and ultimately to `self.unify(a, b)` which uses subtyping or LUB.
This means that when the target is an inference variable, coercion effectively becomes
subtyping, and the inference variable gets constrained to the source type.

### Both are Inference Variables

When both are inference variables and we are in LUB mode, a fresh third variable is
created and `Coerce` predicate obligations are emitted for both sides. This defers the
actual coercion decision until more type information is available.

## 5. The Coercion "Adjustments" System

### Data Structures

Coercions are recorded as a list of `Adjustment` steps attached to each expression:

```rust
pub struct Adjustment<'tcx> {
    pub kind: Adjust,
    pub target: Ty<'tcx>,  // the type AFTER this adjustment step
}

pub enum Adjust {
    NeverToAny,
    Deref(DerefAdjustKind),
    Borrow(AutoBorrow),
    Pointer(PointerCoercion),
    GenericReborrow(hir::Mutability),
}
```

For example, coercing `&mut Vec<i32>` to `&[i32]` produces:
```
Deref(Builtin)       -> Vec<i32>       // strip the &mut
Deref(Overloaded)    -> [i32]          // Deref impl for Vec
Borrow(Ref)          -> &[i32; N]      // reborrow  
Pointer(Unsize)      -> &[i32]         // unsize
```

### Storage in TypeckResults

Adjustments are stored in the `TypeckResults` structure, keyed by `HirId`:

```rust
pub struct TypeckResults<'tcx> {
    adjustments: ItemLocalMap<Vec<ty::adjustment::Adjustment<'tcx>>>,
    // ...
}
```

The `apply_adjustments` method on `FnCtxt` stores adjustments and also handles
side-effects like tracking diverging type variables and enforcing const effects for
overloaded derefs.

### How Adjustments are Applied

When an expression's coercion succeeds, adjustments are applied via:
```rust
let (adjustments, _) = self.register_infer_ok_obligations(ok);
self.apply_adjustments(expr, adjustments);
```

Importantly, in `try_find_coercion_lub`, adjustments may be **retroactively applied**
to previously-checked expressions:
```rust
// If we can coerce prev_ty to new_ty, apply adjustments to ALL previous expressions
for expr in exprs {
    self.apply_adjustments(expr, adjustments.clone());
}
```

This is why `CoerceMany` tracks all previous expressions -- it may need to go back
and annotate them with coercion adjustments once a later arm reveals the common type.

### Consumption by Later Passes

Later passes (THIR construction, MIR building) read adjustments from `TypeckResults`
to generate the appropriate intermediate operations. For instance, a `Pointer(Unsize)`
adjustment tells MIR building to generate the appropriate fat pointer construction.

## 6. Variance and Subtyping for Type Parameters

### How Variance is Computed

Variance for each type parameter is pre-computed by `rustc_hir_analysis` (the
`variances_of` query). The rules:

- `&'a T`: covariant in `'a`, covariant in `T`
- `&'a mut T`: covariant in `'a`, **invariant** in `T`
- `*const T`: covariant in `T`
- `*mut T`: invariant in `T`
- `fn(T) -> U`: contravariant in `T`, covariant in `U`
- User-defined types: computed from field usage

### Variance Composition (xform)

The `Variance::xform` method composes variances:

```rust
pub fn xform(self, v: Variance) -> Variance {
    match (self, v) {
        (Covariant, Covariant) => Covariant,
        (Covariant, Contravariant) => Contravariant,
        (Contravariant, Covariant) => Contravariant,
        (Contravariant, Contravariant) => Covariant,
        (Invariant, _) => Invariant,
        (Bivariant, _) => Bivariant,
        // ...
    }
}
```

### How Subtyping Respects Variance

In `TypeRelating::relate_with_variance`:
```rust
fn relate_with_variance<T: Relate<TyCtxt<'tcx>>>(
    &mut self, variance: ty::Variance, _info: ..., a: T, b: T,
) -> RelateResult<'tcx, T> {
    let old_ambient_variance = self.ambient_variance;
    self.ambient_variance = self.ambient_variance.xform(variance);
    let r = if self.ambient_variance == ty::Bivariant {
        Ok(a)
    } else {
        self.relate(a, b)
    };
    self.ambient_variance = old_ambient_variance;
    r
}
```

And in `relate_ty_args`:
```rust
fn relate_ty_args(...) -> RelateResult<'tcx, Ty<'tcx>> {
    if self.ambient_variance == ty::Invariant {
        // Optimization: skip fetching variances
        relate_args_invariantly(self, a_args, b_args)?;
    } else {
        let variances = self.cx().variances_of(def_id);
        combine_ty_args(self.infcx, self, a_ty, b_ty, variances, a_args, b_args, ...);
    }
}
```

### Example: Vec<&'a T> vs Vec<&'b T>

`Vec<T>` is covariant in `T` (because it only stores `T`, never takes `T` as input).
So when checking `Vec<&'a i32> <: Vec<&'b i32>`:

1. `TypeRelating` starts with `ambient_variance = Covariant` (for `sub`)
2. Looks up `variances_of(Vec)` -> `[Covariant]` for the type parameter
3. Calls `relate_with_variance(Covariant, ...)` for the type argument
4. New ambient variance = `Covariant.xform(Covariant) = Covariant`
5. Now relates `&'a i32` with `&'b i32` in a covariant context
6. References are covariant in their region, so: `Covariant.xform(Covariant) = Covariant`
7. Relates regions `'a` and `'b` covariantly -> emits constraint `'b: 'a` (i.e., `'a`
   outlives `'b` is what we need for `&'a T <: &'b T`)

For `Cell<&'a T>` vs `Cell<&'b T>`:
- `Cell<T>` is **invariant** in `T`
- `Covariant.xform(Invariant) = Invariant`
- Must prove `&'a T == &'b T`, which requires `'a == 'b`

### Bivariant Parameters

Parameters can be bivariant when they appear only in where-clauses but not in fields:
```rust
struct Foo<A, B> where A: Iterator<Item = B> { data: A }
// B is bivariant -- not directly used in fields
```

When subtyping encounters bivariant parameters, it skips relating them entirely
(`Ok(a)`) but emits `WellFormed` obligations to ensure the types are still valid:
```rust
if has_unconstrained_bivariant_arg {
    relation.register_predicates([
        ty::ClauseKind::WellFormed(a_ty.into()),
        ty::ClauseKind::WellFormed(b_ty.into()),
    ]);
}
```

## Summary of Key Architectural Decisions

1. **Subtyping is variance-parameterized**: A single `TypeRelating` struct handles eq,
   sub, and sup by varying the initial `ambient_variance`.

2. **Coercion is strictly more powerful than subtyping**: Every coercion path includes
   a subtyping check as a fallback (`self.unify(a, b)`).

3. **Match arms use incremental coercion LUB**: Each arm is coerced to the running
   merged type, with retroactive adjustment application when a later arm forces a
   different common type.

4. **Inference variables cause coercion to defer**: When types are not yet known,
   coercion emits `Coerce` predicate obligations rather than making irreversible
   decisions.

5. **Adjustments are the bridge to codegen**: The typed HIR records precisely which
   coercion steps are needed, allowing MIR construction to generate the right code
   without re-running type inference.

6. **Variance is pre-computed and consumed structurally**: The `variances_of` query
   provides per-parameter variance, which `TypeRelating` and `LatticeOp` use to
   transform their ambient variance at each recursive step.
