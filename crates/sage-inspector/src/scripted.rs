use std::collections::HashMap;

use sage_reflect::{
    Badge, BadgeTone, ScalarValue, SymbolPresentation, SymbolReference, ValueField, ValueNode,
};

use crate::actor::{InspectionProvider, Provided};
use crate::protocol::*;

pub struct ScriptedProvider {
    revision: RevisionId,
    runs: HashMap<RunHandle, RunObservation>,
}

impl Default for ScriptedProvider {
    fn default() -> Self {
        let mut runs = HashMap::new();
        runs.insert("run_1".to_owned(), symbol_index_run());
        runs.insert("run_2".to_owned(), signature_run());
        runs.insert("run_3".to_owned(), body_run());
        for (id, target, operation) in [
            ("run_4", LOCAL_DB_METHOD, "source"),
            ("run_5", LOCAL_DB_METHOD, "identity"),
            ("run_6", LOCAL_DB_METHOD, "concrete-ir"),
            ("run_7", EXTERNAL_CLONE_METHOD, "signature"),
        ] {
            runs.insert(id.to_owned(), generic_run(id, target, operation));
        }
        Self {
            revision: "rev_0".to_owned(),
            runs,
        }
    }
}

impl InspectionProvider for ScriptedProvider {
    fn current_revision(&self) -> RevisionId {
        self.revision.clone()
    }

    fn revision(&mut self) -> Result<Provided<()>, ApiError> {
        Ok(self.provided(()))
    }

    fn session(&mut self) -> Result<Provided<Session>, ApiError> {
        Ok(self.provided(Session {
            protocol_version: "1".to_owned(),
            target: CargoTarget {
                package: "db-drop-guard".to_owned(),
                target_kind: TargetKind::Lib,
                target_name: "db_drop_guard".to_owned(),
            },
            workspace_root: "source".to_owned(),
            capabilities: vec![
                Capability::SymbolIndex,
                Capability::Products,
                Capability::Runs,
                Capability::Events,
                Capability::Revisions,
                Capability::RevisionComparison,
            ],
            retained_revisions: RetainedRevisionRange {
                first: Some(self.revision.clone()),
                last: Some(self.revision.clone()),
            },
        }))
    }

    fn symbols(&mut self) -> Result<Provided<SymbolIndex>, ApiError> {
        let mut provided = self.provided(symbol_index());
        provided.run_id = Some("run_1".to_owned());
        Ok(provided)
    }

    fn symbol(&mut self, path: &str) -> Result<Provided<SelectedSymbol>, ApiError> {
        let symbol = match path {
            LOCAL_DB_METHOD => local_db_method(),
            EXTERNAL_CLONE_METHOD => external_clone_method(),
            "local/db-drop-guard" => selected_crate(),
            "local/db-drop-guard/Db" => selected_struct("Db"),
            "local/db-drop-guard/DbDropGuard" => selected_struct("DbDropGuard"),
            "local/db-drop-guard/impl-DbDropGuard" => selected_impl(),
            _ => {
                return Err(ApiError::new(
                    "symbol-not-found",
                    format!("unknown symbol path `{path}`"),
                ));
            }
        };
        Ok(self.provided(symbol))
    }

    fn product(&mut self, symbol: &str, product: &str) -> Result<Provided<ProductPage>, ApiError> {
        let (page, run_id) = match (symbol, product) {
            (LOCAL_DB_METHOD, "identity") => (identity_page(symbol), Some("run_5")),
            (LOCAL_DB_METHOD, "source") => (source_page(), Some("run_4")),
            (LOCAL_DB_METHOD, "concrete-ir") => (concrete_page(), Some("run_6")),
            (LOCAL_DB_METHOD, "signature") => (signature_page(), Some("run_2")),
            (LOCAL_DB_METHOD, "typed-ir") => (body_page(), Some("run_3")),
            (LOCAL_DB_METHOD, "diagnostics") => (diagnostics_page(), Some("run_3")),
            (LOCAL_DB_METHOD, "invented-review-card") => (review_card_page(), None),
            (EXTERNAL_CLONE_METHOD, "identity") => (identity_page(symbol), None),
            (EXTERNAL_CLONE_METHOD, "signature") => (external_signature_page(), Some("run_7")),
            _ => {
                return Err(ApiError::new(
                    "product-not-found",
                    format!("product `{product}` is not listed for `{symbol}`"),
                ));
            }
        };
        let mut provided = self.provided(page);
        provided.run_id = run_id.map(str::to_owned);
        Ok(provided)
    }

