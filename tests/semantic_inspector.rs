#![feature(rustc_private)]

use sage::driver::{SageHost, run_sage_host_with};
use sage::inspector::LiveInspectionProvider;
use sage_inspector::{
    FileUpdate, InspectionProvider, RenderNode, RevisionEvent, TraceDisposition, TraceNode,
    TraceSource,
};
use sage_reflect::{SymbolReference, ValueNode};

struct ChildGuard(std::process::Child);

struct TestHttpResponse {
    status: u16,
    headers: std::collections::BTreeMap<String, String>,
    body: String,
}

fn http_get(address: &str, path: &str, accept_json: bool) -> TestHttpResponse {
    use std::io::{Read as _, Write as _};

    let mut stream = std::net::TcpStream::connect(address).unwrap();
    let accept = if accept_json {
        "Accept: application/json\r\n"
    } else {
        ""
    };
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\n{accept}Connection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (head, body) = response
        .split_once("\r\n\r\n")
        .expect("HTTP response has a body separator");
    let mut lines = head.lines();
    let status = lines
        .next()
        .expect("HTTP response has a status line")
        .split_whitespace()
        .nth(1)
        .expect("HTTP status line has a status code")
        .parse()
        .unwrap();
    let headers = lines
        .map(|line| {
            let (name, value) = line
                .split_once(':')
                .expect("HTTP response header has a colon");
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    TestHttpResponse {
        status,
        headers,
        body: body.to_owned(),
    }
}

fn assert_json_response(response: &TestHttpResponse, status: u16) {
    assert_eq!(response.status, status);
    assert_eq!(
        response.headers.get("content-type").map(String::as_str),
        Some("application/json; charset=utf-8")
    );
    assert_eq!(
        response.headers.get("cache-control").map(String::as_str),
        Some("no-store")
    );
    assert!(response.body.ends_with('\n'));
}

fn protocol_response_snapshot(label: &str, response: &TestHttpResponse, output: &mut String) {
    use std::fmt::Write as _;

    writeln!(output, "## {label}").unwrap();
    writeln!(output, "status: {}", response.status).unwrap();
    writeln!(output, "content-type: {}", response.headers["content-type"]).unwrap();
    writeln!(
        output,
        "cache-control: {}",
        response.headers["cache-control"]
    )
    .unwrap();
    output.push_str(&response.body);
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct TempProject(std::path::PathBuf);

fn inspector_project() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test-projects/semantic-inspector/db-drop-guard")
}

fn percent_encode(value: &str) -> String {
    use std::fmt::Write as _;

    value.bytes().fold(String::new(), |mut encoded, byte| {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            write!(encoded, "%{byte:02X}").unwrap();
        }
        encoded
    })
}

impl TempProject {
    fn new(label: &str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "sage-semantic-inspector-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[salsa::tracked]
fn panicking_inspection_query(_db: &dyn sage_ir::Db) -> u32 {
    panic!("intentional tracing test panic")
}

#[salsa::tracked]
fn repeated_leaf_query(_db: &dyn sage_ir::Db) -> u32 {
    22
}

fn external_metadata_operations<'a>(node: &'a TraceNode, output: &mut Vec<&'a str>) {
    if node.source == TraceSource::ExternalMetadata {
        output.push(match &node.key {
            sage_inspector::TraceKey::Semantic { value } => value,
            sage_inspector::TraceKey::Unmapped { ingredient } => ingredient,
        });
    }
    for child in &node.children {
        external_metadata_operations(child, output);
    }
}

fn first_external_reference(value: &ValueNode) -> Option<&SymbolReference> {
    match value {
        ValueNode::Reference { target } if target.path.starts_with("external/") => Some(target),
        ValueNode::Record { fields, .. } | ValueNode::Variant { fields, .. } => fields
            .iter()
            .find_map(|field| first_external_reference(&field.value)),
        ValueNode::Sequence { items, .. } => items.iter().find_map(first_external_reference),
        ValueNode::Shared { value, .. } => first_external_reference(value),
        ValueNode::Scalar { .. }
        | ValueNode::Reference { .. }
        | ValueNode::SharedReference { .. }
        | ValueNode::Truncated { .. } => None,
    }
}

fn contains_reference_label(value: &ValueNode, label: &str) -> bool {
    match value {
        ValueNode::Reference { target } => target.label == label,
        ValueNode::Record { fields, .. } | ValueNode::Variant { fields, .. } => fields
            .iter()
            .any(|field| contains_reference_label(&field.value, label)),
        ValueNode::Sequence { items, .. } => items
            .iter()
            .any(|item| contains_reference_label(item, label)),
        ValueNode::Shared { value, .. } => contains_reference_label(value, label),
        ValueNode::Scalar { .. }
        | ValueNode::SharedReference { .. }
        | ValueNode::Truncated { .. } => false,
    }
}

fn trace_nodes<'a>(node: &'a TraceNode, output: &mut Vec<&'a TraceNode>) {
    output.push(node);
    for child in &node.children {
        trace_nodes(child, output);
    }
}

fn trace_operation_families(node: &TraceNode, output: &mut std::collections::BTreeSet<String>) {
    output.insert(format!("{:?}:{}", node.source, node.operation));
    for child in &node.children {
        trace_operation_families(child, output);
    }
}

fn has_phase(node: &TraceNode, phase: sage_inspector::TracePhase) -> bool {
    node.phase == phase || node.children.iter().any(|child| has_phase(child, phase))
}

fn phase_nodes<'a>(
    node: &'a TraceNode,
    phase: sage_inspector::TracePhase,
    output: &mut Vec<&'a TraceNode>,
) {
    if node.phase == phase {
        output.push(node);
    }
    for child in &node.children {
        phase_nodes(child, phase, output);
    }
}

fn value_shape(node: &ValueNode) -> String {
    fn write(node: &ValueNode, indent: usize, output: &mut String) {
        let padding = "  ".repeat(indent);
        match node {
            ValueNode::Record { type_name, fields } => {
                output.push_str(&format!("{padding}record {type_name}\n"));
                for field in fields {
                    output.push_str(&format!("{padding}  .{}\n", field.name));
                    write(&field.value, indent + 2, output);
                }
            }
            ValueNode::Variant {
                enum_name,
                variant_name,
                fields,
            } => {
                output.push_str(&format!("{padding}variant {enum_name}::{variant_name}\n"));
                for field in fields {
                    output.push_str(&format!("{padding}  .{}\n", field.name));
                    write(&field.value, indent + 2, output);
                }
            }
            ValueNode::Sequence { type_name, items } => {
                output.push_str(&format!(
                    "{padding}sequence {type_name} [{}]\n",
                    items.len()
                ));
                for item in items {
                    write(item, indent + 1, output);
                }
            }
            ValueNode::Scalar { type_name, .. } => {
                output.push_str(&format!("{padding}scalar {type_name}\n"));
            }
            ValueNode::Reference { target } => {
                output.push_str(&format!("{padding}reference {}\n", target.path));
            }
            ValueNode::Shared { value, .. } => {
                output.push_str(&format!("{padding}shared\n"));
                write(value, indent + 1, output);
            }
            ValueNode::SharedReference { .. } => {
                output.push_str(&format!("{padding}shared-reference\n"));
            }
            ValueNode::Truncated { summary, .. } => {
                output.push_str(&format!("{padding}truncated {summary}\n"));
            }
        }
    }

    let mut output = String::new();
    write(node, 0, &mut output);
    output
}

