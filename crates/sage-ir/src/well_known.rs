//! Cached semantic identities for compiler-known library items.
//!
//! Fixed paths are a bootstrap mechanism: consumers use the typed symbols
//! returned here and do not depend on those spellings directly.

use sage_stash::Stash;

use crate::cst::paths::{Path, PathAnchor, PathAnchorKind, PathSegment};
use crate::name::Name;
use crate::resolve::{Namespace, Resolution, Resolver};
use crate::scope::{LocalCrateSymbol, ScopeSymbol};
use crate::span::RelativeSpan;
use crate::symbol::{FnSymbol, Symbol, TraitSymbol};
use crate::ty::TraitItemDef;

/// An overloadable binary-operator trait known to the Rust language.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum OperatorTrait {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    PartialEq,
    PartialOrd,
}

impl OperatorTrait {
    fn path(self) -> (&'static str, &'static str) {
        match self {
            Self::Add => ("ops", "Add"),
            Self::Sub => ("ops", "Sub"),
            Self::Mul => ("ops", "Mul"),
            Self::Div => ("ops", "Div"),
            Self::Rem => ("ops", "Rem"),
            Self::BitAnd => ("ops", "BitAnd"),
            Self::BitOr => ("ops", "BitOr"),
            Self::BitXor => ("ops", "BitXor"),
            Self::Shl => ("ops", "Shl"),
            Self::Shr => ("ops", "Shr"),
            Self::PartialEq => ("cmp", "PartialEq"),
            Self::PartialOrd => ("cmp", "PartialOrd"),
        }
    }
}

/// A method selected by overloadable binary-operator syntax.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum OperatorMethod {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl OperatorMethod {
    fn trait_and_name(self) -> (OperatorTrait, &'static str) {
        match self {
            Self::Add => (OperatorTrait::Add, "add"),
            Self::Sub => (OperatorTrait::Sub, "sub"),
            Self::Mul => (OperatorTrait::Mul, "mul"),
            Self::Div => (OperatorTrait::Div, "div"),
            Self::Rem => (OperatorTrait::Rem, "rem"),
            Self::BitAnd => (OperatorTrait::BitAnd, "bitand"),
            Self::BitOr => (OperatorTrait::BitOr, "bitor"),
            Self::BitXor => (OperatorTrait::BitXor, "bitxor"),
            Self::Shl => (OperatorTrait::Shl, "shl"),
            Self::Shr => (OperatorTrait::Shr, "shr"),
            Self::Eq => (OperatorTrait::PartialEq, "eq"),
            Self::Ne => (OperatorTrait::PartialEq, "ne"),
            Self::Lt => (OperatorTrait::PartialOrd, "lt"),
            Self::Le => (OperatorTrait::PartialOrd, "le"),
            Self::Gt => (OperatorTrait::PartialOrd, "gt"),
            Self::Ge => (OperatorTrait::PartialOrd, "ge"),
        }
    }
}

/// The typed owner and function identities for one operator method.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct OperatorMethodSymbols<'db> {
    pub trait_symbol: TraitSymbol<'db>,
    pub function: FnSymbol<'db>,
}

// ANCHOR: well_known_operator_queries
/// Resolve one well-known operator trait through its ordinary `core` path.
#[salsa::tracked]
pub fn operator_trait<'db>(
    db: &'db dyn crate::Db,
    local_crate: LocalCrateSymbol<'db>,
    operator: OperatorTrait,
) -> Option<TraitSymbol<'db>> {
    let (module, trait_name) = operator.path();
    let span = RelativeSpan { start: 0, end: 0 };
    let mut stash = Stash::new();
    let no_type_args = stash.alloc_slice(&[]);
    let segments = stash.alloc_slice(&[
        PathSegment {
            name: Name::new(db, module.to_owned()),
            type_args: no_type_args,
            span,
        },
        PathSegment {
            name: Name::new(db, trait_name.to_owned()),
            type_args: no_type_args,
            span,
        },
    ]);
    let path = Path::Anchored(
        PathAnchor {
            kind: PathAnchorKind::ExternCrate(Name::new(db, "core".to_owned())),
            span,
        },
        segments,
    );
    let mut resolver = Resolver::new(db, ScopeSymbol::Crate(local_crate));
    let results = resolver.resolve_path(&stash, path, Namespace::Type);
    let &[Resolution::Sym(symbol)] = results.as_slice() else {
        return None;
    };
    symbol.trait_symbol(db)
}

