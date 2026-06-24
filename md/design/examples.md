# Examples

Three progressive walk-throughs showing how sage processes code from
source to typed IR.

---

## Example 1: Struct signature

```rust
struct Pair<T> { first: T, second: T }
```

### CST

Lowering produces a `StructCstData` stored in a per-item stash:

```rust
pub struct StructCstData<'db> {
    pub attrs: Slice<AttrCst<'db>>,
    pub name: Name<'db>,                     // "Pair"
    pub generics: Slice<GenericParamCst<'db>>, // [Type { name: "T", .. }]
    pub fields: Slice<FieldCst<'db>>,        // [{ name: "first", ty: Path(T) }, ...]
    pub where_clauses: Slice<WhereClauseCst<'db>>,
    pub span: RelativeSpan,
}
```

The `StructCst` type alias is `Stashed<Ptr<StructCstData<'db>>>` — a
self-contained bundle of the stash and a root pointer.

### `LocalStructSym::sig(db)`

```rust
let (src, cst) = self.cst(db).open_deref();
let mut cx = Check::new(src, Resolver::new(db, self.scope(db)));

let parent: Symbol<'db> = self.into();
let generics = cst.generics.check(db, &mut cx, parent);

let struct_sig = StructSig { dummy: PhantomData };
let binder = Binder::new(struct_sig, generics);
cx.finish(binder)
```

Step by step:

1. **Open CST.** `self.cst(db).open_deref()` returns a reference to the
   stash (`src`) and the dereferenced `StructCstData`.

2. **Create context.** `Check::new(src, resolver)` sets up the two-stash
   bridge: `src` is read-only input, `target_stash` is fresh output.

3. **Mint generic params.** `cst.generics.check(db, &mut cx, parent)`
   iterates over `[GenericParamCst::Type { name: "T", .. }]`, creates
   an `AstGenericParam` tracked struct for each, wraps it in
   `GenericParam::Ast(...)`, adds it to ribs (`RibEntry::Param`), and
   allocates the list into `target_stash`.

4. **Wrap in Binder.** `Binder::new(struct_sig, generics)` pairs the
   (currently empty) `StructSig` payload with the generics slice.

5. **Finish.** `cx.finish(binder)` produces `Stashed<Binder<StructSig>>` —
   the output stash frozen with its root value.

### `LocalStructSym::fields(db)`

```rust
let (src, cst) = self.cst(db).open_deref();
let mut cx = Check::new(src, Resolver::new(db, self.scope(db)));

// Bring generic params from sig into scope
cx.resolver.ribs.add_generic_params(db, self.sig(db).iter_symbols());

let field_sigs: Vec<_> = src[cst.fields]
    .iter()
    .map(|f| {
        let ty_val = cx.src[f.ty].check(&mut cx);
        let ty = cx.target_stash.alloc(ty_val);
        FieldSig { name: f.name, ty }
    })
    .collect();
let fields = cx.target_stash.alloc_slice(&field_sigs);
cx.finish(StructFields { fields })
```

Key point: `self.sig(db).iter_symbols()` retrieves the *same*
`GenericParam` symbols minted by `sig()`. They are added to ribs so
that when `TypeCst::check` encounters the path `T`, it resolves to
`Resolution::Param(param)` and produces `Ty::Param(param)`.

Result: `Stashed<StructFields>` containing two `FieldSig` entries,
each with `ty: Ptr<Ty::Param(T)>`.

---

## Example 2: Function that reads struct fields

```rust
fn first<T>(p: Pair<T>) -> T { p.first }
```

### `LocalFnSym::sig(db)`

Same pattern as the struct:

```rust
let (src, cst) = self.cst(db).open_deref();
let mut cx = Check::new(src, Resolver::new(db, self.scope(db)));

let parent: Symbol<'db> = self.into();
let generics = cst.generics.check(db, &mut cx, parent);  // mints T

// Lower parameter types
let param_tys: Vec<_> = cx.src[cst.params]
    .iter()
    .map(|p| {
        let ty = cx.src[p.ty].check(&mut cx);  // TypeCst for Pair<T>
        cx.target_stash.alloc(ty)
    })
    .collect();
let params = cx.target_stash.alloc_slice(&param_tys);

// Lower return type
let ret_ty = cx.src[cst.ret.unwrap()].check(&mut cx);  // TypeCst for T -> Ty::Param(T)
let ret = cx.target_stash.alloc(ret_ty);

let fn_sig = FnSig { params, ret };
let binder = Binder::new(fn_sig, generics);
cx.finish(binder)
```

When `TypeCst::check` processes the path `Pair<T>`:

1. `PathCst::resolve(cx, Namespace::Type)` looks up `Pair` — ribs
   miss, falls through to `resolver.resolve_segments`.
2. Resolver walks the module's MEM-map, finds `MemmapEntry::Item(struct_sym)`.
3. Returns `Resolution::Sym(pair_symbol)`.
4. `TypeCst::check` also processes the type argument `T` -> `Ty::Param(T)`.
5. Final type: `Ty::Adt(pair_symbol, [Ptr<Ty::Param(T)>])`.

