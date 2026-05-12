//! Lower tree-sitter CST nodes into salsa tracked IR structs.

use tree_sitter::Node;

use sage_stash::{Ptr, Stash, Stashed};

use crate::Db;
use crate::body::*;
use crate::item::*;
use crate::name::Name;
use crate::source::SourceFile;
use crate::span::{SpanIndices, SpanTable};
use crate::types::*;

/// Parse a source file and return its top-level items.
#[salsa::tracked(returns(ref))]
pub fn file_item_tree<'db>(db: &'db dyn Db, file: SourceFile) -> Vec<Item<'db>> {
    db.log_query(format!("file_item_tree(\"{}\")", file.path(db)));
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
        let mut pending_attrs = Vec::new();
        let mut cursor = parent.walk();
        for child in parent.children(&mut cursor) {
            if !child.is_named() {
                continue;
            }
            match child.kind() {
                "attribute_item" | "inner_attribute_item" => {
                    pending_attrs.push(self.lower_attr(child));
                }
                "line_comment" => {
                    let text = self.node_text(child);
                    if let Some(doc) = text.strip_prefix("///") {
                        let doc = doc.strip_prefix(' ').unwrap_or(doc).trim_end();
                        pending_attrs.push(self.make_doc_attr(child, doc, false));
                    } else if let Some(doc) = text.strip_prefix("//!") {
                        let doc = doc.strip_prefix(' ').unwrap_or(doc).trim_end();
                        pending_attrs.push(self.make_doc_attr(child, doc, true));
                    }
                    // Regular comments are skipped.
                }
                "block_comment" | "empty_statement" => continue,
                _ => {
                    let attrs = std::mem::take(&mut pending_attrs);
                    items.push(self.lower_item(child, attrs));
                }
            }
        }
        items
    }

    fn lower_attr(&self, node: Node<'_>) -> Attr<'db> {
        let is_inner = node.kind() == "inner_attribute_item";
        let text = self.node_text(node);
        // Strip #[ ] or #![ ]
        let inner = text
            .trim_start_matches("#![")
            .trim_start_matches("#[")
            .trim_end_matches(']')
            .trim();
        // Split into path and args at first '('
        let (path_text, args_text) = match inner.find('(') {
            Some(i) => (&inner[..i], Some(&inner[i..])),
            None => (inner, None),
        };
        let path = Path::new(
            self.db,
            vec![Name::new(self.db, path_text.trim().to_owned())],
            self.span(node),
        );
        let args = args_text.map(|a| TokenTree::new(self.db, a.to_owned(), self.span(node)));
        Attr::new(
            self.db,
            AttrKind::Normal,
            path,
            args,
            self.span(node),
            is_inner,
        )
    }

    fn make_doc_attr(&self, node: Node<'_>, text: &str, is_inner: bool) -> Attr<'db> {
        let path = Path::new(
            self.db,
            vec![Name::new(self.db, "doc".to_owned())],
            self.span(node),
        );
        let args = Some(TokenTree::new(self.db, text.to_owned(), self.span(node)));
        Attr::new(
            self.db,
            AttrKind::DocComment,
            path,
            args,
            self.span(node),
            is_inner,
        )
    }

    fn lower_item(&mut self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> Item<'db> {
        match node.kind() {
            "function_item" => Item::Function(self.lower_function(node, attrs)),
            "struct_item" => Item::Struct(self.lower_struct(node, attrs)),
            "enum_item" => Item::Enum(self.lower_enum(node, attrs)),
            "trait_item" => Item::Trait(self.lower_trait(node, attrs)),
            "impl_item" => Item::Impl(self.lower_impl(node, attrs)),
            "type_item" => Item::TypeAlias(self.lower_type_alias(node, attrs)),
            "const_item" => Item::Const(self.lower_const(node, attrs)),
            "static_item" => Item::Static(self.lower_static(node, attrs)),
            "mod_item" => Item::Mod(self.lower_mod(node, attrs)),
            "use_declaration" => self.lower_use(node, attrs),
            "macro_definition" => self.lower_macro_def_item(node),
            "expression_statement" => self.lower_expression_statement(node),
            _ => Item::Error(self.span(node)),
        }
    }

    fn lower_function(&mut self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> FunctionItem<'db> {
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

        let body = self.lower_body(node);

        FunctionItem::new(
            self.db, name, attrs, params, ret_type, is_async, is_unsafe, body, span_table, span,
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

    fn lower_struct(&mut self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> StructItem<'db> {
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

        StructItem::new(self.db, name, attrs, fields, span_table, span)
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

    fn lower_enum(&mut self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> EnumItem<'db> {
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

        EnumItem::new(self.db, name, attrs, variants, span_table, span)
    }

    fn lower_trait(&mut self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> TraitItem<'db> {
        let name = self.intern_name(node.child_by_field_name("name").expect("trait has no name"));
        let span = self.span(node);
        let span_table = self.make_span_table(node);

        let items = node
            .child_by_field_name("body")
            .map(|body| self.lower_items(body))
            .unwrap_or_default();

        TraitItem::new(self.db, name, attrs, items, span_table, span)
    }

    fn lower_impl(&mut self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> ImplItem<'db> {
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

        ImplItem::new(self.db, attrs, self_ty, trait_path, items, span_table, span)
    }

    fn lower_type_alias(&mut self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> TypeAliasItem<'db> {
        let name = self.intern_name(
            node.child_by_field_name("name")
                .expect("type alias has no name"),
        );
        let span = self.span(node);
        let span_table = self.make_span_table(node);
        let ty = node.child_by_field_name("type").map(|n| self.lower_type(n));
        TypeAliasItem::new(self.db, name, attrs, ty, span_table, span)
    }

    fn lower_const(&mut self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> ConstItem<'db> {
        let name = self.intern_name(node.child_by_field_name("name").expect("const has no name"));
        let span = self.span(node);
        let span_table = self.make_span_table(node);
        let ty = node.child_by_field_name("type").map(|n| self.lower_type(n));
        ConstItem::new(self.db, name, attrs, ty, span_table, span)
    }

    fn lower_static(&mut self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> StaticItem<'db> {
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
        StaticItem::new(self.db, name, attrs, ty, is_mut, span_table, span)
    }

    fn lower_mod(&mut self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> ModItem<'db> {
        let name = self.intern_name(node.child_by_field_name("name").expect("mod has no name"));
        let span = self.span(node);
        let span_table = self.make_span_table(node);
        let items = node
            .child_by_field_name("body")
            .map(|body| self.lower_items(body));
        ModItem::new(self.db, name, attrs, items, span_table, span)
    }

    fn lower_use(&mut self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> Item<'db> {
        let span = self.span(node);
        let span_table = self.make_span_table(node);
        let mut imports = Vec::new();
        // The use tree is the first named child after visibility/`use` keyword.
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "visibility_modifier" => {}
                _ => {
                    let mut prefix = Vec::new();
                    self.flatten_use_tree(child, &mut prefix, &mut imports);
                    break;
                }
            }
        }
        Item::Use(UseGroup::new(self.db, attrs, imports, span_table, span))
    }

    fn flatten_use_tree(
        &self,
        node: Node<'_>,
        prefix: &mut Vec<Name<'db>>,
        out: &mut Vec<UseImport<'db>>,
    ) {
        match node.kind() {
            "scoped_use_list" => {
                // `foo::bar::{A, B}` — extend prefix from "path" child, recurse into "list"
                let saved = prefix.len();
                if let Some(path_node) = node.child_by_field_name("path") {
                    self.collect_path_segments(path_node, prefix);
                }
                if let Some(list) = node.child_by_field_name("list") {
                    self.flatten_use_tree(list, prefix, out);
                }
                prefix.truncate(saved);
            }
            "use_list" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.flatten_use_tree(child, prefix, out);
                }
            }
            "use_as_clause" => {
                // `foo::Bar as Baz` or `foo::Bar as _`
                let mut path_segs = prefix.to_vec();
                if let Some(path_node) = node.child_by_field_name("path") {
                    self.collect_path_segments(path_node, &mut path_segs);
                }
                let alias_node = node.child_by_field_name("alias");
                let alias_text = alias_node.map(|n| self.node_text(n));
                let kind = match alias_text {
                    Some("_") => UseKind::Unnamed,
                    Some(t) => UseKind::Named(Name::new(self.db, t.to_owned())),
                    None => {
                        let last = path_segs
                            .last()
                            .copied()
                            .unwrap_or_else(|| Name::new(self.db, "?".to_owned()));
                        UseKind::Named(last)
                    }
                };
                let path = Path::new(self.db, path_segs, self.span(node));
                out.push(UseImport::new(self.db, path, kind, self.span(node)));
            }
            "use_wildcard" => {
                // `foo::*` — the scoped part is in a child "path" if present
                let mut path_segs = prefix.to_vec();
                // In tree-sitter-rust 0.24, use_wildcard's path child has no
                // field name. Find the first named child that isn't `::` or `*`.
                let path_node = node.named_children(&mut node.walk()).find(|c| {
                    matches!(
                        c.kind(),
                        "identifier" | "scoped_identifier" | "self" | "crate" | "super"
                    )
                });
                if let Some(pn) = path_node {
                    self.collect_path_segments(pn, &mut path_segs);
                }
                let path = Path::new(self.db, path_segs, self.span(node));
                out.push(UseImport::new(
                    self.db,
                    path,
                    UseKind::Glob,
                    self.span(node),
                ));
            }
            "scoped_identifier" => {
                // `foo::Bar` as a leaf in a use tree
                let mut path_segs = prefix.to_vec();
                self.collect_path_segments(node, &mut path_segs);
                let last = path_segs
                    .last()
                    .copied()
                    .unwrap_or_else(|| Name::new(self.db, "?".to_owned()));
                let path = Path::new(self.db, path_segs, self.span(node));
                out.push(UseImport::new(
                    self.db,
                    path,
                    UseKind::Named(last),
                    self.span(node),
                ));
            }
            "identifier" | "type_identifier" => {
                let name = self.intern_name(node);
                let mut path_segs = prefix.to_vec();
                path_segs.push(name);
                let path = Path::new(self.db, path_segs, self.span(node));
                out.push(UseImport::new(
                    self.db,
                    path,
                    UseKind::Named(name),
                    self.span(node),
                ));
            }
            "self" => {
                // `use foo::{self}` → imports the module itself under its name
                let path_segs = prefix.to_vec();
                let alias = prefix
                    .last()
                    .copied()
                    .unwrap_or_else(|| Name::new(self.db, "self".to_owned()));
                let path = Path::new(self.db, path_segs, self.span(node));
                out.push(UseImport::new(
                    self.db,
                    path,
                    UseKind::Named(alias),
                    self.span(node),
                ));
            }
            "crate" | "super" => {
                let name = Name::new(self.db, node.kind().to_owned());
                let mut path_segs = prefix.to_vec();
                path_segs.push(name);
                let path = Path::new(self.db, path_segs, self.span(node));
                out.push(UseImport::new(
                    self.db,
                    path,
                    UseKind::Named(name),
                    self.span(node),
                ));
            }
            _ => {
                // Fallback: treat as single identifier
                let name = Name::new(self.db, self.node_text(node).to_owned());
                let mut path_segs = prefix.to_vec();
                path_segs.push(name);
                let path = Path::new(self.db, path_segs, self.span(node));
                out.push(UseImport::new(
                    self.db,
                    path,
                    UseKind::Named(name),
                    self.span(node),
                ));
            }
        }
    }

    // -- Macros -------------------------------------------------------------

    fn lower_macro_def_item(&self, node: Node<'_>) -> Item<'db> {
        let name_node = match node.child_by_field_name("name") {
            Some(n) => n,
            None => return Item::Error(self.span(node)),
        };
        let name = self.intern_name(name_node);
        let span = self.span(node);
        let body_tokens = crate::ts_helpers::extract_macro_body_tokens(node, self.text);

        Item::MacroDef(MacroDefItem::new(self.db, name, body_tokens, span))
    }

    fn lower_expression_statement(&self, node: Node<'_>) -> Item<'db> {
        // Item-level macro invocation: expression_statement > macro_invocation
        let invoc = node
            .named_children(&mut node.walk())
            .find(|c| c.kind() == "macro_invocation");
        match invoc {
            Some(invoc_node) => self.lower_macro_invocation_item(invoc_node),
            None => Item::Error(self.span(node)),
        }
    }

    fn lower_macro_invocation_item(&self, node: Node<'_>) -> Item<'db> {
        let macro_node = match node.child_by_field_name("macro") {
            Some(n) => n,
            None => return Item::Error(self.span(node)),
        };
        let segments =
            crate::ts_helpers::collect_macro_path_segments(self.db, macro_node, self.text);
        if segments.is_empty() {
            return Item::Error(self.span(node));
        }
        let span = self.span(node);
        let path = Path::new(self.db, segments, span);
        let input_tokens = crate::ts_helpers::extract_macro_invocation_tokens(node, self.text);
        Item::MacroInvocation(MacroInvocationItem::new(self.db, path, input_tokens, span))
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
        let mut segments = Vec::new();
        self.collect_path_segments(node, &mut segments);
        Path::new(self.db, segments, self.span(node))
    }

    fn collect_path_segments(&self, node: Node<'_>, out: &mut Vec<Name<'db>>) {
        match node.kind() {
            "scoped_identifier" | "scoped_type_identifier" => {
                if let Some(prefix) = node.child_by_field_name("path") {
                    self.collect_path_segments(prefix, out);
                } else if node.child(0).is_some_and(|c| c.kind() == "::") {
                    // Leading `::` — emit empty sentinel for absolute path
                    out.push(Name::new(self.db, String::new()));
                }
                if let Some(name) = node.child_by_field_name("name") {
                    out.push(self.intern_name(name));
                }
            }
            "generic_type" => {
                // Recurse into the type child, ignore type_arguments
                if let Some(ty) = node.child_by_field_name("type") {
                    self.collect_path_segments(ty, out);
                }
            }
            "identifier" | "type_identifier" | "primitive_type" | "metavariable" => {
                out.push(self.intern_name(node));
            }
            "self" => out.push(Name::new(self.db, "self".to_owned())),
            "crate" => out.push(Name::new(self.db, "crate".to_owned())),
            "super" => out.push(Name::new(self.db, "super".to_owned())),
            _ => {
                // Fallback: use full text as single segment
                out.push(Name::new(self.db, self.node_text(node).to_owned()));
            }
        }
    }

    fn make_error_type(&self, node: Node<'_>) -> TypeRef<'db> {
        TypeRef::new(self.db, TypeRefKind::Error, self.span(node))
    }

    // -- Body lowering -----------------------------------------------------

    fn lower_body(&self, fn_node: Node<'_>) -> FunctionBody<'db> {
        let mut bcx = BodyLowerCtx::new(self.db, self.text);
        let body_node = fn_node.child_by_field_name("body");
        let root_expr = body_node
            .map(|n| bcx.lower_expr(n))
            .unwrap_or_else(|| bcx.missing_expr(fn_node));
        let body = bcx.stash.alloc(Body {
            root: root_expr,
            span: self.span(fn_node),
        });
        Stashed::new(bcx.stash, body)
    }
}

// ===========================================================================
// Body lowering — expressions, statements, patterns into a Stash
// ===========================================================================

struct BodyLowerCtx<'db> {
    db: &'db dyn Db,
    text: &'db str,
    stash: Stash,
}

impl<'db> BodyLowerCtx<'db> {
    fn new(db: &'db dyn Db, text: &'db str) -> Self {
        Self {
            db,
            text,
            stash: Stash::new(),
        }
    }

    fn node_text(&self, node: Node<'_>) -> &'db str {
        &self.text[node.byte_range()]
    }

    fn span(&self, node: Node<'_>) -> SpanIndices {
        SpanIndices {
            start: node.start_byte() as u32,
            end: node.end_byte() as u32,
        }
    }

    fn name(&self, node: Node<'_>) -> Name<'db> {
        Name::new(self.db, self.node_text(node).to_owned())
    }

    fn missing_expr(&mut self, node: Node<'_>) -> Ptr<Expr<'db>> {
        self.stash.alloc(Expr {
            kind: ExprKind::Missing,
            span: self.span(node),
        })
    }

    fn lower_path(&self, node: Node<'_>) -> Path<'db> {
        let mut segments = Vec::new();
        self.collect_path_segments(node, &mut segments);
        Path::new(self.db, segments, self.span(node))
    }

    fn collect_path_segments(&self, node: Node<'_>, out: &mut Vec<Name<'db>>) {
        match node.kind() {
            "scoped_identifier" | "scoped_type_identifier" => {
                if let Some(prefix) = node.child_by_field_name("path") {
                    self.collect_path_segments(prefix, out);
                } else if node.child(0).is_some_and(|c| c.kind() == "::") {
                    out.push(Name::new(self.db, String::new()));
                }
                if let Some(name) = node.child_by_field_name("name") {
                    out.push(self.name(name));
                }
            }
            "generic_type" => {
                if let Some(ty) = node.child_by_field_name("type") {
                    self.collect_path_segments(ty, out);
                }
            }
            "identifier" | "type_identifier" | "primitive_type" | "metavariable" => {
                out.push(self.name(node));
            }
            "self" => out.push(Name::new(self.db, "self".to_owned())),
            "crate" => out.push(Name::new(self.db, "crate".to_owned())),
            "super" => out.push(Name::new(self.db, "super".to_owned())),
            _ => {
                out.push(Name::new(self.db, self.node_text(node).to_owned()));
            }
        }
    }

    // -- Expressions -------------------------------------------------------

    fn lower_expr(&mut self, node: Node<'_>) -> Ptr<Expr<'db>> {
        let span = self.span(node);
        let kind = match node.kind() {
            "block" => return self.lower_block(node),
            "integer_literal" => ExprKind::Literal(Literal::Int),
            "float_literal" => ExprKind::Literal(Literal::Float),
            "string_literal" | "raw_string_literal" => ExprKind::Literal(Literal::String),
            "char_literal" => ExprKind::Literal(Literal::Char),
            "boolean_literal" => ExprKind::Literal(Literal::Bool(self.node_text(node) == "true")),
            "identifier" | "scoped_identifier" | "self" => ExprKind::Path(self.lower_path(node)),
            "unit_expression" => ExprKind::Tuple(self.stash.alloc_slice(&[])),
            "tuple_expression" => {
                let elems = self.lower_expr_children(node);
                ExprKind::Tuple(self.stash.alloc_slice(&elems))
            }
            "array_expression" => {
                let elems = self.lower_expr_children(node);
                ExprKind::Array(self.stash.alloc_slice(&elems))
            }
            "parenthesized_expression" => {
                let inner = node
                    .named_child(0)
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                return inner;
            }
            "call_expression" => {
                let func = node
                    .child_by_field_name("function")
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                let args = node
                    .child_by_field_name("arguments")
                    .map(|n| self.lower_expr_children(n))
                    .unwrap_or_default();
                ExprKind::Call(func, self.stash.alloc_slice(&args))
            }
            "field_expression" => {
                let obj = node
                    .child_by_field_name("value")
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                let field = node
                    .child_by_field_name("field")
                    .map(|n| self.name(n))
                    .unwrap_or_else(|| Name::new(self.db, "?".to_owned()));
                ExprKind::Field(obj, field)
            }
            "method_call_expression" | "generic_function" => {
                // tree-sitter puts method calls as call_expression with field_expression inside,
                // but also has a dedicated method node in some grammars. Handle both.
                return self.lower_method_or_call(node);
            }
            "binary_expression" => {
                let lhs = node
                    .child_by_field_name("left")
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                let rhs = node
                    .child_by_field_name("right")
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                let op = node
                    .child_by_field_name("operator")
                    .map(|n| lower_binary_op(self.node_text(n)))
                    .unwrap_or(BinaryOp::Add);
                ExprKind::Binary(lhs, op, rhs)
            }
            "unary_expression" => {
                let op_text = node.child(0).map(|n| self.node_text(n)).unwrap_or("!");
                let op = match op_text {
                    "-" => UnaryOp::Neg,
                    "*" => UnaryOp::Deref,
                    _ => UnaryOp::Not,
                };
                let operand = node
                    .named_child(0)
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                ExprKind::Unary(op, operand)
            }
            "reference_expression" => {
                let is_mut = node
                    .children(&mut node.walk())
                    .any(|c| c.kind() == "mutable_specifier");
                let inner = node
                    .child_by_field_name("value")
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                let m = if is_mut {
                    Mutability::Mut
                } else {
                    Mutability::Shared
                };
                ExprKind::Ref(inner, m)
            }
            "if_expression" => return self.lower_if(node),
            "match_expression" => return self.lower_match(node),
            "loop_expression" => {
                let body = node
                    .child_by_field_name("body")
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                ExprKind::Loop(body)
            }
            "while_expression" => {
                let cond_node = node.child_by_field_name("condition");
                let body = node
                    .child_by_field_name("body")
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));

                if let Some(cond) = cond_node {
                    if cond.kind() == "let_condition" {
                        let pat = cond
                            .child_by_field_name("pattern")
                            .map(|n| self.lower_pat(n))
                            .unwrap_or_else(|| self.missing_pat(cond));
                        let scrutinee = cond
                            .child_by_field_name("value")
                            .map(|n| self.lower_expr(n))
                            .unwrap_or_else(|| self.missing_expr(cond));
                        return self.stash.alloc(Expr {
                            kind: ExprKind::WhileLet(pat, scrutinee, body),
                            span,
                        });
                    }
                }

                let cond = cond_node
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                ExprKind::While(cond, body)
            }
            "for_expression" => {
                let pat = node
                    .child_by_field_name("pattern")
                    .map(|n| self.lower_pat(n))
                    .unwrap_or_else(|| self.missing_pat(node));
                let iter = node
                    .child_by_field_name("value")
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                let body = node
                    .child_by_field_name("body")
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                ExprKind::For(pat, iter, body)
            }
            "break_expression" => {
                let val = node.named_child(0).map(|n| self.lower_expr(n));
                ExprKind::Break(val)
            }
            "continue_expression" => ExprKind::Continue,
            "return_expression" => {
                let val = node.named_child(0).map(|n| self.lower_expr(n));
                ExprKind::Return(val)
            }
            "assignment_expression" | "compound_assignment_expr" => {
                let lhs = node
                    .child_by_field_name("left")
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                let rhs = node
                    .child_by_field_name("right")
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                ExprKind::Assign(lhs, rhs)
            }
            "await_expression" => {
                let inner = node
                    .named_child(0)
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                ExprKind::Await(inner)
            }
            "try_expression" => {
                let inner = node
                    .named_child(0)
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                ExprKind::Try(inner)
            }
            "closure_expression" => return self.lower_closure(node),
            "index_expression" => {
                let obj = node
                    .named_child(0)
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                let idx = node
                    .named_child(1)
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                ExprKind::Index(obj, idx)
            }
            "type_cast_expression" => {
                let expr = node
                    .child_by_field_name("value")
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                let ty = node
                    .child_by_field_name("type")
                    .map(|n| {
                        TypeRef::new(self.db, TypeRefKind::Path(self.lower_path(n)), self.span(n))
                    })
                    .unwrap_or_else(|| TypeRef::new(self.db, TypeRefKind::Error, self.span(node)));
                ExprKind::Cast(expr, ty)
            }
            "struct_expression" => return self.lower_struct_lit(node),
            "range_expression" => {
                let children: Vec<_> = node.named_children(&mut node.walk()).collect();
                let (lo, hi) = match children.len() {
                    0 => (None, None),
                    1 => {
                        let c = self.lower_expr(children[0]);
                        // Heuristic: if child starts at range start, it's the lower bound
                        if children[0].start_byte() == node.start_byte() {
                            (Some(c), None)
                        } else {
                            (None, Some(c))
                        }
                    }
                    _ => {
                        let lo = self.lower_expr(children[0]);
                        let hi = self.lower_expr(children[1]);
                        (Some(lo), Some(hi))
                    }
                };
                ExprKind::Range(lo, hi)
            }
            "macro_invocation" => {
                let path = node
                    .child_by_field_name("macro")
                    .map(|n| self.lower_path(n))
                    .unwrap_or_else(|| {
                        Path::new(
                            self.db,
                            vec![Name::new(self.db, "?".to_owned())],
                            self.span(node),
                        )
                    });
                let args_node = node
                    .named_children(&mut node.walk())
                    .find(|c| c.kind() == "token_tree");
                let args = TokenTree::new(
                    self.db,
                    args_node
                        .map(|n| self.node_text(n))
                        .unwrap_or("")
                        .to_owned(),
                    args_node.map(|n| self.span(n)).unwrap_or(self.span(node)),
                );
                ExprKind::MacroCall(path, args)
            }
            // Catch-all: anything we don't handle becomes Missing
            "let_condition" => {
                // `if let Some(x) = expr` — lower as the inner expression
                let inner = node.named_children(&mut node.walk()).last();
                return inner
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
            }
            "async_block" => {
                let body = node
                    .child_by_field_name("body")
                    .or_else(|| node.named_child(0))
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(node));
                return body;
            }
            // Skip non-expression nodes that appear in blocks
            "line_comment"
            | "block_comment"
            | "attribute_item"
            | "inner_attribute_item"
            | "empty_statement"
            | "use_declaration"
            | "const_item" => ExprKind::Missing,
            // Catch-all
            _ => ExprKind::Missing,
        };
        self.stash.alloc(Expr { kind, span })
    }

    fn lower_expr_children(&mut self, node: Node<'_>) -> Vec<Expr<'db>> {
        let mut exprs = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            let ptr = self.lower_expr(child);
            exprs.push(self.stash[ptr]);
        }
        exprs
    }

    fn lower_block(&mut self, node: Node<'_>) -> Ptr<Expr<'db>> {
        let span = self.span(node);
        let mut stmts = Vec::new();
        let mut tail: Option<Ptr<Expr<'db>>> = None;
        let mut cursor = node.walk();
        let children: Vec<_> = node.named_children(&mut cursor).collect();

        for (i, child) in children.iter().enumerate() {
            let is_last = i == children.len() - 1;
            match child.kind() {
                // Skip non-statement nodes
                "line_comment"
                | "block_comment"
                | "attribute_item"
                | "inner_attribute_item"
                | "empty_statement"
                | "use_declaration"
                | "const_item" => continue,
                "let_declaration" => {
                    stmts.push(self.lower_let(*child));
                }
                "expression_statement" => {
                    if let Some(expr_node) = child.named_child(0) {
                        let expr = self.lower_expr(expr_node);
                        stmts.push(Stmt {
                            kind: StmtKind::Expr(expr),
                            span: self.span(*child),
                        });
                    }
                }
                _ if is_last => {
                    // Last expression without semicolon = tail
                    tail = Some(self.lower_expr(*child));
                }
                _ => {
                    let expr = self.lower_expr(*child);
                    stmts.push(Stmt {
                        kind: StmtKind::Expr(expr),
                        span: self.span(*child),
                    });
                }
            }
        }

        let stmts = self.stash.alloc_slice(&stmts);
        self.stash.alloc(Expr {
            kind: ExprKind::Block(stmts, tail),
            span,
        })
    }

    fn lower_let(&mut self, node: Node<'_>) -> Stmt<'db> {
        let pat = node
            .child_by_field_name("pattern")
            .map(|n| self.lower_pat(n))
            .unwrap_or_else(|| self.missing_pat(node));
        let ty = node
            .child_by_field_name("type")
            .map(|n| TypeRef::new(self.db, TypeRefKind::Path(self.lower_path(n)), self.span(n)));
        let init = node
            .child_by_field_name("value")
            .map(|n| self.lower_expr(n));
        Stmt {
            kind: StmtKind::Let(pat, ty, init),
            span: self.span(node),
        }
    }

    fn lower_if(&mut self, node: Node<'_>) -> Ptr<Expr<'db>> {
        let span = self.span(node);
        let cond_node = node.child_by_field_name("condition");
        let then = node
            .child_by_field_name("consequence")
            .map(|n| self.lower_expr(n))
            .unwrap_or_else(|| self.missing_expr(node));
        let else_ = node
            .child_by_field_name("alternative")
            .and_then(|n| n.named_child(0))
            .map(|n| self.lower_expr(n));

        if let Some(cond) = cond_node {
            if cond.kind() == "let_condition" {
                let pat = cond
                    .child_by_field_name("pattern")
                    .map(|n| self.lower_pat(n))
                    .unwrap_or_else(|| self.missing_pat(cond));
                let scrutinee = cond
                    .child_by_field_name("value")
                    .map(|n| self.lower_expr(n))
                    .unwrap_or_else(|| self.missing_expr(cond));
                return self.stash.alloc(Expr {
                    kind: ExprKind::IfLet(pat, scrutinee, then, else_),
                    span,
                });
            }
        }

        let cond = cond_node
            .map(|n| self.lower_expr(n))
            .unwrap_or_else(|| self.missing_expr(node));
        self.stash.alloc(Expr {
            kind: ExprKind::If(cond, then, else_),
            span,
        })
    }

    fn lower_match(&mut self, node: Node<'_>) -> Ptr<Expr<'db>> {
        let span = self.span(node);
        let scrutinee = node
            .child_by_field_name("value")
            .map(|n| self.lower_expr(n))
            .unwrap_or_else(|| self.missing_expr(node));
        let mut arms = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.named_children(&mut cursor) {
                if child.kind() == "match_arm" {
                    let pat = child
                        .child_by_field_name("pattern")
                        .map(|n| self.lower_pat(n))
                        .unwrap_or_else(|| self.missing_pat(child));
                    let guard = child
                        .child_by_field_name("condition") // tree-sitter uses "condition" for guard
                        .map(|n| self.lower_expr(n));
                    let body = child
                        .child_by_field_name("value")
                        .map(|n| self.lower_expr(n))
                        .unwrap_or_else(|| self.missing_expr(child));
                    arms.push(MatchArm {
                        pat,
                        guard,
                        body,
                        span: self.span(child),
                    });
                }
            }
        }
        let arms = self.stash.alloc_slice(&arms);
        self.stash.alloc(Expr {
            kind: ExprKind::Match(scrutinee, arms),
            span,
        })
    }

    fn lower_closure(&mut self, node: Node<'_>) -> Ptr<Expr<'db>> {
        let span = self.span(node);
        let mut params = Vec::new();
        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.named_children(&mut cursor) {
                let pat = self.lower_pat(child);
                params.push(ClosureParam {
                    pat,
                    ty: None,
                    span: self.span(child),
                });
            }
        }
        let body = node
            .child_by_field_name("body")
            .map(|n| self.lower_expr(n))
            .unwrap_or_else(|| self.missing_expr(node));
        let params = self.stash.alloc_slice(&params);
        self.stash.alloc(Expr {
            kind: ExprKind::Closure(params, body),
            span,
        })
    }

    fn lower_method_or_call(&mut self, node: Node<'_>) -> Ptr<Expr<'db>> {
        // tree-sitter-rust doesn't have a dedicated method_call node;
        // it's a call_expression whose function is a field_expression.
        // Just lower as a regular call.
        let span = self.span(node);
        let func = node
            .child_by_field_name("function")
            .map(|n| self.lower_expr(n))
            .unwrap_or_else(|| self.missing_expr(node));
        let args = node
            .child_by_field_name("arguments")
            .map(|n| self.lower_expr_children(n))
            .unwrap_or_default();
        let args = self.stash.alloc_slice(&args);
        self.stash.alloc(Expr {
            kind: ExprKind::Call(func, args),
            span,
        })
    }

    fn lower_struct_lit(&mut self, node: Node<'_>) -> Ptr<Expr<'db>> {
        let span = self.span(node);
        let path = node
            .child_by_field_name("name")
            .map(|n| self.lower_path(n))
            .unwrap_or_else(|| {
                Path::new(
                    self.db,
                    vec![Name::new(self.db, "?".to_owned())],
                    self.span(node),
                )
            });
        let mut fields = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.named_children(&mut cursor) {
                if child.kind() == "field_initializer" {
                    let name = child
                        .child_by_field_name("field")
                        .map(|n| self.name(n))
                        .unwrap_or_else(|| Name::new(self.db, "?".to_owned()));
                    let value = child
                        .child_by_field_name("value")
                        .map(|n| self.lower_expr(n))
                        .unwrap_or_else(|| self.missing_expr(child));
                    fields.push(FieldInit {
                        name,
                        value,
                        span: self.span(child),
                    });
                } else if child.kind() == "shorthand_field_initializer" {
                    let name = self.name(child);
                    let value = self.stash.alloc(Expr {
                        kind: ExprKind::Path(self.lower_path(child)),
                        span: self.span(child),
                    });
                    fields.push(FieldInit {
                        name,
                        value,
                        span: self.span(child),
                    });
                }
            }
        }
        let fields = self.stash.alloc_slice(&fields);
        self.stash.alloc(Expr {
            kind: ExprKind::StructLit(path, fields),
            span,
        })
    }

    // -- Patterns ----------------------------------------------------------

    fn lower_pat(&mut self, node: Node<'_>) -> Ptr<Pat<'db>> {
        let span = self.span(node);
        let kind = match node.kind() {
            // match_pattern wraps the actual pattern in match arms
            "match_pattern" => {
                return node
                    .named_child(0)
                    .map(|n| self.lower_pat(n))
                    .unwrap_or_else(|| self.missing_pat(node));
            }
            "_" | "wildcard_pattern" => PatKind::Wildcard,
            "identifier" => PatKind::Bind(self.name(node), Mutability::Shared),
            "mut_pattern" => {
                let inner = node.named_child(0);
                let name = inner
                    .map(|n| self.name(n))
                    .unwrap_or_else(|| Name::new(self.db, "?".to_owned()));
                PatKind::Bind(name, Mutability::Mut)
            }
            "tuple_pattern" => {
                let pats = self.lower_pat_children(node);
                PatKind::Tuple(self.stash.alloc_slice(&pats))
            }
            "tuple_struct_pattern" => {
                let path = node
                    .child_by_field_name("type")
                    .map(|n| self.lower_path(n))
                    .unwrap_or_else(|| {
                        Path::new(
                            self.db,
                            vec![Name::new(self.db, "?".to_owned())],
                            self.span(node),
                        )
                    });
                let pats = self.lower_pat_children(node);
                PatKind::TupleStruct(path, self.stash.alloc_slice(&pats))
            }
            "struct_pattern" => {
                let path = node
                    .child_by_field_name("type")
                    .map(|n| self.lower_path(n))
                    .unwrap_or_else(|| {
                        Path::new(
                            self.db,
                            vec![Name::new(self.db, "?".to_owned())],
                            self.span(node),
                        )
                    });
                let mut field_pats = Vec::new();
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.kind() == "field_pattern" {
                        let name = child
                            .child_by_field_name("name")
                            .map(|n| self.name(n))
                            .unwrap_or_else(|| Name::new(self.db, "?".to_owned()));
                        let pat = child
                            .child_by_field_name("pattern")
                            .map(|n| self.lower_pat(n))
                            .unwrap_or_else(|| {
                                // Shorthand: `Foo { x }` means `Foo { x: x }`
                                self.stash.alloc(Pat {
                                    kind: PatKind::Bind(name, Mutability::Shared),
                                    span: self.span(child),
                                })
                            });
                        field_pats.push(FieldPat {
                            name,
                            pat,
                            span: self.span(child),
                        });
                    }
                }
                PatKind::Struct(path, self.stash.alloc_slice(&field_pats))
            }
            "ref_pattern" => {
                let is_mut = node
                    .children(&mut node.walk())
                    .any(|c| c.kind() == "mutable_specifier");
                let inner = node
                    .named_child(0)
                    .map(|n| self.lower_pat(n))
                    .unwrap_or_else(|| self.missing_pat(node));
                let m = if is_mut {
                    Mutability::Mut
                } else {
                    Mutability::Shared
                };
                PatKind::Ref(inner, m)
            }
            "or_pattern" => {
                let pats = self.lower_pat_children(node);
                PatKind::Or(self.stash.alloc_slice(&pats))
            }
            "scoped_identifier" | "scoped_type_identifier" => PatKind::Path(self.lower_path(node)),
            "integer_literal" => PatKind::Literal(Literal::Int),
            "string_literal" => PatKind::Literal(Literal::String),
            "boolean_literal" => PatKind::Literal(Literal::Bool(self.node_text(node) == "true")),
            "negative_literal" => PatKind::Literal(Literal::Int),
            "rest_pattern" | ".." => PatKind::Rest,
            _ => PatKind::Missing,
        };
        self.stash.alloc(Pat { kind, span })
    }

    fn lower_pat_children(&mut self, node: Node<'_>) -> Vec<Pat<'db>> {
        let mut pats = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "identifier"
                || child.kind() == "scoped_identifier"
                || child.kind() == "scoped_type_identifier"
            {
                if node.kind() == "tuple_struct_pattern"
                    && node.child_by_field_name("type") == Some(child)
                {
                    continue;
                }
            }
            let ptr = self.lower_pat(child);
            pats.push(self.stash[ptr]);
        }
        pats
    }

    fn missing_pat(&mut self, node: Node<'_>) -> Ptr<Pat<'db>> {
        self.stash.alloc(Pat {
            kind: PatKind::Missing,
            span: self.span(node),
        })
    }
}

fn lower_binary_op(op: &str) -> BinaryOp {
    match op {
        "+" => BinaryOp::Add,
        "-" => BinaryOp::Sub,
        "*" => BinaryOp::Mul,
        "/" => BinaryOp::Div,
        "%" => BinaryOp::Rem,
        "&&" => BinaryOp::And,
        "||" => BinaryOp::Or,
        "&" => BinaryOp::BitAnd,
        "|" => BinaryOp::BitOr,
        "^" => BinaryOp::BitXor,
        "<<" => BinaryOp::Shl,
        ">>" => BinaryOp::Shr,
        "==" => BinaryOp::Eq,
        "!=" => BinaryOp::Ne,
        "<" => BinaryOp::Lt,
        "<=" => BinaryOp::Le,
        ">" => BinaryOp::Gt,
        ">=" => BinaryOp::Ge,
        _ => BinaryOp::Add,
    }
}