    fn continuation(&mut self, handle: &str) -> Result<Provided<ContinuationValue>, ApiError> {
        if handle == "fixture-continuation" {
            return Ok(self.provided(ContinuationValue {
                continuation: handle.to_owned(),
                items: vec![ValueNode::record(
                    "ContinuedValue",
                    vec![ValueField::new(
                        "value",
                        ValueNode::scalar("String", "loaded on demand"),
                    )],
                )],
                next: None,
            }));
        }
        Err(ApiError::new(
            "continuation-not-found",
            format!("unknown continuation `{handle}`"),
        ))
    }

    fn run(&mut self, handle: &str) -> Result<Provided<RunObservation>, ApiError> {
        let run = self
            .runs
            .get(handle)
            .cloned()
            .ok_or_else(|| ApiError::new("run-not-found", format!("unknown run `{handle}`")))?;
        Ok(self.provided(run))
    }

    fn revisions(&mut self, _cursor: Option<&str>) -> Result<Provided<RevisionPage>, ApiError> {
        Ok(self.provided(RevisionPage {
            revisions: vec![RevisionSummary {
                revision_id: self.revision.clone(),
                cause: RevisionCause::Initial,
                input_delta_count: 0,
                run_count: self.runs.len() as u32,
            }],
            next_cursor: None,
        }))
    }

    fn revision_detail(&mut self, revision: &str) -> Result<Provided<RevisionDetail>, ApiError> {
        if revision != self.revision {
            return Err(ApiError::new(
                "revision-not-found",
                format!("unknown revision `{revision}`"),
            ));
        }
        let mut runs: Vec<_> = self.runs.keys().cloned().collect();
        runs.sort();
        Ok(self.provided(RevisionDetail {
            summary: RevisionSummary {
                revision_id: self.revision.clone(),
                cause: RevisionCause::Initial,
                input_delta_count: 0,
                run_count: self.runs.len() as u32,
            },
            input_deltas: vec![],
            runs,
        }))
    }

    fn compare(
        &mut self,
        from: &str,
        to: &str,
        symbol: &str,
        product: &str,
    ) -> Result<Provided<RunComparison>, ApiError> {
        if from != self.revision || to != self.revision {
            return Err(ApiError::new(
                "revision-not-found",
                "the scripted provider retains only rev_0",
            ));
        }
        Ok(self.provided(RunComparison {
            from_revision: from.to_owned(),
            to_revision: to.to_owned(),
            symbol: symbol.to_owned(),
            product: product.to_owned(),
            value_changed: false,
            executed_only_before: vec![],
            executed_only_after: vec![],
            reused_only_before: vec![],
            reused_only_after: vec![],
        }))
    }

    fn apply_updates(
        &mut self,
        _updates: Vec<crate::actor::FileUpdate>,
    ) -> Result<Provided<RevisionEvent>, ApiError> {
        Err(ApiError::new(
            "fixture-is-read-only",
            "the scripted fixture does not accept file updates",
        ))
    }

    fn reload_workspace(&mut self, _reason: Issue) -> Result<Provided<RevisionEvent>, ApiError> {
        Err(ApiError::new(
            "fixture-is-read-only",
            "the scripted fixture cannot reload its workspace",
        ))
    }
}

impl ScriptedProvider {
    fn provided<T>(&self, value: T) -> Provided<T> {
        Provided::without_run(self.revision.clone(), value)
    }
}

const LOCAL_DB_METHOD: &str = "local/db-drop-guard/impl-DbDropGuard/db";
const EXTERNAL_CLONE_METHOD: &str = "external/core-1/clone/Clone/clone";

fn presentation(eyebrow: &str) -> SymbolPresentation {
    SymbolPresentation {
        eyebrow: Some(eyebrow.to_owned()),
        badges: vec![],
    }
}

fn external_presentation(eyebrow: &str) -> SymbolPresentation {
    SymbolPresentation {
        eyebrow: Some(eyebrow.to_owned()),
        badges: vec![Badge {
            label: "External symbol".to_owned(),
            tone: BadgeTone::Accent,
        }],
    }
}

