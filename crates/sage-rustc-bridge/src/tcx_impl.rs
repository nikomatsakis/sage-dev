extern crate proc_macro;

use std::sync::Arc;

use rustc_expand::proc_macro::DeriveProcMacro;
use rustc_hir::def::DefKind;
use rustc_hir::def::MacroKinds;
use rustc_hir::def_id::{CRATE_DEF_INDEX, CrateNum as RustcCrateNum, DefId};
use rustc_hir::find_attr;
use rustc_metadata::creader::CStore;
use rustc_middle::ty::{self, TyCtxt};
use rustc_proc_macro::bridge::server::SAME_THREAD;
use rustc_span::def_id::DefIndex as RustcDefIndex;

use sage_ir::resolve::{MacroKind, Namespace};
use sage_ir::symbol::SymExtKind;
use sage_ir::symbol::{CrateNum, DefIndex};
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
            let def_kind = self.tcx.def_kind(child_did);
            let sym_ext_kind = sym_ext_kind_for_def_kind(def_kind);

            for ns in namespaces_for_def_kind(def_kind) {
                results.push(RawChild {
                    name: child_name.clone(),
                    crate_num: cn,
                    def_index: di,
                    namespace: ns,
                    kind: sym_ext_kind,
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

    pub fn item_name(&self, crate_num: CrateNum, def_index: DefIndex) -> Option<String> {
        let def_id = DefId {
            krate: RustcCrateNum::from_u32(crate_num.0),
            index: rustc_hir::def_id::DefIndex::from_u32(def_index.0),
        };
        Some(self.tcx.item_name(def_id).to_ident_string())
    }

    pub fn def_path(&self, crate_num: CrateNum, def_index: DefIndex) -> Option<String> {
        let def_id = DefId {
            krate: RustcCrateNum::from_u32(crate_num.0),
            index: rustc_hir::def_id::DefIndex::from_u32(def_index.0),
        };
        Some(self.tcx.def_path_str(def_id))
    }

    pub fn structured_def_path(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
    ) -> Option<sage_ir::tcx::ExternalDefPath> {
        use sage_ir::tcx::{ExternalDefPath, ExternalDefPathSegment};

        let def_id = DefId {
            krate: RustcCrateNum::from_u32(crate_num.0),
            index: rustc_hir::def_id::DefIndex::from_u32(def_index.0),
        };
        let crate_name = self.tcx.crate_name(def_id.krate).to_string();
        let mut segments = Vec::new();
        let mut current = def_id.index;
        while current != CRATE_DEF_INDEX {
            let segment_def_id = DefId {
                krate: def_id.krate,
                index: current,
            };
            let key = self.tcx.def_key(segment_def_id);
            if let Some(name) = key.disambiguated_data.data.get_opt_name() {
                segments.push(ExternalDefPathSegment {
                    name: name.to_string(),
                    kind: sym_ext_kind_for_def_kind(self.tcx.def_kind(segment_def_id)),
                });
            }
            current = key.parent?;
        }
        segments.reverse();
        Some(ExternalDefPath {
            krate: crate_name,
            segments,
        })
    }

    pub fn trait_signature(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
    ) -> Option<sage_ir::tcx::RawTraitSignature> {
        use sage_ir::tcx::{
            RawGenericParam, RawGenericParamKind, RawTraitSemantics, RawTraitSignature,
        };

        let def_id = rustc_def_id(crate_num, def_index);
        if !matches!(self.tcx.def_kind(def_id), DefKind::Trait) {
            return None;
        }

        let generics = self.tcx.generics_of(def_id);
        let mut raw_generics = Vec::with_capacity(generics.count());
        let mut complete = true;
        for index in 0..generics.count() {
            let param = generics.param_at(index, self.tcx);
            let kind = match param.kind {
                ty::GenericParamDefKind::Type { .. } => RawGenericParamKind::Type,
                ty::GenericParamDefKind::Lifetime => RawGenericParamKind::Lifetime,
                ty::GenericParamDefKind::Const { .. } => {
                    complete = false;
                    RawGenericParamKind::Const
                }
            };
            raw_generics.push(RawGenericParam {
                index: param.index,
                name: Some(param.name.as_str().to_owned()),
                kind,
            });
        }
        let self_param_index = raw_generics
            .iter()
            .find(|param| param.name.as_deref() == Some("Self"))?
            .index;

        let instantiated = self
            .tcx
            .predicates_of(def_id)
            .instantiate_identity(self.tcx);
        let identity_trait_ref = ty::TraitRef::identity(self.tcx, def_id);
        let mut predicates = Vec::new();
        for clause in instantiated.predicates {
            match clause.kind().skip_binder() {
                ty::ClauseKind::Trait(predicate) => {
                    // `predicates_of(Trait)` includes the implicit
                    // `Self: Trait<...>` identity fact. It is not a defining
                    // prerequisite for each impl candidate; local trait
                    // lowering likewise records only declared bounds.
                    if predicate.trait_ref == identity_trait_ref {
                        continue;
                    }
                    let Some(predicate) = raw_trait_predicate(self.tcx, predicate.trait_ref) else {
                        complete = false;
                        continue;
                    };
                    predicates.push(predicate);
                }
                ty::ClauseKind::RegionOutlives(_) | ty::ClauseKind::TypeOutlives(_) => {}
                ty::ClauseKind::Projection(_)
                | ty::ClauseKind::ConstArgHasType(..)
                | ty::ClauseKind::WellFormed(_)
                | ty::ClauseKind::ConstEvaluatable(_)
                | ty::ClauseKind::HostEffect(_)
                | ty::ClauseKind::UnstableFeature(_) => complete = false,
            }
        }

        let semantics = if self.tcx.lang_items().sized_trait() == Some(def_id) {
            RawTraitSemantics::Sized
        } else if self.tcx.lang_items().meta_sized_trait() == Some(def_id) {
            RawTraitSemantics::MetaSized
        } else {
            RawTraitSemantics::Ordinary
        };
        Some(RawTraitSignature {
            generics: raw_generics,
            self_param_index,
            predicates,
            semantics,
            complete,
        })
    }

    pub fn associated_items(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
    ) -> Option<sage_ir::tcx::RawAssociatedItems> {
        use sage_ir::tcx::{
            RawAssociatedItem, RawAssociatedItemKind, RawAssociatedItems, RawDefId,
        };

        let def_id = rustc_def_id(crate_num, def_index);
        if !matches!(self.tcx.def_kind(def_id), DefKind::Trait) {
            return None;
        }

        let complete = true;
        let mut items = Vec::new();
        for item in self.tcx.associated_items(def_id).in_definition_order() {
            let (name, kind, symbol_kind) = match item.kind {
                ty::AssocKind::Fn { name, .. } => {
                    (Some(name), RawAssociatedItemKind::Function, SymExtKind::Fn)
                }
                ty::AssocKind::Type {
                    data: ty::AssocTypeData::Normal(name),
                } => (
                    Some(name),
                    RawAssociatedItemKind::Type,
                    SymExtKind::TypeAlias,
                ),
                ty::AssocKind::Const { name, .. } => {
                    (Some(name), RawAssociatedItemKind::Const, SymExtKind::Const)
                }
                ty::AssocKind::Type {
                    data: ty::AssocTypeData::Rpitit(_),
                } => {
                    // RPITIT projections are anonymous compiler-generated
                    // items, so omitting them cannot hide a source-level name.
                    (None, RawAssociatedItemKind::Type, SymExtKind::TypeAlias)
                }
            };
            let Some(name) = name else {
                continue;
            };
            items.push(RawAssociatedItem {
                def: RawDefId {
                    crate_num: CrateNum(item.def_id.krate.as_u32()),
                    def_index: DefIndex(item.def_id.index.as_u32()),
                    kind: symbol_kind,
                },
                name: name.as_str().to_owned(),
                kind,
            });
        }
        Some(RawAssociatedItems { items, complete })
    }

    pub fn fn_signature(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
    ) -> Option<sage_ir::tcx::RawFnSignature> {
        use sage_ir::cst::Mutability;
        use sage_ir::tcx::{RawFnSignature, RawGenericParam, RawGenericParamKind, RawReceiver};

        let def_id = rustc_def_id(crate_num, def_index);
        if !matches!(self.tcx.def_kind(def_id), DefKind::AssocFn | DefKind::Fn) {
            return None;
        }

        let generics = self.tcx.generics_of(def_id);
        let mut raw_generics = Vec::with_capacity(generics.count());
        let mut complete = true;
        for index in 0..generics.count() {
            let param = generics.param_at(index, self.tcx);
            let kind = match param.kind {
                ty::GenericParamDefKind::Type { .. } => RawGenericParamKind::Type,
                ty::GenericParamDefKind::Lifetime => RawGenericParamKind::Lifetime,
                ty::GenericParamDefKind::Const { .. } => {
                    complete = false;
                    RawGenericParamKind::Const
                }
            };
            raw_generics.push(RawGenericParam {
                index: param.index,
                name: Some(param.name.as_str().to_owned()),
                kind,
            });
        }

        let assoc_item = matches!(self.tcx.def_kind(def_id), DefKind::AssocFn)
            .then(|| self.tcx.associated_item(def_id));
        let owner_trait = assoc_item
            .and_then(|item| item.trait_container(self.tcx))
            .and_then(|trait_def_id| {
                raw_trait_predicate(self.tcx, ty::TraitRef::identity(self.tcx, trait_def_id))
            });
        if assoc_item.is_some_and(|item| item.trait_container(self.tcx).is_some())
            && owner_trait.is_none()
        {
            complete = false;
        }

        let signature = self.tcx.fn_sig(def_id).instantiate_identity().skip_binder();
        if signature.safety != rustc_hir::Safety::Safe
            || signature.abi != rustc_abi::ExternAbi::Rust
            || signature.c_variadic
        {
            complete = false;
        }
        let mut inputs = signature.inputs().iter().copied();
        let receiver = match assoc_item.map(|item| item.kind) {
            Some(ty::AssocKind::Fn { has_self: true, .. }) => {
                let Some(source_receiver) = inputs.next() else {
                    return None;
                };
                let owner_self = owner_trait.as_ref().map(|predicate| &predicate.self_ty);
                match (source_receiver.kind(), owner_self) {
                    (ty::TyKind::Ref(_, inner, mutability), Some(owner_self))
                        if raw_ty(self.tcx, *inner).as_ref() == Some(owner_self) =>
                    {
                        Some(RawReceiver::Ref(match mutability {
                            rustc_hir::Mutability::Not => Mutability::Shared,
                            rustc_hir::Mutability::Mut => Mutability::Mut,
                        }))
                    }
                    (_, Some(owner_self))
                        if raw_ty(self.tcx, source_receiver).as_ref() == Some(owner_self) =>
                    {
                        Some(RawReceiver::Value)
                    }
                    _ => {
                        complete = false;
                        None
                    }
                }
            }
            _ => None,
        };

        let mut params = Vec::new();
        for input in inputs {
            let Some(input) = raw_ty(self.tcx, input) else {
                complete = false;
                continue;
            };
            params.push(input);
        }
        let ret = raw_ty(self.tcx, signature.output())?;

        let instantiated = self
            .tcx
            .predicates_of(def_id)
            .instantiate_identity(self.tcx);
        let mut predicates = Vec::new();
        for clause in instantiated.predicates {
            match clause.kind().skip_binder() {
                ty::ClauseKind::Trait(predicate) => {
                    let Some(predicate) = raw_trait_predicate(self.tcx, predicate.trait_ref) else {
                        complete = false;
                        continue;
                    };
                    if owner_trait.as_ref() == Some(&predicate) {
                        continue;
                    }
                    predicates.push(predicate);
                }
                ty::ClauseKind::RegionOutlives(_) | ty::ClauseKind::TypeOutlives(_) => {}
                ty::ClauseKind::Projection(_)
                | ty::ClauseKind::ConstArgHasType(..)
                | ty::ClauseKind::WellFormed(_)
                | ty::ClauseKind::ConstEvaluatable(_)
                | ty::ClauseKind::HostEffect(_)
                | ty::ClauseKind::UnstableFeature(_) => complete = false,
            }
        }

        Some(RawFnSignature {
            generics: raw_generics,
            owner_trait,
            receiver,
            params,
            ret,
            predicates,
            complete,
        })
    }

    pub fn adt_signature(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
    ) -> Option<sage_ir::tcx::RawAdtSignature> {
        use sage_ir::tcx::{
            RawAdtSignature, RawGenericDefault, RawGenericParam, RawGenericParamKind,
            RawTraitPredicate,
        };

        let def_id = rustc_def_id(crate_num, def_index);
        if !matches!(
            self.tcx.def_kind(def_id),
            DefKind::Struct | DefKind::Enum | DefKind::Union
        ) {
            return None;
        }

        let generics = self.tcx.generics_of(def_id);
        let mut raw_generics = Vec::with_capacity(generics.count());
        let mut defaults = Vec::with_capacity(generics.count());
        let mut ordinary_complete = true;
        let mut deferred_complete = true;
        for index in 0..generics.count() {
            let param = generics.param_at(index, self.tcx);
            let (kind, default) = match param.kind {
                ty::GenericParamDefKind::Type { has_default, .. } => {
                    let default = if has_default {
                        match raw_ty(
                            self.tcx,
                            self.tcx.type_of(param.def_id).instantiate_identity(),
                        ) {
                            Some(default) => RawGenericDefault::Type(default),
                            None => RawGenericDefault::Unsupported,
                        }
                    } else {
                        RawGenericDefault::Absent
                    };
                    (RawGenericParamKind::Type, default)
                }
                ty::GenericParamDefKind::Lifetime => {
                    (RawGenericParamKind::Lifetime, RawGenericDefault::Absent)
                }
                ty::GenericParamDefKind::Const { .. } => {
                    ordinary_complete = false;
                    deferred_complete = false;
                    (RawGenericParamKind::Const, RawGenericDefault::Absent)
                }
            };
            raw_generics.push(RawGenericParam {
                index: param.index,
                name: Some(param.name.as_str().to_owned()),
                kind,
            });
            defaults.push(default);
        }

        let instantiated = self
            .tcx
            .predicates_of(def_id)
            .instantiate_identity(self.tcx);
        let mut predicates: Vec<RawTraitPredicate> = Vec::new();
        for clause in instantiated.predicates {
            if !clause.kind().bound_vars().is_empty() {
                ordinary_complete = false;
                deferred_complete = false;
                continue;
            }
            match clause.kind().skip_binder() {
                ty::ClauseKind::Trait(predicate) => {
                    let Some(predicate) = raw_trait_predicate(self.tcx, predicate.trait_ref) else {
                        ordinary_complete = false;
                        deferred_complete = false;
                        continue;
                    };
                    predicates.push(predicate);
                }
                ty::ClauseKind::RegionOutlives(_) | ty::ClauseKind::TypeOutlives(_) => {}
                ty::ClauseKind::HostEffect(_) => deferred_complete = false,
                ty::ClauseKind::Projection(_)
                | ty::ClauseKind::ConstArgHasType(..)
                | ty::ClauseKind::WellFormed(_)
                | ty::ClauseKind::ConstEvaluatable(_)
                | ty::ClauseKind::UnstableFeature(_) => {
                    ordinary_complete = false;
                    deferred_complete = false;
                }
            }
        }

        Some(RawAdtSignature {
            generics: raw_generics,
            defaults,
            predicates,
            ordinary_complete,
            deferred_complete,
        })
    }

    pub fn inherent_method_candidates(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
        method_name: &str,
    ) -> Option<sage_ir::tcx::RawInherentMethodCandidates> {
        use sage_ir::tcx::{RawDefId, RawInherentMethodCandidate, RawInherentMethodCandidates};

        let adt_def_id = rustc_def_id(crate_num, def_index);
        if !matches!(
            self.tcx.def_kind(adt_def_id),
            DefKind::Struct | DefKind::Enum | DefKind::Union
        ) {
            return None;
        }

        let mut candidates = Vec::new();
        for &impl_def_id in self.tcx.inherent_impls(adt_def_id) {
            for item in self.tcx.associated_items(impl_def_id).in_definition_order() {
                let name = match item.kind {
                    ty::AssocKind::Fn {
                        name,
                        has_self: true,
                    } => name,
                    ty::AssocKind::Fn {
                        has_self: false, ..
                    }
                    | ty::AssocKind::Type { .. }
                    | ty::AssocKind::Const { .. } => continue,
                };
                if name.as_str() != method_name {
                    continue;
                }
                candidates.push(RawInherentMethodCandidate {
                    function: RawDefId {
                        crate_num: CrateNum(item.def_id.krate.as_u32()),
                        def_index: DefIndex(item.def_id.index.as_u32()),
                        kind: SymExtKind::Fn,
                    },
                    impl_def: RawDefId {
                        crate_num: CrateNum(impl_def_id.krate.as_u32()),
                        def_index: DefIndex(impl_def_id.index.as_u32()),
                        kind: SymExtKind::Impl,
                    },
                    externally_visible: self.tcx.visibility(item.def_id).is_public(),
                });
            }
        }
        candidates.sort_by_key(|candidate| {
            (
                candidate.function.crate_num.0,
                candidate.function.def_index.0,
            )
        });
        candidates.dedup_by_key(|candidate| candidate.function);
        Some(RawInherentMethodCandidates {
            candidates,
            complete: true,
        })
    }

    pub fn relevant_trait_impls(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
        self_head: Option<sage_ir::tcx::RawSelfTypeHead>,
    ) -> Option<sage_ir::tcx::RawRelevantImpls> {
        use sage_ir::tcx::{RawDefId, RawRelevantImpls};

        let trait_def_id = rustc_def_id(crate_num, def_index);
        if !matches!(self.tcx.def_kind(trait_def_id), DefKind::Trait) {
            return None;
        }

        let mut impls = Vec::new();
        for impl_def_id in self.tcx.all_impls(trait_def_id) {
            if impl_def_id.is_local() {
                continue;
            }
            if let Some(query_head) = self_head {
                let impl_ty = self.tcx.type_of(impl_def_id).instantiate_identity();
                if simplify_self_type_head(self.tcx, impl_ty).is_some_and(|head| head != query_head)
                {
                    continue;
                }
            }
            impls.push(RawDefId {
                crate_num: CrateNum(impl_def_id.krate.as_u32()),
                def_index: DefIndex(impl_def_id.index.as_u32()),
                kind: SymExtKind::Impl,
            });
        }
        impls.sort_by_key(|impl_def| (impl_def.crate_num.0, impl_def.def_index.0));
        impls.dedup();
        // `all_impls` is exhaustive only for explicit impls. Lang-item traits
        // can also have compiler-built candidates (for example scalar `Copy`
        // or coroutine `Iterator`), and auto traits are structural. Sage must
        // not turn the absence of an explicit header into `No` until it models
        // the corresponding built-in source. A coroutine is not represented
        // by Sage's rigid ADT self-type head, however, so an `Iterator` query
        // for a known ADT cannot overlap rustc's coroutine candidate.
        // `Sized` and `MetaSized` bypass this operation through their dedicated
        // structural candidates.
        let rigid_head_excludes_iterator_builtin =
            matches!(self_head, Some(sage_ir::tcx::RawSelfTypeHead::Adt(_)))
                && self.tcx.lang_items().iterator_trait() == Some(trait_def_id);
        let explicit_impls_are_exhaustive = !self.tcx.trait_is_auto(trait_def_id)
            && (self.tcx.lang_items().from_def_id(trait_def_id).is_none()
                || rigid_head_excludes_iterator_builtin);
        Some(RawRelevantImpls {
            impls,
            complete: explicit_impls_are_exhaustive,
        })
    }

    pub fn impl_signature(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
    ) -> Option<sage_ir::tcx::RawImplSignature> {
        use sage_ir::tcx::{
            RawGenericParam, RawGenericParamKind, RawImplSignature, RawTraitPredicate,
        };

        let impl_def_id = rustc_def_id(crate_num, def_index);
        if !matches!(
            self.tcx.def_kind(impl_def_id),
            DefKind::Impl { of_trait: true }
        ) {
            return None;
        }

        let generics = self.tcx.generics_of(impl_def_id);
        let mut raw_generics = Vec::with_capacity(generics.count());
        let mut complete = true;
        for index in 0..generics.count() {
            let param = generics.param_at(index, self.tcx);
            let kind = match param.kind {
                ty::GenericParamDefKind::Type { .. } => RawGenericParamKind::Type,
                ty::GenericParamDefKind::Lifetime => RawGenericParamKind::Lifetime,
                ty::GenericParamDefKind::Const { .. } => {
                    complete = false;
                    RawGenericParamKind::Const
                }
            };
            raw_generics.push(RawGenericParam {
                index: param.index,
                name: Some(param.name.as_str().to_owned()),
                kind,
            });
        }

        let trait_ref = self.tcx.impl_trait_ref(impl_def_id).instantiate_identity();
        let trait_ref = raw_trait_predicate(self.tcx, trait_ref)?;
        if self.tcx.impl_polarity(impl_def_id) != ty::ImplPolarity::Positive {
            complete = false;
        }
        if self.tcx.defaultness(impl_def_id).is_default() {
            complete = false;
        }

        let instantiated = self
            .tcx
            .predicates_of(impl_def_id)
            .instantiate_identity(self.tcx);
        let mut predicates: Vec<RawTraitPredicate> = Vec::new();
        for clause in instantiated.predicates {
            if !clause.kind().bound_vars().is_empty() {
                complete = false;
                continue;
            }
            match clause.kind().skip_binder() {
                ty::ClauseKind::Trait(predicate) => {
                    let Some(predicate) = raw_trait_predicate(self.tcx, predicate.trait_ref) else {
                        complete = false;
                        continue;
                    };
                    predicates.push(predicate);
                }
                ty::ClauseKind::RegionOutlives(_) | ty::ClauseKind::TypeOutlives(_) => {}
                ty::ClauseKind::Projection(_)
                | ty::ClauseKind::ConstArgHasType(..)
                | ty::ClauseKind::WellFormed(_)
                | ty::ClauseKind::ConstEvaluatable(_)
                | ty::ClauseKind::HostEffect(_)
                | ty::ClauseKind::UnstableFeature(_) => complete = false,
            }
        }

        Some(RawImplSignature {
            generics: raw_generics,
            trait_ref,
            predicates,
            complete,
        })
    }

    pub fn associated_type_value(
        &self,
        impl_crate_num: CrateNum,
        impl_def_index: DefIndex,
        associated_crate_num: CrateNum,
        associated_def_index: DefIndex,
    ) -> Option<sage_ir::tcx::RawAssociatedTypeValue> {
        use sage_ir::tcx::RawAssociatedTypeValue;

        let impl_def_id = rustc_def_id(impl_crate_num, impl_def_index);
        let associated_def_id = rustc_def_id(associated_crate_num, associated_def_index);
        if !matches!(
            self.tcx.def_kind(impl_def_id),
            DefKind::Impl { of_trait: true }
        ) || !matches!(self.tcx.def_kind(associated_def_id), DefKind::AssocTy)
        {
            return None;
        }
        let &impl_item = self
            .tcx
            .impl_item_implementor_ids(impl_def_id)
            .get(&associated_def_id)?;
        if !matches!(self.tcx.def_kind(impl_item), DefKind::AssocTy) {
            return None;
        }
        let generics = self.tcx.generics_of(impl_item);
        let complete = generics.own_params.is_empty();
        let value = self.tcx.type_of(impl_item).instantiate_identity();
        Some(RawAssociatedTypeValue {
            value: raw_ty(self.tcx, value)?,
            complete,
        })
    }

    pub fn adt_is_always_sized(&self, crate_num: CrateNum, def_index: DefIndex) -> Option<bool> {
        let def_id = rustc_def_id(crate_num, def_index);
        if !matches!(self.tcx.def_kind(def_id), DefKind::Struct | DefKind::Union) {
            return None;
        }
        let adt_ty = self.tcx.type_of(def_id).instantiate_identity();
        let typing_env = ty::TypingEnv::non_body_analysis(self.tcx, def_id);
        Some(adt_ty.is_sized(self.tcx, typing_env))
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

        let kind = self.tcx.def_kind(def_id);
        if !matches!(kind, DefKind::Macro(kinds) if kinds.contains(MacroKinds::DERIVE)) {
            return None;
        }

        let cstore = CStore::from_tcx(self.tcx);
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

        // SAFETY: rustc constructs `SyntaxExtensionKind::Derive` only from a
        // `DeriveProcMacro` erased behind `MultiItemModifier`; matching that
        // variant establishes the concrete allocation type for this cast.
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

    pub fn expand_proc_macro_bang(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
        input_tokens: &str,
    ) -> Option<String> {
        let def_id = DefId {
            krate: RustcCrateNum::from_u32(crate_num.0),
            index: RustcDefIndex::from_u32(def_index.0),
        };

        let cstore = CStore::from_tcx(self.tcx);
        let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cstore.load_macro_untracked(self.tcx, def_id)
        }))
        .ok()?;

        use rustc_expand::base::SyntaxExtensionKind;
        use rustc_metadata::creader::LoadedMacro;
        let LoadedMacro::ProcMacro(ext) = loaded else {
            return None;
        };
        let SyntaxExtensionKind::Bang(ref arc) = ext.kind else {
            return None;
        };

        // SAFETY: rustc constructs `SyntaxExtensionKind::Bang` only from a
        // concrete `BangProcMacro` erased behind the `BangProcMacro` trait;
        // matching that variant establishes the allocation type.
        let client = unsafe {
            let ptr = Arc::as_ref(arc) as *const dyn rustc_expand::base::BangProcMacro
                as *const rustc_expand::proc_macro::BangProcMacro;
            (*ptr).client
        };

        let input: proc_macro2::TokenStream = input_tokens.parse().ok()?;
        match client.run(&SAME_THREAD, SageServer::new(), input, false) {
            Ok(output) => Some(output.to_string()),
            Err(_) => None,
        }
    }

    pub fn expand_proc_macro_attr(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
        attr_args: &str,
        item_source: &str,
    ) -> Option<String> {
        let def_id = DefId {
            krate: RustcCrateNum::from_u32(crate_num.0),
            index: RustcDefIndex::from_u32(def_index.0),
        };

        let cstore = CStore::from_tcx(self.tcx);
        let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cstore.load_macro_untracked(self.tcx, def_id)
        }))
        .ok()?;

        use rustc_expand::base::SyntaxExtensionKind;
        use rustc_metadata::creader::LoadedMacro;
        let LoadedMacro::ProcMacro(ext) = loaded else {
            return None;
        };
        let SyntaxExtensionKind::Attr(ref arc) = ext.kind else {
            return None;
        };

        // SAFETY: rustc constructs `SyntaxExtensionKind::Attr` only from a
        // concrete `AttrProcMacro` erased behind the `AttrProcMacro` trait;
        // matching that variant establishes the allocation type.
        let client = unsafe {
            let ptr = Arc::as_ref(arc) as *const dyn rustc_expand::base::AttrProcMacro
                as *const rustc_expand::proc_macro::AttrProcMacro;
            (*ptr).client
        };

        let args: proc_macro2::TokenStream = attr_args.parse().ok()?;
        let input: proc_macro2::TokenStream = item_source.parse().ok()?;
        match client.run(&SAME_THREAD, SageServer::new(), args, input, false) {
            Ok(output) => Some(output.to_string()),
            Err(_) => None,
        }
    }
}

