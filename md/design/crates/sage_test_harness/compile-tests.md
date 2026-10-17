# Inline Compilation Tests

The [Inline Compilation Tests RFD](../../../rfds/inline-compile-tests/README.md)
records the design rationale. `sage_test_harness::compile_test!` is the inline
runner; implementation and test evidence is reported by the generated anchors.

`compile_test!` puts Rust fixtures, source annotations, and expected observations
inside ordinary Cargo tests. The runner produces semantic observations through
Sage's production workspace and compiler paths. Every fixture also runs through
the pinned rustc and checks its diagnostics against the source markers. It
complements direct unit tests
and the independent [oracle comparisons](../sage_oracle_harness/oracle-guide.md).

## Authoring a test

The anonymous form uses a fixed root `src/lib.rs`:

```rust,ignore
use sage_test_harness::compile_test;
use expect_test::expect;
use indoc::indoc;

#[test]
#[sage::test(crates.sage_ir.inference.numeric_domains)]
fn literal_type() {
    compile_test! {
        source: indoc! {r#"
            fn example() -> u16 {
                22
                //# ty
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: literal `22`
              type: u16
        "#]],
        diagnostics: expect![""],
    }
}
```

Tests using the embedded rustc metadata provider need `#![feature(rustc_private)]`
at the top of their Rust test crate, using the repository's pinned nightly.
`#[sage::test(...)]` additionally uses the existing registered `sage` tool.

Observation snapshots include filenames but omit line numbers,
columns, and ranges; positions are used internally to select expressions.

For a crate with directories, use explicit relative paths:

```rust,ignore
#[test]
fn mismatched_return_in_submodule() {
    compile_test! {
        edition: "2021",
        files: {
            "src/lib.rs": indoc! {r#"
                mod model;
            "#},
            "src/model.rs": indoc! {r#"
                pub fn value() -> u16 {
                    true //# ERROR
                }
            "#},
        },
        diagnostics: expect![""], // Fill with the rendered mismatch diagnostic.
    }
}
```

The empty diagnostic expectation in this second example is intentionally
unblessed: the source marker declares the error before `UPDATE_EXPECT=1` is
used to capture its presentation. `queries: expect![...]` optionally adds a
snapshot of the checking run's query log. An omitted diagnostic expectation
means an empty expected diagnostic output; an omitted query expectation makes
no query-log assertion. A missing observation expectation is an error when observation markers are present.

Unqualified diagnostic markers must match both compilers. Use compiler-qualified
markers for deliberate differences. For example, rustc warning tests explicitly
enable the lint because ordinary warning-by-default lints are quiet:

```rust,ignore
compile_test! {
    source: "#[warn(dead_code)] fn unused() {} //# WARN if rustc",
}
```

This fixture expects one rustc warning and no Sage diagnostics. The rustc
comparison does not add another snapshot field.

## Cargo integration and fixture inputs

{anchor}`invocation`
An inline compilation test runs inside a standard Rust `#[test]` under the
ordinary Cargo test harness. `compile_test!` accepts fixture source expressions
and explicit `expect_test::expect!` values at the call site. Assertions use the
existing `#[sage::test(...)]` association attribute; no separate test discovery
or anchor attribute is required. A failing observation fails that Rust test.

{anchor}`fixtures`
A fixture provides exactly one anonymous `source` or one `files` map. Anonymous
source becomes `src/lib.rs`. A files map must contain `src/lib.rs`, the fixed
crate root. There is no root-selection option or automatic root detection.
Paths are relative to the materialized project and imply directories; files become modules only through Rust's module-loading rules. An implicit `mod child;`
with both `child.rs` and `child/mod.rs` present is rejected as ambiguous;
an explicit `#[path]` selects its named file.
Duplicate paths, absolute paths, parent traversal, a missing `src/lib.rs`, and
file/directory conflicts are fixture errors. The generated `Cargo.toml` and
`Cargo.lock`, `.cargo` configuration, and a root `build.rs` are reserved and
cannot be supplied as fixture files. Unreferenced files are not
implicitly compiled; annotations in files outside the selected crate are
rejected rather than silently left unchecked.

The anonymous form supplies a Rust expression evaluating to source text:

```rust,ignore
compile_test! {
    source: indoc! {r#"
        pub fn value() -> u16 { 22 }
    "#},
}
```

The named-file form maps string-literal paths to source-text expressions.
Slashes in a path create directories; there is no separate directory syntax:

