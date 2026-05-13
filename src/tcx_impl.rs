//! `TcxDb` implementation backed by `TyCtxt<'tcx>`.
//!
//! `RustcTcxDb` is never sent across threads — it stays on the original
//! `after_expansion` thread. The salsa thread communicates with it via
//! channels (see `driver.rs`).

extern crate proc_macro;

use std::sync::Arc;

use rustc_expand::proc_macro::DeriveProcMacro;
use rustc_hir::def::DefKind;
use rustc_hir::def::MacroKinds;
use rustc_hir::def_id::{CrateNum as RustcCrateNum, DefId};
use rustc_hir::find_attr;
use rustc_metadata::creader::CStore;
use rustc_middle::ty::TyCtxt;
use rustc_proc_macro::bridge::server::SAME_THREAD;
use rustc_span::def_id::DefIndex as RustcDefIndex;

use sage_ir::module::{CrateNum, DefIndex};
use sage_ir::resolve::{MacroKind, Namespace};
use sage_ir::tcx::RawChild;

use crate::proc_macro_srv::SageServer;

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

    pub fn is_module(&self, crate_num: CrateNum, def_index: DefIndex) -> bool {
        assert!(
            crate_num.0 != 0,
            "TcxDb must not be called with LOCAL_CRATE"
        );

        let def_id = DefId {
            krate: RustcCrateNum::from_u32(crate_num.0),
            index: rustc_hir::def_id::DefIndex::from_u32(def_index.0),
        };

        matches!(self.tcx.def_kind(def_id), DefKind::Mod)
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
        crate_num: CrateNum,
        def_index: DefIndex,
        item_source: &str,
    ) -> Option<String> {
        let def_id = DefId {
            krate: RustcCrateNum::from_u32(crate_num.0),
            index: RustcDefIndex::from_u32(def_index.0),
        };

        // Only proc-macro crates can be loaded via load_macro_untracked.
        let kind = self.tcx.def_kind(def_id);
        if !matches!(kind, DefKind::Macro(kinds) if kinds.contains(MacroKinds::DERIVE)) {
            return None;
        }

        let cstore = CStore::from_tcx(self.tcx);
        // catch_unwind guards against ICEs in load_macro_untracked for
        // re-exported macros whose DefId doesn't point to a proc-macro crate.
        let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cstore.load_macro_untracked(self.tcx, def_id)
        }))
        .ok()?;

        use rustc_expand::base::SyntaxExtensionKind;
        use rustc_metadata::creader::LoadedMacro;
        let LoadedMacro::ProcMacro(ext) = loaded else {
            return None;
        };
        let SyntaxExtensionKind::Derive(ref arc) = ext.kind else {
            return None;
        };

        // SAFETY: `load_macro_untracked` on a proc-macro crate always wraps
        // `ProcMacro::CustomDerive { client }` in `Arc::new(DeriveProcMacro { client })`.
        // `DeriveProcMacro` is a single-field struct and `Client` is `Copy`.
        // `MultiItemModifier` doesn't extend `Any`, so we can't downcast.
        let client = unsafe {
            let ptr = Arc::as_ref(arc) as *const dyn rustc_expand::base::MultiItemModifier
                as *const DeriveProcMacro;
            (*ptr).client
        };

        let input: proc_macro2::TokenStream = item_source.parse().ok()?;
        match client.run(&SAME_THREAD, SageServer::new(), input, false) {
            Ok(output) => Some(output.to_string()),
            Err(_) => None,
        }
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
