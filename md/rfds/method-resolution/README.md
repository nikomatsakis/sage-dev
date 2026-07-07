# RFD: Method and Trait Resolution

**Status:** Draft

**Depends on:**
- [Trait System](./trait-system.md) — `TraitRef`, `WherePredicate`, `ImplSignature`
- [Type Inference](./type-inference.md) — `InferCtx`, `VersionedEGraph`, inference variables
- [Per-kind symbol data](./per-kind-symbol-data.md) — `FnSymbol`, `ImplSymbol`, `TraitSymbol`

## Problem

The type checker currently stubs out method calls — it resolves expressions to `fresh_ty_var()` without actually finding the method. Similarly, function calls on paths work only for free functions (resolving `FnSymbol::Local` via `def_to_ty`), but cannot resolve associated functions (`Vec::new`, `String::from`) or methods on receivers.

This blocks:

1. **`x.foo()` method calls** — the checker produces a fresh var instead of the method's return type.
2. **Associated function calls** — `Foo::bar()` where `bar` is in an `impl Foo` block.
3. **Trait method calls** — `x.foo()` where `foo` is defined by a trait and dispatched through an impl.
4. **Generic bounds** — `fn f<T: Clone>(x: T) { x.clone() }` cannot find `clone` on `T`.
5. **UFCS** — `<Foo as Bar>::method()`.

## Scope

This RFD covers the **resolution algorithm**: given a receiver type (or a path like `Foo::bar`) and a method name, how do we find the `FnSymbol` to call? It covers:

- The inherent method index (which methods are available on a given type)
- Trait method resolution (which impl provides a method for a given type)
- Auto-ref and auto-deref during method lookup
- The interaction with inference variables (awaiting bounds)

It does NOT cover:

