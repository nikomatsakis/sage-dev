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
cargo run -p sage-meta -- serve 3001
```

Open <http://127.0.0.1:3001/>. Restart the command after model or Rust source
edits. The generated book works without this server.

## Checking the specification

From the repository root, run:

```sh
cargo run -p sage-meta -- check
cargo run -p sage-meta -- check --json
cargo test -p sage-meta
cargo test --all --workspace
```

The shared check validates the model, Rust correspondences, implementation/test
entries, and saved review decisions. The workspace integration test runs that
same check. See [reviewing specification tests](../design/meta/reviews.md) when
the check identifies new or changed claim/test inputs.

For a self-contained HTML export, run `cargo run -p sage-meta -- browse` and open
`target/sage-browser.html`. Exported and served views are snapshots. Rebuild or
restart after model or Rust source changes to refresh the embedded source pane.
