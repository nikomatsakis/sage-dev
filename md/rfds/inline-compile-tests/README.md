# Inline Compilation Tests

**Status: Draft — documentation and design only.**

## Summary

Introduce `compile_test!` inside ordinary Rust `#[test]` functions. Tests contain
inline, dedented Rust source; source-line type observations; exhaustive
ERROR/WARN markers; and `expect-test` snapshots. The first version runs Sage
with the real standard library, checks the whole local crate, and can snapshot
its query work. Rustc execution and narrower demand modes follow separately.

The normative proposal is [Inline Compilation
Tests](../../design/crates/sage_test_harness/compile-tests.md). Its authoring
examples cover anonymous source and a multi-file crate. This RFD records why
those choices fit Sage and how to establish their evidence.

## Motivation

The next inference anchors need both focused transaction tests and Rust programs
that exercise those transactions through ordinary checking. Existing
`with_test_crate` callbacks can inspect types, but each test manually finds a
function and walks Typed IR to reach an expression. This obscures the claim's
scenario and makes tests sensitive to incidental tree structure.

The existing oracle harness supports diagnostic source annotations and exact
independent emissions. Its filesystem fixtures and separate discovery runner
serve a different purpose: broad conformance. An inline test should instead
be easy to annotate with `#[sage::test(...)]`, run by name through `cargo test`,
and review as one function containing input and expected output.

Type strings can be large. One combined type expectation keeps them outside the
Rust input while allowing automatic updates when representations deliberately
change. Bare `//# ty` markers avoid managing names for individual observations.
Type output retains filenames but omits line numbers, columns, and ranges,
reducing snapshot churn when unrelated source lines move.
Diagnostics need a different balance: their expected location and count should
be obvious in the source, while one snapshot captures the complete rendered
output. Error slugs add indirection without enough benefit.

## Design rationale

### Use ordinary tests and existing expectation tooling

A macro is an authoring convenience around a shared runner, not a replacement
for Cargo's test harness. The function's source remains the review unit.
`expect!` expressions stay at their call sites, where `UPDATE_EXPECT=1` can
update them. The macro's source does not enter every semantic claim snapshot.
The runner itself has independent tests and anchors.

Use `indoc!` explicitly for fixture strings, and `expect-test` for expectations.
This avoids a second dedenting algorithm and a new snapshot dependency when
`expect-test` already serves repository tests. A later tooling change must
preserve the blessing and source-review contracts.

### Exercise real semantic paths with useful defaults

An anonymous snippet is a library crate rooted at `src/lib.rs`, not a source
fragment injected into constructed semantic state. Named paths are relative
to the project and must include that same fixed root; they allow nested modules
and directories without an entry-selection option.
The runner materializes a private Cargo project and invokes production
workspace/compiler facilities with real standard-library metadata. The
checked-in inline source plus deterministic generated configuration defines
the project; temporary files are an execution artifact, not hidden inputs. The
fixture receives ordinary library compilation, with `cfg(test)` disabled and
no enabled Cargo features, independently of its enclosing Cargo test.

The default edition follows the pinned toolchain's latest supported edition.
Tests for historical edition behavior choose it explicitly. Upgrading the
toolchain can therefore intentionally change default semantics; edition
regressions should use pinned edition fixtures.

All local bodies are checked by default. Otherwise adding a type marker could
change which errors become visible, or a broken unselected function could
escape the diagnostic contract. This policy is deliberately separate from
future point-demand tests, which will make their narrower entry point explicit.

Real metadata support must not require successful rustc compilation of the
fixture: negative tests must still reach Sage. This follows the separation
between dependency facts and local semantic work. Reusing the production host
may require extracting a shared setup layer; adding a dependency cycle from
`sage-test-harness` back to its consumers is not an acceptable shortcut.

### Keep annotations local and observations readable

