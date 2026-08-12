mod attrs;
mod exprs;
mod generics;
mod items;
mod paths;
mod types;
mod util;

use crate::Db;
use crate::local_syms::LocalModItemSym;
use crate::scope::ScopeSymbol;
use crate::span::ParseSource;

pub(crate) struct Parser<'a, 'db> {
    pub db: &'db dyn Db,
    pub source: ParseSource<'db>,
    pub scope: ScopeSymbol<'db>,
    pub text: &'a str,
}

// ANCHOR: architecture_parse_module_entry
pub fn parse_str_to_cst<'db>(
    db: &'db dyn Db,
    source: ParseSource<'db>,
    text: &str,
    scope: ScopeSymbol<'db>,
) -> Vec<LocalModItemSym<'db>> {
    let parser = Parser {
        db,
        source,
        scope,
        text,
    };
    let tree = tree_sitter_parse(text);
    parser.parse_item_list(tree.root_node())
}
// ANCHOR_END: architecture_parse_module_entry

fn tree_sitter_parse(text: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    let language = tree_sitter_rust::LANGUAGE;
    parser
        .set_language(&language.into())
        .expect("failed to set tree-sitter language");
    parser.parse(text, None).expect("tree-sitter parse failed")
}

impl<'a, 'db> Parser<'a, 'db> {
    // ANCHOR: architecture_parse_item_dispatch
    pub(crate) fn parse_item_list(
        &self,
        list_node: tree_sitter::Node<'a>,
    ) -> Vec<LocalModItemSym<'db>> {
        let mut items = Vec::new();
        let mut pending_attrs: Vec<tree_sitter::Node<'a>> = Vec::new();
        let mut cursor = list_node.walk();

        for child in list_node.children(&mut cursor) {
            match child.kind() {
                "attribute_item" => {
                    pending_attrs.push(child);
                }
                "inner_attribute_item" if self.inner_attr_is_known_inert(child) => {
                    // Lint-only module attributes do not affect expansion or
                    // candidate completeness. Other inner attributes remain
                    // explicit errors until module attributes are represented.
                }
                "function_item" | "function_signature_item" => {
                    items.push(self.parse_fn(child, &pending_attrs));
                    pending_attrs.clear();
                }
                "struct_item" => {
                    items.push(self.parse_struct(child, &pending_attrs));
                    pending_attrs.clear();
                }
                "enum_item" => {
                    items.push(self.parse_enum(child, &pending_attrs));
                    pending_attrs.clear();
                }
                "trait_item" => {
                    items.push(self.parse_trait(child, &pending_attrs));
                    pending_attrs.clear();
                }
                "impl_item" => {
                    items.push(self.parse_impl(child, &pending_attrs));
                    pending_attrs.clear();
                }
                "mod_item" => {
                    items.push(self.parse_mod(child, &pending_attrs));
                    pending_attrs.clear();
                }
                "use_declaration" => {
                    items.push(self.parse_use(child, &pending_attrs));
                    pending_attrs.clear();
                }
                "const_item" => {
                    items.push(self.parse_const(child, &pending_attrs));
                    pending_attrs.clear();
                }
                "static_item" => {
                    items.push(self.parse_static(child, &pending_attrs));
                    pending_attrs.clear();
                }
                "type_item" => {
                    items.push(self.parse_type_alias(child, &pending_attrs));
                    pending_attrs.clear();
                }
                "macro_definition" => {
                    items.push(self.parse_macro_def(child, &pending_attrs));
                    pending_attrs.clear();
                }
                "expression_statement" => {
                    if let Some(item) = self.try_parse_macro_invocation(child, &pending_attrs) {
                        items.push(item);
                    } else {
                        items.push(self.error_item(child, &pending_attrs));
                    }
                    pending_attrs.clear();
                }
                "macro_invocation" => {
                    items.push(self.parse_macro_invocation_node(child, &pending_attrs));
                    pending_attrs.clear();
                }
                "line_comment" | "block_comment" | "{" | "}" | ";" => {
                    // Comments and delimiters do not consume pending attrs.
                }
                "ERROR" => {
                    items.push(self.error_item(child, &pending_attrs));
                    pending_attrs.clear();
                }
                _ => {
                    items.push(self.error_item(child, &pending_attrs));
                    pending_attrs.clear();
                }
            }
        }
        if let (Some(first), Some(last)) = (pending_attrs.first(), pending_attrs.last()) {
            items.push(LocalModItemSym::Error(crate::span::AbsoluteSpan {
                source: self.source,
                start: first.start_byte() as u32,
                end: last.end_byte() as u32,
            }));
        }
        items
    }
    // ANCHOR_END: architecture_parse_item_dispatch

    fn error_item(
        &self,
        node: tree_sitter::Node<'a>,
        pending_attrs: &[tree_sitter::Node<'a>],
    ) -> LocalModItemSym<'db> {
        let start = util::item_start(node, pending_attrs);
        LocalModItemSym::Error(util::absolute_span(self.source, node, start))
    }
}