/// Resolve one well-known operator method through its owning trait's items.
#[salsa::tracked]
pub fn operator_method<'db>(
    db: &'db dyn crate::Db,
    local_crate: LocalCrateSymbol<'db>,
    operator: OperatorMethod,
) -> Option<OperatorMethodSymbols<'db>> {
    let (trait_kind, method_name) = operator.trait_and_name();
    let trait_symbol = operator_trait(db, local_crate, trait_kind)?;
    let items = trait_symbol.items(db)?;
    let method_name = Name::new(db, method_name.to_owned());
    let matching: Vec<_> = items.stash()[items.root().value]
        .iter()
        .copied()
        .filter(|item| {
            let symbol = match item {
                TraitItemDef::Function(function) => Symbol::from(*function),
                TraitItemDef::Type(associated_type) => Symbol::from(*associated_type),
                TraitItemDef::Const(constant) => Symbol::from(*constant),
            };
            symbol.name(db).is_some_and(|(name, namespace)| {
                name == method_name && namespace == Namespace::Value
            })
        })
        .collect();
    let [TraitItemDef::Function(function)] = matching.as_slice() else {
        return None;
    };
    Some(OperatorMethodSymbols {
        trait_symbol,
        function: *function,
    })
}
// ANCHOR_END: well_known_operator_queries

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use sage_stash::{Stash, Stashed};
    use salsa::Database as _;

    use super::*;
    use crate::db::Database;
    use crate::local_syms::mods::{LocalModSym, ModBodySource};
    use crate::scope::{Edition, local_crate};
    use crate::source::SourceFile;
    use crate::span::{AbsoluteSpan, ParseSource};
    use crate::symbol::{CrateNum, DefIndex, SymExtKind};
    use crate::tcx::{
        ExternalDefPath, RawAssociatedItem, RawAssociatedItemKind, RawAssociatedItems, RawChild,
        RawDefId, RawFnSignature, RawRelevantImpls, RawSelfTypeHead, RawTraitSignature, TcxDb,
    };

    const CORE: CrateNum = CrateNum(1);
    const REEXPORTED_OWNER: CrateNum = CrateNum(2);
    const OPS_MODULE: DefIndex = DefIndex(1);
    const CMP_MODULE: DefIndex = DefIndex(2);
    const REEXPORTED_ADD: DefIndex = DefIndex(60);

    const TRAIT_CASES: &[(OperatorTrait, &str, &str, DefIndex)] = &[
        (OperatorTrait::Add, "ops", "Add", DefIndex(10)),
        (OperatorTrait::Sub, "ops", "Sub", DefIndex(11)),
        (OperatorTrait::Mul, "ops", "Mul", DefIndex(12)),
        (OperatorTrait::Div, "ops", "Div", DefIndex(13)),
        (OperatorTrait::Rem, "ops", "Rem", DefIndex(14)),
        (OperatorTrait::BitAnd, "ops", "BitAnd", DefIndex(15)),
        (OperatorTrait::BitOr, "ops", "BitOr", DefIndex(16)),
        (OperatorTrait::BitXor, "ops", "BitXor", DefIndex(17)),
        (OperatorTrait::Shl, "ops", "Shl", DefIndex(18)),
        (OperatorTrait::Shr, "ops", "Shr", DefIndex(19)),
        (OperatorTrait::PartialEq, "cmp", "PartialEq", DefIndex(20)),
        (OperatorTrait::PartialOrd, "cmp", "PartialOrd", DefIndex(21)),
    ];

    const METHOD_CASES: &[(OperatorMethod, OperatorTrait, &str, DefIndex)] = &[
        (
            OperatorMethod::Add,
            OperatorTrait::Add,
            "add",
            DefIndex(100),
        ),
        (
            OperatorMethod::Sub,
            OperatorTrait::Sub,
            "sub",
            DefIndex(101),
        ),
        (
            OperatorMethod::Mul,
            OperatorTrait::Mul,
            "mul",
            DefIndex(102),
        ),
        (
            OperatorMethod::Div,
            OperatorTrait::Div,
            "div",
            DefIndex(103),
        ),
        (
            OperatorMethod::Rem,
            OperatorTrait::Rem,
            "rem",
            DefIndex(104),
        ),
        (
            OperatorMethod::BitAnd,
            OperatorTrait::BitAnd,
            "bitand",
            DefIndex(105),
        ),
        (
            OperatorMethod::BitOr,
            OperatorTrait::BitOr,
            "bitor",
            DefIndex(106),
        ),
        (
            OperatorMethod::BitXor,
            OperatorTrait::BitXor,
            "bitxor",
            DefIndex(107),
        ),
        (
            OperatorMethod::Shl,
            OperatorTrait::Shl,
            "shl",
            DefIndex(108),
        ),
        (
            OperatorMethod::Shr,
            OperatorTrait::Shr,
            "shr",
            DefIndex(109),
        ),
        (
            OperatorMethod::Eq,
            OperatorTrait::PartialEq,
            "eq",
            DefIndex(110),
        ),
        (
            OperatorMethod::Ne,
            OperatorTrait::PartialEq,
            "ne",
            DefIndex(111),
        ),
        (
            OperatorMethod::Lt,
            OperatorTrait::PartialOrd,
            "lt",
            DefIndex(112),
        ),
        (
            OperatorMethod::Le,
            OperatorTrait::PartialOrd,
            "le",
            DefIndex(113),
        ),
        (
            OperatorMethod::Gt,
            OperatorTrait::PartialOrd,
            "gt",
            DefIndex(114),
        ),
        (
            OperatorMethod::Ge,
            OperatorTrait::PartialOrd,
            "ge",
            DefIndex(115),
        ),
    ];

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    enum TraitFault {
        None,
        Missing,
        Duplicate,
        WrongKind,
        Reexport,
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    enum MethodFault {
        None,
        MissingMetadata,
        IncompleteMetadata,
        Missing,
        MissingName,
        WrongKind,
        Duplicate,
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    struct FixtureMode {
        trait_fault: TraitFault,
        method_fault: MethodFault,
    }

    impl Default for FixtureMode {
        fn default() -> Self {
            Self {
                trait_fault: TraitFault::None,
                method_fault: MethodFault::None,
            }
        }
    }

    type CallLog = Arc<Mutex<Vec<String>>>;

    #[derive(Clone)]
    struct OperatorTcx {
        mode: FixtureMode,
        calls: CallLog,
    }

    impl OperatorTcx {
        fn new(mode: FixtureMode) -> (Self, CallLog) {
            let calls = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    mode,
                    calls: calls.clone(),
                },
                calls,
            )
        }

        fn record(&self, call: String) {
            self.calls.lock().unwrap().push(call);
        }

        fn child(
            name: &str,
            crate_num: CrateNum,
            def_index: DefIndex,
            kind: SymExtKind,
        ) -> RawChild {
            RawChild {
                name: name.to_owned(),
                crate_num,
                def_index,
                namespace: Namespace::Type,
                kind,
            }
        }

        fn operator_traits_in(&self, module: &str) -> Vec<RawChild> {
            let mut children: Vec<_> = TRAIT_CASES
                .iter()
                .filter(|(_, candidate_module, _, _)| *candidate_module == module)
                .map(|(_, _, name, def_index)| {
                    Self::child(name, CORE, *def_index, SymExtKind::Trait)
                })
                .collect();
            if module != "ops" {
                return children;
            }
            match self.mode.trait_fault {
                TraitFault::None => {}
                TraitFault::Missing => {
                    children.retain(|child| child.name != "Add");
                }
                TraitFault::Duplicate => {
                    children.push(Self::child("Add", CORE, REEXPORTED_ADD, SymExtKind::Trait));
                }
                TraitFault::WrongKind => {
                    let add = children
                        .iter_mut()
                        .find(|child| child.name == "Add")
                        .expect("Add fixture");
                    add.kind = SymExtKind::Struct;
                }
                TraitFault::Reexport => {
                    let add = children
                        .iter_mut()
                        .find(|child| child.name == "Add")
                        .expect("Add fixture");
                    add.crate_num = REEXPORTED_OWNER;
                    add.def_index = REEXPORTED_ADD;
                }
            }
            children
        }

        fn trait_kind(&self, crate_num: CrateNum, def_index: DefIndex) -> Option<OperatorTrait> {
            if self.mode.trait_fault == TraitFault::Reexport
                && crate_num == REEXPORTED_OWNER
                && def_index == REEXPORTED_ADD
            {
                return Some(OperatorTrait::Add);
            }
            TRAIT_CASES.iter().find_map(|(kind, _, _, candidate)| {
                (crate_num == CORE && def_index == *candidate).then_some(*kind)
            })
        }

        fn associated_items_for(
            &self,
            owner_crate: CrateNum,
            trait_kind: OperatorTrait,
        ) -> RawAssociatedItems {
            let mut items: Vec<_> = METHOD_CASES
                .iter()
                .filter(|(_, owner, _, _)| *owner == trait_kind)
                .map(|(_, _, name, def_index)| RawAssociatedItem {
                    def: RawDefId {
                        crate_num: owner_crate,
                        def_index: *def_index,
                        kind: SymExtKind::Fn,
                    },
                    name: (*name).to_owned(),
                    kind: RawAssociatedItemKind::Function,
                })
                .collect();

            if trait_kind == OperatorTrait::Add {
                match self.mode.method_fault {
                    MethodFault::None
                    | MethodFault::MissingMetadata
                    | MethodFault::IncompleteMetadata
                    | MethodFault::MissingName => {}
                    MethodFault::Missing => items.clear(),
                    MethodFault::WrongKind => {
                        let add = items.first_mut().expect("Add method fixture");
                        add.def.kind = SymExtKind::Const;
                        add.kind = RawAssociatedItemKind::Const;
                    }
                    MethodFault::Duplicate => items.push(RawAssociatedItem {
                        def: RawDefId {
                            crate_num: owner_crate,
                            def_index: DefIndex(190),
                            kind: SymExtKind::Fn,
                        },
                        name: "add".to_owned(),
                        kind: RawAssociatedItemKind::Function,
                    }),
                }
            }

            RawAssociatedItems {
                items,
                complete: self.mode.method_fault != MethodFault::IncompleteMetadata,
            }
        }
    }

    impl TcxDb for OperatorTcx {
        fn extern_crate(&self, name: &str) -> Option<CrateNum> {
            self.record(format!("extern_crate({name})"));
            (name == "core").then_some(CORE)
        }

        fn module_children(&self, crate_num: CrateNum, def_index: DefIndex) -> Vec<RawChild> {
            self.record(format!("module_children({},{})", crate_num.0, def_index.0));
            match (crate_num, def_index) {
                (CORE, DefIndex(0)) => vec![
                    Self::child("ops", CORE, OPS_MODULE, SymExtKind::Mod),
                    Self::child("cmp", CORE, CMP_MODULE, SymExtKind::Mod),
                ],
                (CORE, OPS_MODULE) => self.operator_traits_in("ops"),
                (CORE, CMP_MODULE) => self.operator_traits_in("cmp"),
                (CrateNum(_), DefIndex(_)) => Vec::new(),
            }
        }

        fn item_name(&self, crate_num: CrateNum, def_index: DefIndex) -> Option<String> {
            self.record(format!("item_name({},{})", crate_num.0, def_index.0));
            if self.mode.method_fault == MethodFault::MissingName && def_index == DefIndex(100) {
                return None;
            }
            match (crate_num, def_index) {
                (CORE, OPS_MODULE) => return Some("ops".to_owned()),
                (CORE, CMP_MODULE) => return Some("cmp".to_owned()),
                (CORE, REEXPORTED_ADD) | (REEXPORTED_OWNER, REEXPORTED_ADD) => {
                    return Some("Add".to_owned());
                }
                (CORE, DefIndex(190)) => return Some("add".to_owned()),
                (CrateNum(_), DefIndex(_)) => {}
            }
            if let Some((_, _, name, _)) = TRAIT_CASES
                .iter()
                .find(|(_, _, _, candidate)| crate_num == CORE && def_index == *candidate)
            {
                return Some((*name).to_owned());
            }
            METHOD_CASES.iter().find_map(|(_, _, name, candidate)| {
                (def_index == *candidate).then(|| (*name).to_owned())
            })
        }

        fn is_module(&self, crate_num: CrateNum, def_index: DefIndex) -> bool {
            matches!(
                (crate_num, def_index),
                (CORE, DefIndex(0)) | (CORE, OPS_MODULE) | (CORE, CMP_MODULE)
            )
        }

        fn is_builtin_derive(&self, _crate_num: CrateNum, _def_index: DefIndex) -> bool {
            false
        }

        fn def_path(&self, _crate_num: CrateNum, _def_index: DefIndex) -> Option<String> {
            None
        }

        fn structured_def_path(
            &self,
            _crate_num: CrateNum,
            _def_index: DefIndex,
        ) -> Option<ExternalDefPath> {
            None
        }

        fn associated_items(
            &self,
            crate_num: CrateNum,
            def_index: DefIndex,
        ) -> Option<RawAssociatedItems> {
            self.record(format!("associated_items({},{})", crate_num.0, def_index.0));
            let trait_kind = self.trait_kind(crate_num, def_index)?;
            if trait_kind == OperatorTrait::Add
                && self.mode.method_fault == MethodFault::MissingMetadata
            {
                return None;
            }
            Some(self.associated_items_for(crate_num, trait_kind))
        }

        fn trait_signature(
            &self,
            _crate_num: CrateNum,
            _def_index: DefIndex,
        ) -> Option<RawTraitSignature> {
            panic!("operator identity lookup must not request a trait signature")
        }

        fn fn_signature(
            &self,
            _crate_num: CrateNum,
            _def_index: DefIndex,
        ) -> Option<RawFnSignature> {
            panic!("operator identity lookup must not request a function signature")
        }

        fn relevant_trait_impls(
            &self,
            _crate_num: CrateNum,
            _def_index: DefIndex,
            _self_head: Option<RawSelfTypeHead>,
        ) -> Option<RawRelevantImpls> {
            panic!("operator identity lookup must not request trait implementations")
        }

        fn expand_proc_macro_derive(
            &self,
            _crate_num: CrateNum,
            _def_index: DefIndex,
            _item_source: &str,
        ) -> Option<String> {
            None
        }

        fn expand_proc_macro_bang(
            &self,
            _crate_num: CrateNum,
            _def_index: DefIndex,
            _input_tokens: &str,
        ) -> Option<String> {
            None
        }

        fn expand_proc_macro_attr(
            &self,
            _crate_num: CrateNum,
            _def_index: DefIndex,
            _attr_args: &str,
            _item_source: &str,
        ) -> Option<String> {
            None
        }
    }

    #[salsa::tracked]
    fn test_crate<'db>(db: &'db dyn crate::Db, source_file: SourceFile) -> LocalCrateSymbol<'db> {
        let mut attrs = Stash::new();
        let empty = attrs.alloc_slice::<crate::cst::attrs::AttrCst>(&[]);
        let source = ParseSource::SourceFile(source_file);
        let root = LocalModSym::new(
            db,
            Name::new(db, String::new()),
            Edition::Rust2021,
            None,
            ModBodySource::File(source_file),
            Stashed::new(attrs, empty),
            AbsoluteSpan {
                source,
                start: 0,
                end: 0,
            },
        );
        local_crate(db, root)
    }

    fn with_fixture(
        mode: FixtureMode,
        test: impl for<'db> FnOnce(&'db dyn crate::Db, LocalCrateSymbol<'db>),
    ) {
        let (tcx, _) = OperatorTcx::new(mode);
        let mut database = Database::new(tcx);
        let source_file = database.add_source_file("lib.rs".to_owned(), String::new());
        database.attach(|db| test(db, test_crate(db, source_file)));
    }

    fn trait_def(operator: OperatorTrait) -> DefIndex {
        TRAIT_CASES
            .iter()
            .find_map(|(candidate, _, _, def_index)| (*candidate == operator).then_some(*def_index))
            .expect("operator trait fixture")
    }

    fn assert_trait_identity(
        db: &dyn crate::Db,
        actual: TraitSymbol<'_>,
        crate_num: CrateNum,
        def_index: DefIndex,
    ) {
        match actual {
            TraitSymbol::Ext(external) => {
                assert_eq!(external.crate_num(db), crate_num);
                assert_eq!(external.def_index(db), def_index);
                assert_eq!(external.kind(db), SymExtKind::Trait);
            }
            TraitSymbol::Local(_) => panic!("well-known operator trait must be external"),
        }
    }

    fn assert_function_identity(
        db: &dyn crate::Db,
        actual: FnSymbol<'_>,
        crate_num: CrateNum,
        def_index: DefIndex,
    ) {
        match actual {
            FnSymbol::Ext(external) => {
                assert_eq!(external.crate_num(db), crate_num);
                assert_eq!(external.def_index(db), def_index);
                assert_eq!(external.kind(db), SymExtKind::Fn);
            }
            FnSymbol::Local(_) => panic!("well-known operator method must be external"),
        }
    }

    fn take_calls(calls: &CallLog) -> Vec<String> {
        std::mem::take(&mut *calls.lock().unwrap())
    }

    #[test]
    fn all_operator_traits_and_methods_resolve_to_typed_symbols() {
        with_fixture(FixtureMode::default(), |db, local_crate| {
            for &(operator, _, _, def_index) in TRAIT_CASES {
                let symbol =
                    operator_trait(db, local_crate, operator).expect("well-known operator trait");
                assert_trait_identity(db, symbol, CORE, def_index);
            }

            for &(operator, owner, _, def_index) in METHOD_CASES {
                let symbols =
                    operator_method(db, local_crate, operator).expect("well-known operator method");
                assert_trait_identity(db, symbols.trait_symbol, CORE, trait_def(owner));
                assert_function_identity(db, symbols.function, CORE, def_index);
            }
        });
    }

    #[test]
    fn operator_lookup_follows_core_export_identity() {
        with_fixture(
            FixtureMode {
                trait_fault: TraitFault::Reexport,
                method_fault: MethodFault::None,
            },
            |db, local_crate| {
                let symbols = operator_method(db, local_crate, OperatorMethod::Add)
                    .expect("reexported Add::add");
                assert_trait_identity(db, symbols.trait_symbol, REEXPORTED_OWNER, REEXPORTED_ADD);
                assert_function_identity(db, symbols.function, REEXPORTED_OWNER, DefIndex(100));
            },
        );
    }

    #[test]
    fn operator_trait_lookup_rejects_missing_ambiguous_or_wrong_kind_paths() {
        for trait_fault in [
            TraitFault::Missing,
            TraitFault::Duplicate,
            TraitFault::WrongKind,
        ] {
            with_fixture(
                FixtureMode {
                    trait_fault,
                    method_fault: MethodFault::None,
                },
                |db, local_crate| {
                    assert_eq!(
                        operator_trait(db, local_crate, OperatorTrait::Add),
                        None,
                        "fault: {trait_fault:?}"
                    );
                },
            );
        }
    }

    #[test]
    fn operator_method_lookup_rejects_unavailable_or_wrong_items() {
        for method_fault in [
            MethodFault::MissingMetadata,
            MethodFault::IncompleteMetadata,
            MethodFault::Missing,
            MethodFault::MissingName,
            MethodFault::WrongKind,
            MethodFault::Duplicate,
        ] {
            with_fixture(
                FixtureMode {
                    trait_fault: TraitFault::None,
                    method_fault,
                },
                |db, local_crate| {
                    assert_eq!(
                        operator_method(db, local_crate, OperatorMethod::Add),
                        None,
                        "fault: {method_fault:?}"
                    );
                },
            );
        }
    }

    #[test]
    fn operator_identity_queries_are_layered_cached_and_narrow() {
        let (tcx, calls) = OperatorTcx::new(FixtureMode::default());
        let mut database = Database::new(tcx);
        let source_file = database.add_source_file("lib.rs".to_owned(), String::new());

        database.attach(|db| {
            let local_crate = test_crate(db, source_file);
            assert!(operator_trait(db, local_crate, OperatorTrait::Add).is_some());
        });
        let trait_calls = take_calls(&calls);
        let trait_queries = database.take_query_log();
        assert!(
            trait_calls.iter().any(|call| call == "extern_crate(core)")
                && trait_calls
                    .iter()
                    .any(|call| call == "module_children(1,1)")
        );
        assert!(trait_queries.contains("operator_trait"));

        database.attach(|db| {
            let local_crate = test_crate(db, source_file);
            assert!(operator_method(db, local_crate, OperatorMethod::Add).is_some());
        });
        let method_calls = take_calls(&calls);
        let method_queries = database.take_query_log();
        assert!(
            method_calls
                .iter()
                .any(|call| call == "associated_items(1,10)")
        );
        assert!(
            method_calls
                .iter()
                .all(|call| !call.starts_with("extern_crate")
                    && !call.starts_with("module_children")),
            "the method query should reuse its cached owner trait: {method_calls:?}"
        );
        assert!(method_queries.contains("operator_method"));
        assert!(!method_queries.contains("operator_trait"));

        database.attach(|db| {
            let local_crate = test_crate(db, source_file);
            assert!(operator_method(db, local_crate, OperatorMethod::Add).is_some());
        });
        assert_eq!(take_calls(&calls), Vec::<String>::new());
        assert_eq!(database.take_query_log(), "");
    }
}
