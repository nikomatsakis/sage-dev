# Real Salsa contract fixture

This standalone Cargo workspace holds test-only Sage nodes and real annotated
Rust queries. `cargo test -p sage-meta --test contracts` compiles and runs it;
it can also be run with `cargo test --manifest-path
crates/sage-meta/tests/fixtures/contracts/Cargo.toml`.

The `.sage` files are loaded from `md/design/crates/` at session creation,
using the source directory configured by this fixture's `book.toml`. The Rust index is
also loaded from disk and compared with `include_str!("queries.rs")` embedded
in the test binary before source matching. Reviewed expectations in `expected/`
are written independently of that index. They name both Sage operations and
Rust declarations, intentionally using different names on either side.

The model is valid: Root may call Helper and transitively forbids Leaf; Helper
is a leaf. A runtime input deliberately causes Helper's Rust implementation to
request Leaf. Tests require both the undeclared call and ancestor prohibition,
including when Leaf's result was already cached. Other tests cover warm reuse,
internal validation reexecution, separate methods in one tracked impl, the
accumulator API, and unwinding.

The fixture consumes the reusable `sage_meta::salsa_contracts::SalsaObserver`
behind its optional `salsa` feature. The adapter is confined to one database
and one thread; unpaired source and cross-thread execution prevent a passing
report. It has no receiver projection or async propagation.
Missing, ambiguous, stale, and swapped mappings cannot produce a conformance
success. General generated-query source matching is deliberately unsupported
and tested as an explicit coverage gap. This fixture does not establish that
every compiler query or ordinary function is instrumented.
