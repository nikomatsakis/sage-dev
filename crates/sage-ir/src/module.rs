use crate::item::ModItem;
use crate::source::SourceFile;

/// Opaque crate number (matches rustc's CrateNum).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct CrateNum(pub u32);

/// Opaque definition index within a crate.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct DefIndex(pub u32);

/// A resolved module — either a local source file, an inline `mod`
/// declaration, or an external crate module.
#[salsa::interned(debug)]
pub struct Module<'db> {
    pub source: ModuleSource<'db>,
}

impl<'db> Module<'db> {
    /// Walk up `LocalInline` parents to find the backing source file,
    /// if any. Returns `None` for external modules.
    pub fn containing_file(self, db: &'db dyn crate::Db) -> Option<SourceFile> {
        let mut current = self;
        loop {
            match current.source(db) {
                ModuleSource::Local { file, .. } => return Some(file),
                ModuleSource::LocalInline { parent, .. } => current = parent,
                ModuleSource::External(..) => return None,
            }
        }
    }
}

/// Where a module's content comes from.
#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum ModuleSource<'db> {
    /// Workspace module backed by a source file (`mod foo;` → `foo.rs`).
    Local {
        file: SourceFile,
        /// The parent module, if any. None for the crate root.
        parent: Option<Module<'db>>,
        /// The `mod foo;` item in the parent that declared this
        /// module. None only for the crate root (lib.rs/main.rs has
        /// no declaring item). Useful so that a path resolving to a
        /// module itself can produce `Symbol::Local(Item::Mod(m))` in
        /// O(1) instead of searching the parent's items.
        declaration: Option<ModItem<'db>>,
    },
    /// Workspace module declared inline in its parent's file
    /// (`mod foo { ... }`). First-class Module — its `ModItem`'s
    /// tracked `items` field feeds `module_items` / `module_memmap`
    /// directly, so macro-expanded inline modules are walkable.
    LocalInline {
        parent: Module<'db>,
        mod_item: ModItem<'db>,
    },
    /// External crate module, queryable via TcxDb.
    External(CrateNum, DefIndex),
}
