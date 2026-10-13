#![feature(register_tool)]
#![register_tool(sage)]

use sage_meta::correspondence::{CorrespondenceIndex, RustSource};
use sage_meta::evidence::{EvidenceStatus, placeholder_status, test_explanation};

fn index(text: &str) -> CorrespondenceIndex {
    CorrespondenceIndex::from_sources(&[RustSource {
        crate_name: "fixture".into(),
        target: "lib:fixture".into(),
        file: "src/lib.rs".into(),
        source: text.into(),
    }])
    .unwrap()
}

#[test]
#[sage::test(meta.anchors.implementation_references)]
fn only_actual_line_comments_declare_implementation_entries() {
    let source = r####"#!/usr/bin/env ignored(with-unbalanced-parenthesis
const TEXT: &str = "// {anchor}`example.string`";
const RAW: &str = r###"
// {anchor}`example.raw`<<
// >>
"###;
const BYTE: &[u8] = br#"// {anchor}`example.byte`"#;
const C: &std::ffi::CStr = c"// {anchor}`example.c`";
const RAW_C: &std::ffi::CStr = cr#"// {anchor}`example.raw_c`"#;
const ESCAPED: &str = "quote \" // {anchor}`example.escaped`";
const CHAR: char = '\'';
/* Nested comment /* // {anchor}`example.block` */
// >>
*/
/// {anchor}`example.doc`
fn café<'a>() {
    let café = "é"; // {anchor}`example.real`
    // Explanation belongs to a real source comment.
}
macro_rules! source_comments {
    () => {
        // {anchor}`example.macro_input`
        ()
    }
}
"####;
    let rust = index(source);
    assert!(rust.diagnostics.is_empty(), "{:?}", rust.diagnostics);
    assert_eq!(
        rust.implementations
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["example.macro_input", "example.real"]
    );
    let site = &rust.implementations["example.real"][0];
    assert_eq!(site.module, "fixture");
    assert_eq!(site.file, "src/lib.rs");
    assert_eq!(site.line, 17);
    assert_eq!(
        &source[site.annotation_span.start..site.annotation_span.end],
        "// {anchor}`example.real`"
    );
    assert!(
        rust.entries.is_empty(),
        "comments are not sage::item mappings"
    );
    assert!(rust.tests.is_empty(), "comments are not annotated tests");
}

#[test]
#[sage::test(meta.anchors.point_excerpts)]
fn point_excerpts_include_five_following_lines_and_truncate_at_eof() {
    let source = "// {anchor}`example.point`\r\n// one\r\n// two\r\n// three\r\n// four\r\n// five\r\n// excluded\r\n// {anchor}`example.eof`\r\n// last";
    let rust = index(source);
    assert!(rust.diagnostics.is_empty());
    let point = &rust.implementations["example.point"][0];
    assert_eq!((point.line, point.end_line), (1, 6));
    assert!(!point.region);
    assert_eq!(
        point.excerpt,
        "// {anchor}`example.point`\r\n// one\r\n// two\r\n// three\r\n// four\r\n// five\r\n"
    );
    assert_eq!(&source[point.span.start..point.span.end], point.excerpt);
    let eof = &rust.implementations["example.eof"][0];
    assert_eq!((eof.line, eof.end_line), (8, 9));
    assert_eq!(eof.excerpt, "// {anchor}`example.eof`\r\n// last");
    assert_eq!(&source[eof.span.start..eof.span.end], eof.excerpt);
    let final_line = index("// {anchor}`example.last`\n");
    assert_eq!(final_line.implementations["example.last"][0].end_line, 1);
}

#[test]
#[sage::test(meta.anchors.region_excerpts)]
fn nested_regions_close_the_nearest_opener_and_points_keep_their_own_preview() {
    let source = "// {anchor}`example.outer`<<\n// outer\n// {anchor}`example.inner`<<\n// {anchor}`example.point`\n// >>\n// after inner\n// >>\n// after outer\n// last\n";
    let rust = index(source);
    assert!(rust.diagnostics.is_empty());
    let outer = &rust.implementations["example.outer"][0];
    let inner = &rust.implementations["example.inner"][0];
    let point = &rust.implementations["example.point"][0];
    assert!(outer.region && inner.region && !point.region);
    assert_eq!((outer.line, outer.end_line), (1, 7));
    assert_eq!((inner.line, inner.end_line), (3, 5));
    assert_eq!((point.line, point.end_line), (4, 9));
    assert_eq!(
        outer.excerpt,
        source.lines().take(7).collect::<Vec<_>>().join("\n") + "\n"
    );
    assert_eq!(
        inner.excerpt,
        "// {anchor}`example.inner`<<\n// {anchor}`example.point`\n// >>\n"
    );
    assert_eq!(&source[point.span.start..point.span.end], point.excerpt);
    assert!(point.excerpt.ends_with("// last\n"));
}

