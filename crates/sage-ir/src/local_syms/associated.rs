use sage_stash::{Slice, Stash, StashCopy, Stashed};

use crate::cst::traits::TraitItemCst;
use crate::generic_param::GenericParam;
use crate::local_syms::LocalAssociatedOwner;
use crate::local_syms::consts::LocalConstSym;
use crate::local_syms::fns::LocalFnSym;
use crate::local_syms::type_aliases::LocalTypeAliasSym;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;
use crate::ty::{Binder, TraitItemDef, TraitItems};

pub(crate) fn lower_items<'db>(
    db: &'db dyn crate::Db,
    owner: LocalAssociatedOwner<'db>,
    scope: ScopeSymbol<'db>,
    owner_span: AbsoluteSpan<'db>,
    source: &Stash,
    source_items: Slice<TraitItemCst<'db>>,
    owner_generics: impl IntoIterator<Item = GenericParam<'db>>,
) -> Stashed<TraitItems<'db>> {
    let mut target = Stash::new();
    let mut items = Vec::new();

    for item in &source[source_items] {
        let definition = match *item {
            TraitItemCst::Fn(source_ptr) => {
                let data = source[source_ptr];
                let mut item_stash = Stash::new();
                let target_ptr = source_ptr.stash_copy(source, &mut item_stash);
                let cst = Stashed::new(item_stash, target_ptr);
                TraitItemDef::Function(
                    LocalFnSym::new(
                        db,
                        data.name,
                        scope,
                        Some(owner),
                        cst,
                        owner_span.resolve(data.span),
                        owner_span,
                    )
                    .into(),
                )
            }
            TraitItemCst::Type(source_ptr) => {
                let data = source[source_ptr];
                let mut item_stash = Stash::new();
                let target_ptr = source_ptr.stash_copy(source, &mut item_stash);
                let cst = Stashed::new(item_stash, target_ptr);
                TraitItemDef::Type(
                    LocalTypeAliasSym::new(
                        db,
                        data.name,
                        scope,
                        Some(owner),
                        cst,
                        owner_span.resolve(data.span),
                        owner_span,
                    )
                    .into(),
                )
            }
            TraitItemCst::Const(source_ptr) => {
                let data = source[source_ptr];
                let mut item_stash = Stash::new();
                let target_ptr = source_ptr.stash_copy(source, &mut item_stash);
                let cst = Stashed::new(item_stash, target_ptr);
                TraitItemDef::Const(
                    LocalConstSym::new(
                        db,
                        data.name,
                        scope,
                        Some(owner),
                        cst,
                        owner_span.resolve(data.span),
                        owner_span,
                    )
                    .into(),
                )
            }
        };
        items.push(definition);
    }

    let items = target.alloc_slice(&items);
    let generics: Vec<_> = owner_generics.into_iter().collect();
    let generics = target.alloc_slice(&generics);
    Stashed::new(target, Binder::new(items, generics))
}