All directives share one target-line rule, including inline diagnostic
comments and stacked annotation lines. Dedenting happens before interpreting
line numbers. Tabs are rejected for consistent fixture formatting; Unicode
visual widths do not matter because no column is selected.

`//# ty` observes every source expression whose span starts on its target
line. Multiple siblings and nested expressions starting on that line are all
included; expressions starting elsewhere are omitted. This avoids repeating
the function-body block and other enclosing expressions for every observation.
A selected expression may extend across later lines. Among selected expressions,
children appear before parents and siblings follow source order. Stable source
identities distinguish equal-span expressions. Meaningful coercions appear as
wrappers after their original expressions, exposing both the completed original
type and the coerced type. Other generated elaboration nodes do not multiply
source observations. Compact, single-line source descriptions omit annotation
comments and normalize whitespace between tokens, preserving literal contents
and escaping them where necessary. This keeps formatting edits from obscuring
changes to observed types.

These are observations of completed checking, never constraints. Expected
text cannot make inference choose a type, and missing completed results must
not turn into a successful snapshot of an invented placeholder.

### Match structured diagnostics, then snapshot presentation

The renderer is not a reliable source of diagnostic identity or location:
it prints context, secondary labels, notes, and potentially macro locations.
Matching uses the diagnostic's primary span and that span's expansion ancestry.
Label locations cannot make a diagnostic eligible: a marker should identify
where the error is reported, rather than a related declaration or contextual
label. Labels and notes remain visible in the rendered snapshot. Each diagnostic
counts only once. Empty spans match the line containing their position, so
insertion-point diagnostics remain expressible with ordinary line markers.
All compilation stages contribute source diagnostics; repeated reporting of
one diagnostic is deduplicated without collapsing distinct identical-looking
diagnostics.

Exhaustive one-to-one assignment expresses both location and count. It also
handles overlapping eligible locations correctly: the runner must not fail
because it greedily assigned a diagnostic with a multi-line primary span or
several macro-ancestry locations to the only marker another diagnostic could use. Notes remain attached presentation, not extra
ERROR/WARN events.

Compiler conditions use `//# ERROR if SAGE` and `//# ERROR if rustc`, with the
same grammar for WARN. Neither type nor diagnostic markers use slugs. V1 parses the
conditions but runs only Sage. A later rustc adapter will use structured
compiler output to check lines, severity, and counts, without requiring rustc
text snapshots or type extraction. This extension does not replace exact
oracle comparison of independent semantic emissions.

### Observe demand as well as results

A correct result can still be produced through an overly broad dependency.
An optional query snapshot makes this work reviewable alongside the source.
The first version observes a cold whole-crate demand, separating project setup
from local semantic demand and freezing observations before rendering.

The Inspector already specifies stable semantic trace projection. Reuse that
contract: preserve meaningful parentage and multiplicity, expose unknown work,
and remove incidental IDs rather than comparing raw Salsa debug output.
Do not sort away meaningful execution order to make a snapshot pass. A future
warm/edit/point-demand extension should build on the same capture boundary.

### Bless output without blessing unexpected errors

Automatic updates are useful for unwieldy type representations and diagnostic
formatting. They must not redefine the program's intended behavior. Before
updating any expectation in a test, the runner validates every marker and
selection and matches all diagnostics. It also finishes query projection and
renders every requested output before any updating assertion runs. A failure
while preparing one output must not leave another expectation already updated.
An unexpected error prevents every
snapshot update in that invocation, including otherwise valid type snapshots.

This guarantee follows from control flow: preparation returns all validated,
rendered results or fails, and only a successful return reaches expectation
comparison. A comment at the implementation boundary should explain this
ordering. Reviewing that separation can establish the guarantee without a
dedicated test that injects failures into every preparation stage. Any test
N/A association must explain this structural justification; it does not count
as an approved executable test or an automated implementation review.

