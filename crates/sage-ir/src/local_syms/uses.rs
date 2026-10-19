// {anchor}`crates.sage_test_harness.compile_tests.checking_scope`
// {anchor}`crates.sage_test_harness.compile_tests.diagnostic_counts`
use sage_stash::{Slice, StashDirect, Stashed};

use crate::cst::attrs::AttrCst;
use crate::cst::uses::UseImports;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;

/// A use declaration, desugared into flat imports.
#[salsa::tracked(debug)]
#[sage::item(crate::symbols::LocalUseSym)]
// {anchor}`crates.sage_ir.symbols.local_identity`
pub struct LocalUseSym<'db> {
    pub scope: ScopeSymbol<'db>,

    #[tracked]
    #[returns(ref)]
    pub attrs: Stashed<Slice<AttrCst<'db>>>,

    #[tracked]
    #[returns(ref)]
    pub imports: UseImports<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalUseSym<'_> {}

impl<'db> LocalUseSym<'db> {
    pub fn get_attrs(
        self,
        db: &'db dyn crate::Db,
    ) -> (&'db sage_stash::Stash, &'db [AttrCst<'db>]) {
        let (stash, attrs) = self.attrs(db).open();
        (stash, &stash[attrs])
    }
}

impl<'db> LocalUseSym<'db> {
    /// Validate imports even when no body subsequently refers to their names.
    /// Whole-crate clients must demand this explicitly; ordinary lookup remains lazy.
    pub fn diagnostics(self, db: &'db dyn crate::Db) -> Vec<crate::diagnostic::Diagnostic<'db>> {
        use crate::diagnostic::{Diagnostic, Span};
        use crate::resolve::{MacroKind, Namespace, Resolver};
        let mut resolver = Resolver::new(db, self.scope(db));
        let (stash, imports) = self.imports(db).open();
        let mut diagnostics = Vec::new();
        for import in &stash[imports] {
            let resolved = [
                Namespace::Type,
                Namespace::Value,
                Namespace::Macro(MacroKind::Bang),
                Namespace::Macro(MacroKind::Attr),
                Namespace::Macro(MacroKind::Derive),
            ]
            .into_iter()
            .any(|namespace| {
                !resolver
                    .resolve_path(stash, stash[import.path], namespace)
                    .is_empty()
            });
            if !resolved {
                diagnostics.push(Diagnostic::error(
                    Span::Relative(crate::local_syms::LocalModItemSym::Use(self), import.span),
                    "unresolved import",
                ));
            }
        }
        diagnostics
    }
}
