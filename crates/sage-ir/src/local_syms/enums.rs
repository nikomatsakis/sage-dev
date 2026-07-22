use sage_stash::{StashDirect, Stashed};

use crate::cst::enums::{EnumCst, VariantCst};
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;
use crate::symbol::Symbol;
use crate::ty::{Binder, EnumSig};

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

#[salsa::tracked]
impl<'db> LocalEnumSym<'db> {
    /// Computes enum generics, variants, and the ADT well-formedness
    /// environment under one coherent binder.
    #[salsa::tracked]
    pub fn sig(self, db: &'db dyn crate::Db) -> Stashed<Binder<'db, EnumSig<'db>>> {
        use crate::check::Check;
        use crate::cst::generics::CheckGenerics;
        use crate::resolve::Resolver;
        use crate::ty::{FieldSig, VariantSig};

        let (source, cst) = self.cst(db).open_deref();
        let mut cx = Check::new(db, source, Resolver::new(db, self.scope(db)));
        cx.current_sym = Some(crate::local_syms::LocalModItemSym::Enum(self));

        let parent: Symbol<'db> = self.into();
        let generics = cst.generics.check(db, &mut cx, parent);
        let variants: Vec<_> = source[cst.variants]
            .iter()
            .map(|variant| {
                let fields: Vec<_> = source[variant.fields]
                    .iter()
                    .map(|field| {
                        let ty = source[field.ty].check(&mut cx);
                        FieldSig {
                            name: field.name,
                            ty: cx.target_stash.alloc(ty),
                        }
                    })
                    .collect();
                VariantSig {
                    name: variant.name,
                    fields: cx.target_stash.alloc_slice(&fields),
                }
            })
            .collect();
        let variants = cx.target_stash.alloc_slice(&variants);
        let (where_clauses, solver_eligibility) = crate::check::trait_env::lower_predicates(
            &mut cx,
            cst.generics,
            generics,
            cst.where_clauses,
        );
        let parameter_env = cx.complete_parameter_env(where_clauses, solver_eligibility);
        cx.finish(Binder::new(
            EnumSig {
                variants,
                parameter_env,
            },
            generics,
        ))
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
