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
| Async type checker | [async-type-checker] | Split BodyCheck, async check_expr |
| Stash hardening | [stash-safety], [stash-faster-collision-chains] | |
| Signatures & resolution | [symbol-signatures], [resolve-at-position] | |
| Numeric types | [numeric-inference-variables] | |
| Trait system | [trait-system] | Early design phase |

## Planned

<!-- Add groups here as design solidifies -->

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
[async-type-checker]: ../rfds/async-type-checker/README.md
[stash-safety]: ../rfds/stash-safety/README.md
[stash-faster-collision-chains]: ../rfds/stash-faster-collision-chains/README.md
[symbol-signatures]: ../rfds/symbol-signatures/README.md
[resolve-at-position]: ../rfds/resolve-at-position/README.md
[numeric-inference-variables]: ../rfds/numeric-inference-variables/README.md
[trait-system]: ../rfds/trait-system/README.md
