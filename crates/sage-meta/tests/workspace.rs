//! Run the same repository validation as `sage-meta check`, without a CLI subprocess.
#![feature(register_tool)]
#![register_tool(sage)]
use std::path::Path;

#[test]
#[sage::test(meta.reviews.gate)]
fn repository_docs_model_and_rust_correspond() -> Result<(), sage_meta::check::CheckError> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    sage_meta::check(&root).ensure_valid()
}
