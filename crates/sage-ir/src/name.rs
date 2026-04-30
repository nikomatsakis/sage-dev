/// Interned identifier. Equality is O(1) integer comparison.
#[salsa::interned(debug)]
pub struct Name<'db> {
    #[returns(ref)]
    pub text: String,
}
