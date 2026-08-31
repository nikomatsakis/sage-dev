use sage_stash::{Slice, Stash, StashCopy, Stashed};

use crate::cst::traits::TraitItemCst;
use crate::generic_param::GenericParam;
use crate::local_syms::consts::LocalConstSym;
use crate::local_syms::fns::LocalFnSym;
use crate::local_syms::type_aliases::LocalTypeAliasSym;
use crate::local_syms::{LocalAssociatedOwner, LocalModItemSym};
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;
use crate::ty::{Binder, TraitItemDef, TraitItems};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum LocalAssociatedItemKind {
    Function,
    Type,
    Const,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct LocalAssociatedItemKey<'db> {
    pub kind: LocalAssociatedItemKind,
    pub name: Name<'db>,
    pub occurrence: u32,
}

pub(crate) fn lower_items<'db>(
    db: &'db dyn crate::Db,
    owner: LocalAssociatedOwner<'db>,
    source: &Stash,
    source_items: Slice<TraitItemCst<'db>>,
    owner_generics: impl IntoIterator<Item = GenericParam<'db>>,
) -> Stashed<TraitItems<'db>> {
    let mut target = Stash::new();
    let mut items = Vec::new();
    let mut occurrences = std::collections::HashMap::new();

    for item in &source[source_items] {
        let (kind, name) = item_kind_and_name(source, *item);
        let occurrence = occurrences.entry((kind, name)).or_insert(0);
        let key = LocalAssociatedItemKey {
            kind,
            name,
            occurrence: *occurrence,
        };
        *occurrence += 1;
        let definition = match owner {
            LocalAssociatedOwner::Impl(impl_sym) => local_impl_associated_item(db, impl_sym, key)
                .expect("the keyed impl item was read from the same CST"),
            LocalAssociatedOwner::Trait(trait_sym) => {
                local_trait_associated_item(db, trait_sym, key)
                    .expect("the keyed trait item was read from the same CST")
            }
        };
        items.push(trait_item_def(definition));
    }

    let items = target.alloc_slice(&items);
    let generics: Vec<_> = owner_generics.into_iter().collect();
    let generics = target.alloc_slice(&generics);
    Stashed::new(target, Binder::new(items, generics))
}

/// Enumerate the associated symbols owned directly by a trait or impl without
/// checking the owner's signature or any associated item body.
///
/// This is the membership boundary used by symbol browsing and by other
/// clients which need identity but not semantic item values.
pub fn local_associated_items<'db>(
    db: &'db dyn crate::Db,
    owner: LocalAssociatedOwner<'db>,
) -> &'db [LocalModItemSym<'db>] {
    match owner {
        LocalAssociatedOwner::Trait(owner) => local_trait_associated_items(db, owner),
        LocalAssociatedOwner::Impl(owner) => local_impl_associated_items(db, owner),
    }
}

#[salsa::tracked(returns(ref))]
fn local_trait_associated_items<'db>(
    db: &'db dyn crate::Db,
    owner: crate::local_syms::traits::LocalTraitSym<'db>,
) -> Vec<LocalModItemSym<'db>> {
    compute_local_associated_items(db, LocalAssociatedOwner::Trait(owner))
}

#[salsa::tracked(returns(ref))]
fn local_impl_associated_items<'db>(
    db: &'db dyn crate::Db,
    owner: crate::local_syms::impls::LocalImplSym<'db>,
) -> Vec<LocalModItemSym<'db>> {
    compute_local_associated_items(db, LocalAssociatedOwner::Impl(owner))
}

fn compute_local_associated_items<'db>(
    db: &'db dyn crate::Db,
    owner: LocalAssociatedOwner<'db>,
) -> Vec<LocalModItemSym<'db>> {
    let (source, source_items) = match owner {
        LocalAssociatedOwner::Trait(owner) => {
            let (source, cst) = owner.cst(db).open_deref();
            (source, cst.items)
        }
        LocalAssociatedOwner::Impl(owner) => {
            let (source, cst) = owner.cst(db).open_deref();
            (source, cst.items)
        }
    };

    let mut occurrences = std::collections::HashMap::new();
    source[source_items]
        .iter()
        .map(|item| {
            let (kind, name) = item_kind_and_name(source, *item);
            let occurrence = occurrences.entry((kind, name)).or_insert(0);
            let key = LocalAssociatedItemKey {
                kind,
                name,
                occurrence: *occurrence,
            };
            *occurrence += 1;
            match owner {
                LocalAssociatedOwner::Trait(owner) => local_trait_associated_item(db, owner, key)
                    .expect("the keyed trait item was read from the same CST"),
                LocalAssociatedOwner::Impl(owner) => local_impl_associated_item(db, owner, key)
                    .expect("the keyed impl item was read from the same CST"),
            }
        })
        .collect()
}

