//! Explicit implementation/test placeholders for unfinished contract guarantees.
//!
//! Existing correspondence, runtime-engine, and Salsa fixture tests exercise the
//! implemented portions. These stubs have no assertions and supply no behavioral
//! evidence; replace them as the remaining portions are implemented.
#![feature(register_tool)]
#![register_tool(sage)]

mod todo {
    // {anchor}`meta.contracts.validated_session`<<
    #[test]
    #[sage::test(meta.contracts.validated_session)]
    fn automatic_runtime_loading_is_cached_and_rejects_unavailable_models() {
        // TODO: Exercise missing files, explicit roots, parallel first use, and
        // model edits becoming visible only when a new loading session starts.
    }
    // >>

    // {anchor}`meta.contracts.cache_invocations`<<
    #[test]
    #[sage::test(meta.contracts.cache_invocations)]
    fn ordinary_rust_call_boundaries_are_instrumented() {
        // TODO: Observe real ordinary function entries, including two functions
        // mapped to the same operation, through the selected instrumentation.
    }
    // >>

    // {anchor}`meta.contracts.observation_coverage`<<
    #[test]
    #[sage::test(meta.contracts.observation_coverage)]
    fn declared_effects_and_async_boundaries_have_complete_scope_accounting() {
        // TODO: Exercise effect hooks and async/task propagation; reject absent
        // observation within the declared scope and identify its test context.
    }
    // >>
}
