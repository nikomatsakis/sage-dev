use sage_stash::StashDirect;

use crate::cst::enums::{EnumCst, VariantCst};
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;
use crate::symbol::Symbol;

#[salsa::tracked(debug)]
pub struct LocalEnumSym<'db> {
    pub name: Name<'db>,
    pub scope: ScopeSymbol<'db>,

    #[returns(ref)]
    pub cst: EnumCst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}

impl StashDirect for LocalEnumSym<'_> {}

impl<'db> LocalEnumSym<'db> {
    pub fn attrs(
        self,
        db: &'db dyn crate::Db,
    ) -> (
        &'db sage_stash::Stash,
        &'db [crate::cst::attrs::AttrCst<'db>],
    ) {
        let (stash, data) = self.cst(db).open_deref();
        (stash, &stash[data.attrs])
    }
}

#[salsa::tracked(debug)]
pub struct LocalVariantSym<'db> {
    pub name: Name<'db>,
    pub parent_enum: LocalEnumSym<'db>,
    pub cst: VariantCst<'db>,
    #[tracked]
    pub span: AbsoluteSpan<'db>,
    pub is_tuple: bool,
}

impl StashDirect for LocalVariantSym<'_> {}

impl<'db> LocalVariantSym<'db> {
    pub fn has_fields(self, db: &'db dyn crate::Db) -> bool {
        let parent_enum = self.parent_enum(db);
        let (stash, _) = parent_enum.cst(db).open_deref();
        !stash[self.cst(db).fields].is_empty()
    }
}

#[salsa::tracked(debug)]
pub struct LocalVariantCtorSym<'db> {
    pub name: Name<'db>,
    pub variant: LocalVariantSym<'db>,
}

impl StashDirect for LocalVariantCtorSym<'_> {}

#[salsa::tracked(returns(ref))]
pub fn enum_variants<'db>(db: &'db dyn crate::Db, sym: LocalEnumSym<'db>) -> Vec<Symbol<'db>> {
    let (stash, data) = sym.cst(db).open_deref();
    let variants = &stash[data.variants];
    let enum_span = sym.span(db);

    let mut symbols = Vec::new();
    for v in variants {
        let abs_span = AbsoluteSpan {
            source: enum_span.source,
            start: enum_span.start + v.span.start,
            end: enum_span.start + v.span.end,
        };

        let is_tuple = is_tuple_variant(db, stash, v);

        let variant_sym = LocalVariantSym::new(db, v.name, sym, *v, abs_span, is_tuple);
        symbols.push(variant_sym.into());

        if is_tuple {
            let ctor = LocalVariantCtorSym::new(db, v.name, variant_sym);
            symbols.push(ctor.into());
        }
    }
    symbols
}

fn is_tuple_variant<'db>(
    db: &'db dyn crate::Db,
    stash: &sage_stash::Stash,
    v: &VariantCst<'db>,
) -> bool {
    let fields = &stash[v.fields];
    if fields.is_empty() {
        return false;
    }
    // Tuple variants have positional field names ("0", "1", ...)
    fields[0]
        .name
        .text(db)
        .starts_with(|c: char| c.is_ascii_digit())
}
