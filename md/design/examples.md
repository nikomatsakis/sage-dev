# Examples

These walkthroughs follow small Rust programs through sage's implementation.
They are ordered so that each chapter adds one major subsystem. Implementation
extracts use `mdbook-ezanchor` to include named `ANCHOR` regions and link to
their locations on GitHub; when the code changes, the book builds from the new
source rather than retaining a stale copy.

1. [Struct signature](./examples/struct-signature.md) follows a parsed struct
   through generic-parameter lowering and field type checking.
2. [Function body and field access](./examples/function-body.md) adds function
   signatures, body inference, and generic field substitution.
3. [Macro-generated struct](./examples/macro-generated-struct.md) shows how
   macro expansion feeds the same symbol and resolution pipeline.
4. [A direct trait obligation](./examples/direct-trait-obligation.md) follows a
   ground `Widget: Marker` goal from a call-site obligation to a canonical
   solver response.
5. [A nested trait proof](./examples/nested-trait-proof.md) adds a generic impl,
   a where-clause body, concurrent alternatives, and a child proof frame.
6. [An external trait method call](./examples/external-trait-method.md) follows
   `self.db.clone()` through prelude trait discovery, owned rustc metadata, a
   fixed-trait proof, and explicit receiver elaboration.
7. [An oracle-checked method body](./examples/oracle-checked-method.md) shows
   how Sage and rustc independently project that completed body before an exact
   serialized comparison.

The [architecture pages](./README.md) specify the system as a whole. These
examples are the code-reading path into that specification.

The [review-packet contract](./validation/README.md#review-packets) identifies
the evidence that a mature walkthrough should collect. The module-expansion
and oracle-checked-body examples are the initial complete packets; earlier
examples remain useful narrower code tours.
