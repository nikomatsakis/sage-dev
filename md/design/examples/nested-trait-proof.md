# A nested trait proof

Add a generic wrapper impl whose applicability depends on another trait goal:

```rust
trait Display {}

struct Name {}
struct Wrapper<T> { value: T }

impl Display for Name {}

impl<T> Display for Wrapper<T>
where
    T: Display,
{}

fn require_display<T: Display>(value: T) {}

fn demo(value: Wrapper<Name>) {
    require_display(value);
}
```

The call-site path from the preceding chapter produces the ground root goal
`Wrapper<Name>: Display`. Its successful proof tree is:

```text
Wrapper<Name>: Display
├─ impl Display for Name                       head mismatch
└─ impl<T> Display for Wrapper<T>              T := Name
   └─ where T: Display
      └─ Name: Display
         └─ impl Display for Name              success
```

This example shows why candidates have isolated inference contexts and why a
candidate body recursively returns to the whiteboard.

## 1. Opening a generic impl clause

Candidate instantiation opens every impl type parameter as a fresh variable in
the candidate's child version, copies the impl head, and lowers impl and trait
predicates into body goals:

```{anchor}
example_instantiate_impl
```

For the wrapper impl, this produces the logical clause:

```text
Display for Wrapper<?candidate_T>
    if ?candidate_T: Display
```

The fresh variable belongs to this candidate's version. It cannot escape into
the sibling trying `impl Display for Name`, and neither branch can mutate the
frame's canonical inputs directly.

## 2. Alternatives run as scoped futures

The frame creates one scoped future per assembled candidate. Each future
re-instantiates the canonical query in an independent proof context, selects
its candidate by stable index, branches a local transaction, matches the head,
and proves the complete body:

```{anchor}
example_run_trait_candidates
```

Both `Display` impls are assembled by the current trait-only filter. The
`Name` impl future fails when its head is matched against `Wrapper<Name>`; the
wrapper impl future continues. Scoped cancellation and joining ensure that an
early unconditional winner cannot leave a sibling using dropped proof state.

Head matching is transactional structural unification of the self type and
trait arguments:

```{anchor}
example_match_candidate_head
```

Unifying `Wrapper<Name>` with `Wrapper<?candidate_T>` binds only the
candidate-local variable, yielding `?candidate_T = Name`. Rebuilding makes
that equality visible when the body is read.

## 3. The where-clause becomes a nested goal

The copied body contains `?candidate_T: Display`, which now substitutes to
`Name: Display`. Candidate bodies use the ordinary structural goal path, so an
`All` body runs in another exclusive child transaction:

```{anchor}
example_prove_conjunction
```

With one conjunct, the loop proves `Name: Display` and removes it after an
unconditional `Yes`. With several where-clauses, a sibling equality or hard
hint can advance the semantic revision and cause a previously stable conjunct
to be retried. If no revision changes, remaining uncertain goals become
`Maybe`, while definite residual conditions are returned in `modulo`.

`Name: Display` is atomic, so it is canonicalized and requested from the
whiteboard with the outer frame as its parent. Its producer discovers
`impl Display for Name` and returns `Yes`. If the exact canonical query already
appeared in the ancestor chain, today's inductive cycle rule would return `No`
instead of recursing forever. The planned alternatives are discussed in the
[Trait Solver Cycle Semantics
RFD](../../rfds/trait-solver-cycle-semantics/README.md).

## 4. Candidate responses are merged

After the candidate futures complete, the frame merges their canonical
responses:

```{anchor}
example_merge_candidates
```

The failed `Name`-head alternative contributes `No`; the wrapper alternative
contributes an unconditional `Yes`. That answer wins and is applied to the
caller exactly as in the direct example.

More generally, merging keeps non-dominated conditional `Yes` answers,
preserves `Maybe` from incomplete search, and anti-unifies necessary hints when
several possibilities remain. Candidate arrival order is normalized before
the final choice, supporting the requirement that scheduling not affect the
canonical return value for fixed inputs and limits.

## What this example does not yet show

The current solver executes conjunctions sequentially even though disjunctive
candidate alternatives are futures. Fair polling, concurrent conjunctions,
and intermediate necessary-condition envelopes are planned separately in the
[Scheduling and Fairness](../../rfds/trait-solver-scheduling/README.md) and
[Incremental Trait Results](../../rfds/incremental-trait-results/README.md)
RFDs.