This is a validation barrier, not a promise to roll back every file write if an
I/O error occurs, nor a transaction across all Cargo tests. Independent tests
may update independently. Changing a marker is a deliberate assertion change:
agents and people must explain the semantic reason before blessing.

## Specification changes

All new anchors use the prefix `crates.sage_test_harness.compile_tests`.

| Design rules | Purpose |
|---|---|
| [invocation](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.invocation), [fixtures](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.fixtures) | Ordinary Cargo tests, anonymous input, named files and directories |
| [edition](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.edition), [dependencies](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.dependencies), [project_isolation](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.project_isolation), [production_execution](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.production_execution), [checking_scope](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.checking_scope) | Edition configuration, real std, project isolation, production execution, whole-crate demand |
| [coordinates](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.coordinates), [target_line](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.target_line), [annotation_validation](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.annotation_validation) | Precise marker parsing and rejection of mistakes |
| [starting_expressions](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.starting_expressions), [completed_types](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.completed_types) | Observations of expressions starting on the target line |
| [diagnostic_locations](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.diagnostic_locations), [diagnostic_counts](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.diagnostic_counts), [compiler_conditions](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.compiler_conditions) | Structured, exhaustive matching and future compiler qualification |
| [rendering](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.rendering), [query_capture](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.query_capture), [query_projection](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.query_projection) | Deterministic result and demand evidence |
| [blessing_gate](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.blessing_gate), [callsite_updates](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.callsite_updates), [review_visibility](../../design/crates/sage_test_harness/compile-tests.md#crates.sage_test_harness.compile_tests.review_visibility) | Safe updating and directly reviewable test intent |

The [source-evidence foundation](../architectural-foundations/README.md#semantic-evidence-starts-from-source)
is clarified to include inline checked-in sources materialized as Cargo
projects. The harness crate overview and contributing guidance link the new
proposal. The existing oracle contracts and claim snapshot boundary are
unchanged.

## Alternatives

- **Manual Typed IR traversal:** retain for focused structural tests, but avoid
  requiring it for ordinary source-line type assertions.
- **Filesystem-only fixtures and a custom runner:** useful for the existing
  oracle corpus; less convenient for directly annotated, self-contained tests.
- **Caret or range selection:** permits narrower observations, but requires
  horizontal alignment and column rules. Line-based observations include all
  expressions starting on the line and make those selectors unnecessary.
- **All expressions intersecting or enclosing the line:** repeats the body block
  and other broad ancestors at nearly every marker. Start-line selection keeps
  observations local while still showing nested expressions on that line.
- **One smallest enclosing expression:** omits other expressions starting on
  the line and requires a choice among equal-span semantic nodes.
- **Type slugs with separate snapshots:** require naming each observation and
  maintaining a map; bare markers and one ordered snapshot are sufficient.
- **Error slugs with separate snapshots:** adds naming and matching overhead;
  inline severity/count markers plus one output snapshot are clearer.
- **Grepping rendered diagnostic output:** confuses displayed context with
  diagnostic locations and complicates future rustc integration.
- **Blessing every observed diagnostic:** silently changes acceptance criteria;
  marker intent must remain a prerequisite for representation updates.
- **A new snapshot library:** unnecessary for v1; existing `expect-test` already
  provides call-site expectations and the established update workflow.

## Remaining implementation decisions

Before writing the runner, settle the shared production-host adapter placement
without introducing a crate dependency cycle, the exact type/query snapshot
format, and the source-expression provenance mechanism for equal spans and
macro-generated expressions. These are implementation choices constrained by
the linked claims, not permission to omit whole-crate checks or substitute
hand-built semantic answers. If provenance requires changing another Design
contract, propose that change before implementing it.

This draft adds no Rust placeholders and no review approvals. The new anchors
will be reported as lacking implementation/test associations by the shared
coverage checker until the first implementation step accounts for them.
Existing reviewed claims remain unchanged.

## Implementation

See [Implementation plan and status](implementation.md).
