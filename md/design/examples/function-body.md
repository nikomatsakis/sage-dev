# Function body and field access

Build on `Pair<T>` with a function that reads one field:

```rust
struct Pair<T> {
    first: T,
    second: T,
}

fn first<T>(pair: Pair<T>) -> T {
    pair.first
}
```

This adds two layers: checking a function signature and importing that
signature into an inference context for the body.

## Function signature

`LocalFnSym::sig(db)` follows the same two-stash pattern as the struct query,
but also lowers parameter and return types:

```{anchor}
example_fn_sig
```

When it checks `Pair<T>`, resolution finds the `Pair` symbol in the module and
finds the function's `T` in the ribs. The checked parameter type is therefore
an ADT application whose argument is the function parameter:

```text
Ty::Adt(Pair, [Ty::Param(T_first)])
```

The return type is the same `Ty::Param(T_first)`. Any function where-clauses
are lowered into the signature's `CheckedParameterEnv` beside those types.

## Entering the body checker

The body query imports the signature into the body stash, installs its
parameter environment, binds the value parameter `pair`, and runs the async
expression checker:

```{anchor}
example_fn_body
```

The source CST stash and body target stash remain separate. `import_fn_sig`
copies the checked types into the target before any expression can retain
them. The declared return type becomes a constraint on the completed body
expression, and finalization resolves or reports remaining inference
variables.

## Resolving `pair.first`

The expression dispatcher first checks `pair`, resolves its current type, and
then delegates the field lookup:

```{anchor}
example_check_field_expression
```

The lookup recognizes `Pair<T_first>` as a local struct, queries the checked
fields, and substitutes the ADT's actual arguments for the struct signature's
own parameters:

```{anchor}
example_lookup_field_type
```

There are two distinct parameters named `T`: one belongs to `Pair`, and one
belongs to `first`. The substitution maps `T_pair` to `T_first`; it is semantic
identity, not spelling, that keeps them apart. The returned field type is thus
`Ty::Param(T_first)`, which agrees with the declared return type.