fn simplify_self_type_head(
    tcx: TyCtxt<'_>,
    ty: ty::Ty<'_>,
) -> Option<sage_ir::tcx::RawSelfTypeHead> {
    use sage_ir::tcx::{RawDefId, RawSelfTypeHead};

    Some(match ty.kind() {
        ty::TyKind::Adt(def, _) => {
            let def_id = def.did();
            RawSelfTypeHead::Adt(RawDefId {
                crate_num: CrateNum(def_id.krate.as_u32()),
                def_index: DefIndex(def_id.index.as_u32()),
                kind: sym_ext_kind_for_def_kind(tcx.def_kind(def_id)),
            })
        }
        ty::TyKind::Bool => RawSelfTypeHead::Bool,
        ty::TyKind::Char => RawSelfTypeHead::Char,
        ty::TyKind::Int(_) => RawSelfTypeHead::Int,
        ty::TyKind::Uint(_) => RawSelfTypeHead::Uint,
        ty::TyKind::Float(_) => RawSelfTypeHead::Float,
        ty::TyKind::Str => RawSelfTypeHead::Str,
        ty::TyKind::Ref(..) => RawSelfTypeHead::Ref,
        ty::TyKind::Tuple(_) => RawSelfTypeHead::Tuple,
        ty::TyKind::Slice(_) => RawSelfTypeHead::Slice,
        ty::TyKind::Array(..) => RawSelfTypeHead::Array,
        ty::TyKind::FnPtr(..) => RawSelfTypeHead::FnPtr,
        ty::TyKind::Never => RawSelfTypeHead::Never,
        ty::TyKind::Param(_)
        | ty::TyKind::Pat(..)
        | ty::TyKind::RawPtr(..)
        | ty::TyKind::Foreign(_)
        | ty::TyKind::FnDef(..)
        | ty::TyKind::UnsafeBinder(_)
        | ty::TyKind::Dynamic(..)
        | ty::TyKind::Closure(..)
        | ty::TyKind::CoroutineClosure(..)
        | ty::TyKind::Coroutine(..)
        | ty::TyKind::CoroutineWitness(..)
        | ty::TyKind::Alias(..)
        | ty::TyKind::Bound(..)
        | ty::TyKind::Placeholder(_)
        | ty::TyKind::Infer(_)
        | ty::TyKind::Error(_) => return None,
    })
}

