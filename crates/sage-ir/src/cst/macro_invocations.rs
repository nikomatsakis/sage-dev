use sage_stash::{AllocStashData, Ptr, Slice, Stashed};

use crate::cst::paths::Path;
use crate::span::RelativeSpan;

pub type MacroInvocationCst<'db> = Stashed<Ptr<MacroInvocationCstData<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct MacroInvocationCstData<'db> {
    pub path: Ptr<Path<'db>>,
    pub input_tokens: Slice<u8>,
    pub span: RelativeSpan,
}
