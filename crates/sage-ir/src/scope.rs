use crate::Db;
use crate::item::ItemAst;
use crate::module::{ModSymbol, ModSymbolData};
use crate::resolve::{Resolver, SourceRoot};
use crate::source::SourceFile;
use crate::span::{AbsoluteSpan, ParseSource};
use crate::symbol::{EnumSymbol, FnSymbol, StructSymbol};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum ScopeSymbol<'db> {
    Module(ModSymbol<'db>),
}

impl<'db> ScopeSymbol<'db> {
    pub fn module(self) -> ModSymbol<'db> {
        match self {
            ScopeSymbol::Module(m) => m,
        }
    }

    pub fn resolver(self, db: &'db dyn Db, source_root: SourceRoot) -> Resolver<'db> {
        Resolver::new(db, source_root, self)
    }
}

/// Find the module that defines a given struct symbol.
pub fn struct_defining_module<'db>(
    db: &'db dyn Db,
    sym: StructSymbol<'db>,
    source_root: SourceRoot,
    fallback: ModSymbol<'db>,
) -> ModSymbol<'db> {
    defining_module_from_span(db, sym.as_ast().map(|a| a.span(db)), source_root, fallback)
}

/// Find the module that defines a given function symbol.
pub fn fn_defining_module<'db>(
    db: &'db dyn Db,
    sym: FnSymbol<'db>,
    source_root: SourceRoot,
    fallback: ModSymbol<'db>,
) -> ModSymbol<'db> {
    defining_module_from_span(db, sym.as_ast().map(|a| a.span(db)), source_root, fallback)
}

/// Find the module that defines a given enum symbol.
pub fn enum_defining_module<'db>(
    db: &'db dyn Db,
    sym: EnumSymbol<'db>,
    source_root: SourceRoot,
    fallback: ModSymbol<'db>,
) -> ModSymbol<'db> {
    defining_module_from_span(db, sym.as_ast().map(|a| a.span(db)), source_root, fallback)
}

fn defining_module_from_span<'db>(
    db: &'db dyn Db,
    span: Option<AbsoluteSpan<'db>>,
    source_root: SourceRoot,
    fallback: ModSymbol<'db>,
) -> ModSymbol<'db> {
    let Some(span) = span else {
        return fallback;
    };
    let file = match span.source {
        ParseSource::SourceFile(f) => f,
        ParseSource::MacroExpansion(_) => return fallback,
    };
    find_module_for_file(db, source_root, file).unwrap_or(fallback)
}

/// Walk the module tree to find which module is backed by a given source file.
fn find_module_for_file<'db>(
    db: &'db dyn Db,
    source_root: SourceRoot,
    target_file: SourceFile,
) -> Option<ModSymbol<'db>> {
    let files = source_root.files(db);
    let root_file = files.iter().find(|f| {
        let p = f.path(db);
        p == "lib.rs" || p == "main.rs"
    })?;
    let root_mod = crate::item::ModAst::crate_root(db, *root_file);
    let root = ModSymbol::ast(root_mod);
    search_module_tree(db, root, source_root, target_file)
}

fn search_module_tree<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
    target_file: SourceFile,
) -> Option<ModSymbol<'db>> {
    let ast = match module.data() {
        ModSymbolData::Ast(a) => a,
        ModSymbolData::Ext(_) => return None,
    };

    if ast.file(db) == Some(target_file) {
        return Some(module);
    }

    for item in ast.unexpanded_items(db) {
        if let ItemAst::Mod(child_mod_decl) = item {
            if let Some(resolved) =
                crate::resolve::resolve_mod(db, module, child_mod_decl, source_root)
            {
                if let Some(found) = search_module_tree(db, resolved, source_root, target_file) {
                    return Some(found);
                }
            }
        }
    }
    None
}
