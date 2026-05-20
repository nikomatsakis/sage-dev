/// Interned identifier. Equality is O(1) integer comparison.
#[salsa::interned(debug)]
pub struct Name<'db> {
    #[returns(ref)]
    pub text: String,
}

impl sage_stash::StashDirect for Name<'_> {}

unsafe impl<'db> sage_stash::StashData<'db> for Name<'db> {
    fn static_type_id() -> std::any::TypeId {
        std::any::TypeId::of::<Name<'static>>()
    }
}

impl<'db> sage_stash::AllocStashData<'db> for Name<'db> {}
