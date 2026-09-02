# Requests for Discussion (RFDs)

RFDs are a way of planning out larger changes. They aren't required but they can be useful.

The basic idea is that you open a PR adding an RFD based on the [RFD template](./TEMPLATE/README.md) into the `rfds` directory. Each RFD is itself a subdirectory like `rfds/my-rfd/README.md` with a companion `implementation.md` for tracking steps. Be sure to add it to the SUMMARY.md file. RFDs can have subchapters or other accompanying material.

An unsettled proposal is listed under *Draft* in `SUMMARY.md`. Acceptance moves
it to *Accepted* and makes its implementation plan eligible to begin. Draft
RFDs may record alternatives and proof obligations, but destination design
pages must distinguish their planned mechanisms from built behavior.

Large RFDs should identify their load-bearing, non-obvious requirements as
**design anchors** where those requirements are explained. An anchor has a
stable RFD-scoped identifier such as `SI-A4`, a short normative statement, and
the verification required to establish it. The surrounding prose explains the
design; the anchor states what implementation and review must preserve. Link
implementation steps and eventual tests back to the anchor rather than
duplicating its rule in several places. Design anchors are different from
`ezanchor` source excerpts: a design anchor states a requirement, while an
`ezanchor` shows the code which eventually implements it.

If the PR is accepted, the RFD will be merged in. At that point you open implementation PRs based on the RFD until it is completed. Each implementation PR should update the RFD's `implementation.md` to reflect its status.

Before marking the RFD complete, account for every design anchor. Promote its
living destination rule and required verification into the relevant
architecture chapter, retaining the identifier when it remains meaningful, or
record why an accepted design revision retired it. Link the chapter's Current
Status evidence to the implementation which established the anchor. Promote
any genuinely cross-cutting choice into a `D<n>` entry in [Architecture
Decisions](../design/decisions.md); leave feature-local rationale in the RFD.

Finally, move it from *Accepted* to *Completed* in SUMMARY.md (the path stays
the same).

The latest version of the overall design is documented in the [Architecture & Design section](../design/README.md) — RFDs describe the journey, architecture pages describe the destination.
