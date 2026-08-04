# An external trait method call

The preceding examples enter the solver with a trait already named. A method
call begins one step earlier: method-name lookup must discover candidate traits
before the solver can answer a fixed-trait question.

This chapter follows the first implemented external-method vertical slice:

```rust
#[derive(Clone)]
struct Db {
    shared: bool,
}

struct DbDropGuard {
    db: Db,
}

impl DbDropGuard {
    fn db(&self) -> Db {
        self.db.clone()
    }
}
```

The source expression is a field access followed by a method call. Its completed
typed form is conceptually:

```text
Call <Db as Clone>::clone(
    RefShared[Dummy](Field DbDropGuard::db(Deref(Local self)))
)
```

## 1. Discover traits before proving one

Method resolution clones the expression scope's resolver and asks it for traits
which can participate in method lookup:

```{anchor}
example_traits_in_method_scope
```

The current slice sees traits defined directly in the module, traits introduced
by resolved explicit imports or external glob imports, and traits from the
crate's actual edition prelude. A local glob remains incomplete until its
reexport and macro-provider edges can be enumerated recursively. The edition
is retained on the crate and every module so root macro expansion and ordinary
checking make the same choice. The returned
`complete` bit is as important as the list. Unresolved expansion, active
attributes, or an unresolved import makes the source incomplete, so absence
from the returned list cannot justify `NotFound` or selecting a sole visible
candidate.

For every discovered trait, method lookup first requests its associated-item
list and filters functions by the source name:

```{anchor}
example_discover_trait_methods
```

This is name discovery, not trait solving. In particular, the solver is never
asked an existential question such as “which trait gives `Db` a method named
`clone`?”

## 2. Cross the external metadata boundary narrowly

`Clone` and `Clone::clone` belong to an external crate. `TcxDb` returns owned raw
metadata, and tracked lowering queries convert that data into Sage symbols,
types, and binders. Associated-item enumeration is separate from signature
loading:

```{anchor}
example_external_trait_items
```

Only after a function name matches does lookup request the defining trait and
function signatures. The function query is deliberately a separate incremental
boundary:

```{anchor}
example_external_fn_signature
```

This means an unrelated method body—or every signature in every prelude
trait—does not automatically become a dependency of `DbDropGuard::db`.

## 3. Ask the fixed-trait question

For the matching `Clone::clone` item, the current implementation accepts only
an eligible trait whose sole type parameter is `Self`. It constructs the
specific proposition `Db: Clone` and classifies its solver result:

```{anchor}
example_classify_trait_candidate
```

The classification entry point canonicalizes and proves that goal through the
ordinary trait solver:

```{anchor}
example_classify_fixed_trait_goal
```

This first slice is intentionally read-only. A goal containing a caller
inference variable becomes `Maybe`, as does a conditional `Yes` with a
non-trivial residual. Only an unconditional `Yes` with no caller inference
variables and a trivial residual makes the method a definite candidate. The
local impl emitted by `#[derive(Clone)]` supplies that proof for `Db`.

## 4. Instantiate and select the method

Once the fixed-trait proof succeeds, lookup loads `Clone::clone`, substitutes
`Db` for the trait's `Self`, and records a statically dispatched call target:

```{anchor}
example_instantiate_trait_method
```

Within the represented trait tier, selection accounts for incomplete
information. Exactly one definite candidate is returned only when no competing
or unsupported source—including an unhandled inherent provider—is unknown:

```{anchor}
example_select_trait_method
```

This preserves the distinction between “no method exists” and “the represented
subset cannot decide.” It also prevents trait enumeration order from choosing
among multiple applicable methods.

## 5. Consume method syntax into completed IR

Expression checking applies the selected signature, checks ordinary arguments,
submits its parameter environment, and materializes `&self` as an explicit
shared reference. The result is `ResolvedCall`, not a retained method-name node:

```{anchor}
example_elaborate_trait_method_call
```

The reference carries `Lifetime::Dummy`, matching Sage's current lifetime
boundary. The completed body now records the selected function, the static
`Db: Clone` dispatch, the resolved `DbDropGuard::db` field, the dereference of
the method's own `&self`, and the borrow required by `Clone::clone`.

## 6. Prioritize a represented external inherent method

The same name-keyed external boundary is now also a selection boundary for a
rigid external ADT. A complete empty result permits trait fallback. A nonempty
result filters visibility and requires exactly one visible function before its
signature is loaded:

```{anchor}
example_select_external_inherent_method
```

The selected signature retains how many leading binder parameters belong to
the owning impl. Method checking creates fresh variables for all represented
owner and method type generics, matches the owner self type against the
receiver, and matches every ordinary argument inside one child inference
version. The function parameter environment is staged beside that child and is
published only after commit:

```{anchor}
example_instantiate_external_inherent_method
```

For `Option<Frame>::ok_or(ParseError::EndOfStream)`, receiver matching binds
the owner `T` to `Frame`, while argument matching independently binds the
method `E` to `ParseError`. The selected call uses direct dispatch; it does not
ask trait proof to return an impl.

## 7. Retain the semantic call in shared IR

Completed calls preserve these choices instead of requiring a downstream
consumer to reconstruct them. The shared dispatch variant distinguishes a
direct inherent call from static trait dispatch:

```{anchor}
example_call_dispatch_schema
```

The call itself retains owner and method substitutions separately:

```{anchor}
example_call_substitution_schema
```

Sage emits those fields from `ResolvedCallTarget`; the rustc oracle projects
its selected associated item and node substitutions into the same schema
without reading Sage output. The isolated `Parse::next` test checks the
semantic fields and then requires both independent trees to equal one exact
snapshot:

```{anchor}
example_exact_parse_next_snapshot
```

## Current boundary

This is a deliberately narrow vertical slice, not the complete method algorithm
specified by the [Method Resolution RFD](../../rfds/method-resolution/README.md).
Local-glob provider enumeration, explicit-bound provider enumeration, local
and builtin inherent-method selection, generic trait methods, receiver
autoderef, conditional candidates, and retained lookup obligations remain
planned. The current function does not resolve local or builtin inherent
methods. It conservatively reports uncertainty when a local
inherent impl could provide a competitor. For a rigid external ADT, it can
audit or select from complete same-name receiver-bearing metadata; other
external or builtin receiver forms remain unenumerated. Every unrepresented
candidate source stays uncertain rather than becoming a false unique result.
