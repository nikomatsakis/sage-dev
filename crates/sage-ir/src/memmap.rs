//! Module MEM-map: the single source of truth for "what's in a module."
//!
//! `module_memmap` replaces `module_items` as the canonical query for
//! module-level name resolution. It contains named members (items +
//! use-redirects), glob stems, macro uses, and anonymous items.

use crate::Db;
use crate::item::Item;
use crate::module::{Module, ModuleSource};
use crate::name::Name;
use crate::resolve::Namespace;
use crate::span::SpanIndices;
use crate::types::Path;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// The MEM-map for a single module.
#[salsa::tracked(debug)]
pub struct ModuleMemmap<'db> {
    #[returns(ref)]
    pub entries: Vec<MemmapEntry<'db>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum MemmapEntry<'db> {
    Named(NamedMember<'db>),
    MacroUse(MacroUse<'db>),
    Glob(GlobStem<'db>),
    Anon(Item<'db>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct NamedMember<'db> {
    pub name: Name<'db>,
    pub ns: Namespace,
    pub kind: NamedMemberKind<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum NamedMemberKind<'db> {
    /// A regular item (struct, enum, fn, mod, const, etc.)
    Item(Item<'db>),
    /// A use-redirect: `use foo::bar` or `use foo::bar as baz`
    Redirect { target: Path<'db> },
    /// A `macro_rules!` definition (Phase 2+)
    MacroDef(MacroDef<'db>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct MacroUse<'db> {
    pub path: Path<'db>,
    pub state: MacroUseState<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum MacroUseState<'db> {
    Unresolved,
    Unexpanded(MacroDef<'db>),
    Expanded(Vec<MemmapEntry<'db>>),
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct GlobStem<'db> {
    pub source_module: Module<'db>,
}

#[salsa::tracked(debug)]
pub struct MacroDef<'db> {
    pub name: Name<'db>,
    #[returns(ref)]
    pub body_tokens: String,
    pub span: SpanIndices,
}

// ---------------------------------------------------------------------------
// module_memmap query — seeds from CST
// ---------------------------------------------------------------------------

use crate::lower::file_item_tree;
use crate::resolve::{SourceRoot, item_in_namespace, item_name, resolve_use_path_to_module};
use crate::types::UseKind;

/// Compute the MEM-map for a module. Seeds named members, use-redirects,
/// glob stems, and anonymous items from the CST.
#[salsa::tracked(returns(ref))]
pub fn module_memmap<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
) -> ModuleMemmap<'db> {
    let entries = match module.source(db) {
        ModuleSource::Local { file, .. } => {
            let items = file_item_tree(db, file);
            seed_from_items(db, module, source_root, crate_root, items)
        }
        ModuleSource::External(..) => Vec::new(),
    };
    ModuleMemmap::new(db, entries)
}

/// Seed MEM-map entries from the file_item_tree output.
fn seed_from_items<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    items: &[Item<'db>],
) -> Vec<MemmapEntry<'db>> {
    let mut entries = Vec::new();

    for &item in items {
        match item {
            Item::Use(group) => {
                for import in group.imports(db) {
                    match import.kind(db) {
                        UseKind::Named(alias) => {
                            entries.push(MemmapEntry::Named(NamedMember {
                                name: alias,
                                ns: Namespace::Type, // redirects resolve at lookup time
                                kind: NamedMemberKind::Redirect {
                                    target: import.path(db),
                                },
                            }));
                        }
                        UseKind::Glob => {
                            // Resolve the glob's target module eagerly
                            if let Ok(target) = resolve_use_path_to_module(
                                db,
                                module,
                                source_root,
                                crate_root,
                                *import,
                            ) {
                                entries.push(MemmapEntry::Glob(GlobStem {
                                    source_module: target,
                                }));
                            }
                        }
                        UseKind::Unnamed => {}
                    }
                }
            }
            Item::Impl(_) => {
                entries.push(MemmapEntry::Anon(item));
            }
            Item::Error(_) => {}
            _ => {
                if let Some(name) = item_name(db, item) {
                    if item_in_namespace(db, item, Namespace::Type) {
                        entries.push(MemmapEntry::Named(NamedMember {
                            name,
                            ns: Namespace::Type,
                            kind: NamedMemberKind::Item(item),
                        }));
                    }
                    if item_in_namespace(db, item, Namespace::Value) {
                        entries.push(MemmapEntry::Named(NamedMember {
                            name,
                            ns: Namespace::Value,
                            kind: NamedMemberKind::Item(item),
                        }));
                    }
                }
            }
        }
    }

    entries
}
