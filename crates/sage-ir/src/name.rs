/// Interned identifier. Equality is O(1) integer comparison.
#[salsa::interned]
pub struct Name<'db> {
    #[returns(ref)]
    pub text: String,
}
