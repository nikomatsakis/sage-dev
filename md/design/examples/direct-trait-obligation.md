# A direct trait obligation

Consider one trait, one impl, and a generic function that requires the trait:

```rust
trait Marker {}

struct Widget {}

impl Marker for Widget {}

fn require_marker<T: Marker>(value: T) {}

fn demo(widget: Widget) {
    require_marker(widget);
}
```

The logical question is `Widget: Marker`. This chapter follows that question
from the call expression to a `Yes` response. The next chapter opens up the
candidate proof itself.

## 1. The call publishes an obligation

The function signature checker lowers `T: Marker` into the function's
`CheckedParameterEnv`. When body checking resolves the callee, it instantiates
the generic signature with fresh call-site inference variables and publishes
that instantiated environment:

```{anchor}
example_instantiate_callee_obligations
```

At this instant the predicate is conceptually `?T: Marker`. Checking the
argument subsequently equates `?T` with `Widget`. The obligation manager can
attempt the non-ground form and receive `Maybe`, or observe the equality first;
either way, the committed equality wakes and re-canonicalizes the retained
obligation.

Publishing a parameter environment turns each represented predicate into an
atomic solver goal:

```{anchor}
example_publish_parameter_env
```

The solver IR separates logical structure from atomic predicates:

```{anchor}
example_solver_goal_ir
```

For the settled call-site type, the relevant value is:

```text
Goal::Atom(Atom::TraitImpl {
    self_ty: Widget,
    trait_ref: Marker<[]>,
})
```

## 2. Canonicalization makes a context-free query

Inference-variable IDs and stash pointers are local implementation details, so
they cannot be Salsa keys or shared proof-frame identities. `canonicalize_goal`
copies the environment and goal into a fresh stash, replacing free inference
variables and rigid parameters with ordered canonical parameters:

```{anchor}
example_canonicalize_goal
```

The returned pair has two sides:

- `data` is the context-free `GoalQueryData`, including the local crate,
  assumptions, canonical variables, and goal;
- `mapping` remembers how canonical inputs correspond to this caller's rigid
  parameters or inference variables.

Once `?T` has become `Widget`, this query has no canonical variables. If it is
attempted earlier, `?T` becomes an `ExistentialInput` and the mapping points
back to the caller variable.

## 3. Salsa enters the per-query proof runtime

An obligation attempt interns the canonical data, invokes the tracked `prove`
query, and transactionally applies the returned response:

```{anchor}
example_run_trait_query
```

The tracked method is deliberately thin:

```{anchor}
example_goal_query
```

`prove_query` validates the canonical input, instantiates it in a fresh
producer-owned proof context, creates the per-execution whiteboard, drives the
root future, and extracts a canonical response:

```{anchor}
example_prove_query
```

Salsa memoizes completed canonical queries. The whiteboard has a different
job: it shares in-progress atomic work within this one proof execution.

## 4. The atom gets a whiteboard frame

Every atomic goal is canonicalized at its current inference version before it
requests a frame. The completed frame response is then applied back into that
requester's version:

```{anchor}
example_prove_atom
```

The whiteboard first checks the parent chain for an inductive cycle, then looks
for an identical frame under the same parent and remaining depth:

```{anchor}
example_whiteboard_lookup
```

For this first request there is no ancestor or existing frame, so the
whiteboard starts a producer for `Widget: Marker`. Another sibling requesting
the same canonical atom under the same parent would subscribe to that producer
instead of recomputing it.

## 5. Candidate discovery finds the impl

The frame first considers matching assumptions and then local impl clauses:

```{anchor}
example_assemble_candidates
```

`impl Marker for Widget` is eligible and has the requested trait, so it becomes
the sole applicable clause. Matching its head against `Widget: Marker` succeeds
without a substitution, and its empty body is trivially true.

The implementation shown above deliberately exposes two current MVP limits:
it scans only local impls, and it scans them before filtering by trait. The
[Trait Impl Candidate Discovery
RFD](../../rfds/trait-impl-candidate-discovery/README.md) specifies the planned
global, trait-keyed query and its incremental tests.

## 6. `Yes` returns to inference

With one unconditional candidate, merging returns
`Yes { subst: [], modulo: true }`. Applying any response happens in a child
inference version: substitutions are copied and unified there, then the child
is collapsed only if the whole application succeeds.

```{anchor}
example_apply_response_transaction
```

There is no substitution to apply for this ground query. The obligation
manager sees an unconditional `Yes`, marks the obligation terminal, and body
checking can complete. A `Maybe` would instead retain the original obligation;
a `Yes` with non-trivial `modulo` would retain the residual proof condition.

The complete path is therefore:

```text
function bound
  -> instantiated call-site obligation
  -> canonical GoalQuery
  -> atomic whiteboard frame
  -> local impl candidate
  -> canonical Yes response
  -> transactional application
```