fn symbol_index() -> SymbolIndex {
    let root = "local/db-drop-guard";
    let summaries = [
        (
            root,
            None,
            "db_drop_guard",
            "db_drop_guard",
            "db_drop_guard crate",
            "Local crate",
            ChildCompleteness::Complete,
        ),
        (
            "local/db-drop-guard/Db",
            Some(root),
            "Db",
            "db_drop_guard::Db",
            "db db_drop_guard::Db struct",
            "Local struct",
            ChildCompleteness::NotApplicable,
        ),
        (
            "local/db-drop-guard/DbDropGuard",
            Some(root),
            "DbDropGuard",
            "db_drop_guard::DbDropGuard",
            "dbdropguard db_drop_guard::DbDropGuard struct",
            "Local struct",
            ChildCompleteness::Complete,
        ),
        (
            "local/db-drop-guard/impl-DbDropGuard",
            Some("local/db-drop-guard/DbDropGuard"),
            "impl DbDropGuard",
            "impl db_drop_guard::DbDropGuard",
            "impl DbDropGuard",
            "Local impl",
            ChildCompleteness::Complete,
        ),
        (
            LOCAL_DB_METHOD,
            Some("local/db-drop-guard/impl-DbDropGuard"),
            "db",
            "db_drop_guard::DbDropGuard::db",
            "db dbdropguard method function",
            "Local associated function",
            ChildCompleteness::NotApplicable,
        ),
    ];
    SymbolIndex {
        root: root.to_owned(),
        symbols: summaries
            .into_iter()
            .map(
                |(path, parent, label, display_path, search_text, eyebrow, children)| {
                    let mut symbol_presentation = presentation(eyebrow);
                    if path == LOCAL_DB_METHOD {
                        symbol_presentation.badges.push(Badge {
                            label: "Source-written".to_owned(),
                            tone: BadgeTone::Neutral,
                        });
                    }
                    SymbolSummary {
                        path: path.to_owned(),
                        parent: parent.map(str::to_owned),
                        label: label.to_owned(),
                        display_path: display_path.to_owned(),
                        search_text: search_text.to_owned(),
                        presentation: symbol_presentation,
                        children,
                    }
                },
            )
            .collect(),
    }
}

fn local_db_method() -> SelectedSymbol {
    SelectedSymbol {
        path: LOCAL_DB_METHOD.to_owned(),
        label: "db".to_owned(),
        display_path: "db_drop_guard::DbDropGuard::db".to_owned(),
        presentation: SymbolPresentation {
            eyebrow: Some("Local associated function".to_owned()),
            badges: vec![Badge {
                label: "Local symbol".to_owned(),
                tone: BadgeTone::Success,
            }],
        },
        parent: Some(SymbolReference {
            path: "local/db-drop-guard/impl-DbDropGuard".to_owned(),
            label: "impl DbDropGuard".to_owned(),
            presentation: presentation("Local impl"),
        }),
        products: [
            ("identity", "Identity"),
            ("source", "Source"),
            ("concrete-ir", "Concrete IR"),
            ("signature", "Signature"),
            ("typed-ir", "Typed body"),
            ("diagnostics", "Diagnostics"),
            ("invented-review-card", "Review card"),
        ]
        .into_iter()
        .map(|(id, label)| descriptor(LOCAL_DB_METHOD, id, label))
        .collect(),
    }
}

fn external_clone_method() -> SelectedSymbol {
    SelectedSymbol {
        path: EXTERNAL_CLONE_METHOD.to_owned(),
        label: "clone".to_owned(),
        display_path: "core::clone::Clone::clone".to_owned(),
        presentation: external_presentation("External associated function"),
        parent: Some(SymbolReference {
            path: "external/core-1/clone/Clone".to_owned(),
            label: "core::clone::Clone".to_owned(),
            presentation: presentation("External trait"),
        }),
        products: [("identity", "Identity"), ("signature", "Signature")]
            .into_iter()
            .map(|(id, label)| descriptor(EXTERNAL_CLONE_METHOD, id, label))
            .collect(),
    }
}

fn selected_crate() -> SelectedSymbol {
    basic_selected("local/db-drop-guard", "db_drop_guard", "Local crate")
}
fn selected_struct(name: &str) -> SelectedSymbol {
    basic_selected(&format!("local/db-drop-guard/{name}"), name, "Local struct")
}
fn selected_impl() -> SelectedSymbol {
    basic_selected(
        "local/db-drop-guard/impl-DbDropGuard",
        "impl DbDropGuard",
        "Local impl",
    )
}

fn basic_selected(path: &str, label: &str, eyebrow: &str) -> SelectedSymbol {
    SelectedSymbol {
        path: path.to_owned(),
        label: label.to_owned(),
        display_path: label.to_owned(),
        presentation: presentation(eyebrow),
        parent: None,
        products: vec![descriptor(path, "identity", "Identity")],
    }
}

