use sage_stash::{AllocStashData, Ptr, Stashed};

use crate::cst::paths::Path;
use crate::span::RelativeSpan;

pub type MacroInvocationCst<'db> = Stashed<Ptr<MacroInvocationCstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct MacroInvocationCstData<'db> {
    pub path: Ptr<Path<'db>>,
    pub input_tokens: InputTokens<'db>,
    pub span: RelativeSpan,
}

#[salsa::tracked(debug)]
pub struct InputTokens<'db> {
    #[returns(ref)]
    pub text: String,
}

impl sage_stash::StashDirect for InputTokens<'_> {}