### `LocalFnSym::body(db)`

```rust
let sig = self.sig(db);
let (src, cst) = self.cst(db).open_deref();
let mut bx = BodyCheck::new(db, src, Resolver::new(db, self.scope(db)));

// Bring generics into scope
bx.resolver.ribs.add_generic_params(db, sig.iter_symbols());

// Import sig types into body stash
let imported = bx.import_fn_sig(&sig);

// Bind parameters as locals
bx.bind_params(&imported.params, params_cst);

// Walk body CST
let body_expr = src[cst.body.unwrap()].check(&mut bx);

// Constrain body type against return type
bx.require_coerce(body_ty, imported.ret);

// Resolve inference variables
bx.finalize();

bx.finish(body_expr, cst.span)
```

**The field access `p.first`:**

1. The body expression CST is `ExprCst::Field(ExprCst::Path("p"), "first")`.

2. `ExprCst::Path("p")` resolves: ribs contain `p` as `RibEntry::Local(0)`.
   Result: `TyExprData::Path(Res::Local(LocalId(0)))` with type = the
   imported `Pair<T>` type.

3. For `ExprCst::Field(base, "first")`, body checking:
   - Looks at `base.ty` = `Ty::Adt(pair_sym, [T])`.
   - Queries `pair_sym.fields(db)` to get `StructFields`.
   - Finds field `"first"` with type `Ty::Param(T_pair)`.
   - Substitutes the struct's generic params with the actual type args:
     `T_pair` -> `T_fn` (the function's own `T`).
   - Allocates a fresh inference variable, constrains it equal to the
     substituted field type.

4. Result: `TyExprData::Field(base_ptr, "first")` with `ty` pointing
   to a `Ptr<Ty>` that, after finalization, resolves to `Ty::Param(T)`.

**Finalization:** `bx.require_coerce(body_ty, imported.ret)` constrains
the body's type against the declared return type `T`. Since both are
the same param, this is a no-op. `bx.finalize()` walks all inference
variables, promoting `Bound::AtLeast` to `Bound::Exactly` and reporting
errors for unresolved variables.

Output: `TyBody` = `Stashed<Ptr<TyBodyData>>` containing the typed
expression tree and local variable metadata.

---

## Example 3: Macro-generated struct

```rust
mod shapes {
    macro_rules! define_shape {
        ($name:ident, $field:ident, $ty:ty) => {
            pub struct $name { pub $field: $ty }
        };
    }

    define_shape!(Circle, radius, f64);
}

fn area(c: shapes::Circle) -> f64 {
    c.radius * c.radius * 3.14159
}
```

### MEM-map expansion

When `expanded_module` runs for the `shapes` module:

1. **Seeding.** `seed::seed_from_items` converts parsed items into
   `MemmapEntry` values:
   - `MemmapEntry::MacroDef(define_shape_sym)` for the `macro_rules!`
   - `MemmapEntry::MacroUse(MacroUse { path: ["define_shape"], input: ..., expansions: [] })`

2. **Expansion loop.** `expand::resolve_and_expand_macros` iterates:
   - Resolves `define_shape` in the module's own entries -> finds
     `MacroCallee::Rules(define_shape_sym)`.
   - Expands the macro against its input tokens.
   - Produces a `StructCstData` for `Circle` and a corresponding
     `LocalStructSym`.
   - Records `Expansion { callee: Rules(define_shape_sym), entries: [Item(circle_struct_sym)] }`.

3. **Result.** The MEM-map now has:
   ```
   [MacroDef(define_shape), MacroUse { ..., expansions: [Expansion { entries: [Item(Circle)] }] }]
   ```

### Resolution from the function

When `LocalFnSym::sig(db)` for `area` processes the type
`shapes::Circle`:

1. `TypeCst::check` encounters `TypeCstKind::Path(path_ptr)`.
2. `PathCst::resolve(cx, Namespace::Type)` is called.
3. Path segments are `["shapes", "Circle"]`.

4. **Ribs miss.** No rib entry for `shapes`.

5. **Module-level resolution.** `resolver.resolve_segments(&names, Namespace::Type)`:
   - `dispatch_first_segment`: resolves `shapes` in the current module's
     MEM-map -> finds `MemmapEntry::Item(shapes_mod_sym)`.
   - Converts to `ModSymbol` via `symbol_to_module` -> calls
     `resolve_mod` to get the resolved module.
   - `resolve_remainder`: resolves `Circle` in the `shapes` module's
     MEM-map.

6. **MEM-map lookup.** `resolve_member_impl` calls `expanded_module(db, shapes_ast, source_root)`.
   The MEM-map was already computed (step above). `walk_entries` finds
   `Circle` inside the `MacroUse`'s expansion entries.

7. **Result.** `Resolution::Sym(circle_sym)` -> `Ty::Adt(circle_sym, [])`.

The key insight: the function never "sees" the macro. It only sees the
expanded MEM-map entries. The macro expansion is fully encapsulated
inside `expanded_module`.
