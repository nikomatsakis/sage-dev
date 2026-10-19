#![feature(rustc_private)]
#![feature(register_tool)]
#![register_tool(sage)]
use expect_test::expect;
use indoc::indoc;
use sage_test_harness::compile_test;

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.expansion_import_ambiguity)]
fn glob_root_ignores_conflicting_value_namespace_bindings() {
    compile_test! {
        source: indoc! {r#"
            mod seed { pub fn path() {} }
            use seed::*;
            mod path { pub struct Item; }
            use path::*;
            macro_rules! build { () => { fn path() {} }; }
            build!();
            fn example(_: Item) {}
        "#},
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.precedence)]
#[sage::test(crates.sage_ir.resolution.lookup.expansion_import_ambiguity)]
fn generated_module_competes_with_glob_import_path() {
    // The imported contents are unused: validating the import itself must
    // reject the expansion-sensitive ambiguity in both compilers.
    compile_test! {
        source: indoc! {r#"
            mod first {
                pub struct Item { pub value: u16 }
            }
            mod second {
                pub struct Item { pub value: bool }
            }
            mod seed { pub(crate) use crate::first as path; }
            use seed::*;
            use path::*; //# ERROR
            macro_rules! build { () => { mod path { pub(crate) use crate::second::Item; } }; }
            build!();
        "#},
        diagnostics: expect![[r#"
            error: ambiguous import: macro-generated binding conflicts with glob import
             --> src/lib.rs:9:5
              |
            9 | use path::*; //# ERROR
              |     ^^^^^^^"#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.precedence)]
#[sage::test(crates.sage_ir.resolution.lookup.expansion_import_ambiguity)]
fn qualified_glob_path_selects_generated_module() {
    compile_test! {
        source: indoc! {r#"
            mod first {
                pub struct Item { pub value: u16 }
            }
            mod second {
                pub struct Item { pub value: bool }
            }
            mod seed { pub(crate) use crate::first as path; }
            use seed::*;
            use crate::path::*;
            macro_rules! build { () => { mod path { pub(crate) use crate::second::Item; } }; }
            build!();
            fn example(item: Item) -> bool { item.value }
        "#},
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.precedence)]
#[sage::test(crates.sage_ir.resolution.lookup.expansion_import_ambiguity)]
fn source_written_module_takes_precedence_over_glob() {
    compile_test! {
        source: indoc! {r#"
            mod first {
                pub struct Item { pub value: u16 }
            }
            mod second {
                pub struct Item { pub value: bool }
            }
            mod seed { pub(crate) use crate::first as path; }
            use seed::*;
            use path::*;
            mod path { pub(crate) use crate::second::Item; }
            fn example(item: Item) -> bool { item.value }
        "#},
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.expansion_import_ambiguity)]
fn generated_named_import_competes_with_glob_root() {
    compile_test! {
        source: indoc! {r#"
            mod first { pub struct Item { pub value: u16 } }
            mod second { pub struct Item { pub value: bool } }
            mod seed { pub(crate) use crate::first as path; }
            use seed::*;
            use path::*; //# ERROR
            macro_rules! build { () => { use crate::second as path; }; }
            build!();
        "#},
        diagnostics: expect![[r#"
            error: ambiguous import: macro-generated binding conflicts with glob import
             --> src/lib.rs:5:5
              |
            5 | use path::*; //# ERROR
              |     ^^^^^^^"#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.expansion_import_ambiguity)]
fn generated_import_and_binding_in_same_expansion_conflict() {
    compile_test! {
        source: indoc! {r#"
            mod first { pub struct Item { pub value: u16 } }
            mod second { pub struct Item { pub value: bool } }
            mod seed { pub(crate) use crate::first as path; }
            use seed::*;
            macro_rules! build { () => {
                mod path { pub(crate) use crate::second::Item; }
                use path::*;
            }; }
            build!(); //# ERROR
        "#},
        diagnostics: expect![[r#"
            error: ambiguous import: macro-generated binding conflicts with glob import
             --> src/lib.rs:9:1
              |
            9 | build!(); //# ERROR
              | ^^^^^^^^"#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.expansion_import_ambiguity)]
fn generated_named_import_and_glob_can_reach_same_definition() {
    compile_test! {
        source: indoc! {r#"
            mod first { pub struct Item { pub value: u16 } }
            mod second { pub struct Item { pub value: bool } }
            mod seed { pub(crate) use crate::first as path; }
            use seed::*;
            use path::*;
            macro_rules! build { () => { use crate::first as path; }; }
            build!();
            fn example(item: Item) -> u16 { item.value }
        "#},
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.expansion_import_ambiguity)]
fn ambiguous_root_of_longer_import_is_rejected() {
    compile_test! {
        source: indoc! {r#"
            mod first { pub struct Item { pub value: u16 } }
            mod second { pub struct Item { pub value: bool } }
            mod seed { pub(crate) use crate::first as path; }
            use seed::*;
            use path::Item; //# ERROR
            macro_rules! build { () => { mod path { pub(crate) use crate::second::Item; } }; }
            build!();
        "#},
        diagnostics: expect![[r#"
            error: ambiguous import: macro-generated binding conflicts with glob import
             --> src/lib.rs:5:5
              |
            5 | use path::Item; //# ERROR
              |     ^^^^^^^^^^"#]],
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.expansion_import_ambiguity)]
fn source_written_alias_to_generated_module_keeps_precedence() {
    compile_test! {
        source: indoc! {r#"
            mod first { pub struct Item { pub value: u16 } }
            mod second { pub struct Item { pub value: bool } }
            mod seed { pub(crate) use crate::first as path; }
            use seed::*;
            use path::*;
            use crate::generated as path;
            macro_rules! build { () => { mod generated { pub(crate) use crate::second::Item; } }; }
            build!();
            fn example(item: Item) -> bool { item.value }
        "#},
    }
}

#[test]
#[sage::test(crates.sage_ir.resolution.lookup.expansion_import_ambiguity)]
fn nested_expansion_competes_with_glob_root() {
    compile_test! {
        source: indoc! {r#"
            mod first { pub struct Item { pub value: u16 } }
            mod second { pub struct Item { pub value: bool } }
            mod seed { pub(crate) use crate::first as path; }
            use seed::*;
            use path::*; //# ERROR
            macro_rules! inner { () => { mod path { pub(crate) use crate::second::Item; } }; }
            macro_rules! outer { () => { inner!(); }; }
            outer!();
        "#},
        diagnostics: expect![[r#"
            error: ambiguous import: macro-generated binding conflicts with glob import
             --> src/lib.rs:5:5
              |
            5 | use path::*; //# ERROR
              |     ^^^^^^^"#]],
    }
}
