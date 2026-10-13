/// Root input: one per file in the workspace.
#[salsa::input(debug)]
#[sage::item(crate::types::SourceFile)]
pub struct SourceFile {
    #[returns(ref)]
    pub path: String,
    #[returns(ref)]
    pub text: String,
}
