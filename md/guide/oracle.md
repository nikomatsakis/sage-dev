# Checking rustc Conformance

From the repository root, run the oracle fixtures with:

```sh
cargo test -p sage-oracle-harness
```

Filter to a particular fixture with the test name:

```sh
cargo test -p sage-oracle-harness -- cross-module
```

To inspect independently emitted outputs for a small program:

```sh
cargo run -p sage-oracle -- test-fixtures/oracle/basics/hello.rs
cargo run -p sage-emit -- test-fixtures/oracle/basics/hello.rs
```

The harness compares the two representations exactly. A difference is a
conformance failure to investigate, not something to normalize away in a
snapshot update. See the [oracle harness](../design/crates/sage_oracle_harness/oracle-guide.md)
for fixture creation, reading differences, and the comparison contract.
