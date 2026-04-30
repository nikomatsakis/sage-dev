/// Root input: one per file in the workspace.
#[salsa::input(debug)]
pub struct SourceFile {
    #[returns(ref)]
    pub path: String,
    #[returns(ref)]
    pub text: String,
}
