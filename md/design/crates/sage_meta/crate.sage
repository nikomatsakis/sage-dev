// Source: crates/sage-meta/Cargo.toml
crate sage_meta {
    """Build-time correspondence and runtime contract checks for Markdown specifications, .sage architectural models, and Rust.
    Validate model structure and dependency envelopes, map Rust declarations to model items, review implementation completeness and test adequacy for named specification paragraphs, and check observed architectural calls.
    Persist one current validation per anchor in a directory named by its complete anchor ID with separate metadata, complete input snapshots, and the agent report. Starting a review captures uniform source fragments and invalidates the previous report. Recorded reads extend the captured inputs; recording preserves them and saves a reviewer, report, and requested edits. The shared gate requires a current completed report with no requested edits and rejects validation directories for removed anchor IDs.
    Dependencies below describe workspace crates, including development dependencies; third-party libraries remain outside this model.
    """
    module language;
}