fn rustc_def_id(crate_num: CrateNum, def_index: DefIndex) -> DefId {
    DefId {
        krate: RustcCrateNum::from_u32(crate_num.0),
        index: RustcDefIndex::from_u32(def_index.0),
    }
}

fn raw_trait_predicate(
    tcx: TyCtxt<'_>,
    trait_ref: ty::TraitRef<'_>,
) -> Option<sage_ir::tcx::RawTraitPredicate> {
    use rustc_middle::ty::GenericArgKind;
    use sage_ir::tcx::{RawDefId, RawTraitPredicate};

    let self_ty = raw_ty(tcx, trait_ref.self_ty())?;
    let mut args = Vec::new();
    for argument in trait_ref.args.iter().skip(1) {
        match argument.kind() {
            GenericArgKind::Type(ty) => args.push(raw_ty(tcx, ty)?),
            GenericArgKind::Lifetime(_) => {}
            GenericArgKind::Const(_) => return None,
        }
    }
    let def_id = trait_ref.def_id;
    Some(RawTraitPredicate {
        self_ty,
        trait_def: RawDefId {
            crate_num: CrateNum(def_id.krate.as_u32()),
            def_index: DefIndex(def_id.index.as_u32()),
            kind: SymExtKind::Trait,
        },
        args,
    })
}