#[test]
fn real_symbol_index_is_complete_local_and_detail_free() {
    let project = inspector_project();
    run_sage_host_with(&project, &[], |host| {
        let mut provider = LiveInspectionProvider::new(host);
        let provided = provider.symbols().unwrap();
        let labels: Vec<_> = provided
            .value
            .symbols
            .iter()
            .map(|symbol| symbol.label.as_str())
            .collect();
        assert!(labels.contains(&"Db"));
        assert!(labels.contains(&"DbDropGuard"));
        assert!(labels.contains(&"impl item"));
        assert!(labels.contains(&"db"));
        let distinct_paths: std::collections::HashSet<_> = provided
            .value
            .symbols
            .iter()
            .map(|symbol| symbol.path.as_str())
            .collect();
        assert_eq!(distinct_paths.len(), provided.value.symbols.len());
        assert!(provided.value.symbols.iter().any(|symbol| {
            symbol
                .presentation
                .badges
                .iter()
                .any(|badge| badge.label == "Generated")
        }));
        assert!(
            provided
                .value
                .symbols
                .iter()
                .all(|symbol| !symbol.path.starts_with("external/"))
        );

        let run = provider.run(provided.run_id.as_deref().unwrap()).unwrap();
        let trace = format!("{:#?}", run.value);
        let mut operation_families = std::collections::BTreeSet::new();
        trace_operation_families(&run.value.root, &mut operation_families);
        expect_test::expect![[r#"
            {
                "ExternalMetadata:tcx::extern_crate",
                "ExternalMetadata:tcx::is_builtin_derive",
                "ExternalMetadata:tcx::item_name",
                "ExternalMetadata:tcx::module_children",
                "Sage:local-symbol-index",
                "Salsa:DeriveExpansion < 'db >::parse_output_",
                "Salsa:SymExt < 'db >::expanded_module_items_",
                "Salsa:SymExt < 'db >::name_",
                "Salsa:SymExt < 'db >::named_children_",
                "Salsa:builtin_derive_output",
                "Salsa:local_crate_with_edition",
                "Salsa:local_expanded_module_items",
                "Salsa:local_impl_associated_item",
                "Salsa:local_impl_associated_items",
                "Salsa:module_expansion_complete_for_symbol_listing",
                "Salsa:setup_root_module",
                "Salsa:unexpanded_items",
            }
        "#]]
        .assert_debug_eq(&operation_families);
        assert!(!trace.contains("LocalFnSym::sig"));
        assert!(!trace.contains("LocalFnSym::body"));
        assert!(!trace.contains("associated_type_value"));
        assert!(!trace.contains("relevant_trait_impls"));
        let mut external_operations = Vec::new();
        external_metadata_operations(&run.value.root, &mut external_operations);
        assert!(!external_operations.is_empty());
        assert!(
            external_operations.iter().all(|operation| {
                operation.starts_with("tcx::extern_crate(")
                    || operation.starts_with("tcx::module_children(")
                    || operation.starts_with("tcx::item_name(")
                    || operation.starts_with("tcx::is_builtin_derive(")
            }),
            "non-expansion metadata demand: {external_operations:#?}"
        );
        assert!(
            run.value
                .root
                .children
                .iter()
                .all(|child| child.source != TraceSource::ExternalMetadata),
            "macro metadata must be nested beneath the query which requested it"
        );
        let mut cold_nodes = Vec::new();
        trace_nodes(&run.value.root, &mut cold_nodes);
        assert!(cold_nodes.iter().all(|node| {
            node.source == TraceSource::Sage || node.disposition != TraceDisposition::Observed
        }));

        let warm = provider.symbols().unwrap();
        let warm_run = provider.run(warm.run_id.as_deref().unwrap()).unwrap();
        let mut warm_nodes = Vec::new();
        trace_nodes(&warm_run.value.root, &mut warm_nodes);
        assert!(
            warm_nodes
                .iter()
                .any(|node| node.disposition == TraceDisposition::Reused),
            "an unchanged second request must expose cache hits"
        );
        assert!(
            warm_nodes
                .iter()
                .all(|node| node.source != TraceSource::ExternalMetadata),
            "a warm directory request must not repeat macro metadata reads"
        );

        let method = provided
            .value
            .symbols
            .iter()
            .find(|symbol| symbol.label == "db")
            .unwrap();
        let selected = provider.symbol(&method.path).unwrap();
        let product_ids: Vec<_> = selected
            .value
            .products
            .iter()
            .map(|product| product.id.as_str())
            .collect();
        assert_eq!(
            product_ids,
            [
                "identity",
                "source",
                "concrete-ir",
                "signature",
                "diagnostics",
                "typed-ir",
            ]
        );
        let selected_run = provider.run(selected.run_id.as_deref().unwrap()).unwrap();
        let selected_trace = format!("{:#?}", selected_run.value);
        assert!(!selected_trace.contains("LocalFnSym::sig"));
        assert!(!selected_trace.contains("LocalFnSym::body"));

        let mut external_reference = None;
        for product in ["concrete-ir", "signature", "typed-ir"] {
            let page = provider.product(&method.path, product).unwrap();
            let run = provider.run(page.run_id.as_deref().unwrap()).unwrap();
            let trace = format!("{:#?}", run.value);
            assert!(
                !trace.contains("module_expansion_complete_for_symbol_listing"),
                "{product} must resolve only its ownership chain, not rebuild the eager directory"
            );
            assert!(
                has_phase(&run.value.root, sage_inspector::TracePhase::Reflection),
                "{product} must identify structural reflection separately"
            );
            assert!(
                has_phase(&run.value.root, sage_inspector::TracePhase::ViewAssembly),
                "{product} must identify render-tree assembly separately"
            );
            let mut view_nodes = Vec::new();
            phase_nodes(
                &run.value.root,
                sage_inspector::TracePhase::ViewAssembly,
                &mut view_nodes,
            );
            assert!(
                view_nodes.iter().all(|node| node.children.is_empty()),
                "{product} view assembly must not perform semantic reads"
            );
            if product == "typed-ir" {
                let mut nodes = Vec::new();
                trace_nodes(&run.value.root, &mut nodes);
                assert!(nodes.iter().any(|node| {
                    node.source == TraceSource::Solver
                        && node.operation == "trait-candidates"
                        && node.child_order == sage_inspector::ChildOrder::Unordered
                }));
                assert!(nodes.iter().any(|node| {
                    node.source == TraceSource::Solver
                        && node.operation == "solver-query"
                        && node.child_order == sage_inspector::ChildOrder::Sequential
                }));
            }
            let RenderNode::Value { value } = page.value.content else {
                panic!("{product} must be a reflected value tree")
            };
            let shape = value_shape(&value);
            match product {
                "concrete-ir" => expect_test::expect![[r#"
                    record Stashed
                      .root
                        shared
                          record FnCstData
                            .attrs
                              shared
                                sequence slice [0]
                            .name
                              scalar Name
                            .generics
                              shared
                                sequence slice [0]
                            .params
                              shared
                                sequence slice [1]
                                  record ParamCst
                                    .name
                                      variant Option::Some
                                        .0
                                          scalar Name
                                    .ty
                                      shared
                                        record TypeCst
                                          .kind
                                            variant TypeCstKind::Infer
                                          .span
                                            record RelativeSpan
                                              .start
                                                scalar u32
                                              .end
                                                scalar u32
                                    .receiver
                                      variant Option::Some
                                        .0
                                          variant ReceiverCst::Ref
                                            .mutability
                                              variant Mutability::Shared
                                            .lifetime
                                              variant Option::None
                                    .span
                                      record RelativeSpan
                                        .start
                                          scalar u32
                                        .end
                                          scalar u32
                            .ret
                              variant Option::Some
                                .0
                                  shared
                                    record TypeCst
                                      .kind
                                        variant TypeCstKind::Path
                                          .0
                                            shared
                                              variant Path::Relative
                                                .0
                                                  record PathSegment
                                                    .name
                                                      scalar Name
                                                    .type_args
                                                      shared
                                                        sequence slice [0]
                                                    .span
                                                      record RelativeSpan
                                                        .start
                                                          scalar u32
                                                        .end
                                                          scalar u32
                                                .1
                                                  shared
                                                    sequence slice [0]
                                      .span
                                        record RelativeSpan
                                          .start
                                            scalar u32
                                          .end
                                            scalar u32
                            .body
                              variant Option::Some
                                .0
                                  shared
                                    record ExprCst
                                      .kind
                                        variant ExprCstKind::Block
                                          .0
                                            shared
                                              sequence slice [0]
                                          .1
                                            variant Option::Some
                                              .0
                                                shared
                                                  record ExprCst
                                                    .kind
                                                      variant ExprCstKind::MethodCall
                                                        .0
                                                          shared
                                                            record ExprCst
                                                              .kind
                                                                variant ExprCstKind::Field
                                                                  .0
                                                                    shared
                                                                      record ExprCst
                                                                        .kind
                                                                          variant ExprCstKind::Path
                                                                            .0
                                                                              shared
                                                                                variant Path::Anchored
                                                                                  .0
                                                                                    record PathAnchor
                                                                                      .kind
                                                                                        variant PathAnchorKind::Self_
                                                                                      .span
                                                                                        record RelativeSpan
                                                                                          .start
                                                                                            scalar u32
                                                                                          .end
                                                                                            scalar u32
                                                                                  .1
                                                                                    shared-reference
                                                                        .span
                                                                          record RelativeSpan
                                                                            .start
                                                                              scalar u32
                                                                            .end
                                                                              scalar u32
                                                                  .1
                                                                    scalar Name
                                                              .span
                                                                record RelativeSpan
                                                                  .start
                                                                    scalar u32
                                                                  .end
                                                                    scalar u32
                                                        .1
                                                          scalar Name
                                                        .2
                                                          shared
                                                            sequence slice [0]
                                                    .span
                                                      record RelativeSpan
                                                        .start
                                                          scalar u32
                                                        .end
                                                          scalar u32
                                      .span
                                        record RelativeSpan
                                          .start
                                            scalar u32
                                          .end
                                            scalar u32
                            .where_clauses
                              shared
                                sequence slice [0]
                            .span
                              record RelativeSpan
                                .start
                                  scalar u32
                                .end
                                  scalar u32
                "#]].assert_eq(&shape),
                "signature" => expect_test::expect![[r#"
                    record Stashed
                      .root
                        record Binder
                          .value
                            record FnSig
                              .owner_generic_count
                                scalar u32
                              .owner_self_ty
                                variant Option::Some
                                  .0
                                    shared
                                      variant Ty::Adt
                                        .0
                                          reference local/db_drop_guard/type-struct-DbDropGuard
                                        .1
                                          shared
                                            sequence slice [0]
                              .receiver
                                variant Option::Some
                                  .0
                                    record CheckedReceiver
                                      .owner_self_ty
                                        shared-reference
                                      .form
                                        variant MethodReceiver::Ref
                                          .mutability
                                            variant Mutability::Shared
                              .params
                                shared-reference
                              .ret
                                shared
                                  variant Ty::Adt
                                    .0
                                      reference local/db_drop_guard/type-struct-Db
                                    .1
                                      shared-reference
                              .parameter_env
                                record CheckedParameterEnv
                                  .where_clauses
                                    shared
                                      sequence slice [0]
                                  .solver_eligibility
                                    variant SolverEligibility::Eligible
                              .method_candidate_eligibility
                                variant SolverEligibility::Eligible
                              .const_call_complete
                                scalar bool
                          .generics
                            shared
                              sequence slice [0]
                "#]].assert_eq(&shape),
                "typed-ir" => expect_test::expect![[r#"
                    record CheckedBody
                      .body
                        record Stashed
                          .root
                            shared
                              record TyBodyData
                                .root
                                  shared
                                    record TyExpr
                                      .data
                                        variant TyExprData::Block
                                          .0
                                            shared
                                              sequence slice [0]
                                          .1
                                            variant Option::Some
                                              .0
                                                shared
                                                  record TyExpr
                                                    .data
                                                      variant TyExprData::ResolvedCall
                                                        .0
                                                          record ResolvedCallTarget
                                                            .function
                                                              reference external/core-dd2c57927e223d81/module-clone-0/trait-Clone-0/fn-clone-0
                                                            .dispatch
                                                              variant CallDispatch::StaticTrait
                                                                .self_ty
                                                                  shared
                                                                    variant Ty::Adt
                                                                      .0
                                                                        reference local/db_drop_guard/type-struct-Db
                                                                      .1
                                                                        shared
                                                                          sequence slice [0]
                                                                .trait_ref
                                                                  record TraitRef
                                                                    .trait_sym
                                                                      reference external/core-dd2c57927e223d81/module-clone-0/trait-Clone-0
                                                                    .args
                                                                      shared-reference
                                                            .owner_type_args
                                                              shared
                                                                sequence slice [1]
                                                                  shared-reference
                                                            .method_type_args
                                                              shared-reference
                                                        .1
                                                          shared
                                                            sequence slice [1]
                                                              shared
                                                                record TyExpr
                                                                  .data
                                                                    variant TyExprData::Ref
                                                                      .0
                                                                        shared
                                                                          record TyExpr
                                                                            .data
                                                                              variant TyExprData::Field
                                                                                .0
                                                                                  shared
                                                                                    record TyExpr
                                                                                      .data
                                                                                        variant TyExprData::Deref
                                                                                          .0
                                                                                            shared
                                                                                              record TyExpr
                                                                                                .data
                                                                                                  variant TyExprData::Path
                                                                                                    .0
                                                                                                      variant PathResolution::Local
                                                                                                        .0
                                                                                                          record LocalId
                                                                                                            .0
                                                                                                              scalar u32
                                                                                                .ty
                                                                                                  shared
                                                                                                    variant Ty::Ref
                                                                                                      .0
                                                                                                        shared
                                                                                                          variant Ty::Adt
                                                                                                            .0
                                                                                                              reference local/db_drop_guard/type-struct-DbDropGuard
                                                                                                            .1
                                                                                                              shared-reference
                                                                                                      .1
                                                                                                        variant Mutability::Shared
                                                                                                      .2
                                                                                                        variant Lifetime::Dummy
                                                                                                .span
                                                                                                  record RelativeSpan
                                                                                                    .start
                                                                                                      scalar u32
                                                                                                    .end
                                                                                                      scalar u32
                                                                                      .ty
                                                                                        shared-reference
                                                                                      .span
                                                                                        record RelativeSpan
                                                                                          .start
                                                                                            scalar u32
                                                                                          .end
                                                                                            scalar u32
                                                                                .1
                                                                                  record ResolvedField
                                                                                    .owner
                                                                                      variant FieldOwner::Struct
                                                                                        .0
                                                                                          reference local/db_drop_guard/type-struct-DbDropGuard
                                                                                    .index
                                                                                      scalar u32
                                                                            .ty
                                                                              shared-reference
                                                                            .span
                                                                              record RelativeSpan
                                                                                .start
                                                                                  scalar u32
                                                                                .end
                                                                                  scalar u32
                                                                      .1
                                                                        variant Mutability::Shared
                                                                  .ty
                                                                    shared
                                                                      variant Ty::Ref
                                                                        .0
                                                                          shared-reference
                                                                        .1
                                                                          variant Mutability::Shared
                                                                        .2
                                                                          variant Lifetime::Dummy
                                                                  .span
                                                                    record RelativeSpan
                                                                      .start
                                                                        scalar u32
                                                                      .end
                                                                        scalar u32
                                                    .ty
                                                      shared-reference
                                                    .span
                                                      record RelativeSpan
                                                        .start
                                                          scalar u32
                                                        .end
                                                          scalar u32
                                      .ty
                                        shared-reference
                                      .span
                                        record RelativeSpan
                                          .start
                                            scalar u32
                                          .end
                                            scalar u32
                                .locals
                                  shared
                                    sequence slice [1]
                                      record LocalVar
                                        .name
                                          scalar Name
                                        .span
                                          record RelativeSpan
                                            .start
                                              scalar u32
                                            .end
                                              scalar u32
                                .span
                                  record RelativeSpan
                                    .start
                                      scalar u32
                                    .end
                                      scalar u32
                      .diagnostics
                        sequence Vec [0]
                "#]].assert_eq(&shape),
                _ => unreachable!(),
            }
            if product == "typed-ir" {
                external_reference = first_external_reference(&value).cloned();
            }
            let value = serde_json::to_string(&value).unwrap();
            assert!(value.contains("type_name"));
            if product == "typed-ir" {
                assert!(!value.contains("MethodCall"));
                assert!(value.contains("ResolvedCall"));
                assert!(value.contains("StaticTrait"));
                assert!(value.contains("reference"));
            }
        }

        let external_reference = external_reference.expect("typed call has an external target");
        assert!(
            external_reference.path.starts_with("external/core-")
                && external_reference
                    .path
                    .strip_prefix("external/")
                    .unwrap()
                    .split('/')
                    .next()
                    .is_some_and(|root| root.len() > "external/core-".len()),
            "external roots include a crate disambiguator: {}",
            external_reference.path
        );
        let external = provider.symbol(&external_reference.path).unwrap();
        assert_eq!(external.value.label, "clone");
        assert!(
            external
                .value
                .products
                .iter()
                .all(|product| product.id != "source" && product.id != "typed-ir")
        );
        let external_selection_run = provider.run(external.run_id.as_deref().unwrap()).unwrap();
        assert!(
            !format!("{:#?}", external_selection_run.value).contains("local_expanded_module_items")
        );
        let parent = external
            .value
            .parent
            .expect("associated function has a parent");
        let parent = provider.symbol(&parent.path).unwrap();
        assert_eq!(parent.value.label, "Clone");
        assert!(
            parent
                .value
                .products
                .iter()
                .any(|product| product.id == "items")
        );

        let signature = provider
            .product(&external_reference.path, "signature")
            .unwrap();
        assert!(matches!(signature.value.content, RenderNode::Value { .. }));
        let signature_run = provider.run(signature.run_id.as_deref().unwrap()).unwrap();
        assert!(!format!("{:#?}", signature_run.value).contains("local_expanded_module_items"));
    });
}

#[test]
fn edit_matrix_distinguishes_relevant_execution_from_unrelated_validation() {
    let project = inspector_project();
    run_sage_host_with(&project, &[], |host| {
        let original = host.source_text("lib.rs").unwrap();
        let mut provider = LiveInspectionProvider::new(host);
        let index = provider.symbols().unwrap();
        let method_path = index
            .value
            .symbols
            .iter()
            .find(|symbol| symbol.label == "db")
            .unwrap()
            .path
            .clone();
        let before = provider.product(&method_path, "typed-ir").unwrap();
        let before_revision = before.revision_id.clone();

        let relevant = original.replace("self.db.clone()", "Db { shared: self.db.shared }");
        assert_ne!(relevant, original);
        provider
            .apply_updates(vec![FileUpdate {
                path: "lib.rs".to_owned(),
                text: relevant.clone(),
            }])
            .unwrap();
        let after = provider.product(&method_path, "typed-ir").unwrap();
        let after_revision = after.revision_id.clone();
        let comparison = provider
            .compare(&before_revision, &after_revision, &method_path, "typed-ir")
            .unwrap();
        assert!(comparison.value.value_changed);
        let after_run = provider.run(after.run_id.as_deref().unwrap()).unwrap();
        let mut after_nodes = Vec::new();
        trace_nodes(&after_run.value.root, &mut after_nodes);
        assert!(after_nodes.iter().any(|node| {
            node.operation.contains("body") && node.disposition == TraceDisposition::Executed
        }));

        provider
            .apply_updates(vec![FileUpdate {
                path: "lib.rs".to_owned(),
                text: format!("{relevant}\n// unrelated edit\n"),
            }])
            .unwrap();
        let unrelated = provider.product(&method_path, "typed-ir").unwrap();
        let unrelated_revision = unrelated.revision_id.clone();
        let unrelated_comparison = provider
            .compare(
                &after_revision,
                &unrelated_revision,
                &method_path,
                "typed-ir",
            )
            .unwrap();
        assert!(!unrelated_comparison.value.value_changed);
        let unrelated_run = provider.run(unrelated.run_id.as_deref().unwrap()).unwrap();
        let mut unrelated_nodes = Vec::new();
        trace_nodes(&unrelated_run.value.root, &mut unrelated_nodes);
        assert!(
            unrelated_nodes
                .iter()
                .any(|node| node.disposition == TraceDisposition::Validated)
        );
        assert!(before_revision.starts_with("db0-rev"));
        assert_ne!(before_revision, after_revision);
        assert_ne!(after_revision, unrelated_revision);
    });
}

#[test]
fn retained_host_separates_edits_from_demand_and_preserves_symbol_paths() {
    let project = inspector_project();
    run_sage_host_with(&project, &[], |host| {
        let original = host.source_text("lib.rs").unwrap();
        let (watch_root_sender, watch_root_receiver) = std::sync::mpsc::channel();
        let mut provider =
            LiveInspectionProvider::new(host).with_watch_root_observer(watch_root_sender);
        let initial_watch_root = watch_root_receiver.recv().unwrap();
        let initial_index = provider.symbols().unwrap();
        let method_path = initial_index
            .value
            .symbols
            .iter()
            .find(|symbol| symbol.label == "db")
            .unwrap()
            .path
            .clone();
        let before = provider.product(&method_path, "typed-ir").unwrap();
        let before_revision = before.revision_id.clone();
        assert!(before_revision.starts_with("db0-rev"));

        let event = provider
            .apply_updates(vec![FileUpdate {
                path: "lib.rs".to_owned(),
                text: format!("{original}\n// unrelated trailing edit\n"),
            }])
            .unwrap();
        let RevisionEvent::RevisionAdvanced(advanced) = event.value else {
            panic!("expected an incremental revision")
        };
        let after_revision = advanced.revision_id;
        let detail = provider.revision_detail(&after_revision).unwrap();
        assert_eq!(detail.value.input_deltas.len(), 1);
        assert!(detail.value.runs.is_empty(), "an edit is not query demand");

        let after_index = provider.symbols().unwrap();
        assert!(
            after_index
                .value
                .symbols
                .iter()
                .any(|symbol| symbol.path == method_path),
            "an unrelated edit must preserve the canonical ownership path"
        );
        let after = provider.product(&method_path, "typed-ir").unwrap();
        assert_eq!(after.revision_id, after_revision);
        let comparison = provider
            .compare(&before_revision, &after_revision, &method_path, "typed-ir")
            .unwrap();
        assert!(!comparison.value.value_changed);

        let revisions = provider.revisions(None).unwrap();
        assert_eq!(revisions.value.revisions[0].revision_id, after_revision);
        assert!(matches!(
            &revisions.value.revisions[0].cause,
            sage_inspector::RevisionCause::InputEdit { edit_batch } if edit_batch == "edit-1"
        ));
        assert_eq!(revisions.value.revisions[0].input_delta_count, 1);
        assert_eq!(revisions.value.revisions[0].run_count, 2);

        let warm = provider.product(&method_path, "typed-ir").unwrap();
        let warm_run = provider.run(warm.run_id.as_deref().unwrap()).unwrap();
        let mut nodes = Vec::new();
        trace_nodes(&warm_run.value.root, &mut nodes);
        assert!(
            nodes
                .iter()
                .any(|node| node.disposition == TraceDisposition::Reused)
        );

        let reordered = r#"struct DbDropGuard {
    db: Db,
}

#[derive(Clone)]
struct Db {
    shared: bool,
}

impl DbDropGuard {
    fn db(&self) -> Db {
        self.db.clone()
    }
}
"#;
        provider
            .apply_updates(vec![FileUpdate {
                path: "lib.rs".to_owned(),
                text: reordered.to_owned(),
            }])
            .unwrap();
        let reordered_index = provider.symbols().unwrap();
        assert!(
            reordered_index
                .value
                .symbols
                .iter()
                .any(|symbol| symbol.path == method_path)
        );

        let renamed_source = reordered.replace("fn db(&self)", "fn database(&self)");
        provider
            .apply_updates(vec![FileUpdate {
                path: "lib.rs".to_owned(),
                text: renamed_source.clone(),
            }])
            .unwrap();
        let stale = provider.symbol(&method_path).unwrap_err();
        assert_eq!(stale.code, "symbol-not-found");
        let renamed_index = provider.symbols().unwrap();
        let renamed = renamed_index
            .value
            .symbols
            .iter()
            .find(|symbol| symbol.label == "database")
            .expect("the renamed method is indexed");
        assert_ne!(renamed.path, method_path);
        let renamed_path = renamed.path.clone();

        provider
            .apply_updates(vec![FileUpdate {
                path: "lib.rs".to_owned(),
                text: renamed_source.replace("#[derive(Clone)]", "#[derive(Clone, Debug)]"),
            }])
            .unwrap();
        let incomplete_index = provider.symbols().unwrap();
        assert!(matches!(
            incomplete_index.value.symbols[0].children,
            sage_inspector::ChildCompleteness::Incomplete { ref reason }
                if reason.code == "macro-expansion-incomplete"
        ));

        let fresh_reference = r#"mod unrelated {
    pub struct Noise;
}

fn fresh_value() -> u32 {
    22
}

struct DbDropGuard;

impl DbDropGuard {
    fn database(&self) -> u32 {
        fresh_value()
    }
}
"#;
        provider
            .apply_updates(vec![FileUpdate {
                path: "lib.rs".to_owned(),
                text: fresh_reference.to_owned(),
            }])
            .unwrap();
        let direct_product = provider
            .product(&renamed_path, "typed-ir")
            .expect("a direct post-edit product request must refresh reflected links");
        let RenderNode::Value { value } = &direct_product.value.content else {
            panic!("typed IR must be a reflected value")
        };
        assert!(
            contains_reference_label(value, "fresh_value"),
            "fresh reflected value: {value:#?}"
        );
        let direct_run = provider
            .run(direct_product.run_id.as_deref().unwrap())
            .unwrap();
        let mut reflection_nodes = Vec::new();
        phase_nodes(
            &direct_run.value.root,
            sage_inspector::TracePhase::Reflection,
            &mut reflection_nodes,
        );
        let expansion_requests: u64 = reflection_nodes
            .iter()
            .filter(|node| node.operation.contains("expanded_module_items"))
            .map(|node| node.observations)
            .sum();
        assert_eq!(
            expansion_requests, 1,
            "fresh-link reflection may enumerate the owner module but not an unrelated sibling subtree"
        );

        // Cache a directory containing the referenced function, then rename
        // that function without refreshing the directory. A direct product
        // read must not emit the previous revision's path or label.
        provider.symbols().unwrap();
        let renamed_reference = fresh_reference.replace("fresh_value", "renamed_value");
        provider
            .apply_updates(vec![FileUpdate {
                path: "lib.rs".to_owned(),
                text: renamed_reference,
            }])
            .unwrap();
        let renamed_link = provider
            .product(&renamed_path, "typed-ir")
            .expect("a direct product request must resolve links in the current revision");
        let RenderNode::Value { value } = &renamed_link.value.content else {
            panic!("typed IR must be a reflected value")
        };
        assert!(contains_reference_label(value, "renamed_value"));
        assert!(!contains_reference_label(value, "fresh_value"));

        let previous_revision = provider.current_revision();
        let unknown = provider
            .apply_updates(vec![sage_inspector::FileUpdate {
                path: "src/new_module.rs".to_owned(),
                text: "pub fn new_item() {}\n".to_owned(),
            }])
            .unwrap_err();
        assert_eq!(unknown.code, "unrepresented-source-file");
        assert_eq!(provider.current_revision(), previous_revision);
        let reload = provider
            .reload_workspace(sage_inspector::Issue {
                code: "test-reload".to_owned(),
                message: "exercise the explicit database reconstruction boundary".to_owned(),
            })
            .unwrap();
        let RevisionEvent::WorkspaceReloaded(reloaded) = reload.value else {
            panic!("expected a workspace reload")
        };
        assert_eq!(watch_root_receiver.recv().unwrap(), initial_watch_root);
        assert_eq!(reloaded.previous_revision_id, previous_revision);
        assert!(reloaded.revision_id.starts_with("db1-rev"));
        let reload_detail = provider.revision_detail(&reloaded.revision_id).unwrap();
        assert!(matches!(
            &reload_detail.value.summary.cause,
            sage_inspector::RevisionCause::WorkspaceReload {
                previous_revision_id,
                reason,
            } if previous_revision_id == &previous_revision && reason.code == "test-reload"
        ));
        assert!(reload_detail.value.input_deltas.is_empty());
        assert!(reload_detail.value.runs.is_empty());
        let rebuilt = provider.symbols().unwrap();
        assert!(
            rebuilt
                .value
                .symbols
                .iter()
                .any(|symbol| symbol.path == method_path)
        );
    });
}

#[test]
fn workspace_reload_publishes_a_manifest_moved_source_root() {
    let project = TempProject::new("watch-root");
    let member = project.0.join("member");
    let helper = project.0.join("helper");
    std::fs::create_dir_all(member.join("src")).unwrap();
    std::fs::create_dir_all(helper.join("src")).unwrap();
    std::fs::write(
        project.0.join("Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\nexclude = [\"helper\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    std::fs::write(
        helper.join("Cargo.toml"),
        "[package]\nname = \"helper\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(helper.join("src/lib.rs"), "pub fn helper() {}\n").unwrap();
    let working_manifest = "[package]\nname = \"watch-root\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\nname = \"watch_target\"\n";
    std::fs::write(member.join("Cargo.toml"), working_manifest).unwrap();
    std::fs::write(member.join("src/lib.rs"), "pub fn value() -> u32 { 1 }\n").unwrap();

    run_sage_host_with(&member, &[], |host| {
        let (watch_root_sender, watch_root_receiver) = std::sync::mpsc::channel();
        let mut provider =
            LiveInspectionProvider::new(host).with_watch_root_observer(watch_root_sender);
        let session = provider.session().unwrap().value;
        assert_eq!(session.workspace_root, project.0.display().to_string());
        assert_eq!(session.target.package, "watch-root");
        assert_eq!(session.target.target_name, "watch_target");
        assert_eq!(
            watch_root_receiver.recv().unwrap(),
            sage::inspector::SourceWatchRoots {
                workspace_root: project.0.clone(),
                package_root: member.clone(),
                source_root: member.join("src"),
            }
        );

        let previous_revision = provider.current_revision();
        std::fs::write(member.join("Cargo.toml"), "this is not a manifest\n").unwrap();
        let failure = provider
            .reload_workspace(sage_inspector::Issue {
                code: "workspace-configuration-changed".to_owned(),
                message: "exercise a transient invalid manifest".to_owned(),
            })
            .unwrap_err();
        assert_eq!(failure.code, "workspace-reload-failed");
        assert_eq!(provider.current_revision(), previous_revision);
        assert!(watch_root_receiver.try_recv().is_err());

        std::fs::create_dir_all(member.join("generated/source")).unwrap();
        std::fs::write(
            member.join("generated/source/lib.rs"),
            "pub fn value() -> u32 { 2 }\n",
        )
        .unwrap();
        std::fs::write(
            member.join("Cargo.toml"),
            "[package]\nname = \"watch-root\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\nname = \"watch_target\"\npath = \"generated/source/lib.rs\"\n",
        )
        .unwrap();

        provider
            .reload_workspace(sage_inspector::Issue {
                code: "workspace-configuration-changed".to_owned(),
                message: "the target root moved".to_owned(),
            })
            .unwrap();
        assert_eq!(
            watch_root_receiver.recv().unwrap(),
            sage::inspector::SourceWatchRoots {
                workspace_root: project.0.clone(),
                package_root: member.clone(),
                source_root: member.join("generated/source"),
            }
        );
        let update = provider
            .apply_updates(vec![FileUpdate {
                path: "lib.rs".to_owned(),
                text: "pub fn value() -> u32 { 3 }\n".to_owned(),
            }])
            .expect("the reconstructed host must register the new source tree");
        assert!(matches!(update.value, RevisionEvent::RevisionAdvanced(_)));
    });
}

#[test]
fn live_host_requires_one_explicit_workspace_target() {
    let project = TempProject::new("target-selection");
    std::fs::write(
        project.0.join("Cargo.toml"),
        "[workspace]\nmembers = [\"first\", \"second\", \"helper\", \"broken\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    for package in ["first", "helper", "broken"] {
        let root = project.0.join(package);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            format!("[package]\nname = \"{package}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )
        .unwrap();
        let source = if package == "broken" {
            "this is not Rust\n"
        } else {
            "pub fn value() {}\n"
        };
        std::fs::write(root.join("src/lib.rs"), source).unwrap();
    }
    let second = project.0.join("second");
    std::fs::create_dir_all(second.join("src")).unwrap();
    std::fs::write(
        second.join("Cargo.toml"),
        "[package]\nname = \"second\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\ntype = { package = \"helper\", path = \"../helper\" }\n",
    )
    .unwrap();
    std::fs::write(
        second.join("src/lib.rs"),
        "pub fn value() { r#type::value(); }\n",
    )
    .unwrap();

    let error = match SageHost::try_open(&project.0, &[]) {
        Ok(_) => panic!("an ambiguous workspace must not silently select its first target"),
        Err(error) => error,
    };
    assert!(
        error.contains("requires exactly one library target"),
        "{error}"
    );

    let host = SageHost::try_open(&project.0, &["second".to_owned()]).unwrap();
    assert_eq!(host.package_name(), "second");
    assert_eq!(host.target().name, "second");
    host.with_context(|context| {
        assert_eq!(context.direct_dependencies, ["type"]);
    });
}

#[test]
fn salsa_request_spans_are_balanced_when_a_query_unwinds() {
    let project = inspector_project();
    run_sage_host_with(&project, &[], |host| {
        let _ = host.take_inspection_log();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            host.with_context(|context| panicking_inspection_query(context.db));
        }));
        assert!(panic.is_err());
        let events: Vec<_> = host
            .take_inspection_log()
            .into_iter()
            .filter(|event| match event {
                sage_ir::db::InspectionEvent::QueryEnter { key }
                | sage_ir::db::InspectionEvent::QueryExit { key, .. }
                | sage_ir::db::InspectionEvent::QueryLeaf { key, .. } => {
                    key.contains("panicking_inspection_query")
                }
                sage_ir::db::InspectionEvent::ExternalMetadata { .. }
                | sage_ir::db::InspectionEvent::PhaseEnter { .. }
                | sage_ir::db::InspectionEvent::PhaseExit { .. }
                | sage_ir::db::InspectionEvent::SpanEnter { .. }
                | sage_ir::db::InspectionEvent::SpanExit { .. } => false,
            })
            .collect();
        assert!(matches!(
            events.as_slice(),
            [
                sage_ir::db::InspectionEvent::QueryEnter { .. },
                sage_ir::db::InspectionEvent::QueryExit {
                    disposition: salsa::QueryDisposition::Cancelled,
                    ..
                }
            ]
        ));
    });
}

#[test]
fn salsa_request_spans_include_pre_fetch_revision_cancellation() {
    use salsa::Database as _;

    let database = sage_ir::db::Database::default();
    let _ = database.take_inspection_log();
    database.cancellation_token().cancel();
    let cancelled = salsa::Cancelled::catch(std::panic::AssertUnwindSafe(|| {
        repeated_leaf_query(&database)
    }));
    assert!(matches!(cancelled, Err(salsa::Cancelled::Local)));
    let events: Vec<_> = database
        .take_inspection_log()
        .into_iter()
        .filter(|event| match event {
            sage_ir::db::InspectionEvent::QueryEnter { key }
            | sage_ir::db::InspectionEvent::QueryExit { key, .. }
            | sage_ir::db::InspectionEvent::QueryLeaf { key, .. } => {
                key.contains("repeated_leaf_query")
            }
            sage_ir::db::InspectionEvent::ExternalMetadata { .. }
            | sage_ir::db::InspectionEvent::PhaseEnter { .. }
            | sage_ir::db::InspectionEvent::PhaseExit { .. }
            | sage_ir::db::InspectionEvent::SpanEnter { .. }
            | sage_ir::db::InspectionEvent::SpanExit { .. } => false,
        })
        .collect();
    assert!(matches!(
        events.as_slice(),
        [
            sage_ir::db::InspectionEvent::QueryEnter { .. },
            sage_ir::db::InspectionEvent::QueryExit {
                disposition: salsa::QueryDisposition::Cancelled,
                ..
            }
        ]
    ));
}

#[test]
fn repeated_leaf_requests_retain_multiplicity_without_repeating_nodes() {
    use salsa::Database as _;

    let db = sage_ir::db::Database::default();
    db.attach(|db| {
        assert_eq!(repeated_leaf_query(db), 22);
        assert_eq!(repeated_leaf_query(db), 22);
        assert_eq!(repeated_leaf_query(db), 22);
    });
    assert!(db.take_inspection_log().iter().any(|event| matches!(
        event,
        sage_ir::db::InspectionEvent::QueryLeaf {
            key,
            disposition: salsa::QueryDisposition::Reused,
            observations: 2,
        } if key.contains("repeated_leaf_query")
    )));
}

#[test]
fn inspector_reports_an_occupied_port_without_panicking() {
    let project = inspector_project();
    let occupied = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = occupied.local_addr().unwrap().port().to_string();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cargo-sage"))
        .args(["sage", "inspect", "--port", &port, "--no-open"])
        .current_dir(project)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("failed to bind the inspector"), "{stderr}");
    assert!(stderr.contains("--port <PORT>"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[test]
fn real_command_serves_embedded_assets_and_correlated_request_logs() {
    use expect_test::expect;
    use std::io::{BufRead as _, Read as _};

    let project = inspector_project();
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_cargo-sage"))
        .args(["sage", "inspect", "--port", "0", "--no-open"])
        .current_dir(project)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = ChildGuard(child);
    let mut stdout = std::io::BufReader::new(child.0.stdout.take().unwrap());
    let mut ready = String::new();
    stdout.read_line(&mut ready).unwrap();
    if ready.is_empty() {
        let status = child.0.wait().unwrap();
        let mut stderr = String::new();
        child
            .0
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        panic!("inspector exited before readiness ({status}): {stderr}");
    }
    let ready: serde_json::Value = serde_json::from_str(&ready).unwrap();
    let address = ready["url"]
        .as_str()
        .unwrap()
        .strip_prefix("http://")
        .unwrap();

    let direct_route = http_get(address, "/symbols/local%2Fdb-drop-guard/signature", false);
    assert_eq!(direct_route.status, 200);
    assert_eq!(
        direct_route.headers.get("content-type").map(String::as_str),
        Some("text/html")
    );
    assert!(direct_route.body.contains("<div id=\"root\"></div>"));
    let missing_asset = http_get(address, "/assets/not-built.js", false);
    assert_eq!(missing_asset.status, 404);

    let json = |path: &str| {
        let response = http_get(address, path, true);
        assert_json_response(&response, 200);
        serde_json::from_str::<serde_json::Value>(&response.body).unwrap()
    };
    let revision_response = http_get(address, "/api/v1/revision", true);
    assert_json_response(&revision_response, 200);
    let revision: serde_json::Value = serde_json::from_str(&revision_response.body).unwrap();
    let session = json("/api/v1/session");
    let symbols = json("/api/v1/symbols");
    let method_path = symbols["value"]["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .find(|symbol| symbol["label"] == "db")
        .and_then(|symbol| symbol["path"].as_str())
        .expect("the sample project contains DbDropGuard::db");
    let symbol = json(&format!(
        "/api/v1/symbol?path={}",
        percent_encode(method_path)
    ));
    let signature = json(&format!(
        "/api/v1/product?symbol={}&product=signature",
        percent_encode(method_path)
    ));
    let signature_run = signature["run_id"]
        .as_str()
        .expect("signature inspection records a run");
    let run = json(&format!("/api/v1/runs/{signature_run}"));

    assert!(
        revision["revision_id"]
            .as_str()
            .is_some_and(|revision| revision.starts_with("db0-rev"))
    );
    assert_eq!(session["value"]["protocol_version"], "1");
    assert_eq!(symbols["value"]["root"], "local/db_drop_guard");
    assert_eq!(symbol["value"]["label"], "db");
    assert_eq!(signature["value"]["title"], "Checked function signature");
    assert_eq!(run["value"]["request"]["kind"], "product");

    let db_path = symbols["value"]["symbols"]
        .as_array()
        .expect("the live symbol index contains an array")
        .iter()
        .find(|symbol| symbol["label"] == "Db")
        .and_then(|symbol| symbol["path"].as_str())
        .expect("the sample project contains Db");
    let db = json(&format!("/api/v1/symbol?path={}", percent_encode(db_path)));
    let identity_href = db["value"]["products"]
        .as_array()
        .unwrap()
        .iter()
        .find(|product| product["id"] == "identity")
        .and_then(|product| product["href"].as_str())
        .expect("Db advertises its identity page");
    let db_identity_response = http_get(address, identity_href, true);
    assert_json_response(&db_identity_response, 200);
    let db_identity: serde_json::Value = serde_json::from_str(&db_identity_response.body).unwrap();
    assert_eq!(db_identity["value"]["title"], "Symbol identity");

    let malformed_product = http_get(
        address,
        &format!("/api/v1/product?symbol={}", percent_encode(db_path)),
        true,
    );
    assert_json_response(&malformed_product, 400);
    let missing_symbol = http_get(
        address,
        "/api/v1/symbol?path=local%2Fdb_drop_guard%2Fmissing",
        true,
    );
    assert_json_response(&missing_symbol, 404);
    let unadvertised_product = http_get(
        address,
        &format!(
            "/api/v1/product?symbol={}&product=not-advertised",
            percent_encode(db_path)
        ),
        true,
    );
    assert_json_response(&unadvertised_product, 404);
    let unknown_api = http_get(address, "/api/v1/not-a-route", true);
    assert_json_response(&unknown_api, 404);

    let mut protocol_snapshot = String::new();
    protocol_response_snapshot("revision", &revision_response, &mut protocol_snapshot);
    protocol_response_snapshot("Db identity", &db_identity_response, &mut protocol_snapshot);
    protocol_response_snapshot(
        "malformed product",
        &malformed_product,
        &mut protocol_snapshot,
    );
    protocol_response_snapshot("missing symbol", &missing_symbol, &mut protocol_snapshot);
    protocol_response_snapshot(
        "unadvertised product",
        &unadvertised_product,
        &mut protocol_snapshot,
    );
    protocol_response_snapshot("unknown API route", &unknown_api, &mut protocol_snapshot);
    expect![[r###"
        ## revision
        status: 200
        content-type: application/json; charset=utf-8
        cache-control: no-store
        {
          "revision_id": "db0-rev1",
          "request_id": "request-1",
          "run_id": null,
          "value": null
        }
        ## Db identity
        status: 200
        content-type: application/json; charset=utf-8
        cache-control: no-store
        {
          "revision_id": "db0-rev1",
          "request_id": "request-8",
          "run_id": "run_5",
          "value": {
            "id": "identity",
            "title": "Symbol identity",
            "content": {
              "kind": "group",
              "layout": "block",
              "children": [
                {
                  "kind": "heading",
                  "level": 2,
                  "text": "Canonical identity"
                },
                {
                  "kind": "text",
                  "text": "local/db_drop_guard/type-struct-Db"
                },
                {
                  "kind": "text",
                  "text": "db_drop_guard::Db"
                }
              ]
            }
          }
        }
        ## malformed product
        status: 400
        content-type: application/json; charset=utf-8
        cache-control: no-store
        {
          "revision_id": "db0-rev1",
          "request_id": "request-9",
          "run_id": null,
          "error": {
            "code": "invalid-request",
            "message": "missing or invalid symbol/product query"
          }
        }
        ## missing symbol
        status: 404
        content-type: application/json; charset=utf-8
        cache-control: no-store
        {
          "revision_id": "db0-rev1",
          "request_id": "request-10",
          "run_id": null,
          "error": {
            "code": "symbol-not-found",
            "message": "unknown symbol path `local/db_drop_guard/missing`"
          }
        }
        ## unadvertised product
        status: 404
        content-type: application/json; charset=utf-8
        cache-control: no-store
        {
          "revision_id": "db0-rev1",
          "request_id": "request-11",
          "run_id": null,
          "error": {
            "code": "product-not-found",
            "message": "product `not-advertised` is not listed for `local/db_drop_guard/type-struct-Db`"
          }
        }
        ## unknown API route
        status: 404
        content-type: application/json; charset=utf-8
        cache-control: no-store
        {
          "revision_id": "db0-rev1",
          "request_id": "request-12",
          "run_id": null,
          "error": {
            "code": "not-found",
            "message": "unknown API route `/api/v1/not-a-route`"
          }
        }
    "###]].assert_eq(&protocol_snapshot);

    child.0.kill().unwrap();
    let status = child.0.wait().unwrap();
    assert!(
        !status.success(),
        "the smoke test intentionally terminates the server"
    );
    let mut stderr = String::new();
    child
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    let transcript: Vec<_> = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| event["event"] == "inspection-request")
        .map(|event| {
            serde_json::json!({
                "request_id": event["request_id"],
                "operation": event["operation"],
                "arguments": event["arguments"],
                "revision_id": event["revision_id"],
                "status": event["status"],
                "run_id": event["run_id"],
            })
        })
        .collect();
    assert_eq!(
        transcript.len(),
        12,
        "every HTTP request is visible in the actor demand log"
    );
    let evidence = serde_json::json!({
        "visible": {
            "target": session["value"]["target"],
            "symbol_root": symbols["value"]["root"],
            "selected": symbol["value"]["display_path"],
            "product": signature["value"]["title"],
            "trace_root": run["value"]["root"]["operation"],
        },
        "demand": transcript,
        "live_regression": {
            "advertised_db_identity": db_identity["value"]["title"],
        },
    });
    expect![[r#"
        {
          "demand": [
            {
              "arguments": [],
              "operation": "current-revision",
              "request_id": "request-1",
              "revision_id": "db0-rev1",
              "run_id": null,
              "status": "ok"
            },
            {
              "arguments": [],
              "operation": "session",
              "request_id": "request-2",
              "revision_id": "db0-rev1",
              "run_id": null,
              "status": "ok"
            },
            {
              "arguments": [],
              "operation": "local-symbol-index",
              "request_id": "request-3",
              "revision_id": "db0-rev1",
              "run_id": "run_1",
              "status": "ok"
            },
            {
              "arguments": [
                "local/db_drop_guard/impl-08431cf62f880d5d/value-fn-db"
              ],
              "operation": "symbol",
              "request_id": "request-4",
              "revision_id": "db0-rev1",
              "run_id": "run_2",
              "status": "ok"
            },
            {
              "arguments": [
                "local/db_drop_guard/impl-08431cf62f880d5d/value-fn-db",
                "signature"
              ],
              "operation": "product",
              "request_id": "request-5",
              "revision_id": "db0-rev1",
              "run_id": "run_3",
              "status": "ok"
            },
            {
              "arguments": [
                "run_3"
              ],
              "operation": "run",
              "request_id": "request-6",
              "revision_id": "db0-rev1",
              "run_id": null,
              "status": "ok"
            },
            {
              "arguments": [
                "local/db_drop_guard/type-struct-Db"
              ],
              "operation": "symbol",
              "request_id": "request-7",
              "revision_id": "db0-rev1",
              "run_id": "run_4",
              "status": "ok"
            },
            {
              "arguments": [
                "local/db_drop_guard/type-struct-Db",
                "identity"
              ],
              "operation": "product",
              "request_id": "request-8",
              "revision_id": "db0-rev1",
              "run_id": "run_5",
              "status": "ok"
            },
            {
              "arguments": [],
              "operation": "current-revision",
              "request_id": "request-9",
              "revision_id": "db0-rev1",
              "run_id": null,
              "status": "ok"
            },
            {
              "arguments": [
                "local/db_drop_guard/missing"
              ],
              "operation": "symbol",
              "request_id": "request-10",
              "revision_id": "db0-rev1",
              "run_id": null,
              "status": "error"
            },
            {
              "arguments": [
                "local/db_drop_guard/type-struct-Db",
                "not-advertised"
              ],
              "operation": "product",
              "request_id": "request-11",
              "revision_id": "db0-rev1",
              "run_id": null,
              "status": "error"
            },
            {
              "arguments": [],
              "operation": "current-revision",
              "request_id": "request-12",
              "revision_id": "db0-rev1",
              "run_id": null,
              "status": "ok"
            }
          ],
          "live_regression": {
            "advertised_db_identity": "Symbol identity"
          },
          "visible": {
            "product": "Checked function signature",
            "selected": "db_drop_guard::impl item::db",
            "symbol_root": "local/db_drop_guard",
            "target": {
              "package": "db-drop-guard",
              "target_kind": "lib",
              "target_name": "db_drop_guard"
            },
            "trace_root": "product"
          }
        }
    "#]]
    .assert_eq(&format!(
        "{}\n",
        serde_json::to_string_pretty(&evidence).unwrap()
    ));
}
