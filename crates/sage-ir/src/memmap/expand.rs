//! Macro resolution and expansion within the MEM-map.
//!
//! `expand_macro` is a salsa tracked query that produces a
//! `MacroExpansion` (text + provenance). The fixpoint loop parses
//! and seeds the result into `MemmapEntry`s.
//!
//! The loop iterates until no new callees are discovered:
//! each pass resolves macro paths, expands new callees, and
//! recursively processes nested `MacroUse`s in expansion output.

use crate::Db;
use crate::module::ModSymbol;
use crate::resolve::SourceRoot;
use crate::span::{MacroExpansion, ParseSource};

use super::data::*;
use super::resolve_path::resolve_macro_path;
use super::seed::seed_from_items;

/// Maximum nesting depth for macro expansion (same as rustc's default).
const MAX_EXPANSION_DEPTH: usize = 128;

/// Resolve and expand all `MacroUse` entries in `entries`, iterating
/// until no new callees are discovered.
pub(super) fn resolve_and_expand_macros<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
    entries: &mut Vec<MemmapEntry<'db>>,
) {
    loop {
        let snapshot: Vec<MemmapEntry<'db>> = entries.clone();
        let changed = resolve_expand_pass(db, module, source_root, entries, &snapshot, 0);
        if !changed {
            break;
        }
    }
}

/// Single pass: walk all `MacroUse` entries (including nested ones inside
/// expansions), resolve paths, expand new callees. Returns true if any
/// new expansion was added. `depth` tracks nesting level — expansion
/// stops at `MAX_EXPANSION_DEPTH`.
fn resolve_expand_pass<'db>(
    db: &'db dyn Db,
    module: ModSymbol<'db>,
    source_root: SourceRoot,
    entries: &mut Vec<MemmapEntry<'db>>,
    root_entries: &[MemmapEntry<'db>],
    depth: usize,
) -> bool {
    if depth >= MAX_EXPANSION_DEPTH {
        return false;
    }

    let mut changed = false;

    for i in 0..entries.len() {
        if let MemmapEntry::MacroUse(mu) = &entries[i] {
            let path = mu.path;
            let input = mu.input;
            let existing_callees: Vec<MacroCallee<'db>> =
                mu.expansions.iter().map(|e| e.callee).collect();

            let callees = resolve_macro_path(db, module, source_root, root_entries, path);

            let new_callees: Vec<MacroCallee<'db>> = callees
                .into_iter()
                .filter(|c| !existing_callees.contains(c))
                .collect();

            if new_callees.is_empty() {
                // Recurse into existing expansions to resolve nested macros.
                // Pass the top-level root_entries so nested macros can find
                // definitions from the enclosing module scope.
                if let MemmapEntry::MacroUse(mu) = &mut entries[i] {
                    for exp in &mut mu.expansions {
                        if resolve_expand_pass(
                            db,
                            module,
                            source_root,
                            &mut exp.entries,
                            root_entries,
                            depth + 1,
                        ) {
                            changed = true;
                        }
                    }
                }
                continue;
            }

            let mut new_expansions: Vec<Expansion<'db>> = Vec::new();
            for callee in &new_callees {
                let expansion_result = expand_macro(db, *callee, input);
                let text = expansion_result.text(db);
                let expanded_entries = if text.is_empty() {
                    Vec::new()
                } else {
                    let parse_source = ParseSource::MacroExpansion(expansion_result);
                    let items = parse_source.parse(db);
                    seed_from_items(db, items)
                };
                new_expansions.push(Expansion {
                    callee: *callee,
                    entries: expanded_entries,
                });
            }

            if let MemmapEntry::MacroUse(mu) = &mut entries[i] {
                mu.expansions.extend(new_expansions);
            }
            changed = true;
        }
    }

    changed
}

/// Expand a macro invocation with a specific callee.
///
/// Returns a `MacroExpansion` with provenance linking back to the
/// invocation site. Memoized by salsa — identical `(callee, input)`
/// pairs share the same expansion result.
///
/// Does NOT parse or seed entries — the fixpoint loop handles that
/// via `ParseSource::parse()` and `seed_from_items`.
#[salsa::tracked]
pub fn expand_macro<'db>(
    db: &'db dyn Db,
    callee: MacroCallee<'db>,
    input: MacroInput<'db>,
) -> MacroExpansion<'db> {
    let text = match callee {
        MacroCallee::Rules(def) => {
            let body = def.body_tokens(db);
            if body.is_empty() {
                String::new()
            } else {
                body.clone()
            }
        }
        MacroCallee::Builtin(_) | MacroCallee::Proc { .. } => String::new(),
    };
    MacroExpansion::new(db, input, text)
}
