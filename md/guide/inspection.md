# Inspecting Compiler Results

Run `cargo sage inspect` in the Rust project you want to inspect. Sage opens a
local web application for browsing symbols, signatures, and checked bodies.
Use `--package <name>` to select a workspace package, or `--port 2442 --no-open`
to choose a port without opening a browser automatically.

For a repository fixture without installing the command, run from
`test-projects/semantic-inspector/db-drop-guard`:

```sh
cargo run --manifest-path ../../../Cargo.toml --bin cargo-sage -- \
  sage inspect --port 2442 --no-open
```

The inspector displays results from a concrete compiler session. The
[Crates explorer](../design/crates/README.md) instead displays the architectural
model. See the [inspector architecture](../design/crates/sage_inspector/inspector-guide.md)
for its service boundaries, supported products, and query traces.