```rust,ignore
compile_test! {
    files: {
        "src/lib.rs": indoc! {r#"
            pub mod model;
        "#},
        "src/model.rs": indoc! {r#"
            pub mod detail;
        "#},
        "src/model/detail.rs": indoc! {r#"
            pub fn value() -> u16 { 22 }
        "#},
    },
}
```

Top-level fields use `name: value` syntax, separated by commas. The `files`
map uses `"relative/path.rs": source_expression` entries, also separated by
commas. Both accept trailing commas. Field order is immaterial; duplicate or
unknown field names are rejected. `source` and `files` are mutually exclusive
and one is required. The crate root is always `src/lib.rs`. Source expressions may
be ordinary or raw string literals, or expressions such as `indoc!` invocations.
Both forms may also include `edition`, `observations`, `diagnostics`, and `queries`
as illustrated in [Authoring a test](#authoring-a-test).

{anchor}`edition`
The default fixture uses the latest Rust edition supported by the repository's
pinned toolchain. An explicit `edition` fixes an older supported edition;
unknown editions fail setup.

{anchor}`dependencies`
The default fixture has the real standard library and its dependency metadata.
V1 supplies the standard library; arbitrary third-party dependency
configuration is a later extension.

{anchor}`project_isolation`
Each test gets an isolated materialized Cargo project whose source is the
supplied fixture, with deterministic logical paths and no dependence on the
caller's working directory or enclosing workspace's default members. The fixture
is compiled as an ordinary library, with `cfg(test)` disabled and no enabled
Cargo features; running the enclosing Rust test does not enable test compilation
for the fixture.

{anchor}`production_execution`
Fixture evaluation uses the production workspace-loading, parsing, expansion,
resolution, and checking paths with real dependency metadata. Inline sources
and generated project configuration are inputs; expected types, diagnostics,
and query logs are outputs, never providers of semantic answers. Invalid Rust
must reach Sage even if it would fail rustc checking. Using rustc to provide
standard-library metadata does not count as running a rustc conformance test.
Setup and provider failures fail the test directly, not as expected source
diagnostics.

{anchor}`rustc_execution`
Every inline fixture is checked by both Sage and the rustc binary paired with
Sage's embedded compiler and standard-library sysroot. There is no implicit
skip or opt-out. rustc checks the same materialized `src/lib.rs`, files, edition,
and host target as an ordinary library, with no test configuration or enabled
features. It emits metadata only; fixture code is never executed. rustc checking
is separate from dependency metadata loading and Sage query capture. Source
errors in either compiler do not prevent running the other compiler. Missing
executables, malformed compiler output, abnormal termination, and an exit status
inconsistent with the reported errors fail preparation as infrastructure errors.

{anchor}`rustc_lints`
rustc starts with warning-by-default lints individually allowed, using the
pinned compiler's lint inventory, to keep incidental unused-item and naming
lints out of ordinary fixtures. Deny-by-default lints remain enabled. Source-level lint attributes may enable specific
warnings or deny them; emitted warnings and errors must then have matching
markers. This is not a lint cap and does not suppress hard language errors.

{anchor}`checking_scope`
V1 requests signatures and checks every supported body in the selected local
crate, including reachable submodules, associated bodies, and expanded items,
regardless of visibility or selector presence. It does not eagerly check
external dependency bodies. Unsupported in-scope work must be reported as a
failure or explicit diagnostic, never silently omitted as success. Adding a
type marker cannot change this demand policy. A future explicit entry-point
mode may select a narrower demand; it is not part of v1.

## Source annotations

Annotations are recognized in actual Rust line comments beginning `//#`;
marker-like text inside literals or block comments is not an annotation.
The initial directive vocabulary is:

```text
//# ty
//# sym
//# ERROR
//# WARN
//# ERROR if SAGE
//# ERROR if rustc
//# WARN if SAGE
//# WARN if rustc
```

Type and symbol observations select a source line, not a column or character range.
Indentation before `//#` does not affect selection. Neither observation markers nor
diagnostic markers have slugs. Compiler conditions use the exact names `SAGE` and
`rustc`. They are selectors, not arbitrary condition expressions.

{anchor}`coordinates`
The runner inspects the source string after `indoc!` or another supplied
source expression has evaluated. It does not independently dedent it again.
Line numbers refer to that resulting source, including annotation lines.
Actual tab characters remain rejected for consistent fixture formatting.
Type selection depends only on the target line: Unicode character widths and
horizontal alignment do not affect which expressions are observed.

{anchor}`target_line`
For every directive, non-whitespace source content before the annotation
comment makes its target the current line. Otherwise its target is the most
recent preceding nonempty line that is not an annotation-only line. Blank
lines and consecutive annotation-only lines are skipped; ordinary comments
are not skipped. Absence of a target is an annotation error. The same rule
applies to `ty`, `sym`, ERROR, and WARN; there is no caret or range syntax.

{anchor}`annotation_validation`
Unknown or malformed directives, invalid compiler conditions, missing or unused
observation expectations, and invalid selections fail the test. A fixture containing
any `//# ty` or `//# sym` marker requires one combined `observations: expect![...]` expectation;
a `observations` expectation without observation markers is rejected. Annotation validation
is independent of snapshot blessing, and no directive is silently ignored
because it is misspelled or cannot be resolved.

## Combined observations

{anchor}`grouped_observations`
`observations: expect![...]` holds one snapshot for all `ty` and `sym` markers.
An expression selected by both markers has one header, then indented `symbol:`
and `type:` fields in that fixed order. Only requested fields are printed.
Duplicate markers are idempotent and marker order does not change output.
Ordering follows the type observation source-tree postorder rule. Diagnostics
and query snapshots remain separate.

{anchor}`resolved_symbols`
`//# sym` observes explicit path expressions starting on the target line,
including local variable uses and qualified paths. It reports the definition
selected by completed checking, not the spelling of an import alias. Local
item identities include their kind and a `crate::`-qualified name. Local
bindings use their name and a one-based occurrence among same-named bindings
in declaration order within the body; unrelated declarations do not renumber
them. External definitions use the metadata definition path. No allocation IDs
or source coordinates appear. An actual resolution error is `<unresolved>`;
missing or unsupported semantic correspondence fails the observation. A target
line containing no path expression fails rather than producing an empty result.
Associated local item identities are not yet supported and fail explicitly.
V1 symbol selection excludes method names, field names, type syntax, patterns,
and implicit calls introduced by elaboration. These can gain selectors later.
Names are frozen before ending query capture, as with types.

For example, selecting both markers on `module::value()` produces:

```text
src/lib.rs: path `module::value`
  symbol: fn crate::module::value
  type: fn() -> u16
src/lib.rs: call `module::value()`
  type: u16
```

## Type observations

{anchor}`starting_expressions`
A `ty` observation includes every source expression whose span starts on
its target line in the same file. An expression may extend across subsequent
lines; its start alone determines inclusion. Expressions starting on earlier
or later lines are omitted, even if they contain or intersect the target line.
Multiple sibling expressions starting on one line are all included. Each
selected source expression appears once, with its logical filename, expression
kind and readable source description, and completed type. The snapshot omits
line numbers, columns, and ranges. Blocks follow
the same start-line rule; a function-body block starting earlier is not included
merely because it contains the target line.

All type markers contribute to the same snapshot. Files are ordered by logical
path, then marked target lines by source order. Within each target line, output
follows source-tree postorder: children before parents, with siblings in source
order. Repeated markers targeting the same line do not duplicate observations.
Distinct source expressions remain distinct entries even if their rendered text
is identical. Source nesting distinguishes expressions with equal spans.
Generated elaboration nodes do not add duplicate observations, except that
meaningful coercion wrappers are reported after their wrapped expression. Each
wrapper belongs to the selected source expression and exposes its resulting
type, preserving child-before-parent order through a chain of coercions.
A target line with no expression starting on it, or lacking a reliable
source-to-semantic correspondence, fails explicitly. V1 observes expressions;
binding declarations and other semantic entities require future selectors.

For example, an inline marker targets the code on its own line. The `observations`
expectation contains the complete ordered observation:

```rust,ignore
use expect_test::expect;
use indoc::indoc;

#[test]
fn tuple_types() {
    compile_test! {
        source: indoc! {r#"
            fn example() -> (u16, (bool, u8)) {
                (1u16, (true, 2u8)) //# ty
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: literal `1u16`
              type: u16
            src/lib.rs: literal `true`
              type: bool
            src/lib.rs: literal `2u8`
              type: u8
            src/lib.rs: tuple `(true, 2u8)`
              type: (bool, u8)
            src/lib.rs: tuple `(1u16, (true, 2u8))`
              type: (u16, (bool, u8))
        "#]],
        diagnostics: expect![""],
    }
}
```

The first tuple element precedes the second element's subtree. Within that
subtree, both literals precede their tuple. The function-body block starts on
the preceding line, so it is omitted. The function declaration itself is not
an expression.

A multi-line call demonstrates that enclosing expressions starting on earlier
lines are omitted:

```rust,ignore
use expect_test::expect;
use indoc::indoc;

#[test]
fn call_argument_types() {
    compile_test! {
        source: indoc! {r#"
            fn inner(value: u16) -> u32 { value as u32 }
            fn outer(first: u32, second: u16) -> u64 { first as u64 + second as u64 }

            fn example() -> u64 {
                outer(
                    inner(22u16), //# ty
                    33u16,
                )
            }
        "#},
        observations: expect![[r#"
            src/lib.rs: path `inner`
              type: fn(u16) -> u32
            src/lib.rs: literal `22u16`
              type: u16
            src/lib.rs: call `inner(22u16)`
              type: u32
        "#]],
        diagnostics: expect![""],
    }
}
```

The callee path `outer`, the `outer(...)` call, and the function-body block
start on earlier lines, so they are omitted. The literal `33u16` starts on a
later line and is also omitted. Moving the marker to the line immediately
after `outer(` would instead observe the path `outer` followed by the complete
`outer(...)` call, even though that call ends several lines later.
These expectation literals illustrate the filename-only output.
Moving an expression to another line without changing observation order does
not itself change its type snapshot.
Expression descriptions use the single-line rendering rules below. Type
representations describe the completed types currently exposed by Sage; the
runner does not manufacture distinctions absent from its typed IR.

{anchor}`completed_types`
Type observations read final types after body checking completes. They neither
constrain inference nor select semantic candidates. All selected expressions
are observed from that completed result. An explicit error type may be
snapshotted when checking produced one, but an absent result or unresolved
successful inference is an observation failure, not an invented type or an
omitted entry. Expected snapshot text cannot influence compilation. When coercion changes an
expression's type, the observation includes both its completed type before
coercion and the resulting type of its coercion wrapper; it does not replace
the original type with the coerced type.

## Diagnostics

{anchor}`diagnostic_locations`
Diagnostic markers match structured diagnostics by severity and logical source
file/line. A diagnostic is eligible if its primary span intersects the target
line, or if that primary span's macro-expansion ancestry contains an invocation
intersecting it. An empty span matches the line containing its position: for
example, a missing-token diagnostic can point between characters without
covering any character. A position at the start of a line belongs to that line;
EOF belongs to the final source line (including an empty final line after a
trailing newline). The same rule applies to empty invocation spans in expansion
ancestry. Nested expansions retain this ancestry. For Sage, the primary
span is `Diagnostic::span`; entries in `Diagnostic::labels` do not participate
in matching, regardless of their label style. Labels, attached notes, and help
remain in the rendered snapshot but do not supply additional matching locations
or ERROR/WARN events. Matching does not parse rendered text. Each diagnostic
counts once even if its primary span or expansion ancestry matches several lines. A source diagnostic with no matchable location remains
unmatched and fails the test.

For rustc, only spans marked `is_primary` in structured JSON diagnostics and
their recursive expansion invocation spans are eligible. Any eligible primary
span may match. Child notes/help and secondary spans do not introduce events
or eligible locations. rustc's spanless final error/warning count summaries and
failure-note explanations are presentation, not additional source diagnostics;
other spanless errors or warnings fail matching. Distinct top-level diagnostics
are never deduplicated by message text.

{anchor}`diagnostic_counts`
The diagnostic set includes all source diagnostics from parsing, expansion,
resolution, signature checking, and body checking. Repeated demands reporting
the same diagnostic do not duplicate it. Distinct diagnostics remain distinct
even when their messages and locations are identical. Malformed source is a
negative-test input, not by itself an infrastructure failure.

There must be a complete one-to-one matching between applicable ERROR/WARN
markers and emitted diagnostics. Each marker consumes one diagnostic, and each
diagnostic consumes one marker. Multiple diagnostics on one line require
multiple markers. Matching considers the complete eligibility relation, so a
greedy choice cannot reject an otherwise valid assignment. Missing diagnostics,
extra diagnostics, and severity mismatches fail even if their rendered output
matches a snapshot. Matching never changes diagnostic output order.

{anchor}`compiler_conditions`
Both Sage and rustc run for every fixture. Unqualified ERROR/WARN markers
apply independently to each compiler; `if SAGE` and `if rustc` apply only to
the named compiler. Each compiler must have a complete one-to-one match against
its applicable markers. One unqualified marker expects one diagnostic from each
compiler, not two from either. rustc contributes diagnostic checks only, not
observation, diagnostic-text, or query snapshots. Compiler-specific markers
record intentional differences and require a semantic explanation when changed.

## Snapshot outputs and query evidence

{anchor}`rendering`
The combined type observations, the complete rendered Sage diagnostic output, and an
optional query log are separate `expect_test::Expect` values, created with
the `expect_test::expect!` macro from the `expect-test` crate. Examples use
`expect!` after `use expect_test::expect;`. The runner uses that library for
snapshot comparison and `UPDATE_EXPECT=1` updates, subject to the
[blessing gate](#crates.sage_test_harness.compile_tests.blessing_gate).
Type descriptions and expression ordering are deterministic. Expression source
descriptions occupy one line, omit annotation comments, and normalize whitespace
between tokens without changing token contents or merging tokens. Literal
contents are preserved; embedded line breaks and delimiter characters are
escaped as needed to keep the description on one line and unambiguous.
Diagnostic rendering preserves meaningful order,
labels, notes, and source excerpts, including annotation comments. Paths use
fixture-relative names rather than temporary directories or machine-specific
roots. Normalization removes incidental presentation details only; it does not
erase semantic distinctions, unresolved results, or unexpected diagnostics.

{anchor}`query_capture`
An optional query expectation observes a cold run in a fresh Sage database.
Project/provider setup finishes before capture begins, without pre-demanding
the fixture's local semantic products. Capture covers local crate discovery,
signature and body demands under the checking policy, and semantic reads needed
to collect requested observations. It ends before rendering or comparing frozen
outputs. Observation-related work is retained rather than hidden, while the
set of checked bodies remains selector-independent. Warm runs, edits, and
explicit point demands are later extensions.

{anchor}`query_projection`
Query snapshots use stable semantic operation names and identities, retain
parentage, multiplicity, and available execution/reuse dispositions, and omit
allocation-dependent IDs and temporary paths. Only siblings explicitly defined
as unordered may be canonically sorted. Unknown operations remain visibly
unmapped rather than disappearing. An unmapped query key keeps its operation
name and omits allocation-dependent debug arguments; it does not pretend to
provide a semantic identity for those arguments. The projection follows the Inspector's
[deterministic trace contract](../sage_inspector/inspection.sage) instead of
inventing an unrelated raw-debug-string golden format.

## Blessing and review

{anchor}`blessing_gate`
Before any expectation in one test invocation is compared in updating mode,
the runner completes fixture validation, compilation, annotation validation,
observation collection, independent exhaustive diagnostic matching for both Sage
and rustc, requested query projection,
and rendering of every requested output. All resulting snapshot strings are
prepared before any updating expectation assertion runs. A failure in any of those
steps prevents all snapshot writes for that invocation. After they succeed,
`UPDATE_EXPECT=1` may update observation, diagnostic, and requested query representations.
It never adds source markers, changes compiler conditions, or accepts an
unmatched diagnostic. Other independent Cargo tests may update their own
expectations; this is not a workspace-wide filesystem transaction.

The implementation separates preparation from expectation comparison. Preparation
returns either all validated, rendered results or a failure, without invoking
`expect-test` assertions. The caller stops on failure and compares the returned
results against the supplied expectations only on success. An implementation
comment at that boundary explains why this ordering prevents partial updates
when preparation fails; no separate transaction mechanism is required.

{anchor}`callsite_updates`
Expectations passed through `compile_test!` retain their author-written source
locations. Blessing edits those expectation literals at the test call site,
not the macro implementation or a generated temporary file. Normal test runs
compare expectations without modifying sources. Updates must be verified with
an independent subsequent run with updating disabled.

{anchor}`review_visibility`
An anchor-associated inline test contains its source fixtures, annotations,
explicit configuration, and expected observations directly in the annotated
Rust function. The ordinary claim/test review captures the entire macro
invocation as part of that function's source. The harness macro implementation
is not included in that snapshot; it is reviewed and tested independently.
Reviewers evaluate whether the visible scenario and assertions establish the
associated claim. A changed ERROR/WARN marker or compiler condition is a changed
assertion of intended behavior and requires semantic justification, not merely
a successful blessing run.
