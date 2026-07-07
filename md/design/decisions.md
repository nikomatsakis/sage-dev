# Architecture decisions

Cross-cutting decisions that shape the overall system. Feature-local decisions stay in
their RFD and are linked from there.

Each entry has a short code (`D<n>`) for easy cross-reference.

## D1: Salsa for incremental computation

We use [salsa](https://github.com/salsa-rs/salsa) as the incremental computation
framework. All "interesting" computations are salsa tracked functions; inputs are salsa
inputs or interned structs.

## D2: Stash for type-level interning

Types and other IR nodes are interned into a per-database `Stash` (a custom arena with
hash-consing) rather than using salsa interning. This gives us pointer-sized handles
(`Ptr<T>`) with zero-copy decomposition for compound types.

## D3: Tree-sitter for parsing

We use tree-sitter-rust for parsing rather than writing a hand-rolled parser. This gives
us error recovery, incremental re-parsing, and avoids duplicating grammar maintenance.

## D4: Oracle test harness

End-to-end correctness is validated by an "oracle" harness that compiles test programs
with both sage and rustc, then compares diagnostics. This ensures sage's behavior
converges with the reference compiler.

## D5: Symbols as the uniform IR unit

Every named entity (function, type, field, variant, generic parameter, etc.) is
represented as a `Symbol` with per-kind data. This gives a uniform shape to queries
and avoids proliferating distinct tracked structs per entity kind.
