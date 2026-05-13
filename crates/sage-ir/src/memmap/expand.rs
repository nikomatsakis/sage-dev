//! Macro resolution and expansion within the MEM-map.
//!
//! Snapshot-based: entries are cloned before mutation so resolution
//! reads the snapshot while expansion mutates live entries.
//!
//! Phase 4: `expand_macro` routes through `file_item_tree` on a
//! synthetic SourceFile, producing real `Item` tracked structs for
//! everything introduced by expansion. Inline `mod foo { .. }` bodies
//! are recursively lowered by `lower_mod` exactly like source-level
//! modules, so `LocalInline`'s `mod_item.items` is populated with
//! proper IR nodes.

use crate::Db;
use crate::item::MacroDefItem;
use crate::lower::file_item_tree;
use crate::module::Module;
use crate::resolve::SourceRoot;
use crate::source::SourceFile;

use super::data::*;
use super::resolve_path::resolve_macro_path;
use super::seed::seed_from_items;

/// Maximum macro expansion depth (same as rustc's default).
const MAX_EXPANSION_DEPTH: usize = 128;

/// Resolve and expand all unresolved `MacroUse` entries in `entries`.
pub(super) fn resolve_and_expand_macros<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    entries: &mut Vec<MemmapEntry<'db>>,
    depth: usize,
) {
    let snapshot: Vec<MemmapEntry<'db>> = entries.clone();
    resolve_with_snapshot(db, module, source_root, entries, &snapshot, depth);
}

/// Inner worker: resolve and expand macros in `entries`, using
/// `root_entries` as the resolution context.
fn resolve_with_snapshot<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    entries: &mut Vec<MemmapEntry<'db>>,
    root_entries: &[MemmapEntry<'db>],
    depth: usize,
) {
    if depth >= MAX_EXPANSION_DEPTH {
        // Depth cap: leave anything still Unresolved as-is; validator
        // reports UnresolvedMacro.
        return;
    }

    for i in 0..entries.len() {
        if let MemmapEntry::MacroUse(mu) = &entries[i] {
            if !matches!(mu.state, MacroUseState::Unresolved) {
                continue;
            }
            let path = mu.path;
            let input_tokens = mu.input_tokens.clone();

            let callees = resolve_macro_path(db, module, source_root, root_entries, path);
            match callees.len() {
                0 => {
                    // Stay Unresolved — next fixpoint iteration may succeed.
                }
                1 => {
                    let callee = callees[0];
                    let def = match callee {
                        MacroCallee::Rules(def) => def,
                        // Phase 3 infrastructure — builtins/proc-macros
                        // aren't classified yet.
                        MacroCallee::Builtin(_) | MacroCallee::Proc { .. } => continue,
                    };
                    let mut expanded = expand_macro(db, def, &input_tokens);
                    resolve_with_snapshot(
                        db,
                        module,
                        source_root,
                        &mut expanded,
                        root_entries,
                        depth + 1,
                    );
                    let expansion = Expansion {
                        callee,
                        entries: expanded,
                    };
                    entries[i] = MemmapEntry::MacroUse(MacroUse {
                        path,
                        input_tokens,
                        state: MacroUseState::Expanded(vec![expansion]),
                    });
                }
                _ => {
                    // Multiple candidates — record them as Resolved(callees).
                    // The validator reports this as AmbiguousMacro.
                    entries[i] = MemmapEntry::MacroUse(MacroUse {
                        path,
                        input_tokens,
                        state: MacroUseState::Resolved(callees),
                    });
                }
            }
        }
    }
}

/// Expand a macro's body into `MemmapEntry` values.
///
/// `input_tokens` is accepted for forward compatibility with real
/// `macro_rules!` matching, but the current expander ignores it — the
/// body is used as the verbatim expansion.
///
/// Routes through `file_item_tree` on a synthetic SourceFile so the
/// expanded items are real tracked structs (Struct, Enum, ModItem,
/// etc.), not `Item::Error` placeholders. Inline `mod foo { .. }`
/// bodies inside the expansion are handled by `lower_mod`'s normal
/// recursion — nothing special needed here.
pub fn expand_macro<'db>(
    db: &'db dyn Db,
    macro_def: MacroDefItem<'db>,
    _input_tokens: &str,
) -> Vec<MemmapEntry<'db>> {
    let body = macro_def.body_tokens(db);
    if body.is_empty() {
        return Vec::new();
    }

    // Synthetic SourceFile: the path encodes the macro's identity so
    // repeated expansions of the same macro can share the input. The
    // text is the macro body — Phase 4 ignores `_input_tokens` and
    // uses the body verbatim.
    let synthetic_path = format!("<macro:{}>", macro_def.name(db).text(db));
    let file = SourceFile::new(db, synthetic_path, body.clone());

    let items = file_item_tree(db, file);

    // Convert items → memmap entries via the same seeder used for
    // real source files.
    seed_from_items(db, items)
}
