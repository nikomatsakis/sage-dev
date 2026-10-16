# Exploring the Design

Run `mdbook serve` from the repository root and open its displayed URL. Under
Design, [Crates](../design/crates/README.md) introduces the compiler's main
subsystems. Its generated subchapters let you search the model, follow method
and dependency links, and reveal corresponding Rust source. Entity URLs can
be copied and revisited; the embedded explorer follows the book's theme.

Open [Anchors](../design/anchor/README.md) to browse specification rules by
namespace. Each row reports implementation and test status independently.
Open a rule to inspect its source context, implementation excerpts, tests,
and saved review. **TODO** means unfinished coverage; **N/A** records an
explicit explanation of why that association does not apply. A listed test
is not a record of execution.

For a standalone model explorer, run:

```sh
cargo meta serve 3001
```

Open <http://127.0.0.1:3001/>. Restart the command after model or Rust source
edits. The generated book works without this server.

## Checking the specification

The repository defines `cargo meta` as an alias for
`cargo run --package sage-meta --bin sage-meta --`. All CLI subcommands and
arguments follow the alias directly.

From the repository root, run:

```sh
cargo meta check
cargo meta check --json
cargo test -p sage-meta
cargo test --all --workspace
```

The shared check validates the model, Rust correspondences, implementation/test
entries, and saved review decisions. The workspace integration test runs that
same check. See [reviewing specification tests](../design/meta/reviews.md) when
the check identifies new or changed claim/test inputs.

For a self-contained HTML export, run `cargo meta browse` and open
`target/sage-browser.html`. Exported and served views are snapshots. Rebuild or
restart after model or Rust source changes to refresh the embedded source pane.

## Validating anchors

Run `cargo meta check` to identify anchors needing review and follow the
`sage-review-checklist` skill. The main agent delegates one subagent per anchor.
The reviewer starts its validation and consumes the printed input:

```sh
cargo meta review start ANCHOR
cargo meta review read ANCHOR path/to/file.rs
cargo meta review read ANCHOR path/to/file.rs --lines 10 40
cargo meta review record ANCHOR /tmp/review-report.json
cargo meta check
```

`start` resets `validations/ANCHOR/` and saves the initial claim, test,
implementation, and instruction fragments before printing them. Use
`--instructions FILE` to capture additional coordinator instructions. Starting
removes the old report and makes the shared validation test fail until a
current complete report with no requested edits is recorded.

`read` saves and returns exactly the selected source. Paths are workspace-relative.
Selectors include whole files (default), `--lines START END` (inclusive),
`--claim ANCHOR`, and `--test CRATE TARGET RUST_PATH`. The agent declares every
additional source dependency; only one reviewer writes an anchor at a time.

The report file contains `reviewer`, `report`, and `requested_edits`. Evaluate
every aspect and explain implementation completeness and test adequacy. Use an
empty edit list only when no changes are needed. Justified TODO/N/A evidence
retains its status. Recording checks the captured inputs against current source,
preserves `input.json`, and saves `output.json` with the report. Fix requested
edits and start fresh reviews for affected anchors before checking again.

Directories use the complete dotted anchor ID, such as
`validations/meta.reviews.gate/`. `cargo meta check` also fails for directories
whose IDs no longer exist. Remove retired records explicitly. For a renamed
anchor, reconcile its specification and associations, remove its old directory,
and review the new ID. Unsupported formats require a fresh `start`.