fn raw_ty(tcx: TyCtxt<'_>, source: ty::Ty<'_>) -> Option<sage_ir::tcx::RawTy> {
    use sage_ir::cst::Mutability;
    use sage_ir::tcx::{RawDefId, RawTy};
    use sage_ir::ty::{FloatTy, IntTy, UintTy};

    Some(match source.kind() {
        ty::TyKind::Bool => RawTy::Bool,
        ty::TyKind::Char => RawTy::Char,
        ty::TyKind::Int(int_ty) => RawTy::Int(match int_ty {
            ty::IntTy::I8 => IntTy::I8,
            ty::IntTy::I16 => IntTy::I16,
            ty::IntTy::I32 => IntTy::I32,
            ty::IntTy::I64 => IntTy::I64,
            ty::IntTy::I128 => IntTy::I128,
            ty::IntTy::Isize => IntTy::Isize,
        }),
        ty::TyKind::Uint(uint_ty) => RawTy::Uint(match uint_ty {
            ty::UintTy::U8 => UintTy::U8,
            ty::UintTy::U16 => UintTy::U16,
            ty::UintTy::U32 => UintTy::U32,
            ty::UintTy::U64 => UintTy::U64,
            ty::UintTy::U128 => UintTy::U128,
            ty::UintTy::Usize => UintTy::Usize,
        }),
        ty::TyKind::Float(float_ty) => RawTy::Float(match float_ty {
            ty::FloatTy::F16 => return None,
            ty::FloatTy::F32 => FloatTy::F32,
            ty::FloatTy::F64 => FloatTy::F64,
            ty::FloatTy::F128 => return None,
        }),
        ty::TyKind::Str => RawTy::Str,
        ty::TyKind::Adt(def, arguments) => {
            let mut args = Vec::new();
            for argument in arguments.iter() {
                match argument.kind() {
                    ty::GenericArgKind::Type(ty) => args.push(raw_ty(tcx, ty)?),
                    ty::GenericArgKind::Lifetime(_) => {}
                    ty::GenericArgKind::Const(_) => return None,
                }
            }
            let def_id = def.did();
            RawTy::Adt(
                RawDefId {
                    crate_num: CrateNum(def_id.krate.as_u32()),
                    def_index: DefIndex(def_id.index.as_u32()),
                    kind: sym_ext_kind_for_def_kind(tcx.def_kind(def_id)),
                },
                args,
            )
        }
        ty::TyKind::Alias(ty::AliasTyKind::Projection, alias) => {
            use sage_ir::tcx::RawProjectionTy;

            let associated_ty = alias.def_id;
            let trait_def = tcx.parent(associated_ty);
            if !matches!(tcx.def_kind(trait_def), DefKind::Trait) {
                return None;
            }
            let parent_count = tcx.generics_of(associated_ty).parent_count;
            let mut parent_args = alias.args.iter().take(parent_count);
            let self_ty = match parent_args.next()?.kind() {
                ty::GenericArgKind::Type(ty) => raw_ty(tcx, ty)?,
                ty::GenericArgKind::Lifetime(_) | ty::GenericArgKind::Const(_) => return None,
            };
            let mut trait_args = Vec::new();
            for argument in parent_args {
                match argument.kind() {
                    ty::GenericArgKind::Type(ty) => trait_args.push(raw_ty(tcx, ty)?),
                    ty::GenericArgKind::Lifetime(_) => {}
                    ty::GenericArgKind::Const(_) => return None,
                }
            }
            let mut args = Vec::new();
            for argument in alias.args.iter().skip(parent_count) {
                match argument.kind() {
                    ty::GenericArgKind::Type(ty) => args.push(raw_ty(tcx, ty)?),
                    ty::GenericArgKind::Lifetime(_) => {}
                    ty::GenericArgKind::Const(_) => return None,
                }
            }
            RawTy::Associated(RawProjectionTy {
                associated_ty: RawDefId {
                    crate_num: CrateNum(associated_ty.krate.as_u32()),
                    def_index: DefIndex(associated_ty.index.as_u32()),
                    kind: SymExtKind::TypeAlias,
                },
                self_ty: Box::new(self_ty),
                trait_def: RawDefId {
                    crate_num: CrateNum(trait_def.krate.as_u32()),
                    def_index: DefIndex(trait_def.index.as_u32()),
                    kind: SymExtKind::Trait,
                },
                trait_args,
                args,
            })
        }
        ty::TyKind::Ref(_, inner, mutability) => RawTy::Ref(
            Box::new(raw_ty(tcx, *inner)?),
            if mutability.is_mut() {
                Mutability::Mut
            } else {
                Mutability::Shared
            },
        ),
        ty::TyKind::Tuple(elements) => RawTy::Tuple(
            elements
                .iter()
                .map(|ty| raw_ty(tcx, ty))
                .collect::<Option<_>>()?,
        ),
        ty::TyKind::Slice(element) => RawTy::Slice(Box::new(raw_ty(tcx, *element)?)),
        ty::TyKind::Param(param) => RawTy::Param(param.index),
        ty::TyKind::Never => RawTy::Never,
        ty::TyKind::Array(_, _)
        | ty::TyKind::Pat(_, _)
        | ty::TyKind::RawPtr(_, _)
        | ty::TyKind::Foreign(_)
        | ty::TyKind::FnDef(_, _)
        | ty::TyKind::FnPtr(..)
        | ty::TyKind::UnsafeBinder(_)
        | ty::TyKind::Dynamic(_, _)
        | ty::TyKind::Closure(_, _)
        | ty::TyKind::CoroutineClosure(_, _)
        | ty::TyKind::Coroutine(_, _)
        | ty::TyKind::CoroutineWitness(_, _)
        | ty::TyKind::Alias(_, _)
        | ty::TyKind::Bound(_, _)
        | ty::TyKind::Placeholder(_)
        | ty::TyKind::Infer(_)
        | ty::TyKind::Error(_) => return None,
    })
}

fn sym_ext_kind_for_def_kind(kind: DefKind) -> SymExtKind {
    use rustc_hir::def::CtorOf;
    match kind {
        DefKind::Fn | DefKind::AssocFn => SymExtKind::Fn,
        DefKind::Struct => SymExtKind::Struct,
        DefKind::Ctor(CtorOf::Struct, _) => SymExtKind::TupleStructCtor,
        DefKind::Enum => SymExtKind::Enum,
        DefKind::Variant => SymExtKind::Variant,
        DefKind::Ctor(CtorOf::Variant, _) => SymExtKind::VariantCtor,
        DefKind::Trait | DefKind::TraitAlias => SymExtKind::Trait,
        DefKind::Impl { .. } => SymExtKind::Impl,
        DefKind::Mod => SymExtKind::Mod,
        DefKind::TyAlias | DefKind::AssocTy => SymExtKind::TypeAlias,
        DefKind::Const { .. } | DefKind::AssocConst { .. } => SymExtKind::Const,
        DefKind::Static { .. } => SymExtKind::Static,
        DefKind::Macro(..) => SymExtKind::MacroDef,
        DefKind::Use => SymExtKind::Use,
        _ => SymExtKind::Other,
    }
}

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
