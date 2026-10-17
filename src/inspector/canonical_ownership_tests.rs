use crate::driver::run_sage_host_with;
use crate::inspector::LiveInspectionProvider;
use sage_inspector::{FileUpdate, InspectionProvider, RenderNode};

#[test]
#[sage::test(crates.sage_inspector.inspection.canonical_ownership)]
fn anonymous_use_paths_survive_sibling_reordering() {
    let project = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test-projects/semantic-inspector/db-drop-guard");
    let paths = run_sage_host_with(&project, &[], |host| {
        let mut provider = LiveInspectionProvider::new(host);
        let declarations = "mod a { pub struct A; }\nmod b { pub struct B; }\n";
        provider
            .apply_updates(vec![FileUpdate {
                path: "lib.rs".to_owned(),
                text: format!("{declarations}use a::A;\nuse b::B;\n"),
            }])
            .unwrap();
        let index = provider.symbols().unwrap();
        let mut paths = Vec::new();
        for symbol in &index.value.symbols {
            if symbol.label == "use item" {
                let product = provider.product(&symbol.path, "source").unwrap();
                let RenderNode::Code { text, .. } = product.value.content else {
                    panic!("a source-written import has source code");
                };
                paths.push((symbol.path.clone(), text));
            }
        }
        assert_eq!(paths.len(), 2, "both distinct imports must be represented");
        provider
            .apply_updates(vec![FileUpdate {
                path: "lib.rs".to_owned(),
                text: format!("{declarations}use b::B;\nuse a::A;\n"),
            }])
            .unwrap();
        provider.symbols().unwrap();
        for (path, expected_source) in &paths {
            let product = provider
                .product(path, "source")
                .expect("sibling movement preserves an existing ownership path");
            let RenderNode::Code { text, .. } = product.value.content else {
                panic!("a source-written import has source code");
            };
            assert_eq!(
                &text, expected_source,
                "path {path} must retain its original import"
            );
        }
        paths
    });
    run_sage_host_with(&project, &[], |host| {
        let mut provider = LiveInspectionProvider::new(host);
        provider
            .apply_updates(vec![FileUpdate {
                path: "lib.rs".to_owned(),
                text: "mod b { pub struct B; }\nmod a { pub struct A; }\nuse b::B;\nuse a::A;\n"
                    .to_owned(),
            }])
            .unwrap();
        provider.symbols().unwrap();
        for (path, expected_source) in paths {
            let product = provider
                .product(&path, "source")
                .expect("reconstructing the host preserves each distinct import path");
            let RenderNode::Code { text, .. } = product.value.content else {
                panic!("a source-written import has source code");
            };
            assert_eq!(
                text, expected_source,
                "reconstructed path {path} must retain its import"
            );
        }
    });
}