#[test]
#[sage::test(meta.anchors.region_balance)]
fn unmatched_regions_fail_and_strings_or_trailing_comments_do_not_close_them() {
    for (source, expected) in [
        ("// >>\n", "closer has no opening"),
        ("// {anchor}`example.open`<<\n", "opener has no closing"),
        (
            "// {anchor}`example.open`<<\nconst TEXT: &str = \"// >>\";\n",
            "opener has no closing",
        ),
        (
            "// {anchor}`example.open`<<\nconst X: u32 = 0; // >>\n",
            "opener has no closing",
        ),
    ] {
        let rust = index(source);
        assert_eq!(
            rust.diagnostics.len(),
            1,
            "{source}: {:?}",
            rust.diagnostics
        );
        assert!(rust.diagnostics[0].message.contains(expected));
        assert_eq!(rust.diagnostics[0].file, "src/lib.rs");
    }
    let rust = index(
        "// {anchor}`example.open`<<\n// >>\nconst TEXT: &str = r#\"// {anchor}`example.fake`<<\"#;\n",
    );
    assert!(rust.diagnostics.is_empty());
    assert_eq!(rust.implementations.len(), 1);
}

#[test]
#[sage::test(meta.anchors.region_balance)]
fn mdbook_excerpt_markers_neither_define_nor_close_canonical_regions() {
    // Split fixture-only excerpt markers so the book's source scanner ignores them.
    let unrelated = index(concat!(
        "// AN",
        "CHOR: example.unclosed\n// AN",
        "CHOR_END: example.other\n"
    ));
    assert!(unrelated.implementations.is_empty());
    assert!(unrelated.diagnostics.is_empty());

    let source = concat!(
        "// {anchor}`example.rule`<<\n// AN",
        "CHOR: explanatory\nconst VALUE: u32 = 4;\n// AN",
        "CHOR_END: explanatory\nconst STILL_INSIDE: u32 = 4;\n",
        "const PAST_POINT_PREVIEW: u32 = 4;\nconst LAST: u32 = 4;\n// >>\n"
    );
    let rust = index(source);
    assert!(rust.diagnostics.is_empty(), "{:?}", rust.diagnostics);
    assert_eq!(rust.implementations.len(), 1);
    let sites = &rust.implementations["example.rule"];
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].excerpt, source);
    assert!(sites[0].excerpt.lines().count() > 6);
    assert!(sites[0].excerpt.contains("STILL_INSIDE"));
    assert!(sites[0].excerpt.ends_with("// >>\n"));
}

#[test]
#[sage::test(meta.anchors.implementation_references)]
fn malformed_reference_names_and_suffixes_are_errors() {
    for text in [
        "// {anchor}missing",
        "// {anchor} `example.space`",
        "// {anchor}`example..empty`",
        "// {anchor}`example.9number`",
        "// {anchor}`éxample.unicode`",
        "// {anchor}`example.ok` trailing prose",
        "// {anchor}`example.ok` <<",
        "// {anchor}`example.ok`<<<",
        "// {anchor}`example.unclosed",
    ] {
        let rust = index(text);
        assert!(rust.implementations.is_empty(), "accepted {text}");
        assert_eq!(rust.diagnostics.len(), 1, "{text}");
        assert!(
            rust.diagnostics[0]
                .message
                .contains("malformed implementation reference")
        );
    }
}

#[test]
#[sage::test(meta.anchors.placeholder_status)]
#[sage::test(meta.anchors.evidence_status)]
fn implementation_status_uses_module_ancestry_and_preserves_cfg_and_explanations() {
    let source = r#"#![cfg(feature = "root")]
#[cfg(unix)]
mod todo {
    mod nested {
        // {anchor}`example.mixed`<<
        // TODO: Connect the runtime checker.
        fn checker() {}
        // >>
    }
}
mod not_applicable {
    // {anchor}`example.mixed`
    // This rule describes authoring practice, not runtime behavior.
}
mod empty { mod todo {} mod not_applicable {} }
#[cfg(feature = "operation")]
fn todo() {
    // {anchor}`example.mixed`
}
"#;
    let rust = index(source);
    assert!(rust.diagnostics.is_empty(), "{:?}", rust.diagnostics);
    let sites = &rust.implementations["example.mixed"];
    assert_eq!(sites.len(), 3, "empty modules declare no entries");
    assert_eq!(sites[0].module, "fixture::todo::nested");
    assert_eq!(sites[0].status, EvidenceStatus::Todo);
    assert_eq!(
        sites[0].cfg,
        ["#![cfg(feature = \"root\")]", "#[cfg(unix)]"]
    );
    assert_eq!(
        sites[0].explanation.as_deref(),
        Some("TODO: Connect the runtime checker.")
    );
    assert_eq!(sites[1].status, EvidenceStatus::NotApplicable);
    assert_eq!(
        sites[1].explanation.as_deref(),
        Some("This rule describes authoring practice, not runtime behavior.")
    );
    assert_eq!(
        sites[2].status,
        EvidenceStatus::Ordinary,
        "a function named todo is not a placeholder module"
    );
    assert_eq!(sites[2].module, "fixture");
    assert_eq!(
        sites[2].cfg,
        [
            "#![cfg(feature = \"root\")]",
            "#[cfg(feature = \"operation\")]"
        ]
    );
    assert!(rust.tests.is_empty());
}

