use std::str;

use sage_stash::StashDirect;

use crate::Db;
use crate::cst::macro_invocations::MacroInvocationCst;
use crate::local_syms::LocalModItemSym;
use crate::scope::ScopeSymbol;
use crate::span::{AbsoluteSpan, ParseSource};
use crate::symbol::MacroDefSymbol;

/// An item-level macro invocation (e.g. `m!()` or `foo::bar::m!()`).
#[salsa::tracked(debug)]
pub struct LocalMacroInvocationSym<'db> {
    pub scope: ScopeSymbol<'db>,

    #[returns(ref)]
    pub cst: MacroInvocationCst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalMacroInvocationSym<'_> {}

#[salsa::tracked]
impl<'db> LocalMacroInvocationSym<'db> {
    #[salsa::tracked(returns(ref))]
    pub fn parse_output(
        self,
        db: &'db dyn Db,
        macro_def: MacroDefSymbol<'db>,
    ) -> Vec<LocalModItemSym<'db>> {
        let source = ParseSource::BangMacro(macro_def, self);
        let scope = self.scope(db);
        let (stash, cst) = self.cst(db).open_deref();
        let input_tokens: &[u8] = &stash[cst.input_tokens];
        let input_str = str::from_utf8(input_tokens).unwrap();
        let output_str = match macro_def.expand(db, input_str) {
            Ok(text) => text,
            Err(_) => return vec![LocalModItemSym::Error(self.span(db))],
        };
        crate::parse::parse_str_to_cst(db, source, &output_str, scope)
    }
}
