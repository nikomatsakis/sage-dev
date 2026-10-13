# Body Checking and Typed-IR Elaboration

Body checking resolves one function body, infers its expression and pattern
types, proves required obligations, and returns completed tree-structured
[Typed IR](typed-ir-guide.md). Source conveniences are consumed: a successful
result records resolved definitions and explicit semantic operations rather
than adjustment side tables.

## Contract

### Granularity

The unit of demand and memoization is one local function or associated-function
body. Temporary inference versions, method candidates, and partially checked
expressions are internal to that query.

### Input

The direct key is `LocalFnSym`. The phase reads the function's body CST and
checked signature, referenced item interfaces, name-resolution results,
external metadata, and canonical trait-solver results required by this body.
It must not read callee bodies.

### Output

`CheckedBody` contains a stash-owned `TyBody` plus diagnostics. On successful
completion every expression and pattern has a final type, paths and fields
refer to semantic identities, method calls are elaborated to resolved call
targets, required dereferences/borrows/coercions are explicit, and no live
inference or obligation work remains.

The output remains tree-structured, between rustc THIR and MIR in intent. It
does not contain control-flow lowering or borrow-checking results.

### Guarantees

Downstream consumers may traverse one self-contained typed tree without
consulting adjustment side tables. Generic substitutions and static trait
dispatch are attached to call targets. Diagnostics accompany recovery nodes;
a body with errors is not evidence of successful type checking.

{spec}`crates.sage_ir.checking.body.elaboration`

Verification intent: Structural Typed-IR snapshots cover paths, fields,
calls, generic substitutions, borrows, dereferences, coercions, and recovery;
completion checks reject every unresolved or live internal form; and selected
slices match the independent rustc oracle by exact text.

{spec}`crates.sage_ir.checking.body.interface_dependencies`

Verification intent: Cold traces whitelist the semantic facts used by a
representative body and reject every callee-body or unrelated-interface
read; an unchanged warm request performs no work; and persistent edits
distinguish relevant interface changes from unrelated body changes.

## Entry points

`LocalFnSym::body(db)` is the tracked phase boundary:

```{anchor}
example_fn_body
```

The method imports the signature, installs generic/value bindings and the
solver environment, asynchronously checks the body expression, relates it to
the declared return type, and finalizes inference and obligations. Completion
then resolves and structurally copies the working tree into a fresh output
stash before freezing the result.

## Construction

An `InferCtx` owns the body output stash, egraph, obligation registry, wake
queues, and diagnostic sink. A `Scope` owns lexical ribs and module
resolution. Expression and pattern checking proceed asynchronously so method
and trait candidates can perform solver work without exposing partial public
results.

Inference operations use child versions transactionally: speculative
equalities collapse into their parent only after the whole operation succeeds.
Trait operations cross a canonical Salsa boundary and import responses back
into the caller. Finalization applies fallback, retries woken obligations,
runs a mandatory terminal obligation pass, and asserts quiescence. A final
copy consults the settled egraph for every type edge and constructs
`CheckedBody` in a separate append-only stash; the transient inference stash
is never rewritten into an apparent result.

```{anchor}
finalize_typed_body_into_fresh_stash
```

{spec}`crates.sage_ir.checking.body.isolated_completion`

Verification intent: Rejected-candidate tests observe no leaked generic
bindings, wakes, or obligations; accepted-candidate tests observe one atomic
commit; unresolved residual obligations become diagnostics; and successful
completion asserts an empty runtime, wake queue, obligation registry, and
root transaction set.

Elaboration happens while checking. For example, a selected trait method call
becomes a `ResolvedCall` whose argument contains explicit reference and
dereference nodes:

```{anchor}
example_elaborate_trait_method_call
```

Expression checking is bidirectional. A context which knows an expected type
passes that expectation into expression checking, where syntax-directed rules
may use it to guide inference and a coercion site may materialize an explicit
conversion. An expectation constrains the resulting expression; it does not
select a trait or alias-normalization candidate which would otherwise be
ambiguous.

{spec}`crates.sage_ir.checking.body.expected_types`

Verification intent: Source-driven tests show expected types guide
representative call arguments, literals, closures, and aggregate or branch
elements; completed Typed IR contains any selected conversions; and an
incompatible expected type cannot choose among otherwise ambiguous solver or
method candidates.

The first implementation establishes this boundary for ordinary direct-call
arguments. It synthesizes the argument and then applies the parameter type as
a coercion expectation; expression-specific inward propagation is incremental:

```{anchor}
initial_bidirectional_expression_checking
```

## Binary operator checking and elaboration

