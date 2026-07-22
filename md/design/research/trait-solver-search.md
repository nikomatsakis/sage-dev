# Trait Solver Search: Related Work

This page records related work informing Sage's trait-solver search design. It
is descriptive rather than normative; the intended Sage contract lives in
[Trait Solver Design](../trait-solver.md).

## miniKanren interleaving

miniKanren represents relational search as lazy streams. Disjunction combines
streams with interleaving rather than exhausting one branch before considering
the next. In the small µKanren formulation, an immature stream is a suspended
computation; encountering a suspension rotates work between alternatives.
This is often described as interleaving depth-first search rather than literal
breadth-first search because the effective distance is the number and placement
of suspension points, not proof-tree depth.

The original conjunction is asymmetric: it binds answers from the left goal
into the right goal. Later work on fair relational conjunction explores more
symmetric scheduling because disjunct fairness alone does not make conjunct
ordering operationally irrelevant.

Relevant sources:

- Jason Hemann and Daniel P. Friedman,
  [µKanren: A Minimal Functional Core for Relational Programming](http://webyrd.net/scheme-2013/papers/HemannMuKanren2013.pdf).
- Kuang-Chen Lu, Weixi Ma, and Daniel P. Friedman,
  [Towards a miniKanren with fair search strategies](https://icfp19.sigplan.org/details/minikanren-2019-papers/1/Towards-a-miniKanren-with-fair-search-strategies).
- Petr Lozov and Dmitry Boulytchev,
  [On Fair Relational Conjunction](https://minikanren.org/workshop/2020/minikanren-2020-paper1.pdf).

For Sage, the main lesson is that intentional future-yield placement defines
the search metric. A FIFO ready queue is not itself a fairness contract if a
self-waking future can be polled repeatedly during one drain operation.

## SLG tabling and Chalk

SLG-style solving stores canonical subgoals in tables. Each table owns answers
and suspended strands; consumers resume as new answers arrive. This makes the
operational structure a graph rather than a recursively duplicated tree and
supports completion of many cyclic searches.

Chalk's on-demand SLG design is especially close to the problem domain. It
describes canonical tables, answer substitutions, suspended strands, and
round-robin strand activation. A table may produce multiple answers lazily for
a non-ground query.

Relevant sources:

- [The Chalk On-Demand SLG Solver](https://rust-lang.github.io/chalk/book/engine/slg.html).
- [Chalk recursive-solver completeness](https://rust-lang.github.io/chalk/book/recursive.html).
- [The `chalk_engine` literature references](https://rust-lang.github.io/chalk/chalk_engine/).

Sage currently has table-shaped ownership but recursive-solver result
semantics: a whiteboard frame has one producer and subscribers, candidates run
inside it, and the frame publishes one aggregate result. Parent-sensitive keys
and immediate inductive cycle cutoff are not full SLG table completion.

## rustc's next-generation solver

rustc's next-generation solver evaluates a canonical goal recursively and
returns one aggregate canonical response. Candidate responses are merged
internally. Its search graph combines a global completed cache with a
provisional cache for cycles and iterates cycle heads toward a fixpoint. Cycle
path kind distinguishes inductive, coinductive, and unknown recursion.

Relevant sources:

- [Next-generation trait solving](https://rustc-dev-guide.rust-lang.org/solve/trait-solving.html).
- [Caching and cycle handling](https://rustc-dev-guide.rust-lang.org/solve/caching.html).
- [`rustc_type_ir::search_graph`](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_type_ir/search_graph/index.html).

Sage's single aggregate frame result is closer to this model than to a table
which externally streams all substitutions. Futures nevertheless allow Sage
to interleave internal alternatives and cancel logically irrelevant work.

## Answer subsumption

Tabled systems may aggregate answers using a partial order instead of retaining
every answer. A more-general or otherwise preferred answer can subsume another.
This resembles Sage's directional reduction of conditional answers and the
anti-unification of hard hints.

Naive answer subsumption can change a tabled program's least fixed point. The
aggregation operation and its interaction with recursive consequence must meet
soundness conditions; a locally plausible pruning rule is not automatically a
valid recursive evaluation rule.

Relevant sources:

- Alexander Vandenbroucke, Maciej Piróg, Benoit Desouter, and Tom Schrijvers,
  [Tabling with Sound Answer Subsumption](https://arxiv.org/abs/1608.00787).
- Terrance Swift and David S. Warren,
  [XSB: Extending Prolog with Tabled Logic Programming](https://arxiv.org/abs/1012.5123).

Sage therefore distinguishes completed-answer reduction, pending-branch
envelope pruning, and recursive publication of provisional table answers. The
third mechanism requires a stronger semantic argument than the first two.

## Resource-bounded recursion

Tabling closes repeated goals but does not stop an infinite sequence of
strictly growing canonical terms. Rust diagnoses such requirements as trait
evaluation overflow. The next-generation solver likewise returns ambiguity
when it exceeds its recursion or fixpoint limits.

Relevant sources:

- [Rust error E0275](https://doc.rust-lang.org/error_codes/E0275.html).
- [rustc solver overflow handling](https://rustc-dev-guide.rust-lang.org/solve/caching.html#dealing-with-overflow).

Sage's completeness contract is consequently resource-bounded. Depth, term
size, logical work, and fixpoint limits must be explicit and deterministic.
