//! `TcxDb` implementation backed by `TyCtxt<'tcx>`.

use rustc_hir::def::DefKind;
use rustc_hir::def::MacroKinds;
use rustc_hir::def_id::{CrateNum as RustcCrateNum, DefId};
use rustc_hir::find_attr;
use rustc_middle::ty::TyCtxt;

use sage_ir::Db;
use sage_ir::module::{CrateNum, DefIndex};
use sage_ir::name::Name;
use sage_ir::resolve::Namespace;
use sage_ir::symbol::{Symbol, SymbolSource};
use sage_ir::tcx::TcxDb;

/// `TcxDb` backed by rustc's `TyCtxt`.
///
/// # Safety
///
/// `TyCtxt<'tcx>` is `!Send + !Sync` (arena-allocated). We implement
/// `Send + Sync` because the `Database` is only used single-threaded
/// within the `after_expansion` callback. The `run_sage_with` pattern
/// ensures no cross-thread access.
pub struct RustcTcxDb<'tcx> {
    tcx: TyCtxt<'tcx>,
}

// SAFETY: single-threaded use within after_expansion callback.
unsafe impl Send for RustcTcxDb<'_> {}
unsafe impl Sync for RustcTcxDb<'_> {}

impl<'tcx> RustcTcxDb<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>) -> Self {
        Self { tcx }
    }
}

impl TcxDb for RustcTcxDb<'_> {
    fn extern_crate(&self, name: &str) -> Option<CrateNum> {
        for &cnum in self.tcx.crates(()) {
            if self.tcx.crate_name(cnum).as_str() == name {
                return Some(CrateNum(cnum.as_u32()));
            }
        }
        None
    }

    fn module_children<'db>(
        &self,
        db: &'db dyn Db,
        crate_num: CrateNum,
        def_index: DefIndex,
    ) -> Vec<(Name<'db>, Symbol<'db>, Namespace)> {
        assert!(
            crate_num.0 != 0,
            "TcxDb must not be called with LOCAL_CRATE"
        );

        let def_id = DefId {
            krate: RustcCrateNum::from_u32(crate_num.0),
            index: rustc_hir::def_id::DefIndex::from_u32(def_index.0),
        };

        let mut results = Vec::new();
        for child in self.tcx.module_children(def_id) {
            let Some(child_did) = child.res.opt_def_id() else {
                continue;
            };
            // Only pub items
            if !child.vis.is_public() {
                continue;
            }

            let child_name = child.ident.name.as_str();
            let name = Name::new(db, child_name.to_owned());
            let cn = CrateNum(child_did.krate.as_u32());
            let di = DefIndex(child_did.index.as_u32());
            let sym = Symbol::new(db, SymbolSource::External(cn, di));

            let kind = self.tcx.def_kind(child_did);
            for ns in namespaces_for_def_kind(kind) {
                results.push((name, sym, ns));
            }
        }
        results
    }

    fn is_builtin_derive(&self, crate_num: CrateNum, def_index: DefIndex) -> bool {
        assert!(
            crate_num.0 != 0,
            "TcxDb must not be called with LOCAL_CRATE"
        );

        let def_id = DefId {
            krate: RustcCrateNum::from_u32(crate_num.0),
            index: rustc_hir::def_id::DefIndex::from_u32(def_index.0),
        };

        #[allow(deprecated)]
        {
            let kind = self.tcx.def_kind(def_id);
            let is_derive_macro =
                matches!(kind, DefKind::Macro(kinds) if kinds.contains(MacroKinds::DERIVE));
            let has_builtin_attr = find_attr!(
                self.tcx,
                def_id,
                rustc_hir::attrs::AttributeKind::RustcBuiltinMacro { .. }
            );
            is_derive_macro && has_builtin_attr
        }
    }
}

/// Map a `DefKind` to the namespace(s) it occupies.
fn namespaces_for_def_kind(kind: DefKind) -> Vec<Namespace> {
    match kind {
        // Type namespace
        DefKind::Mod
        | DefKind::Enum
        | DefKind::Trait
        | DefKind::TraitAlias
        | DefKind::TyAlias
        | DefKind::ForeignTy
        | DefKind::AssocTy
        | DefKind::TyParam
        | DefKind::Union => vec![Namespace::Type],

        // Value namespace
        DefKind::Fn
        | DefKind::AssocFn
        | DefKind::Const { .. }
        | DefKind::AssocConst { .. }
        | DefKind::Static { .. }
        | DefKind::ConstParam
        | DefKind::AnonConst
        | DefKind::InlineConst => vec![Namespace::Value],

        // Struct: type + value (constructor)
        DefKind::Struct => vec![Namespace::Type, Namespace::Value],

        // Variant: type + value
        DefKind::Variant => vec![Namespace::Type, Namespace::Value],

        // Ctor: value only
        DefKind::Ctor(..) => vec![Namespace::Value],

        // Macros — all go to Macro namespace
        DefKind::Macro(_) => vec![Namespace::Macro],

        // Other kinds — skip
        _ => Vec::new(),
    }
}
