//! `TcxDb` implementation backed by `TyCtxt<'tcx>`.
//!
//! `RustcTcxDb` is never sent across threads — it stays on the original
//! `after_expansion` thread. The salsa thread communicates with it via
//! channels (see `driver.rs`).

use rustc_hir::def::DefKind;
use rustc_hir::def::MacroKinds;
use rustc_hir::def_id::{CrateNum as RustcCrateNum, DefId};
use rustc_hir::find_attr;
use rustc_middle::ty::TyCtxt;

use sage_ir::module::{CrateNum, DefIndex};
use sage_ir::resolve::{MacroKind, Namespace};
use sage_ir::tcx::RawChild;

/// `TcxDb` backed by rustc's `TyCtxt`.
///
/// Lives on the original thread only — never crosses thread boundaries.
pub struct RustcTcxDb<'tcx> {
    tcx: TyCtxt<'tcx>,
}

impl<'tcx> RustcTcxDb<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>) -> Self {
        Self { tcx }
    }

    pub fn extern_crate(&self, name: &str) -> Option<CrateNum> {
        for &cnum in self.tcx.crates(()) {
            if self.tcx.crate_name(cnum).as_str() == name {
                return Some(CrateNum(cnum.as_u32()));
            }
        }
        None
    }

    pub fn module_children(&self, crate_num: CrateNum, def_index: DefIndex) -> Vec<RawChild> {
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
            if !child.vis.is_public() {
                continue;
            }

            let child_name = child.ident.name.as_str().to_owned();
            let cn = CrateNum(child_did.krate.as_u32());
            let di = DefIndex(child_did.index.as_u32());
            let kind = self.tcx.def_kind(child_did);

            for ns in namespaces_for_def_kind(kind) {
                results.push(RawChild {
                    name: child_name.clone(),
                    crate_num: cn,
                    def_index: di,
                    namespace: ns,
                });
            }
        }
        results
    }

    pub fn is_builtin_derive(&self, crate_num: CrateNum, def_index: DefIndex) -> bool {
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

    pub fn def_path(&self, crate_num: CrateNum, def_index: DefIndex) -> Option<String> {
        let def_id = DefId {
            krate: RustcCrateNum::from_u32(crate_num.0),
            index: rustc_hir::def_id::DefIndex::from_u32(def_index.0),
        };
        Some(self.tcx.def_path_str(def_id))
    }

    pub fn expand_proc_macro_derive(
        &self,
        _crate_num: CrateNum,
        _def_index: DefIndex,
        _item_source: &str,
    ) -> Option<String> {
        None // Stub — real implementation in Phase 3
    }
}

/// Map a `DefKind` to the namespace(s) it occupies.
fn namespaces_for_def_kind(kind: DefKind) -> Vec<Namespace> {
    match kind {
        DefKind::Mod
        | DefKind::Enum
        | DefKind::Trait
        | DefKind::TraitAlias
        | DefKind::TyAlias
        | DefKind::ForeignTy
        | DefKind::AssocTy
        | DefKind::TyParam
        | DefKind::Union => vec![Namespace::Type],

        DefKind::Fn
        | DefKind::AssocFn
        | DefKind::Const { .. }
        | DefKind::AssocConst { .. }
        | DefKind::Static { .. }
        | DefKind::ConstParam
        | DefKind::AnonConst
        | DefKind::InlineConst => vec![Namespace::Value],

        DefKind::Struct => vec![Namespace::Type, Namespace::Value],
        DefKind::Variant => vec![Namespace::Type, Namespace::Value],
        DefKind::Ctor(..) => vec![Namespace::Value],
        DefKind::Macro(kinds) => {
            let mut ns = Vec::new();
            if kinds.contains(MacroKinds::BANG) {
                ns.push(Namespace::Macro(MacroKind::Bang));
            }
            if kinds.contains(MacroKinds::ATTR) {
                ns.push(Namespace::Macro(MacroKind::Attr));
            }
            if kinds.contains(MacroKinds::DERIVE) {
                ns.push(Namespace::Macro(MacroKind::Derive));
            }
            ns
        }
        _ => Vec::new(),
    }
}
