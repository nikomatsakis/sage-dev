# Struct signature

Start with a generic struct:

```rust
struct Pair<T> {
    first: T,
    second: T,
}
```

This example stops before expression inference. It shows the two-stash flow:
the parser produces a self-contained CST stash, and signature queries produce
new checked stashes whose pointers never refer back into the CST stash.

## The parsed CST

The parser's root points at `StructCstData`. Its slices and pointers belong to
the same `Stashed` value:

```{anchor}
example_struct_cst
```

For `Pair`, `generics` contains the CST for `T`, while `fields` contains two
field records whose type CSTs are both the path `T`. These are syntax-level
objects; `T` does not yet denote a `GenericParam`.

## Checking the signature

`LocalStructSym::sig(db)` is the first semantic query:

```{anchor}
example_struct_sig
```

The important transitions are:

1. `Check` opens the CST stash as read-only input and creates a fresh target
   stash.
2. `CheckGenerics::check` mints the semantic generic parameter for `T` and
   installs it in the resolver's ribs.
3. `lower_predicates` checks any where-clauses in that same scope.
4. `Binder::new` records which semantic parameters bind the `StructSig`.
5. `finish` freezes the target stash and root together as a `Stashed` result.

The signature query owns parameter identity. Later queries retrieve these same
parameters through `self.sig(db)` rather than minting lookalikes.

## Checking the fields

Fields are a separate query because editing a field should not necessarily
invalidate every consumer of the struct's generic environment:

```{anchor}
example_struct_fields
```

The query imports the signature's parameters into fresh ribs. Consequently,
checking each field path `T` produces `Ty::Param(T)` using the exact parameter
owned by `sig(db)`. The resulting `Stashed<StructFields>` has two `FieldSig`
entries, both pointing at that semantic type in the fields query's target
stash.

The next example follows those checked field types into a function body.