fn descriptor(symbol: &str, id: &str, label: &str) -> ProductDescriptor {
    ProductDescriptor {
        id: id.to_owned(),
        label: label.to_owned(),
        href: format!(
            "/api/v1/product?symbol={}&product={id}",
            percent_encode(symbol)
        ),
    }
}

fn percent_encode(value: &str) -> String {
    value.replace('/', "%2F")
}

fn identity_page(symbol: &str) -> ProductPage {
    ProductPage {
        id: "identity".to_owned(),
        title: "Symbol identity".to_owned(),
        content: RenderNode::Text {
            text: symbol.to_owned(),
        },
    }
}

fn source_page() -> ProductPage {
    ProductPage {
        id: "source".to_owned(),
        title: "Source".to_owned(),
        content: RenderNode::Code {
            language: "rust".to_owned(),
            text: "fn db(&self) -> Db {\n    self.db.clone()\n}".to_owned(),
            highlights: vec![],
        },
    }
}

fn concrete_page() -> ProductPage {
    ProductPage {
        id: "concrete-ir".to_owned(),
        title: "Expanded concrete function".to_owned(),
        content: RenderNode::Value {
            value: ValueNode::record(
                "FnCstData",
                vec![
                    ValueField::new("name", ValueNode::scalar("Name", "db")),
                    ValueField::new(
                        "params",
                        ValueNode::Sequence {
                            type_name: "Slice<ParamCst>".to_owned(),
                            items: vec![ValueNode::record(
                                "ParamCst",
                                vec![ValueField::new(
                                    "receiver",
                                    ValueNode::variant(
                                        "ReceiverCst",
                                        "Ref",
                                        vec![ValueField::new(
                                            "mutability",
                                            ValueNode::scalar("Mutability", "Shared"),
                                        )],
                                    ),
                                )],
                            )],
                        },
                    ),
                    ValueField::new(
                        "body",
                        ValueNode::variant(
                            "ExprCstKind",
                            "MethodCall",
                            vec![ValueField::new(
                                "method",
                                ValueNode::scalar("Name", "clone"),
                            )],
                        ),
                    ),
                ],
            ),
        },
    }
}

fn signature_page() -> ProductPage {
    let local_ref = |path: &str, label: &str| ValueNode::Reference {
        target: SymbolReference {
            path: path.to_owned(),
            label: label.to_owned(),
            presentation: presentation("Local struct"),
        },
    };
    let local_ty = |path: &str, label: &str| {
        ValueNode::variant(
            "Ty",
            "Adt",
            vec![
                ValueField::new("symbol", local_ref(path, label)),
                ValueField::new(
                    "arguments",
                    ValueNode::Sequence {
                        type_name: "Slice<Ptr<Ty>>".to_owned(),
                        items: vec![],
                    },
                ),
            ],
        )
    };
    let fn_sig = ValueNode::record(
        "FnSig",
        vec![
            ValueField::new("owner_generic_count", ValueNode::scalar("u32", 0_u32)),
            ValueField::new(
                "owner_self_ty",
                ValueNode::variant(
                    "Option",
                    "Some",
                    vec![ValueField::new(
                        "0",
                        ValueNode::Shared {
                            identity: "ty_self".to_owned(),
                            value: Box::new(local_ty(
                                "local/db-drop-guard/DbDropGuard",
                                "db_drop_guard::DbDropGuard",
                            )),
                        },
                    )],
                ),
            ),
            ValueField::new(
                "receiver",
                ValueNode::variant(
                    "Option",
                    "Some",
                    vec![ValueField::new(
                        "0",
                        ValueNode::record(
                            "CheckedReceiver",
                            vec![
                                ValueField::new(
                                    "owner_self_ty",
                                    ValueNode::SharedReference {
                                        identity: "ty_self".to_owned(),
                                    },
                                ),
                                ValueField::new(
                                    "form",
                                    ValueNode::variant(
                                        "MethodReceiver",
                                        "Ref",
                                        vec![ValueField::new(
                                            "mutability",
                                            ValueNode::scalar("Mutability", "Shared"),
                                        )],
                                    ),
                                ),
                            ],
                        ),
                    )],
                ),
            ),
            ValueField::new(
                "params",
                ValueNode::Sequence {
                    type_name: "Slice<Ptr<Ty>>".to_owned(),
                    items: vec![],
                },
            ),
            ValueField::new(
                "ret",
                local_ty("local/db-drop-guard/Db", "db_drop_guard::Db"),
            ),
            ValueField::new(
                "parameter_env",
                ValueNode::record(
                    "CheckedParameterEnv",
                    vec![
                        ValueField::new(
                            "where_clauses",
                            ValueNode::Sequence {
                                type_name: "Slice<WherePredicate>".to_owned(),
                                items: vec![],
                            },
                        ),
                        ValueField::new(
                            "solver_eligibility",
                            ValueNode::scalar("SolverEligibility", "Eligible"),
                        ),
                    ],
                ),
            ),
            ValueField::new(
                "method_candidate_eligibility",
                ValueNode::scalar("SolverEligibility", "Eligible"),
            ),
            ValueField::new("const_call_complete", ValueNode::scalar("bool", false)),
        ],
    );
    ProductPage {
        id: "signature".to_owned(),
        title: "Checked function signature".to_owned(),
        content: RenderNode::Group {
            layout: GroupLayout::Block,
            children: vec![
                RenderNode::Text {
                    text: "Every Binder and FnSig field is reflected recursively.".to_owned(),
                },
                RenderNode::Value {
                    value: ValueNode::record(
                        "Binder<FnSig>",
                        vec![
                            ValueField::new("value", fn_sig),
                            ValueField::new(
                                "generics",
                                ValueNode::Sequence {
                                    type_name: "Slice<GenericParam>".to_owned(),
                                    items: vec![],
                                },
                            ),
                        ],
                    ),
                },
            ],
        },
    }
}

