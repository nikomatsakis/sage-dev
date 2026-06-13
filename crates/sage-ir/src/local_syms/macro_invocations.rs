use crate::name::Name;
use crate::span::AbsoluteSpan;

/// An item-level macro invocation (e.g. `m!()` or `foo::bar::m!()`).
#[salsa::tracked(debug)]
pub struct LocalMacroInvocationSym<'db> {
    #[returns(ref)]
    pub path: Vec<Name<'db>>,

    /// The token stream passed to the macro at the invocation site — i.e.
    /// the contents of `m!(...)`, with the outer delimiter pair stripped.
    /// Empty for zero-argument invocations like `m!()`.
    #[returns(ref)]
    pub input_tokens: String,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}