#[salsa::tracked]
pub(crate) fn local_trait_associated_item<'db>(
    db: &'db dyn crate::Db,
    trait_sym: crate::local_syms::traits::LocalTraitSym<'db>,
    key: LocalAssociatedItemKey<'db>,
) -> Option<LocalModItemSym<'db>> {
    let (source, cst) = trait_sym.cst(db).open_deref();
    let item = keyed_source_item(source, cst.items, key)?;
    Some(lower_item(
        db,
        LocalAssociatedOwner::Trait(trait_sym),
        trait_sym.scope(db),
        trait_sym.span(db),
        source,
        item,
    ))
}

#[salsa::tracked]
pub(crate) fn local_impl_associated_item<'db>(
    db: &'db dyn crate::Db,
    impl_sym: crate::local_syms::impls::LocalImplSym<'db>,
    key: LocalAssociatedItemKey<'db>,
) -> Option<LocalModItemSym<'db>> {
    let (source, cst) = impl_sym.cst(db).open_deref();
    let item = keyed_source_item(source, cst.items, key)?;
    Some(lower_item(
        db,
        LocalAssociatedOwner::Impl(impl_sym),
        impl_sym.scope(db),
        impl_sym.span(db),
        source,
        item,
    ))
}

fn keyed_source_item<'db>(
    source: &Stash,
    source_items: Slice<TraitItemCst<'db>>,
    key: LocalAssociatedItemKey<'db>,
) -> Option<TraitItemCst<'db>> {
    let mut occurrence = 0;
    source[source_items].iter().copied().find(|item| {
        if item_kind_and_name(source, *item) != (key.kind, key.name) {
            return false;
        }
        let matches = occurrence == key.occurrence;
        occurrence += 1;
        matches
    })
}

fn item_kind_and_name<'db>(
    source: &Stash,
    item: TraitItemCst<'db>,
) -> (LocalAssociatedItemKind, Name<'db>) {
    match item {
        TraitItemCst::Fn { cst, .. } => (LocalAssociatedItemKind::Function, source[cst].name),
        TraitItemCst::Type { cst, .. } => (LocalAssociatedItemKind::Type, source[cst].name),
        TraitItemCst::Const { cst, .. } => (LocalAssociatedItemKind::Const, source[cst].name),
    }
}

fn lower_item<'db>(
    db: &'db dyn crate::Db,
    owner: LocalAssociatedOwner<'db>,
    scope: ScopeSymbol<'db>,
    owner_span: AbsoluteSpan<'db>,
    source: &Stash,
    item: TraitItemCst<'db>,
) -> LocalModItemSym<'db> {
    match item {
        TraitItemCst::Fn {
            cst: source_ptr,
            placement,
        } => {
            let data = source[source_ptr];
            let mut item_stash = Stash::new();
            let target_ptr = source_ptr.stash_copy(source, &mut item_stash);
            let cst = Stashed::new(item_stash, target_ptr);
            let item_span = owner_span.resolve(placement);
            LocalModItemSym::Function(LocalFnSym::new(
                db,
                data.name,
                scope,
                Some(owner),
                cst,
                item_span,
            ))
        }
        TraitItemCst::Type {
            cst: source_ptr,
            placement,
        } => {
            let data = source[source_ptr];
            let mut item_stash = Stash::new();
            let target_ptr = source_ptr.stash_copy(source, &mut item_stash);
            let cst = Stashed::new(item_stash, target_ptr);
            let item_span = owner_span.resolve(placement);
            LocalModItemSym::TypeAlias(LocalTypeAliasSym::new(
                db,
                data.name,
                scope,
                Some(owner),
                cst,
                item_span,
            ))
        }
        TraitItemCst::Const {
            cst: source_ptr,
            placement,
        } => {
            let data = source[source_ptr];
            let mut item_stash = Stash::new();
            let target_ptr = source_ptr.stash_copy(source, &mut item_stash);
            let cst = Stashed::new(item_stash, target_ptr);
            let item_span = owner_span.resolve(placement);
            LocalModItemSym::Const(LocalConstSym::new(
                db,
                data.name,
                scope,
                Some(owner),
                cst,
                item_span,
            ))
        }
    }
}

fn trait_item_def(item: LocalModItemSym<'_>) -> TraitItemDef<'_> {
    match item {
        LocalModItemSym::Function(item) => TraitItemDef::Function(item.into()),
        LocalModItemSym::TypeAlias(item) => TraitItemDef::Type(item.into()),
        LocalModItemSym::Const(item) => TraitItemDef::Const(item.into()),
        LocalModItemSym::Struct(_)
        | LocalModItemSym::Enum(_)
        | LocalModItemSym::Trait(_)
        | LocalModItemSym::Impl(_)
        | LocalModItemSym::Static(_)
        | LocalModItemSym::Mod(_)
        | LocalModItemSym::Use(_)
        | LocalModItemSym::MacroDef(_)
        | LocalModItemSym::MacroInvocation(_)
        | LocalModItemSym::Error(_) => {
            unreachable!("associated-item lowering returned an item of the wrong kind")
        }
    }
}