- The trait solver itself (proving arbitrary `T: Trait` goals) — that's a separate RFD
- Coherence checking / overlap detection
- Associated types (beyond what's needed to instantiate method signatures)

## Design

### Phase 1: Inherent method index

The first, most impactful step is resolving methods defined in `impl Foo { ... }` blocks — no traits involved.

#### The query: `inherent_methods`

```rust
/// For a given type-defining symbol (struct, enum), return all methods
/// from all inherent impl blocks visible in the current crate.
#[salsa::tracked]
fn inherent_methods<'db>(
    db: &'db dyn Db,
    sym: Symbol<'db>,
    source_root: SourceRoot,
) -> Stashed<InherentMethodTable<'db>>;

struct InherentMethodTable<'db> {
    /// Methods indexed by name for fast lookup.
    methods: FxHashMap<Name<'db>, Vec<InherentMethod<'db>>>,
}

struct InherentMethod<'db> {
    fn_sym: FnSymbol<'db>,
    /// The impl this method comes from (needed for generic instantiation).
    impl_sym: ImplSymbol<'db>,
}
```

**Building the index:** Walk all items in the module tree (expanded module), collect `LocalImplSym` entries whose `self_ty` resolves to the target symbol (ignoring generics — just matching the head type constructor). For each impl, collect its function items.

This is a crate-level index: one query per type-defining symbol, recomputed when any impl block in the crate changes (salsa tracks the module items). The index is keyed by `Name` so method lookup is O(1).

#### Resolving `x.method(args)`

```rust
fn resolve_method<'db>(
    cx: &InferCtx<'_, 'db>,
    receiver_ty: Ptr<Ty<'db>>,
    method_name: Name<'db>,
) -> MethodResolution<'db>;

enum MethodResolution<'db> {
    Found {
        fn_sym: FnSymbol<'db>,
        impl_sym: ImplSymbol<'db>,
        /// How the receiver was adjusted (deref steps + autoref).
        adjustments: ReceiverAdjustments,
    },
    /// Receiver is an inference variable — caller should await.
    NeedsMoreInfo,
    NotFound,
}
```

The resolution algorithm (for Phase 1, inherent only):

1. **Canonicalize the receiver.** Follow the egraph to find the canonical type.
2. **If the receiver is `InferVar`** — return `NeedsMoreInfo`. The caller will await the variable's bound.
3. **Extract the head type constructor.** For `Ty::Adt(sym, _)` → `sym`. For primitives, use the corresponding lang item / intrinsic symbol.
4. **Look up in the inherent method table** for that symbol.
5. **Auto-deref chain.** If not found, try one layer of deref:
   - `&T` → try methods on `T`
   - `&mut T` → try methods on `T`
   - `Box<T>` → try methods on `T`
   - Custom `Deref` impls are Phase 2 (requires trait solving).
6. **Auto-ref.** If not found on `T`, try `&T` and `&mut T` as receivers. This handles the common case where the method takes `&self` but the caller has an owned value.

**Priorities** (Rust's method resolution order):
1. Direct inherent methods on the exact receiver type
2. Methods found via auto-deref (one step at a time)
3. Methods found via auto-ref (`&self`, `&mut self`)
4. Trait methods (Phase 2)

### Phase 2: Trait method resolution

Once we have trait signatures (from the trait-system RFD) and a basic solver, we can resolve trait methods.

#### Where-clause methods

The simplest case: methods available from where-clauses on the current function.

```rust
fn f<T: Iterator>(x: T) {
    x.next(); // T: Iterator gives us access to Iterator::next
}
```

Resolution:
1. After failing inherent lookup, scan the in-scope where-clauses.
2. For each `T: Trait` bound where `T` matches the receiver type, check if `Trait` has a method with the target name.
3. If found, the method signature comes from the trait definition, instantiated with the bound's type arguments.

This doesn't require a solver — just matching the receiver against declared bounds.

#### Impl dispatch (requires solver)

For concrete types:
```rust
let v: Vec<i32> = Vec::new();
v.iter(); // Vec<i32>: IntoIterator (or Iterator methods via deref to slice)
```

Resolution:
1. After failing inherent + where-clause lookup, query the solver: "does `ReceiverTy: ?Trait` where `?Trait` has method `name`?"
2. The solver searches all trait impls in scope, finds matching ones, and returns the impl + trait.
3. Instantiate the method signature from the impl.

This is the most complex case and requires the trait solver infrastructure. Deferred to after the solver RFD is written.

### Instantiating method signatures

Once we have a method, we need to produce its type in the current inference context:

```rust
fn instantiate_method<'db>(
    cx: &InferCtx<'_, 'db>,
    method: &InherentMethod<'db>,
    receiver_ty: Ptr<Ty<'db>>,
    type_args: &[Ptr<Ty<'db>>],  // explicit turbofish, if any
) -> InstantiatedMethodSig<'db>;

struct InstantiatedMethodSig<'db> {
    /// Parameter types (excluding self — already matched against receiver).
    params: Vec<Ptr<Ty<'db>>>,
    /// Return type.
    ret: Ptr<Ty<'db>>,
}
```

Steps:
1. Load the method's `FnSig` from its `FnSymbol`.
2. Load the impl's generic parameters (from `ImplSignature`).
3. **Unify the impl's `Self` type against the receiver** to infer the impl's type parameters. For example, if the impl is `impl<T> Vec<T>` and the receiver is `Vec<i32>`, unify `Vec<T>` with `Vec<i32>` → `T = i32`.
4. **Substitute** the impl's type parameters into the method signature.
5. For any remaining method-level generic parameters (from the fn's own generics), allocate fresh inference variables (or use explicit turbofish args).
6. Return the instantiated parameter and return types.

### Interaction with inference variables

When the receiver is not yet known (e.g., `let x = foo(); x.bar()`), the type checker must wait:

```rust
ExprCstKind::MethodCall(obj, name, args) => {
    let ro = obj.check_with(cx, scope).await;
    let receiver_ty = cx.find_mut(cx.stash()[ro].ty);
    
    match resolve_method(cx, receiver_ty, *name) {
        MethodResolution::Found { fn_sym, impl_sym, adjustments } => {
            // Check args against the instantiated signature
            let sig = instantiate_method(cx, fn_sym, impl_sym, receiver_ty, &[]);
            // ... check each arg against sig.params ...
            (TyExprData::MethodCall(ro, *name, args_slice), sig.ret)
        }
        MethodResolution::NeedsMoreInfo => {
            // In Phase 1: just return fresh var (same as today)
            // In Phase 2 (async): await the receiver's bound, then retry
            let ty = cx.fresh_ty_var();
            (TyExprData::MethodCall(ro, *name, args_slice), ty)
        }
        MethodResolution::NotFound => {
            // Record diagnostic, return error type
            cx.record(Diagnostic::error(..., "no method `{name}` on type ..."));
            let ty = cx.alloc_ty(Ty::Error(e));
            (TyExprData::MethodCall(ro, *name, args_slice), ty)
        }
    }
}
```

The async version (Phase 2, once the runtime supports it) would look like:

```rust
let receiver_ty = loop {
    let ty = cx.find_mut(cx.stash()[ro].ty);
    if !ty.is_infer_var() { break ty; }
    cx.await_bound(ty).await;
};
```

### Auto-deref and auto-ref

Rust's method resolution performs a deref chain followed by auto-ref at each step. For the initial implementation, we simplify:

**Phase 1 (no Deref trait):**
- Strip one `&` / `&mut` from the receiver → try inherent methods on the inner type.
- If receiver is `T` (not a reference), try methods that take `&self` by auto-refing to `&T`.

**Phase 2 (with Deref trait):**
- Full deref chain: `T` → `<T as Deref>::Target` → `<<T as Deref>::Target as Deref>::Target` → ...
- At each step, try the method with `self`, `&self`, `&mut self` receivers.
- Stop at a fixed depth (or when Deref is not implemented).

### Associated functions (UFCS and qualified paths)

For `Foo::bar()` or `<Foo as Trait>::bar()`:

```rust
ExprCstKind::Path(path) where path has multiple segments => {
    // e.g., path = ["Vec", "new"] or ["Iterator", "next"]
    // 1. Resolve the type prefix ("Vec" → StructSymbol)
    // 2. Look up the method name in inherent_methods for that symbol
    // 3. If not found, look in trait impls for that type
}
```

This reuses the same `inherent_methods` index, just entered from the type path rather than from a receiver expression.

## Implementation plan

### Step 1: `inherent_methods` query

Build the crate-level index from `impl` blocks → methods, keyed by self-type symbol. This needs:
- Walking the expanded module to find all `LocalImplSym` entries
- For each impl, resolving its `self_ty` CST to determine the head symbol
- Collecting the `FnSymbol` items from the impl's item list

### Step 2: Basic method resolution (inherent only, known types)

Wire `resolve_method` into the `MethodCall` arm of `check_expr`. When the receiver type is a known `Ty::Adt(sym, args)`, look up `inherent_methods(sym)` and find the method by name. Instantiate its signature against the receiver's type args.

### Step 3: Auto-ref

When the method takes `&self` or `&mut self`, match against the receiver type correctly. This means reading the first parameter of the method sig and checking if it's `&Self` or `&mut Self`.

### Step 4: Where-clause methods

Once the trait-system data model is in place: scan the function's where-clauses for `T: Trait` bounds, check if `Trait` defines the method.

### Step 5: Trait impl dispatch

Once the trait solver exists: full trait method resolution for concrete types.

## Open questions

1. **Ext symbols (from dependencies).** The `inherent_methods` query works for local impls. For external crates, we need to query the `SymExt` metadata for methods. How does the ext symbol system expose impl blocks and their methods? (This likely needs new `SymExtKind` infrastructure or a separate query for ext methods.)

2. **Multiple candidates.** When multiple methods match (e.g., from different impls, or inherent vs trait), what are the priority rules? Rust's are: inherent wins over trait; more specific impl wins; ambiguity is an error.

3. **Self type matching.** For `impl<T> Foo<T>`, the self type is `Foo<T>` with a free generic. Matching this against `Foo<i32>` is unification, not equality. Do we reuse the egraph's `require_eq` for this, or implement a lighter-weight pattern match?

4. **Visibility.** Should private methods be visible during resolution (and rejected later), or filtered out during the index build?

5. **Method-level generics.** `foo.bar::<u32>()` (turbofish on methods) — how do we parse and thread explicit type args to `instantiate_method`?

6. **Primitive types.** Methods on `i32`, `str`, `[T]`, etc. need to resolve to impls that aren't in user code. These likely come from the extern prelude / lang-item system.
