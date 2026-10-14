// {anchor}`crates.sage_test_harness.compile_tests.fixtures`
// {anchor}`crates.sage_test_harness.compile_tests.rendering`
/// Root input: one per file in the workspace.
#[salsa::input(debug)]
#[sage::item(crate::types::SourceFile)]
pub struct SourceFile {
    #[returns(ref)]
    pub path: String,
    #[returns(ref)]
    pub text: String,
}

/// Normalize logical paths, retaining leading parent components used by a
/// source root nested inside its package directory.
pub fn normalize_path(path: &str) -> String {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}
