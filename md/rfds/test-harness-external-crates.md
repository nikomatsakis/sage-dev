# RFD: Test Harness External Crate Support

**Status:** Completed

**Depends on:**
- [Oracle Test Framework](./oracle-test-framework.md) — the existing comparison infrastructure

## Goal

Enable oracle tests to exercise paths through external crates (at minimum `std`/`core`). Today, `sage_test_harness` uses `NoopTcxDb`, so external crate resolution always fails — tests like `Option::Some(x)` or `Vec::new()` can't be validated against the oracle.

## Problem

The oracle test harness has an asymmetry:

- **Oracle side** (`sage-oracle`): runs as a rustc driver with full access to external crates. It resolves `Option`, `Vec`, trait impls, etc.
- **Sage side** (`sage_test_harness`): uses `Database::default()` with `NoopTcxDb`. All external crate lookups (`extern_crate`, `module_children`, `item_name`) return `None`/empty.

This means oracle comparison tests can only validate code that uses locally-defined types. Any fixture referencing std types fails on the sage side with `?Error` types.

The full `cargo-sage` binary does have external crate support (via `ProxyTcxDb` connected to a real rustc session), but the oracle harness doesn't use it — it runs sage in-process without rustc.

## Design

### Option A: In-process rustc TcxDb for test harness

Run `rustc_driver` in the test process (like the oracle already does) and wire it into sage's `ProxyTcxDb`. The test harness would:

1. Compile the fixture with rustc (the oracle step already does this).
2. Keep the `TyCtxt` alive.
3. Stand up a `ProxyTcxDb` channel pair.
4. Run sage with that proxy instead of `NoopTcxDb`.

**Pros:** Full fidelity — exercises the exact same code path as `cargo-sage`.
**Cons:** The oracle side already holds a `TyCtxt`; sharing it with sage requires careful thread orchestration. Also, `sage-test-harness` would need `#![feature(rustc_private)]`.

### Option B: Snapshot-based TcxDb

Pre-compute a "metadata snapshot" for a set of external crates (just `std`/`core`/`alloc` for now) and load it from disk during tests:

1. A one-time build step extracts `module_children`, `item_name`, `def_path`, etc. for every reachable DefId into a JSON/bincode file.
2. A `SnapshotTcxDb` implements `TcxDb` by reading from this snapshot.
3. `sage_test_harness` loads the snapshot on first use (lazy static).

**Pros:** Fast, deterministic, no rustc in the test process. Easy to check in and version.
**Cons:** The snapshot is a point-in-time freeze — it doesn't track rustc updates. Also, any new `TcxDb` method needs a snapshot format update.

### Option C: Single-process dual-use (recommended)

The oracle binary already drives rustc. Restructure the oracle test so both sides run in the same process:

1. The oracle test process starts rustc (as it does today).
2. After extracting the oracle output, it passes the `TyCtxt` (or a `ProxyTcxDb`) to a sage database.
3. Sage produces its output with real external crate data.
4. Compare in-process.

This is essentially Option A but acknowledges that the oracle process already has rustc running.

**Pros:** Minimal new infrastructure — the oracle test already has `TyCtxt`. No snapshot files. Full fidelity.
**Cons:** The `sage_output()` path in the oracle harness needs access to a `TcxDb` backed by the same compilation. This may require restructuring the harness to pass the TcxDb through rather than calling `sage_test_harness::with_test_crate` (which always creates `NoopTcxDb`).

## Implementation sketch (Option C)

### Step 1: Extract TcxDb creation from test harness

Add a variant of `with_test_crate` that accepts a `&dyn TcxDb`:

```rust
pub fn with_test_crate_and_tcx<R>(
    source: &str,
    tcx: &dyn TcxDb,
    f: impl FnOnce(&dyn Db, ModSymbol<'_>) -> R,
) -> R
```

### Step 2: Wire oracle harness to pass TcxDb

In `oracle_compare.rs`, after obtaining the oracle output (which keeps `TyCtxt` alive), construct a `RustcTcxDb` and pass it to sage:

```rust
fn run_fixture(fixture: &Fixture, out_dir: &Path) -> Result<(), Failed> {
    // Oracle side (already exists):
    let oracle = fixture.oracle_output()?;

    // Sage side — now with access to external crates:
    let sage = fixture.sage_output_with_tcx(/* somehow pass the TyCtxt */);

    assert_crates_eq(&fixture.name(), &oracle, &sage)
}
```

The challenge: `oracle_output()` compiles with rustc and drops the session. We need to restructure so the rustc session stays alive while sage runs.

### Step 3: Keep the TyCtxt alive across both sides

Restructure fixture processing:

```rust
fn run_fixture(fixture: &Fixture) {
    sage_oracle::with_compilation(fixture.entry(), |tcx| {
        let oracle = emit_oracle(tcx);   // extract oracle output
        let tcx_db = RustcTcxDb::new(tcx);
        let sage = run_sage_with_tcx(fixture, &tcx_db);  // sage with real metadata
        compare(oracle, sage);
    });
}
```

## Open questions

1. **Thread safety.** `TyCtxt` is `!Send`. The oracle and sage both need it on the same thread. This works if we run sage synchronously (no salsa parallelism), but the proxy channel design expects sage on a separate thread. Do we need a synchronous `TcxDb` path for tests?

2. **Scope.** Should we support only `std`/`core`/`alloc`, or any `[dependencies]`? For now, just the standard library suffices — it covers `Option`, `Result`, `Vec`, `String`, iterators, etc.

3. **Performance.** Running rustc for every test fixture is slow. Consider caching the `TyCtxt` across fixtures (all share the same sysroot). Or use Option B (snapshot) as a fast path and Option C for CI.

## Non-goals

- Supporting `proc-macro` crates in test fixtures (they need separate compilation).
- Matching rustc's full resolution for `impl` items and trait methods (that's the trait-system RFD).
- Testing sage's proc-macro expansion against external crates.