Except for lazy `&&` and `||`, binary syntax begins as a retained
operator-trait operation. Checking creates the applicable trait goal,
instantiates its method signature, and relates the expression type to the
method result. The operator token alone neither narrows operand domains nor
equates their types: a selected user-defined impl may accept arbitrary operand
types. It selects only the operator trait and method. Sage obtains those
identities through cached well-known-item queries: for example, the query for
the addition trait resolves `core::ops::Add` through ordinary path lookup, then
finds `add` among that trait's associated items. The fixed path and item name
bootstrap semantic identity; they do not encode an operator signature or
primitive implementation. Candidate discovery uses the ordinary local and
external impl path, so primitive arithmetic and shift implementations arrive
as `core` impl metadata rather than synthetic builtin candidates. This first
slice adds no hardcoded operator-specific inference relationships. Any future
extraction of facts shared by several ambiguous candidates must use the actual
complete candidate answers through the general inference and solver design.

The cached identity boundary is independent from body checking:

```{anchor}
well_known_operator_queries
```

After inference and trait selection settle, every proven non-short-circuit
operator becomes a resolved static trait-method call with its substitutions,
explicit adjustments, and result type. This includes operations over concrete
primitive types and generic parameters. A generic body may be instantiated
with either a primitive or user-defined implementation, so body checking does
not classify the call by its current operand representation. Future codegen
may recognize a supported primitive operator call after substitutions are
concrete and lower it specially.

A failed or ambiguous trait operation produces a terminal diagnostic and
recovery node. Lazy boolean operators bypass trait selection and preserve
short-circuit evaluation. The accepted [Numeric Inference Domains and
Binary Operator Typing RFD][numeric-rfd] gives the complete first-slice design,
and the [rustc operator research][rustc-operator-research] records rustc's
additional inference hints as descriptive precedent, not this slice's
contract.

{spec}`crates.sage_ir.checking.body.operator_calls`

Verification intent: Table-driven checks cover every represented
operator-to-trait mapping, including different integer types for shift
operands. Query traces show primitive proofs loading ordinary `core` impl
identities and headers from external metadata, with no synthetic primitive
candidate. Source-driven tests and independent rustc oracle cases cover
representative positive and negative expressions; concrete primitive,
generic, nonnumeric shift, and user-defined calls; non-`Self` `Rhs` and
`Output`; preservation of `Any` domains before proof selection; lazy boolean
short-circuit nodes; and rejection of `1 + true`.

## Failure and terminal incompleteness

User type errors produce diagnostics and error-typed recovery nodes. An
unsupported expression, incomplete method-provider set, ambiguous trait
answer, unavailable external fact, or inference/solver resource limit likewise
prevents the successful-output guarantee. These are terminal outcomes for the
body query, not pending background work.

Non-ground trait goals may soundly terminate as ambiguous even when a proof
exists. Ground goals must be sound and complete modulo documented overflow and
term-size limits. Body checking turns an unresolved required obligation into a
diagnostic before it returns.

This terminal treatment is the body-phase application of
[Incompleteness is an explicit terminal outcome](../../../rfds/architectural-foundations/README.md#incompleteness-is-an-explicit-terminal-outcome).

## Incremental dependencies

The key is the function symbol. The query may read its body and signature,
interfaces of referenced definitions, relevant name-resolution and candidate
queries, and canonical solver results. It must not read uncalled/unreferenced
interfaces merely because they share a crate, and it must never read a callee
body.

An edit to a referenced signature or relevant impl set should invalidate the
body. An unrelated body edit should not. Repeating the same request in an
unchanged database must reuse the result without rereading external metadata.

## Worked example

For:

```rust
fn first<T>(pair: Pair<T>) -> T {
    pair.first
}
```

the body query imports `Pair<T_first>`, binds `pair`, resolves the field owner
to the `Pair` symbol, substitutes the struct's parameter with `T_first`, and
returns a field expression typed `T_first`. The [function-body
walkthrough](../../examples/function-body.md) follows each boundary.

The more demanding [oracle-checked method
body](../../examples/oracle-checked-method.md) shows `self.db.clone()` elaborated
to static `Clone::clone` dispatch and compared exactly with rustc output.

## Code map

| Path | Responsibility |
|---|---|
| `local_syms/fns.rs` | body query orchestration |
| `check/infer_ctx.rs` and `check/infer/` | inference state, transactions, obligations, finalization |
| `check/expr.rs` | expression/pattern checking and elaboration |
| `check/operator.rs` | binary-token mapping, operator-signature instantiation, and provisional call elaboration |
| `check/method.rs` | method candidate discovery, selection, and instantiation |
| `check/solve/` | canonical trait proof and alias normalization |
| `tytree/` | completed typed body representation |
| `well_known.rs` | cached fixed-path operator trait and associated-method identities |

## Implementation and test evidence

The generated [anchor pages](../../anchor/crates/sage_ir/checking/body.html) report implementation entries,
annotated tests, explicit TODO/N/A gaps, and saved claim/test reviews. These
associations do not themselves establish that a test ran.

The [earlier coverage snapshot](../../../rfds/implementation-sequencing/coverage-history.md#body-checking-and-typed-ir-elaboration)
preserves the former capability descriptions, edit experiments, and limitations
as historical evidence. Current coverage comes from the generated entries;
implementation plans and decision rationale belong in the relevant RFDs.