fn body_page() -> ProductPage {
    let target = SymbolReference {
        path: EXTERNAL_CLONE_METHOD.to_owned(),
        label: "core::clone::Clone::clone".to_owned(),
        presentation: external_presentation("External associated function"),
    };
    ProductPage { id: "typed-ir".to_owned(), title: "Completed typed body".to_owned(), content: RenderNode::Group { layout: GroupLayout::Block, children: vec![
        RenderNode::Notice { tone: NoticeTone::Info, title: Some("Fully elaborated".to_owned()), text: "Method syntax has been consumed; dispatch and receiver adjustments are explicit.".to_owned() },
        RenderNode::Value { value: ValueNode::record("CheckedBody", vec![
            ValueField::new("body", ValueNode::record("TyBodyData", vec![
                ValueField::new("root", ValueNode::record("TyExpr", vec![
                    ValueField::new("data", ValueNode::variant("TyExprData", "ResolvedCall", vec![
                        ValueField::new("function", ValueNode::Reference { target }),
                        ValueField::new("dispatch", ValueNode::scalar("CallDispatch", "StaticTrait")),
                    ])),
                    ValueField::new("ty", ValueNode::variant("Ty", "Adt", vec![
                        ValueField::new("symbol", ValueNode::Reference { target: SymbolReference { path: "local/db-drop-guard/Db".to_owned(), label: "db_drop_guard::Db".to_owned(), presentation: presentation("Local struct") } }),
                        ValueField::new("arguments", ValueNode::Sequence { type_name: "Slice<Ptr<Ty>>".to_owned(), items: vec![] }),
                    ])),
                    ValueField::new("span", ValueNode::record("RelativeSpan", vec![
                        ValueField::new("start", ValueNode::scalar("u32", 11_u32)),
                        ValueField::new("end", ValueNode::scalar("u32", 26_u32)),
                    ])),
                ])),
            ])),
            ValueField::new("diagnostics", ValueNode::Sequence { type_name: "Vec<Diagnostic>".to_owned(), items: vec![] }),
        ]) },
    ]} }
}

fn diagnostics_page() -> ProductPage {
    ProductPage {
        id: "diagnostics".to_owned(),
        title: "Checking diagnostics".to_owned(),
        content: RenderNode::Notice {
            tone: NoticeTone::Neutral,
            title: Some("No diagnostics".to_owned()),
            text: "Body checking completed without diagnostics.".to_owned(),
        },
    }
}
fn review_card_page() -> ProductPage {
    ProductPage {
        id: "invented-review-card".to_owned(),
        title: "Review card".to_owned(),
        content: RenderNode::Notice {
            tone: NoticeTone::Info,
            title: Some("Protocol-driven page".to_owned()),
            text: "This invented product renders without a product-specific frontend case."
                .to_owned(),
        },
    }
}
fn external_signature_page() -> ProductPage {
    ProductPage {
        id: "signature".to_owned(),
        title: "External checked signature".to_owned(),
        content: RenderNode::Value {
            value: ValueNode::record(
                "Binder<FnSig>",
                vec![
                    ValueField::new(
                        "receiver",
                        ValueNode::record(
                            "CheckedReceiver",
                            vec![ValueField::new(
                                "form",
                                ValueNode::scalar("MethodReceiver", "Ref(Shared)"),
                            )],
                        ),
                    ),
                    ValueField::new(
                        "ret",
                        ValueNode::Scalar {
                            type_name: "Ty".to_owned(),
                            value: ScalarValue::String("Self".to_owned()),
                        },
                    ),
                ],
            ),
        },
    }
}