#[test]
#[sage::test(meta.anchors.placeholder_status)]
fn conflicting_placeholder_modules_are_rejected_and_test_explanations_skip_strings() {
    let rust = index("mod todo { mod not_applicable { // {anchor}`example.conflict`\n } }");
    assert_eq!(rust.diagnostics.len(), 1);
    assert!(
        rust.diagnostics[0]
            .message
            .contains("both todo and not_applicable")
    );
    assert!(placeholder_status("fixture::not_applicable::todo").is_err());
    assert_eq!(
        placeholder_status("todo::ordinary"),
        Ok(EvidenceStatus::Ordinary)
    );
    assert_eq!(
        placeholder_status("fixture::r#todo"),
        Ok(EvidenceStatus::Todo)
    );
    assert_eq!(
        test_explanation(
            "#[test] fn unfinished() {\n // TODO: Implement this check.\n let text = r#\"// fake explanation\"#;\n}"
        ),
        Some("TODO: Implement this check.".into())
    );
}

#[test]
#[sage::test(meta.anchors.placeholder_status)]
fn adjacent_points_keep_their_own_explanations_even_when_previews_overlap() {
    let rust = index(
        "mod todo {\n// {anchor}`example.first`\n// TODO: First requirement.\n\n// {anchor}`example.second`\n// TODO: Second requirement.\n}\n",
    );
    assert!(rust.diagnostics.is_empty());
    let first = &rust.implementations["example.first"][0];
    let second = &rust.implementations["example.second"][0];
    assert!(first.excerpt.contains("TODO: Second requirement."));
    assert_eq!(
        first.explanation.as_deref(),
        Some("TODO: First requirement.")
    );
    assert_eq!(
        second.explanation.as_deref(),
        Some("TODO: Second requirement.")
    );
}

#[test]
#[sage::test(meta.anchors.implementation_references)]
fn repeated_references_keep_distinct_crates_and_targets() {
    let sources = [
        RustSource {
            crate_name: "first".into(),
            target: "lib:first".into(),
            file: "first/lib.rs".into(),
            source: "// {anchor}`example.shared`\n".into(),
        },
        RustSource {
            crate_name: "second".into(),
            target: "bin:runner".into(),
            file: "second/main.rs".into(),
            source: "// {anchor}`example.shared`\n// {anchor}`example.shared`\n".into(),
        },
    ];
    let rust = CorrespondenceIndex::from_sources(&sources).unwrap();
    let sites = &rust.implementations["example.shared"];
    assert!(rust.diagnostics.is_empty());
    assert_eq!(sites.len(), 3);
    assert_eq!(sites[0].crate_name, "first");
    assert_eq!(sites[0].target, "lib:first");
    assert_eq!(sites[1].crate_name, "second");
    assert_eq!(sites[1].target, "bin:runner");
    assert_ne!(sites[1].annotation_span, sites[2].annotation_span);
    assert_eq!(rust.files["second/main.rs"], sources[1].source);
}

#[test]
#[sage::test(meta.anchors.implementation_references)]
#[sage::test(meta.anchors.evidence_status)]
fn cargo_owned_out_of_line_sources_retain_inherited_module_and_cfg() {
    let directory =
        std::env::temp_dir().join(format!("sage-evidence-owned-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(directory.join("src")).unwrap();
    std::fs::write(directory.join("Cargo.toml"), "[package]\nname = \"evidence-fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[workspace]\n").unwrap();
    std::fs::write(directory.join("src/lib.rs"), "#[cfg(any())]\nmod todo;\n").unwrap();
    std::fs::write(
        directory.join("src/todo.rs"),
        "// {anchor}`example.pending`\n// TODO: Add the implementation.\n",
    )
    .unwrap();
    std::fs::write(
        directory.join("src/unowned.rs"),
        "// {anchor}`example.unowned`\n",
    )
    .unwrap();
    let rust = CorrespondenceIndex::load(&directory).unwrap();
    std::fs::remove_dir_all(&directory).unwrap();
    assert!(rust.diagnostics.is_empty(), "{:?}", rust.diagnostics);
    assert_eq!(rust.implementations.len(), 1);
    let site = &rust.implementations["example.pending"][0];
    assert_eq!(site.file, "src/todo.rs");
    assert_eq!(site.module, "evidence_fixture::todo");
    assert_eq!(site.status, EvidenceStatus::Todo);
    assert_eq!(site.cfg, ["#[cfg(any())]"]);
    assert_eq!(
        site.explanation.as_deref(),
        Some("TODO: Add the implementation.")
    );
    assert_eq!(site.excerpt, rust.files["src/todo.rs"]);
}
