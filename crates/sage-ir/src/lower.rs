//! Lower tree-sitter CST nodes into salsa tracked IR structs.

use tree_sitter::Node;

use crate::Db;
use crate::item::*;
use crate::name::Name;
use crate::source::SourceFile;
use crate::span::{SpanIndices, SpanTable};
use crate::types::*;

/// Parse a source file and return its top-level items.
#[salsa::tracked(returns(ref))]
pub fn file_item_tree<'db>(db: &'db dyn Db, file: SourceFile) -> Vec<Item<'db>> {
    let text = file.text(db);
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("failed to set tree-sitter-rust language");
    let tree = parser.parse(text, None).expect("tree-sitter parse failed");
    let mut cx = LowerCtx { db, file, text };
    cx.lower_items(tree.root_node())
}

struct LowerCtx<'db> {
    db: &'db dyn Db,
    file: SourceFile,
    text: &'db str,
}

impl<'db> LowerCtx<'db> {
    fn node_text(&self, node: Node<'_>) -> &'db str {
        &self.text[node.byte_range()]
    }

    fn intern_name(&self, node: Node<'_>) -> Name<'db> {
        Name::new(self.db, self.node_text(node).to_owned())
    }

    fn span(&self, node: Node<'_>) -> SpanIndices {
        SpanIndices {
            start: node.start_byte() as u32,
            end: node.end_byte() as u32,
        }
    }

    fn make_span_table(&self, node: Node<'_>) -> SpanTable<'db> {
        SpanTable::new(
            self.db,
            self.file,
            vec![node.start_byte() as u32, node.end_byte() as u32],
        )
    }

    // -- Items -------------------------------------------------------------

    fn lower_items(&mut self, parent: Node<'_>) -> Vec<Item<'db>> {
        let mut items = Vec::new();
        let mut cursor = parent.walk();
        for child in parent.children(&mut cursor) {
            // Skip non-item syntax (comments, attributes, punctuation, etc.)
            if !child.is_named() || is_non_item_node(child.kind()) {
                continue;
            }
            items.push(self.lower_item(child));
        }
        items
    }

    fn lower_item(&mut self, node: Node<'_>) -> Item<'db> {
        match node.kind() {
            "function_item" => Item::Function(self.lower_function(node)),
            "struct_item" => Item::Struct(self.lower_struct(node)),
            "enum_item" => Item::Enum(self.lower_enum(node)),
            "trait_item" => Item::Trait(self.lower_trait(node)),
            "impl_item" => Item::Impl(self.lower_impl(node)),
            "type_item" => Item::TypeAlias(self.lower_type_alias(node)),
            "const_item" => Item::Const(self.lower_const(node)),
            "static_item" => Item::Static(self.lower_static(node)),
            "mod_item" => Item::Mod(self.lower_mod(node)),
            "use_declaration" => self.lower_use(node),
            _ => Item::Error(self.span(node)),
        }
    }

    fn lower_function(&mut self, node: Node<'_>) -> FunctionItem<'db> {
        let name_node = node
            .child_by_field_name("name")
            .expect("function has no name");
        let name = self.intern_name(name_node);
        let span = self.span(node);
        let span_table = self.make_span_table(node);

        let is_async = node.children(&mut node.walk()).any(|c| c.kind() == "async");
        let is_unsafe = node
            .children(&mut node.walk())
            .any(|c| c.kind() == "unsafe");

        let params = node
            .child_by_field_name("parameters")
            .map(|p| self.lower_params(p))
            .unwrap_or_default();

        let ret_type = node
            .child_by_field_name("return_type")
            .map(|n| self.lower_type(n));

        FunctionItem::new(
            self.db, name, params, ret_type, is_async, is_unsafe, span_table, span,
        )
    }

    fn lower_params(&mut self, params_node: Node<'_>) -> Vec<Param<'db>> {
        let mut params = Vec::new();
        let mut cursor = params_node.walk();
        for child in params_node.children(&mut cursor) {
            match child.kind() {
                "parameter" => {
                    let name = child
                        .child_by_field_name("pattern")
                        .map(|n| self.intern_name(n));
                    let ty = child
                        .child_by_field_name("type")
                        .map(|n| self.lower_type(n))
                        .unwrap_or_else(|| self.make_error_type(child));
                    params.push(Param::new(self.db, name, ty, self.span(child)));
                }
                "self_parameter" => {
                    let name = Some(Name::new(self.db, "self".to_owned()));
                    let has_ref = child.children(&mut child.walk()).any(|c| c.kind() == "&");
                    let is_mut = child
                        .children(&mut child.walk())
                        .any(|c| c.kind() == "mutable_specifier");
                    let self_path = Path::new(
                        self.db,
                        vec![Name::new(self.db, "Self".to_owned())],
                        self.span(child),
                    );
                    let self_ty =
                        TypeRef::new(self.db, TypeRefKind::Path(self_path), self.span(child));
                    let ty = if has_ref {
                        let m = if is_mut {
                            Mutability::Mut
                        } else {
                            Mutability::Shared
                        };
                        TypeRef::new(
                            self.db,
                            TypeRefKind::Reference(self_ty, m),
                            self.span(child),
                        )
                    } else {
                        self_ty
                    };
                    params.push(Param::new(self.db, name, ty, self.span(child)));
                }
                _ => {}
            }
        }
        params
    }

    fn lower_struct(&mut self, node: Node<'_>) -> StructItem<'db> {
        let name = self.intern_name(
            node.child_by_field_name("name")
                .expect("struct has no name"),
        );
        let span = self.span(node);
        let span_table = self.make_span_table(node);

        let fields = node
            .child_by_field_name("body")
            .map(|body| self.lower_field_defs(body))
            .unwrap_or_default();

        StructItem::new(self.db, name, fields, span_table, span)
    }

    fn lower_field_defs(&mut self, body: Node<'_>) -> Vec<FieldDef<'db>> {
        let mut fields = Vec::new();
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if child.kind() == "field_declaration" {
                let name = child
                    .child_by_field_name("name")
                    .map(|n| self.intern_name(n))
                    .unwrap_or_else(|| Name::new(self.db, "?".to_owned()));
                let ty = child
                    .child_by_field_name("type")
                    .map(|n| self.lower_type(n))
                    .unwrap_or_else(|| self.make_error_type(child));
                fields.push(FieldDef::new(self.db, name, ty, self.span(child)));
            }
        }
        fields
    }

    fn lower_enum(&mut self, node: Node<'_>) -> EnumItem<'db> {
        let name = self.intern_name(node.child_by_field_name("name").expect("enum has no name"));
        let span = self.span(node);
        let span_table = self.make_span_table(node);

        let mut variants = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if child.kind() == "enum_variant" {
                    let vname = child
                        .child_by_field_name("name")
                        .map(|n| self.intern_name(n))
                        .unwrap_or_else(|| Name::new(self.db, "?".to_owned()));
                    let fields = child
                        .child_by_field_name("body")
                        .map(|b| self.lower_field_defs(b))
                        .unwrap_or_default();
                    variants.push(VariantDef::new(self.db, vname, fields, self.span(child)));
                }
            }
        }

        EnumItem::new(self.db, name, variants, span_table, span)
    }

    fn lower_trait(&mut self, node: Node<'_>) -> TraitItem<'db> {
        let name = self.intern_name(node.child_by_field_name("name").expect("trait has no name"));
        let span = self.span(node);
        let span_table = self.make_span_table(node);

        let items = node
            .child_by_field_name("body")
            .map(|body| self.lower_items(body))
            .unwrap_or_default();

        TraitItem::new(self.db, name, items, span_table, span)
    }

    fn lower_impl(&mut self, node: Node<'_>) -> ImplItem<'db> {
        let span = self.span(node);
        let span_table = self.make_span_table(node);

        let self_ty = node
            .child_by_field_name("type")
            .map(|n| self.lower_type(n))
            .unwrap_or_else(|| self.make_error_type(node));

        let trait_path = node
            .child_by_field_name("trait")
            .map(|n| self.lower_path(n));

        let items = node
            .child_by_field_name("body")
            .map(|body| self.lower_items(body))
            .unwrap_or_default();

        ImplItem::new(self.db, self_ty, trait_path, items, span_table, span)
    }

    fn lower_type_alias(&mut self, node: Node<'_>) -> TypeAliasItem<'db> {
        let name = self.intern_name(
            node.child_by_field_name("name")
                .expect("type alias has no name"),
        );
        let span = self.span(node);
        let span_table = self.make_span_table(node);
        let ty = node.child_by_field_name("type").map(|n| self.lower_type(n));
        TypeAliasItem::new(self.db, name, ty, span_table, span)
    }

    fn lower_const(&mut self, node: Node<'_>) -> ConstItem<'db> {
        let name = self.intern_name(node.child_by_field_name("name").expect("const has no name"));
        let span = self.span(node);
        let span_table = self.make_span_table(node);
        let ty = node.child_by_field_name("type").map(|n| self.lower_type(n));
        ConstItem::new(self.db, name, ty, span_table, span)
    }

    fn lower_static(&mut self, node: Node<'_>) -> StaticItem<'db> {
        let name = self.intern_name(
            node.child_by_field_name("name")
                .expect("static has no name"),
        );
        let span = self.span(node);
        let span_table = self.make_span_table(node);
        let ty = node.child_by_field_name("type").map(|n| self.lower_type(n));
        let is_mut = node
            .children(&mut node.walk())
            .any(|c| c.kind() == "mutable_specifier");
        StaticItem::new(self.db, name, ty, is_mut, span_table, span)
    }

    fn lower_mod(&mut self, node: Node<'_>) -> ModItem<'db> {
        let name = self.intern_name(node.child_by_field_name("name").expect("mod has no name"));
        let span = self.span(node);
        let span_table = self.make_span_table(node);
        let items = node
            .child_by_field_name("body")
            .map(|body| self.lower_items(body));
        ModItem::new(self.db, name, items, span_table, span)
    }

    fn lower_use(&mut self, node: Node<'_>) -> Item<'db> {
        let span = self.span(node);
        let span_table = self.make_span_table(node);
        let text = self.node_text(node);
        let text = text.trim_start_matches("use ").trim_end_matches(';').trim();
        let path = Path::new(self.db, vec![Name::new(self.db, text.to_owned())], span);
        Item::Use(UseItem::new(self.db, path, None, span_table, span))
    }

    // -- Types -------------------------------------------------------------

    fn lower_type(&mut self, node: Node<'_>) -> TypeRef<'db> {
        let span = self.span(node);
        let kind = match node.kind() {
            "type_identifier"
            | "scoped_type_identifier"
            | "generic_type"
            | "scoped_identifier"
            | "primitive_type"
            | "abstract_type"
            | "dynamic_type"
            | "function_type"
            | "macro_invocation"
            | "bounded_type"
            | "qualified_type"
            | "pointer_type"
            | "empty_type"
            | "metavariable" => TypeRefKind::Path(self.lower_path(node)),
            "reference_type" => {
                let is_mut = node
                    .children(&mut node.walk())
                    .any(|c| c.kind() == "mutable_specifier");
                let inner = node
                    .child_by_field_name("type")
                    .map(|n| self.lower_type(n))
                    .unwrap_or_else(|| self.make_error_type(node));
                let mutability = if is_mut {
                    Mutability::Mut
                } else {
                    Mutability::Shared
                };
                TypeRefKind::Reference(inner, mutability)
            }
            "array_type" => {
                let inner = node
                    .child_by_field_name("element")
                    .map(|n| self.lower_type(n))
                    .unwrap_or_else(|| self.make_error_type(node));
                TypeRefKind::Array(inner)
            }
            "slice_type" => {
                let inner = node
                    .child_by_field_name("element")
                    .map(|n| self.lower_type(n))
                    .unwrap_or_else(|| self.make_error_type(node));
                TypeRefKind::Slice(inner)
            }
            "tuple_type" => {
                let mut elems = Vec::new();
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.is_named() && child.kind() != "(" && child.kind() != ")" {
                        elems.push(self.lower_type(child));
                    }
                }
                TypeRefKind::Tuple(TupleTypeRef::new(self.db, elems))
            }
            "never_type" => TypeRefKind::Never,
            "inferred_type" => TypeRefKind::Infer,
            _ => TypeRefKind::Error,
        };
        TypeRef::new(self.db, kind, span)
    }

    fn lower_path(&self, node: Node<'_>) -> Path<'db> {
        // For now, capture the full text as a single segment.
        // A proper implementation would walk scoped_type_identifier etc.
        let text = self.node_text(node);
        Path::new(
            self.db,
            vec![Name::new(self.db, text.to_owned())],
            self.span(node),
        )
    }

    fn make_error_type(&self, node: Node<'_>) -> TypeRef<'db> {
        TypeRef::new(self.db, TypeRefKind::Error, self.span(node))
    }
}

/// Nodes that appear at item level but aren't items themselves.
fn is_non_item_node(kind: &str) -> bool {
    matches!(
        kind,
        "line_comment"
            | "block_comment"
            | "attribute_item"
            | "inner_attribute_item"
            | "expression_statement"
            | "empty_statement"
    )
}
