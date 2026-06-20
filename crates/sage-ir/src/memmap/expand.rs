//! Macro resolution and expansion within the MEM-map.
//!
//! `expand_macro` is a salsa tracked query that produces a
//! `MacroExpansion` (text + provenance). The fixpoint loop parses
//! and seeds the result into `MemmapEntry`s.
//!
//! The loop iterates until no new callees are discovered:
//! each pass resolves macro paths, expands new callees, and
//! recursively processes nested `MacroUse`s in expansion output.

use sage_stash::{Slice, Stash};

use crate::Db;
use crate::local_syms::mods::LocalModSym;
use crate::resolve::SourceRoot;
use crate::span::{MacroExpansion, ParseSource};
use crate::symbol::ModSymbol;

use super::data::*;
use super::resolve_path::resolve_macro_path;
use super::seed::seed_from_items;

/// Maximum nesting depth for macro expansion (same as rustc's default).
const MAX_EXPANSION_DEPTH: usize = 128;

/// Single pass: walk all `MacroUse` entries (including nested ones inside
/// expansions), resolve paths, expand new callees. Returns true if any
/// new expansion was added.
pub fn resolve_expand_pass<'db>(
    db: &'db dyn Db,
    module: LocalModSym<'db>,
    source_root: SourceRoot,
    stash: &mut Stash,
    entries: Slice<MemmapEntry<'db>>,
    root_entries: Slice<MemmapEntry<'db>>,
    depth: usize,
) -> bool {
    if depth >= MAX_EXPANSION_DEPTH {
        return false;
    }

    let mut changed = false;
    let len = stash[entries].len();

    for i in 0..len {
        let entry = stash[entries][i];
        let MemmapEntry::MacroInvocation(mu) = entry else {
            continue;
        };

        let path: Vec<_> = stash[mu.path].to_vec();
        let input = mu.input;
        let existing_callees: Vec<MacroCallee<'db>> =
            stash[mu.expansions].iter().map(|e| e.callee).collect();

        let callees =
            resolve_macro_path(db, module.into(), source_root, stash, root_entries, &path);

        let new_callees: Vec<MacroCallee<'db>> = callees
            .into_iter()
            .filter(|c| !existing_callees.contains(c))
            .collect();

        if new_callees.is_empty() {
            // Recurse into existing expansions to resolve nested macros.
            let expansion_slice = mu.expansions;
            let num_expansions = stash[expansion_slice].len();
            for j in 0..num_expansions {
                let exp = stash[expansion_slice][j];
                if resolve_expand_pass(
                    db,
                    module,
                    source_root,
                    stash,
                    exp.entries,
                    root_entries,
                    depth + 1,
                ) {
                    changed = true;
                }
            }
            continue;
        }

        let mut current_expansions = mu.expansions;
        for callee in &new_callees {
            let expansion_result = expand_macro(db, *callee, input);
            let text = expansion_result.text(db);
            let expanded_entries = if text.is_empty() {
                stash.alloc_slice(&[])
            } else {
                let parse_source = ParseSource::MacroExpansion(expansion_result);
                let items = parse_source.parse(db);
                seed_from_items(db, items, stash)
            };
            let expansion = Expansion {
                callee: *callee,
                entries: expanded_entries,
            };
            current_expansions = stash.append_one(current_expansions, expansion);
        }

        // Update the MacroUse in place with new expansions handle.
        let updated_mu = MacroInvocation {
            path: mu.path,
            input: mu.input,
            expansions: current_expansions,
        };
        stash[entries][i] = MemmapEntry::MacroInvocation(updated_mu);
        changed = true;
    }

    changed
}

/// Expand a macro invocation with a specific callee.
///
/// Returns a `MacroExpansion` with provenance linking back to the
/// invocation site. Memoized by salsa — identical `(callee, input)`
/// pairs share the same expansion result.
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
