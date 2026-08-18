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
    scope: ScopeSymbol<'db>,
    owner_span: AbsoluteSpan<'db>,
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
            LocalAssociatedOwner::Trait(_) => {
                lower_item(db, owner, scope, owner_span, source, *item)
            }
        };
        items.push(trait_item_def(definition));
    }

    let items = target.alloc_slice(&items);
    let generics: Vec<_> = owner_generics.into_iter().collect();
    let generics = target.alloc_slice(&generics);
    Stashed::new(target, Binder::new(items, generics))
}

#[salsa::tracked]
pub(crate) fn local_impl_associated_item<'db>(
    db: &'db dyn crate::Db,
    impl_sym: crate::local_syms::impls::LocalImplSym<'db>,
    key: LocalAssociatedItemKey<'db>,
) -> Option<LocalModItemSym<'db>> {
    let (source, cst) = impl_sym.cst(db).open_deref();
    let mut occurrence = 0;
    let item = source[cst.items].iter().find(|item| {
        if item_kind_and_name(source, **item) != (key.kind, key.name) {
            return false;
        }
        let matches = occurrence == key.occurrence;
        occurrence += 1;
        matches
    })?;
    Some(lower_item(
        db,
        LocalAssociatedOwner::Impl(impl_sym),
        impl_sym.scope(db),
        impl_sym.span(db),
        source,
        *item,
    ))
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