fn trace(
    operation: &str,
    key: &str,
    source: TraceSource,
    disposition: TraceDisposition,
    children: Vec<TraceNode>,
) -> TraceNode {
    TraceNode {
        phase: TracePhase::Analysis,
        source,
        operation: operation.to_owned(),
        key: TraceKey::Semantic {
            value: key.to_owned(),
        },
        disposition,
        child_order: ChildOrder::Sequential,
        observations: 1,
        children,
    }
}
fn symbol_index_run() -> RunObservation {
    RunObservation {
        run_id: "run_1".to_owned(),
        request: RunRequest::SymbolIndex,
        root: trace(
            "local-symbol-index",
            "local/db-drop-guard",
            TraceSource::Sage,
            TraceDisposition::Observed,
            vec![],
        ),
    }
}
fn signature_run() -> RunObservation {
    RunObservation {
        run_id: "run_2".to_owned(),
        request: RunRequest::Product {
            target: LOCAL_DB_METHOD.to_owned(),
            product: "signature".to_owned(),
        },
        root: trace(
            "product",
            "signature",
            TraceSource::Sage,
            TraceDisposition::Observed,
            vec![
                trace(
                    "LocalFnSym::sig",
                    LOCAL_DB_METHOD,
                    TraceSource::Salsa,
                    TraceDisposition::Executed,
                    vec![],
                ),
                TraceNode {
                    phase: TracePhase::Reflection,
                    source: TraceSource::Sage,
                    operation: "reflect Binder<FnSig>".to_owned(),
                    key: TraceKey::Semantic {
                        value: LOCAL_DB_METHOD.to_owned(),
                    },
                    disposition: TraceDisposition::Observed,
                    child_order: ChildOrder::Sequential,
                    observations: 1,
                    children: vec![],
                },
            ],
        ),
    }
}
fn body_run() -> RunObservation {
    RunObservation {
        run_id: "run_3".to_owned(),
        request: RunRequest::Product {
            target: LOCAL_DB_METHOD.to_owned(),
            product: "typed-ir".to_owned(),
        },
        root: trace(
            "product",
            "typed-ir",
            TraceSource::Sage,
            TraceDisposition::Observed,
            vec![
                trace(
                    "LocalFnSym::body",
                    LOCAL_DB_METHOD,
                    TraceSource::Salsa,
                    TraceDisposition::Executed,
                    vec![
                        trace(
                            "LocalFnSym::sig",
                            LOCAL_DB_METHOD,
                            TraceSource::Salsa,
                            TraceDisposition::Reused,
                            vec![],
                        ),
                        TraceNode {
                            phase: TracePhase::Analysis,
                            source: TraceSource::Solver,
                            operation: "Prove".to_owned(),
                            key: TraceKey::Semantic {
                                value: "Db: Clone".to_owned(),
                            },
                            disposition: TraceDisposition::Observed,
                            child_order: ChildOrder::Unordered,
                            observations: 1,
                            children: vec![],
                        },
                    ],
                ),
                TraceNode {
                    phase: TracePhase::Reflection,
                    source: TraceSource::Sage,
                    operation: "reflect CheckedBody".to_owned(),
                    key: TraceKey::Semantic {
                        value: LOCAL_DB_METHOD.to_owned(),
                    },
                    disposition: TraceDisposition::Observed,
                    child_order: ChildOrder::Sequential,
                    observations: 1,
                    children: vec![],
                },
            ],
        ),
    }
}
fn generic_run(id: &str, target: &str, operation: &str) -> RunObservation {
    RunObservation {
        run_id: id.to_owned(),
        request: RunRequest::Product {
            target: target.to_owned(),
            product: operation.to_owned(),
        },
        root: trace(
            "product",
            operation,
            TraceSource::Sage,
            TraceDisposition::Observed,
            vec![],
        ),
    }
}
