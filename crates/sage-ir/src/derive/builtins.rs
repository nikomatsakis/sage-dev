//! Builtin derive expansion.
//!
//! This module deliberately emits ordinary Rust items. Once parsed, a
//! builtin derive's impl participates in the same symbol, signature, and
//! solver queries as a handwritten impl.

use std::fmt::Write as _;

use crate::local_syms::LocalModItemSym;
use crate::resolve::Namespace;
use crate::symbol::{DefIndex, SymExt, SymExtKind, TraitSymbol};

/// Expand the represented subset of a compiler builtin derive.
///
/// `None` means this derive or input shape is not represented yet. In
/// particular, the first vertical slice supports `Clone` for non-generic
/// structs with named fields, which is sufficient for mini-redis's `Db`.
pub(crate) fn expand_builtin_derive(
    db: &dyn crate::Db,
    derive_name: &str,
    item: LocalModItemSym<'_>,
) -> Option<String> {
    match item {
        LocalModItemSym::Struct(struct_sym) if derive_name == "Clone" => {
            let (stash, cst) = struct_sym.cst(db).open_deref();
            if !stash[cst.generics].is_empty()
                || !stash[cst.where_clauses].is_empty()
                || stash[cst.fields].is_empty()
                || stash[cst.fields].iter().any(|field| {
                    field
                        .name
                        .text(db)
                        .bytes()
                        .all(|byte| byte.is_ascii_digit())
                })
            {
                return None;
            }

            let type_name = struct_sym.name(db).text(db);
            let mut output = format!(
                "impl ::core::clone::Clone for {type_name} {{ fn clone(&self) -> Self {{ Self {{"
            );
            for field in &stash[cst.fields] {
                let field_name = field.name.text(db);
                write!(
                    output,
                    "{field_name}: ::core::clone::Clone::clone(&self.{field_name}),"
                )
                .expect("writing to a String cannot fail");
            }
            output.push_str("} } } }");
            Some(output)
        }
        LocalModItemSym::Function(_)
        | LocalModItemSym::Struct(_)
        | LocalModItemSym::Enum(_)
        | LocalModItemSym::Trait(_)
        | LocalModItemSym::Impl(_)
        | LocalModItemSym::TypeAlias(_)
        | LocalModItemSym::Const(_)
        | LocalModItemSym::Static(_)
        | LocalModItemSym::Mod(_)
        | LocalModItemSym::Use(_)
        | LocalModItemSym::MacroDef(_)
        | LocalModItemSym::MacroInvocation(_)
        | LocalModItemSym::Error(_) => None,
    }
}

/// Trait implemented by a known compiler builtin derive.
///
/// These paths are hygienic compiler paths, not ordinary same-name lookup in
/// the source item's scope.
pub(crate) fn builtin_derive_trait<'db>(
    db: &'db dyn crate::Db,
    derive_name: &str,
) -> Option<TraitSymbol<'db>> {
    let path: &[&str] = match derive_name {
        "Clone" => &["clone", "Clone"],
        "Debug" => &["fmt", "Debug"],
        _ => return None,
    };
    let crate_num = db.tcx().extern_crate("core")?;
    let mut container = (crate_num, DefIndex(0));
    for (index, segment) in path.iter().enumerate() {
        let matches: Vec<_> = db
            .tcx()
            .module_children(container.0, container.1)
            .into_iter()
            .filter(|child| child.name == *segment && child.namespace == Namespace::Type)
            .collect();
        let [child] = matches.as_slice() else {
            return None;
        };
        if index + 1 == path.len() {
            if child.kind != SymExtKind::Trait {
                return None;
            }
            return Some(TraitSymbol::Ext(SymExt::new(
                db,
                child.crate_num,
                child.def_index,
                child.kind,
            )));
        }
        if child.kind != SymExtKind::Mod {
            return None;
        }
        container = (child.crate_num, child.def_index);
    }
    None
}
