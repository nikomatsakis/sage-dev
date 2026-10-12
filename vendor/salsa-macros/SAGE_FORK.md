# Sage source-provenance patch

This is the crates.io `salsa-macros` 0.26.1 source, retaining the upstream
licenses and source provenance in `.cargo_vcs_info.json`.

The only code changes are the two `parse_quote_spanned!` calls in
`src/tracked_impl.rs`. Generated tracked-method and associated-function bodies
use the original method identifier's span. Previously their source location
pointed to the enclosing tracked impl annotation, making distinct methods in
one impl indistinguishable to source-index consumers. No Sage identity or
contract is compiled into a query, and the generated public APIs are unchanged.

The real Salsa fixtures under `crates/sage-meta/tests/fixtures/contracts` verify
free-query and method mappings, including multiple methods in the same impl,
and exercise the generated accumulator API. General macro-generated queries
remain unsupported by the source matcher and fail with explicit coverage gaps.

This belongs upstream as a source-provenance fix. The runtime fork selects this
crate through a path dependency until an upstream equivalent is available.
