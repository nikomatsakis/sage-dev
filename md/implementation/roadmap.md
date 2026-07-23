# Build-Out Roadmap

This page tracks the cross-RFD status and ordering of work. Each group corresponds to
one or more RFDs. Status is one of:

- **Done** — landed and working.
- **In flight** — accepted RFD, actively being implemented.
- **Planned** — design agreed or likely, not yet started.

## Done

| Group | RFDs |
|---|---|
| Initial bootstrap (parsing, IR, module tree) | [initial-setup], [parse], [module-sym-tree], [relative-span-model] |
| Macro expansion | [macro-expansion-tracked-query] |
| Symbol system | [per-kind-symbol-data], [generic-params-as-symbols], [enum-variant-symbols], [tuple-struct-ctors] |
| Stash internals | [stash-hash-consing], [memmap-stash-allocation] |
| Type checking foundations | [type-signatures], [type-inference], [error-sentinel-type], [ir-reshape] |
| Diagnostics | [diagnostics-rendering] |
| Testing | [oracle-test-framework], [test-harness-external-crates] |
| Concurrency | [concurrent-type-checking] |

## In flight

| Group | RFDs | Notes |
|---|---|---|
| Async type checker | [async-type-checker] | Phases A-D plus solver-ready scoped tasks and body quiescence recovery landed |
| Stash hardening | [stash-safety], [stash-faster-collision-chains] | |
| Signatures & resolution | [symbol-signatures], [resolve-at-position] | |
| Numeric types | [numeric-inference-variables] | |
| Trait system | [trait-system] | Checked local trait/impl data, deterministic local impl enumeration, and function/struct/enum predicate environments landed; trait/impl item queries remain |
| Trait solving | [trait-solving] | Positive type-only local solver, active per-query whiteboard, transactional query boundary, and body obligation lifecycle landed; final RFD hardening remains |
| Typed IR elaboration | [typed-ir-elaboration] | `DbDropGuard::db` and `Parse::next` method-call slices landed; broader expression families remain |
| Method resolution | [method-resolution] | Conservative external trait and inherent method slices, actual-edition preludes, and import-edge enumeration landed; general lookup remains |
| Associated type normalization | [associated-type-normalization] | Pinned `Parse::next` behavior and trace landed; source-side `cfg` evaluation and required unrelated-edit invalidation isolation remain |

## Planned

| Group | RFDs | Notes |
|---|---|---|
| Trait impl candidate discovery | [trait-impl-candidate-discovery] | Draft design for global visible-impl discovery, mandatory trait keys, conservative self-type refinement, and incremental reuse |
| Trait solver search architecture | [trait-solver-cycle-semantics], [trait-solver-scheduling], [incremental-trait-results] | Draft design for recursive semantics and limits, fair future scheduling, and monotone progress envelopes |

[initial-setup]: ../rfds/initial-setup/README.md
[parse]: ../rfds/parse/README.md
[module-sym-tree]: ../rfds/module-sym-tree/README.md
[relative-span-model]: ../rfds/relative-span-model/README.md
[macro-expansion-tracked-query]: ../rfds/macro-expansion-tracked-query/README.md
[per-kind-symbol-data]: ../rfds/per-kind-symbol-data/README.md
[generic-params-as-symbols]: ../rfds/generic-params-as-symbols/README.md
[enum-variant-symbols]: ../rfds/enum-variant-symbols/README.md
[tuple-struct-ctors]: ../rfds/tuple-struct-ctors/README.md
[stash-hash-consing]: ../rfds/stash-hash-consing/README.md
[memmap-stash-allocation]: ../rfds/memmap-stash-allocation/README.md
[type-signatures]: ../rfds/type-signatures/README.md
[type-inference]: ../rfds/type-inference/README.md
[error-sentinel-type]: ../rfds/error-sentinel-type/README.md
[ir-reshape]: ../rfds/ir-reshape/README.md
[diagnostics-rendering]: ../rfds/diagnostics-rendering/README.md
[oracle-test-framework]: ../rfds/oracle-test-framework/README.md
[test-harness-external-crates]: ../rfds/test-harness-external-crates/README.md
[concurrent-type-checking]: ../rfds/concurrent-type-checking/README.md
[associated-type-normalization]: ../rfds/associated-type-normalization/README.md
[async-type-checker]: ../rfds/async-type-checker/README.md
[stash-safety]: ../rfds/stash-safety/README.md
[stash-faster-collision-chains]: ../rfds/stash-faster-collision-chains/README.md
[symbol-signatures]: ../rfds/symbol-signatures/README.md
[resolve-at-position]: ../rfds/resolve-at-position/README.md
[numeric-inference-variables]: ../rfds/numeric-inference-variables/README.md
[trait-system]: ../rfds/trait-system/README.md
[trait-solving]: ../rfds/trait-solving/README.md
[method-resolution]: ../rfds/method-resolution/README.md
[trait-impl-candidate-discovery]: ../rfds/trait-impl-candidate-discovery/README.md
[trait-solver-cycle-semantics]: ../rfds/trait-solver-cycle-semantics/README.md
[trait-solver-scheduling]: ../rfds/trait-solver-scheduling/README.md
[incremental-trait-results]: ../rfds/incremental-trait-results/README.md
[typed-ir-elaboration]: ../rfds/typed-ir-elaboration/README.md
[mini-redis roadmap]: ./mini-redis.md
