/// Interned identifier. Equality is O(1) integer comparison.
#[salsa::interned(debug)]
pub struct Name<'db> {
    #[returns(ref)]
    pub text: String,
}

impl sage_stash::StashDirect for Name<'_> {}

// SAFETY: `Name<'db>` is a Salsa interned handle whose runtime representation
// and identity do not contain the database lifetime. Replacing only that
// phantom lifetime with `'static` therefore preserves the stored bits and
// gives a unique `StaticSelf` for this handle family.
unsafe impl<'db> sage_stash::StashData<'db> for Name<'db> {
    type StaticSelf = Name<'static>;
}

impl<'db> sage_stash::AllocStashData<'db> for Name<'db> {}

impl<'db> sage_reflect::Reflect<'db> for Name<'db> {
    fn reflect(
        &self,
        context: &mut sage_reflect::ReflectionContext<'_>,
        _stash: Option<&sage_stash::Stash>,
    ) -> sage_reflect::ValueNode {
        use salsa::plumbing::AsId;

        let key = sage_reflect::ReferenceKey {
            family: "name",
            id: self.as_id().as_bits(),
        };
        context.reflect_node("Name", |context| {
            context.reflected_value(&key).unwrap_or_else(|| {
                sage_reflect::ValueNode::scalar("Name", format!("interned:{}", key.id))
            })
        })
    }
}
