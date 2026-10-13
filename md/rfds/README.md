# Requests for Discussion (RFDs)

RFDs are a way of planning out larger changes. They aren't required but they can be useful.

Open a branch and PR containing both an RFD based on the
[RFD template](TEMPLATE/README.md) and the corresponding changes to the
[Design specification](../design/README.md). The RFD records motivation, goals,
alternatives, and the implementation plan. The Design diff states the proposed
normative behavior in specification paragraphs and `.sage` declarations. This
workflow is itself specified by [the book specification](../design/meta/book.md#meta.book.rfd_changes).

Each RFD is a subdirectory like `rfds/my-rfd/README.md` with a companion
`implementation.md` for tracking steps. Add it to `SUMMARY.md`. RFDs can have
subchapters or other accompanying material.

An unsettled proposal is listed under *Draft* in `SUMMARY.md`. Acceptance moves
it to *Accepted* and makes its implementation plan eligible to begin. Draft
RFDs may record alternatives and proof obligations, but destination design
pages must distinguish their planned mechanisms from built behavior.

Identify independently testable requirements as named **specification anchors**
in the Design chapters changed by the branch. Link the RFD's rationale and
implementation steps to those anchors. Include a high-level overview of the
needed changes, with direct links to the affected Design pages and anchors, so
a reviewer can move from the proposal to each normative change. The book and
anchor system are subject
to the same process: changes to how specifications are named, rendered, or
validated change their own Design rules. See the
[authoring contract](../contributing/maintaining-the-docs.md#writing-a-rule) for
the syntax and current tooling limitations.

If the PR is accepted, the RFD will be merged in. At that point you open implementation PRs based on the RFD until it is completed. Each implementation PR should update the RFD's `implementation.md` to reflect its status.

Before marking the RFD complete, account for every affected specification
anchor through the generated inventory. Record genuinely cross-cutting choices
and feature-local rationale in the RFD itself. If preserving reasoning that
predates an RFD, label the record retrospective; do not invent a past proposal
or acceptance date.

Finally, move it from *Accepted* to *Completed* in SUMMARY.md (the path stays
the same).

The latest version of the overall design is documented in the [Architecture & Design section](../design/README.md) — RFDs describe the journey, architecture pages describe the destination.
