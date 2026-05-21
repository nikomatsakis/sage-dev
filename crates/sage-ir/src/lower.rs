//! Lower tree-sitter CST nodes into salsa tracked IR structs.

use tree_sitter::Node;

use sage_stash::{Ptr, Slice, Stash, Stashed};

use crate::Db;
use crate::body::*;
use crate::item::*;
use crate::name::Name;
use crate::sig_ast::*;
use crate::source::SourceFile;
use crate::span::{AbsoluteSpan, MacroExpansion, ParseSource, RelativeSpan};
use crate::types::*;

/// Tracked wrapper: parse a real source file.
#[salsa::tracked(returns(ref))]
pub fn parse_source_file<'db>(db: &'db dyn Db, file: SourceFile) -> Vec<ItemAst<'db>> {
    db.log_query(format!("parse_source_file(\"{}\")", file.path(db)));
    parse_source_text(db, ParseSource::SourceFile(file), file.text(db))
}

/// Tracked wrapper: parse macro expansion output.
#[salsa::tracked(returns(ref))]
pub fn parse_macro_expansion<'db>(db: &'db dyn Db, exp: MacroExpansion<'db>) -> Vec<ItemAst<'db>> {
    parse_source_text(db, ParseSource::MacroExpansion(exp), exp.text(db))
}

/// Untracked core: parse Rust source text into items.
/// Takes a ParseSource for stamping onto AbsoluteSpans during lowering.
pub fn parse_source_text<'db>(
    db: &'db dyn Db,
    source: ParseSource<'db>,
    text: &'db str,
) -> Vec<ItemAst<'db>> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("failed to set tree-sitter-rust language");
    let tree = parser.parse(text, None).expect("tree-sitter parse failed");
    let cx = LowerCtx { db, source, text };
    cx.lower_items(tree.root_node())
}

enum AttrNodeKind {
    Attr,
    OuterDoc,
    InnerDoc,
}

// ===========================================================================
// LowerCtx — file-level, no item_start
// ===========================================================================

struct LowerCtx<'db> {
    db: &'db dyn Db,
    source: ParseSource<'db>,
    text: &'db str,
}

impl<'db> LowerCtx<'db> {
    fn node_text(&self, node: Node<'_>) -> &'db str {
        &self.text[node.byte_range()]
    }

    fn intern_name(&self, node: Node<'_>) -> Name<'db> {
        Name::new(self.db, self.node_text(node).to_owned())
    }

    fn abs_span(&self, node: Node<'_>) -> AbsoluteSpan<'db> {
        AbsoluteSpan {
            source: self.source,
            start: node.start_byte() as u32,
            end: node.end_byte() as u32,
        }
    }

    fn lower_items(&self, parent: Node<'_>) -> Vec<ItemAst<'db>> {
        let mut items = Vec::new();
        let mut pending_attr_nodes: Vec<(Node<'_>, AttrNodeKind)> = Vec::new();
        let mut cursor = parent.walk();
        for child in parent.children(&mut cursor) {
            if !child.is_named() {
                continue;
            }
            match child.kind() {
                "attribute_item" | "inner_attribute_item" => {
                    pending_attr_nodes.push((child, AttrNodeKind::Attr));
                }
                "line_comment" => {
                    let text = self.node_text(child);
                    if text.starts_with("///") {
                        pending_attr_nodes.push((child, AttrNodeKind::OuterDoc));
                    } else if text.starts_with("//!") {
                        pending_attr_nodes.push((child, AttrNodeKind::InnerDoc));
                    }
                }
                "block_comment" | "empty_statement" => continue,
                _ => {
                    let attr_nodes = std::mem::take(&mut pending_attr_nodes);
                    let first_attr_start = attr_nodes.first().map(|(n, _)| n.start_byte() as u32);
                    let item_node_start = child.start_byte() as u32;
                    let item_start = first_attr_start.unwrap_or(item_node_start);

                    let icx = ItemLowerCtx {
                        parent: self,
                        item_start,
                    };

                    let attrs: Vec<Attr<'db>> = attr_nodes
                        .iter()
                        .map(|(n, kind)| match kind {
                            AttrNodeKind::Attr => icx.lower_attr(*n),
                            AttrNodeKind::OuterDoc => {
                                let text = self.node_text(*n);
                                let doc = text.strip_prefix("///").unwrap();
                                let doc = doc.strip_prefix(' ').unwrap_or(doc).trim_end();
                                icx.make_doc_attr(*n, doc, false)
                            }
                            AttrNodeKind::InnerDoc => {
                                let text = self.node_text(*n);
                                let doc = text.strip_prefix("//!").unwrap();
                                let doc = doc.strip_prefix(' ').unwrap_or(doc).trim_end();
                                icx.make_doc_attr(*n, doc, true)
                            }
                        })
                        .collect();
                    items.push(icx.lower_item(child, attrs));
                }
            }
        }
        items
    }
}

// ===========================================================================
// ItemLowerCtx — per-item, carries item_start on the stack
// ===========================================================================

struct ItemLowerCtx<'a, 'db> {
    parent: &'a LowerCtx<'db>,
    item_start: u32,
}

impl<'a, 'db> ItemLowerCtx<'a, 'db> {
    fn db(&self) -> &'db dyn Db {
        self.parent.db
    }

    fn node_text(&self, node: Node<'_>) -> &'db str {
        self.parent.node_text(node)
    }

    fn intern_name(&self, node: Node<'_>) -> Name<'db> {
        self.parent.intern_name(node)
    }

    fn abs_span(&self, node: Node<'_>) -> AbsoluteSpan<'db> {
        AbsoluteSpan {
            source: self.parent.source,
            start: node.start_byte() as u32,
            end: node.end_byte() as u32,
        }
    }

    fn rel_span(&self, node: Node<'_>) -> RelativeSpan {
        RelativeSpan {
            start: node.start_byte() as u32 - self.item_start,
            end: node.end_byte() as u32 - self.item_start,
        }
    }

    // -- Attributes -----------------------------------------------------------

    fn lower_attr(&self, node: Node<'_>) -> Attr<'db> {
        let db = self.db();
        let is_inner = node.kind() == "inner_attribute_item";
        let text = self.node_text(node);
        let inner = text
            .trim_start_matches("#![")
            .trim_start_matches("#[")
            .trim_end_matches(']')
            .trim();
        let (path_text, args_text) = match inner.find('(') {
            Some(i) => (&inner[..i], Some(&inner[i..])),
            None => (inner, None),
        };
        let path = Path::new(
            db,
            vec![Name::new(db, path_text.trim().to_owned())],
            self.rel_span(node),
        );
        let args = args_text.map(|a| TokenTree::new(db, a.to_owned(), self.rel_span(node)));
        Attr::new(
            db,
            AttrKind::Normal,
            path,
            args,
            self.rel_span(node),
            is_inner,
        )
    }

    fn make_doc_attr(&self, node: Node<'_>, text: &str, is_inner: bool) -> Attr<'db> {
        let db = self.db();
        let path = Path::new(
            db,
            vec![Name::new(db, "doc".to_owned())],
            self.rel_span(node),
        );
        let args = Some(TokenTree::new(db, text.to_owned(), self.rel_span(node)));
        Attr::new(
            db,
            AttrKind::DocComment,
            path,
            args,
            self.rel_span(node),
            is_inner,
        )
    }

    // -- Item dispatch --------------------------------------------------------

    fn lower_item(&self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> ItemAst<'db> {
        match node.kind() {
            "function_item" => ItemAst::Function(self.lower_function(node, attrs)),
            "struct_item" => ItemAst::Struct(self.lower_struct(node, attrs)),
            "enum_item" => ItemAst::Enum(self.lower_enum(node, attrs)),
            "trait_item" => ItemAst::Trait(self.lower_trait(node, attrs)),
            "impl_item" => ItemAst::Impl(self.lower_impl(node, attrs)),
            "type_item" => ItemAst::TypeAlias(self.lower_type_alias(node, attrs)),
            "const_item" => ItemAst::Const(self.lower_const(node, attrs)),
            "static_item" => ItemAst::Static(self.lower_static(node, attrs)),
            "mod_item" => ItemAst::Mod(self.lower_mod(node, attrs)),
            "use_declaration" => self.lower_use(node, attrs),
            "macro_definition" => self.lower_macro_def_item(node),
            "macro_invocation" => self.lower_macro_invocation_item(node),
            "expression_statement" => self.lower_expression_statement(node),
            _ => ItemAst::Error(self.abs_span(node)),
        }
    }

    // -- Signature stash lowering helpers --------------------------------------

    fn lower_generic_params_ast(
        &self,
        stash: &mut Stash,
        node: Node<'_>,
    ) -> Slice<GenericParam<'db>> {
        let mut generics = Vec::new();
        if let Some(tp_node) = node.child_by_field_name("type_parameters") {
            let mut cursor = tp_node.walk();
            for child in tp_node.named_children(&mut cursor) {
                match child.kind() {
                    "type_parameter" => {
                        if let Some(name_node) = child.child_by_field_name("name") {
                            generics.push(GenericParam::Type {
                                name: self.intern_name(name_node),
                                span: self.rel_span(child),
                            });
                        }
                    }
                    "lifetime_parameter" => {
                        if let Some(name_node) = child.child_by_field_name("name") {
                            generics.push(GenericParam::Lifetime {
                                name: self.intern_name(name_node),
                                span: self.rel_span(child),
                            });
                        }
                    }
                    "const_parameter" => {
                        if let Some(name_node) = child.child_by_field_name("name") {
                            let ty = child
                                .child_by_field_name("type")
                                .map(|n| self.lower_type_ref_to_stash(stash, n))
                                .unwrap_or_else(|| {
                                    stash.alloc(TypeRefAst {
                                        kind: TypeRefAstKind::Error,
                                        span: self.rel_span(child),
                                    })
                                });
                            generics.push(GenericParam::Const {
                                name: self.intern_name(name_node),
                                ty,
                                span: self.rel_span(child),
                            });
                        }
                    }
                    "constrained_type_parameter" => {
                        if let Some(name_node) = child.child_by_field_name("left") {
                            generics.push(GenericParam::Type {
                                name: self.intern_name(name_node),
                                span: self.rel_span(child),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        stash.alloc_slice(&generics)
    }

    fn lower_fn_sig_ast(&self, node: Node<'_>) -> FnSigAst<'db> {
        let db = self.db();
        let mut stash = Stash::new();
        let generics = self.lower_generic_params_ast(&mut stash, node);
        let mut params = Vec::new();
        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                match child.kind() {
                    "parameter" => {
                        let name = child
                            .child_by_field_name("pattern")
                            .map(|n| self.intern_name(n));
                        let ty = child
                            .child_by_field_name("type")
                            .map(|n| self.lower_type_ref_to_stash(&mut stash, n))
                            .unwrap_or_else(|| {
                                stash.alloc(TypeRefAst {
                                    kind: TypeRefAstKind::Error,
                                    span: self.rel_span(child),
                                })
                            });
                        params.push(ParamAst {
                            name,
                            ty,
                            span: self.rel_span(child),
                        });
                    }
                    "self_parameter" => {
                        let name = Some(Name::new(db, "self".to_owned()));
                        let has_ref = child.children(&mut child.walk()).any(|c| c.kind() == "&");
                        let is_mut = child
                            .children(&mut child.walk())
                            .any(|c| c.kind() == "mutable_specifier");
                        let self_ty =
                            self.make_simple_path_type(&mut stash, "Self", self.rel_span(child));
                        let ty = if has_ref {
                            let m = if is_mut {
                                Mutability::Mut
                            } else {
                                Mutability::Shared
                            };
                            stash.alloc(TypeRefAst {
                                kind: TypeRefAstKind::Reference(self_ty, m),
                                span: self.rel_span(child),
                            })
                        } else {
                            self_ty
                        };
                        params.push(ParamAst {
                            name,
                            ty,
                            span: self.rel_span(child),
                        });
                    }
                    _ => {}
                }
            }
        }
        let params = stash.alloc_slice(&params);
        let ret_type = node
            .child_by_field_name("return_type")
            .map(|n| self.lower_type_ref_to_stash(&mut stash, n));
        let root = stash.alloc(FnSigAstData {
            generics,
            params,
            ret_type,
        });
        Stashed::new(stash, root)
    }

    fn lower_struct_sig_ast(&self, node: Node<'_>) -> StructSigAst<'db> {
        let mut stash = Stash::new();
        let generics = self.lower_generic_params_ast(&mut stash, node);
        let mut fields = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if child.kind() == "field_declaration" {
                    let name = child
                        .child_by_field_name("name")
                        .map(|n| self.intern_name(n))
                        .unwrap_or_else(|| Name::new(self.db(), "?".to_owned()));
                    let ty = child
                        .child_by_field_name("type")
                        .map(|n| self.lower_type_ref_to_stash(&mut stash, n))
                        .unwrap_or_else(|| {
                            stash.alloc(TypeRefAst {
                                kind: TypeRefAstKind::Error,
                                span: self.rel_span(child),
                            })
                        });
                    fields.push(FieldDefAst {
                        name,
                        ty,
                        span: self.rel_span(child),
                    });
                }
            }
        }
        let fields = stash.alloc_slice(&fields);
        let root = stash.alloc(StructSigAstData { generics, fields });
        Stashed::new(stash, root)
    }

    fn lower_enum_sig_ast(&self, node: Node<'_>) -> EnumSigAst<'db> {
        let mut stash = Stash::new();
        let generics = self.lower_generic_params_ast(&mut stash, node);
        let mut variants = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if child.kind() == "enum_variant" {
                    let vname = child
                        .child_by_field_name("name")
                        .map(|n| self.intern_name(n))
                        .unwrap_or_else(|| Name::new(self.db(), "?".to_owned()));
                    let mut vfields = Vec::new();
                    if let Some(vbody) = child.child_by_field_name("body") {
                        let mut vc = vbody.walk();
                        for fc in vbody.children(&mut vc) {
                            if fc.kind() == "field_declaration" {
                                let fname = fc
                                    .child_by_field_name("name")
                                    .map(|n| self.intern_name(n))
                                    .unwrap_or_else(|| Name::new(self.db(), "?".to_owned()));
                                let fty = fc
                                    .child_by_field_name("type")
                                    .map(|n| self.lower_type_ref_to_stash(&mut stash, n))
                                    .unwrap_or_else(|| {
                                        stash.alloc(TypeRefAst {
                                            kind: TypeRefAstKind::Error,
                                            span: self.rel_span(fc),
                                        })
                                    });
                                vfields.push(FieldDefAst {
                                    name: fname,
                                    ty: fty,
                                    span: self.rel_span(fc),
                                });
                            }
                        }
                    }
                    let vfields = stash.alloc_slice(&vfields);
                    variants.push(VariantDefAst {
                        name: vname,
                        fields: vfields,
                        span: self.rel_span(child),
                    });
                }
            }
        }
        let variants = stash.alloc_slice(&variants);
        let root = stash.alloc(EnumSigAstData { generics, variants });
        Stashed::new(stash, root)
    }

    fn lower_impl_sig_ast(&self, node: Node<'_>) -> ImplSigAst<'db> {
        let mut stash = Stash::new();
        let generics = self.lower_generic_params_ast(&mut stash, node);
        let self_ty = node
            .child_by_field_name("type")
            .map(|n| self.lower_type_ref_to_stash(&mut stash, n))
            .unwrap_or_else(|| {
                stash.alloc(TypeRefAst {
                    kind: TypeRefAstKind::Error,
                    span: RelativeSpan { start: 0, end: 0 },
                })
            });
        let trait_path = node
            .child_by_field_name("trait")
            .map(|n| self.lower_path_to_stash(&mut stash, n));
        let root = stash.alloc(ImplSigAstData {
            generics,
            self_ty,
            trait_path,
        });
        Stashed::new(stash, root)
    }

    fn lower_simple_ty_sig_ast(
        &self,
        node: Node<'_>,
    ) -> (
        Slice<GenericParam<'db>>,
        Option<Ptr<TypeRefAst<'db>>>,
        Stash,
    ) {
        let mut stash = Stash::new();
        let generics = self.lower_generic_params_ast(&mut stash, node);
        let ty = node
            .child_by_field_name("type")
            .map(|n| self.lower_type_ref_to_stash(&mut stash, n));
        (generics, ty, stash)
    }

    fn lower_type_ref_to_stash(&self, stash: &mut Stash, node: Node<'_>) -> Ptr<TypeRefAst<'db>> {
        let span = self.rel_span(node);
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
            | "metavariable" => TypeRefAstKind::Path(self.lower_path_to_stash(stash, node)),
            "reference_type" => {
                let is_mut = node
                    .children(&mut node.walk())
                    .any(|c| c.kind() == "mutable_specifier");
                let inner = node
                    .child_by_field_name("type")
                    .map(|n| self.lower_type_ref_to_stash(stash, n))
                    .unwrap_or_else(|| {
                        stash.alloc(TypeRefAst {
                            kind: TypeRefAstKind::Error,
                            span,
                        })
                    });
                let m = if is_mut {
                    Mutability::Mut
                } else {
                    Mutability::Shared
                };
                TypeRefAstKind::Reference(inner, m)
            }
            "array_type" => {
                let inner = node
                    .child_by_field_name("element")
                    .map(|n| self.lower_type_ref_to_stash(stash, n))
                    .unwrap_or_else(|| {
                        stash.alloc(TypeRefAst {
                            kind: TypeRefAstKind::Error,
                            span,
                        })
                    });
                TypeRefAstKind::Array(inner)
            }
            "slice_type" => {
                let inner = node
                    .child_by_field_name("element")
                    .map(|n| self.lower_type_ref_to_stash(stash, n))
                    .unwrap_or_else(|| {
                        stash.alloc(TypeRefAst {
                            kind: TypeRefAstKind::Error,
                            span,
                        })
                    });
                TypeRefAstKind::Slice(inner)
            }
            "unit_type" => TypeRefAstKind::Tuple(stash.alloc_slice(&[])),
            "tuple_type" => {
                let mut elems = Vec::new();
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.is_named() && child.kind() != "(" && child.kind() != ")" {
                        let ty = self.lower_type_ref_to_stash(stash, child);
                        elems.push(stash[ty]);
                    }
                }
                TypeRefAstKind::Tuple(stash.alloc_slice(&elems))
            }
            "never_type" => TypeRefAstKind::Never,
            "inferred_type" => TypeRefAstKind::Infer,
            _ => TypeRefAstKind::Error,
        };
        stash.alloc(TypeRefAst { kind, span })
    }

    fn lower_path_to_stash(&self, stash: &mut Stash, node: Node<'_>) -> Ptr<PathAst<'db>> {
        let mut segments = Vec::new();
        self.collect_path_segments_to_stash(stash, node, &mut segments);
        let segs = stash.alloc_slice(&segments);
        stash.alloc(PathAst {
            segments: segs,
            span: self.rel_span(node),
        })
    }

    fn collect_path_segments_to_stash(
        &self,
        stash: &mut Stash,
        node: Node<'_>,
        out: &mut Vec<PathSegmentAst<'db>>,
    ) {
        let db = self.db();
        let empty_args = stash.alloc_slice::<TypeRefAst<'db>>(&[]);
        match node.kind() {
            "scoped_identifier" | "scoped_type_identifier" => {
                if let Some(prefix) = node.child_by_field_name("path") {
                    self.collect_path_segments_to_stash(stash, prefix, out);
                } else if node.child(0).is_some_and(|c| c.kind() == "::") {
                    out.push(PathSegmentAst {
                        name: Name::new(db, String::new()),
                        type_args: empty_args,
                        span: self.rel_span(node),
                    });
                }
                if let Some(name) = node.child_by_field_name("name") {
                    out.push(PathSegmentAst {
                        name: self.intern_name(name),
                        type_args: empty_args,
                        span: self.rel_span(name),
                    });
                }
            }
            "generic_type" => {
                if let Some(ty) = node.child_by_field_name("type") {
                    self.collect_path_segments_to_stash(stash, ty, out);
                }
                if let Some(args_node) = node.child_by_field_name("type_arguments") {
                    let mut type_args = Vec::new();
                    let mut cursor = args_node.walk();
                    for child in args_node.named_children(&mut cursor) {
                        if child.kind() == "lifetime" {
                            continue;
                        }
                        let ty = self.lower_type_ref_to_stash(stash, child);
                        type_args.push(stash[ty]);
                    }
                    let type_args = stash.alloc_slice(&type_args);
                    if let Some(last) = out.last_mut() {
                        last.type_args = type_args;
                    }
                }
            }
            "identifier" | "type_identifier" | "primitive_type" | "metavariable" => {
                out.push(PathSegmentAst {
                    name: self.intern_name(node),
                    type_args: empty_args,
                    span: self.rel_span(node),
                });
            }
            "self" => out.push(PathSegmentAst {
                name: Name::new(db, "self".to_owned()),
                type_args: empty_args,
                span: self.rel_span(node),
            }),
            "crate" => out.push(PathSegmentAst {
                name: Name::new(db, "crate".to_owned()),
                type_args: empty_args,
                span: self.rel_span(node),
            }),
            "super" => out.push(PathSegmentAst {
                name: Name::new(db, "super".to_owned()),
                type_args: empty_args,
                span: self.rel_span(node),
            }),
            _ => {
                out.push(PathSegmentAst {
                    name: Name::new(db, self.node_text(node).to_owned()),
                    type_args: empty_args,
                    span: self.rel_span(node),
                });
            }
        }
    }

    fn make_simple_path_type(
        &self,
        stash: &mut Stash,
        name: &str,
        span: RelativeSpan,
    ) -> Ptr<TypeRefAst<'db>> {
        let db = self.db();
        let empty_args = stash.alloc_slice::<TypeRefAst<'db>>(&[]);
        let seg = PathSegmentAst {
            name: Name::new(db, name.to_owned()),
            type_args: empty_args,
            span,
        };
        let segs = stash.alloc_slice(&[seg]);
        let path = stash.alloc(PathAst {
            segments: segs,
            span,
        });
        stash.alloc(TypeRefAst {
            kind: TypeRefAstKind::Path(path),
            span,
        })
    }

    // -- Item lowering --------------------------------------------------------

    fn lower_function(&self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> FnAst<'db> {
        let db = self.db();
        let name_node = node
            .child_by_field_name("name")
            .expect("function has no name");
        let name = self.intern_name(name_node);
        let span = self.abs_span(node);

        let is_async = node.children(&mut node.walk()).any(|c| c.kind() == "async");
        let is_unsafe = node
            .children(&mut node.walk())
            .any(|c| c.kind() == "unsafe");

        let signature = self.lower_fn_sig_ast(node);
        let body = self.lower_body(node);

        FnAst::new(db, name, attrs, signature, is_async, is_unsafe, body, span)
    }

    fn lower_struct(&self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> StructAst<'db> {
        let db = self.db();
        let name = self.intern_name(
            node.child_by_field_name("name")
                .expect("struct has no name"),
        );
        let span = self.abs_span(node);

        let body = node.child_by_field_name("body");
        let kind = match body {
            None => StructKind::Unit,
            Some(b) if b.kind() == "field_declaration_list" => StructKind::Braced,
            Some(_) => StructKind::Tuple,
        };

        let signature = self.lower_struct_sig_ast(node);
        StructAst::new(db, name, kind, attrs, signature, span)
    }

    fn lower_enum(&self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> EnumAst<'db> {
        let db = self.db();
        let name = self.intern_name(node.child_by_field_name("name").expect("enum has no name"));
        let span = self.abs_span(node);

        let signature = self.lower_enum_sig_ast(node);
        EnumAst::new(db, name, attrs, signature, span)
    }

    fn lower_trait(&self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> TraitAst<'db> {
        let db = self.db();
        let name = self.intern_name(node.child_by_field_name("name").expect("trait has no name"));
        let span = self.abs_span(node);

        let mut stash = Stash::new();
        let generics = self.lower_generic_params_ast(&mut stash, node);
        let root = stash.alloc(TraitSigAstData { generics });
        let signature = Stashed::new(stash, root);

        let items = node
            .child_by_field_name("body")
            .map(|body| self.parent.lower_items(body))
            .unwrap_or_default();

        TraitAst::new(db, name, attrs, signature, items, span)
    }

    fn lower_impl(&self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> ImplAst<'db> {
        let db = self.db();
        let span = self.abs_span(node);

        let signature = self.lower_impl_sig_ast(node);

        let items = node
            .child_by_field_name("body")
            .map(|body| self.parent.lower_items(body))
            .unwrap_or_default();

        ImplAst::new(db, attrs, signature, items, span)
    }

    fn lower_type_alias(&self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> TypeAliasAst<'db> {
        let db = self.db();
        let name = self.intern_name(
            node.child_by_field_name("name")
                .expect("type alias has no name"),
        );
        let span = self.abs_span(node);
        let (generics, sig_ty, mut stash) = self.lower_simple_ty_sig_ast(node);
        let root = stash.alloc(TypeAliasSigAstData {
            generics,
            ty: sig_ty,
        });
        let signature = Stashed::new(stash, root);
        TypeAliasAst::new(db, name, attrs, signature, span)
    }

    fn lower_const(&self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> ConstAst<'db> {
        let db = self.db();
        let name = self.intern_name(node.child_by_field_name("name").expect("const has no name"));
        let span = self.abs_span(node);
        let mut stash = Stash::new();
        let sig_ty = node
            .child_by_field_name("type")
            .map(|n| self.lower_type_ref_to_stash(&mut stash, n));
        let root = stash.alloc(ConstSigAstData { ty: sig_ty });
        let signature = Stashed::new(stash, root);
        ConstAst::new(db, name, attrs, signature, span)
    }

    fn lower_static(&self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> StaticAst<'db> {
        let db = self.db();
        let name = self.intern_name(
            node.child_by_field_name("name")
                .expect("static has no name"),
        );
        let span = self.abs_span(node);
        let is_mut = node
            .children(&mut node.walk())
            .any(|c| c.kind() == "mutable_specifier");
        let mut stash = Stash::new();
        let sig_ty = node
            .child_by_field_name("type")
            .map(|n| self.lower_type_ref_to_stash(&mut stash, n));
        let root = stash.alloc(StaticSigAstData { ty: sig_ty });
        let signature = Stashed::new(stash, root);
        StaticAst::new(db, name, attrs, signature, is_mut, span)
    }

    fn lower_mod(&self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> ModAst<'db> {
        let db = self.db();
        let name = self.intern_name(node.child_by_field_name("name").expect("mod has no name"));
        let span = self.abs_span(node);
        let items = node
            .child_by_field_name("body")
            .map(|body| self.parent.lower_items(body));
        ModAst::new(db, name, None, None, attrs, items, span)
    }

    fn lower_use(&self, node: Node<'_>, attrs: Vec<Attr<'db>>) -> ItemAst<'db> {
        let db = self.db();
        let span = self.abs_span(node);
        let mut raw_imports = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "visibility_modifier" => {}
                _ => {
                    let mut prefix = Vec::new();
                    self.flatten_use_tree(child, &mut prefix, &mut raw_imports);
                    break;
                }
            }
        }
        let mut stash = sage_stash::Stash::new();
        let import_asts: Vec<_> = raw_imports
            .into_iter()
            .map(|(path_segs, kind, span)| {
                let path = stash.alloc_slice(&path_segs);
                UseImportAst { path, kind, span }
            })
            .collect();
        let imports_slice = stash.alloc_slice(&import_asts);
        let imports = sage_stash::Stashed::new(stash, imports_slice);
        ItemAst::Use(UseGroupAst::new(db, attrs, imports, span))
    }

    fn flatten_use_tree(
        &self,
        node: Node<'_>,
        prefix: &mut Vec<Name<'db>>,
        out: &mut Vec<(Vec<Name<'db>>, UseKind<'db>, RelativeSpan)>,
    ) {
        let db = self.db();
        match node.kind() {
            "scoped_use_list" => {
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
                let mut path_segs = prefix.to_vec();
                if let Some(path_node) = node.child_by_field_name("path") {
                    self.collect_path_segments(path_node, &mut path_segs);
                }
                let alias_node = node.child_by_field_name("alias");
                let alias_text = alias_node.map(|n| self.node_text(n));
                let kind = match alias_text {
                    Some("_") => UseKind::Unnamed,
                    Some(t) => UseKind::Named(Name::new(db, t.to_owned())),
                    None => {
                        let last = path_segs
                            .last()
                            .copied()
                            .unwrap_or_else(|| Name::new(db, "?".to_owned()));
                        UseKind::Named(last)
                    }
                };
                out.push((path_segs, kind, self.rel_span(node)));
            }
            "use_wildcard" => {
                let mut path_segs = prefix.to_vec();
                let path_node = node.named_children(&mut node.walk()).find(|c| {
                    matches!(
                        c.kind(),
                        "identifier" | "scoped_identifier" | "self" | "crate" | "super"
                    )
                });
                if let Some(pn) = path_node {
                    self.collect_path_segments(pn, &mut path_segs);
                }
                out.push((path_segs, UseKind::Glob, self.rel_span(node)));
            }
            "scoped_identifier" => {
                let mut path_segs = prefix.to_vec();
                self.collect_path_segments(node, &mut path_segs);
                let last = path_segs
                    .last()
                    .copied()
                    .unwrap_or_else(|| Name::new(db, "?".to_owned()));
                out.push((path_segs, UseKind::Named(last), self.rel_span(node)));
            }
            "identifier" | "type_identifier" => {
                let name = self.intern_name(node);
                let mut path_segs = prefix.to_vec();
                path_segs.push(name);
                out.push((path_segs, UseKind::Named(name), self.rel_span(node)));
            }
            "self" => {
                let path_segs = prefix.to_vec();
                let alias = prefix
                    .last()
                    .copied()
                    .unwrap_or_else(|| Name::new(db, "self".to_owned()));
                out.push((path_segs, UseKind::Named(alias), self.rel_span(node)));
            }
            "crate" | "super" => {
                let name = Name::new(db, node.kind().to_owned());
                let mut path_segs = prefix.to_vec();
                path_segs.push(name);
                out.push((path_segs, UseKind::Named(name), self.rel_span(node)));
            }
            _ => {
                let name = Name::new(db, self.node_text(node).to_owned());
                let mut path_segs = prefix.to_vec();
                path_segs.push(name);
                out.push((path_segs, UseKind::Named(name), self.rel_span(node)));
            }
        }
    }

    // -- Macros ---------------------------------------------------------------

    fn lower_macro_def_item(&self, node: Node<'_>) -> ItemAst<'db> {
        let db = self.db();
        let name_node = match node.child_by_field_name("name") {
            Some(n) => n,
            None => return ItemAst::Error(self.abs_span(node)),
        };
        let name = self.intern_name(name_node);
        let span = self.abs_span(node);
        let body_tokens = crate::ts_helpers::extract_macro_body_tokens(node, self.parent.text);

        ItemAst::MacroDef(MacroDefAst::new(db, name, body_tokens, span))
    }

    fn lower_expression_statement(&self, node: Node<'_>) -> ItemAst<'db> {
        let invoc = node
            .named_children(&mut node.walk())
            .find(|c| c.kind() == "macro_invocation");
        match invoc {
            Some(invoc_node) => self.lower_macro_invocation_item(invoc_node),
            None => ItemAst::Error(self.abs_span(node)),
        }
    }

    fn lower_macro_invocation_item(&self, node: Node<'_>) -> ItemAst<'db> {
        let db = self.db();
        let macro_node = match node.child_by_field_name("macro") {
            Some(n) => n,
            None => return ItemAst::Error(self.abs_span(node)),
        };
        let segments =
            crate::ts_helpers::collect_macro_path_segments(db, macro_node, self.parent.text);
        if segments.is_empty() {
            return ItemAst::Error(self.abs_span(node));
        }
        let span = self.abs_span(node);
        let input_tokens =
            crate::ts_helpers::extract_macro_invocation_tokens(node, self.parent.text);
        ItemAst::MacroInvocation(MacroInvocationAst::new(db, segments, input_tokens, span))
    }

    fn collect_path_segments(&self, node: Node<'_>, out: &mut Vec<Name<'db>>) {
        let db = self.db();
        match node.kind() {
            "scoped_identifier" | "scoped_type_identifier" => {
                if let Some(prefix) = node.child_by_field_name("path") {
                    self.collect_path_segments(prefix, out);
                } else if node.child(0).is_some_and(|c| c.kind() == "::") {
                    out.push(Name::new(db, String::new()));
                }
                if let Some(name) = node.child_by_field_name("name") {
                    out.push(self.intern_name(name));
                }
            }
            "generic_type" => {
                if let Some(ty) = node.child_by_field_name("type") {
                    self.collect_path_segments(ty, out);
                }
            }
            "identifier" | "type_identifier" | "primitive_type" | "metavariable" => {
                out.push(self.intern_name(node));
            }
            "self" => out.push(Name::new(db, "self".to_owned())),
            "crate" => out.push(Name::new(db, "crate".to_owned())),
            "super" => out.push(Name::new(db, "super".to_owned())),
            _ => {
                out.push(Name::new(db, self.node_text(node).to_owned()));
            }
        }
    }

    // -- Body lowering --------------------------------------------------------

    fn lower_body(&self, fn_node: Node<'_>) -> FunctionBody<'db> {
        let mut bcx = BodyLowerCtx::new(self.db(), self.parent.text, self.item_start);
        let body_node = fn_node.child_by_field_name("body");
        let root_expr = body_node
            .map(|n| bcx.lower_expr(n))
            .unwrap_or_else(|| bcx.missing_expr(fn_node));
        let body = bcx.stash.alloc(Body {
            root: root_expr,
            span: self.rel_span(fn_node),
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
    item_start: u32,
    stash: Stash,
}

impl<'db> BodyLowerCtx<'db> {
    fn new(db: &'db dyn Db, text: &'db str, item_start: u32) -> Self {
        Self {
            db,
            text,
            item_start,
            stash: Stash::new(),
        }
    }

    fn node_text(&self, node: Node<'_>) -> &'db str {
        &self.text[node.byte_range()]
    }

    fn rel_span(&self, node: Node<'_>) -> RelativeSpan {
        RelativeSpan {
            start: node.start_byte() as u32 - self.item_start,
            end: node.end_byte() as u32 - self.item_start,
        }
    }

    fn name(&self, node: Node<'_>) -> Name<'db> {
        Name::new(self.db, self.node_text(node).to_owned())
    }

    fn missing_expr(&mut self, node: Node<'_>) -> Ptr<Expr<'db>> {
        self.stash.alloc(Expr {
            kind: ExprKind::Missing,
            span: self.rel_span(node),
        })
    }

    fn lower_path_ast(&mut self, node: Node<'_>) -> Ptr<PathAst<'db>> {
        let mut segments = Vec::new();
        self.collect_path_segments_ast(node, &mut segments);
        let segs = self.stash.alloc_slice(&segments);
        self.stash.alloc(PathAst {
            segments: segs,
            span: self.rel_span(node),
        })
    }

    fn collect_path_segments_ast(&mut self, node: Node<'_>, out: &mut Vec<PathSegmentAst<'db>>) {
        let empty_args = self.stash.alloc_slice::<TypeRefAst<'db>>(&[]);
        match node.kind() {
            "scoped_identifier" | "scoped_type_identifier" => {
                if let Some(prefix) = node.child_by_field_name("path") {
                    self.collect_path_segments_ast(prefix, out);
                } else if node.child(0).is_some_and(|c| c.kind() == "::") {
                    out.push(PathSegmentAst {
                        name: Name::new(self.db, String::new()),
                        type_args: empty_args,
                        span: self.rel_span(node),
                    });
                }
                if let Some(name) = node.child_by_field_name("name") {
                    out.push(PathSegmentAst {
                        name: self.name(name),
                        type_args: empty_args,
                        span: self.rel_span(name),
                    });
                }
            }
            "generic_type" => {
                if let Some(ty) = node.child_by_field_name("type") {
                    self.collect_path_segments_ast(ty, out);
                }
                if let Some(args_node) = node.child_by_field_name("type_arguments") {
                    let mut type_args = Vec::new();
                    let mut cursor = args_node.walk();
                    for child in args_node.named_children(&mut cursor) {
                        type_args.push(self.lower_type_ref_ast(child));
                    }
                    let type_args = self.stash.alloc_slice(&type_args);
                    if let Some(last) = out.last_mut() {
                        last.type_args = type_args;
                    }
                }
            }
            "identifier" | "type_identifier" | "primitive_type" | "metavariable" => {
                out.push(PathSegmentAst {
                    name: self.name(node),
                    type_args: empty_args,
                    span: self.rel_span(node),
                });
            }
            "self" => out.push(PathSegmentAst {
                name: Name::new(self.db, "self".to_owned()),
                type_args: empty_args,
                span: self.rel_span(node),
            }),
            "crate" => out.push(PathSegmentAst {
                name: Name::new(self.db, "crate".to_owned()),
                type_args: empty_args,
                span: self.rel_span(node),
            }),
            "super" => out.push(PathSegmentAst {
                name: Name::new(self.db, "super".to_owned()),
                type_args: empty_args,
                span: self.rel_span(node),
            }),
            _ => {
                out.push(PathSegmentAst {
                    name: Name::new(self.db, self.node_text(node).to_owned()),
                    type_args: empty_args,
                    span: self.rel_span(node),
                });
            }
        }
    }

    fn lower_type_ref_ast(&mut self, node: Node<'_>) -> TypeRefAst<'db> {
        let span = self.rel_span(node);
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
            | "metavariable" => TypeRefAstKind::Path(self.lower_path_ast(node)),
            "reference_type" => {
                let is_mut = node
                    .children(&mut node.walk())
                    .any(|c| c.kind() == "mutable_specifier");
                let inner = node
                    .child_by_field_name("type")
                    .map(|n| self.lower_type_ref_ast(n))
                    .unwrap_or_else(|| self.make_error_type_ref(node));
                let inner = self.stash.alloc(inner);
                let mutability = if is_mut {
                    Mutability::Mut
                } else {
                    Mutability::Shared
                };
                TypeRefAstKind::Reference(inner, mutability)
            }
            "array_type" => {
                let inner = node
                    .child_by_field_name("element")
                    .map(|n| self.lower_type_ref_ast(n))
                    .unwrap_or_else(|| self.make_error_type_ref(node));
                let inner = self.stash.alloc(inner);
                TypeRefAstKind::Array(inner)
            }
            "slice_type" => {
                let inner = node
                    .child_by_field_name("element")
                    .map(|n| self.lower_type_ref_ast(n))
                    .unwrap_or_else(|| self.make_error_type_ref(node));
                let inner = self.stash.alloc(inner);
                TypeRefAstKind::Slice(inner)
            }
            "tuple_type" => {
                let mut elems = Vec::new();
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.is_named() && child.kind() != "(" && child.kind() != ")" {
                        elems.push(self.lower_type_ref_ast(child));
                    }
                }
                TypeRefAstKind::Tuple(self.stash.alloc_slice(&elems))
            }
            "never_type" => TypeRefAstKind::Never,
            "inferred_type" => TypeRefAstKind::Infer,
            _ => TypeRefAstKind::Error,
        };
        TypeRefAst { kind, span }
    }

    fn make_error_type_ref(&self, node: Node<'_>) -> TypeRefAst<'db> {
        TypeRefAst {
            kind: TypeRefAstKind::Error,
            span: self.rel_span(node),
        }
    }

    fn make_error_path(&mut self, node: Node<'_>) -> Ptr<PathAst<'db>> {
        let empty_args = self.stash.alloc_slice::<TypeRefAst<'db>>(&[]);
        let seg = PathSegmentAst {
            name: Name::new(self.db, "?".to_owned()),
            type_args: empty_args,
            span: self.rel_span(node),
        };
        let segs = self.stash.alloc_slice(&[seg]);
        self.stash.alloc(PathAst {
            segments: segs,
            span: self.rel_span(node),
        })
    }

    // -- Expressions ----------------------------------------------------------

    fn lower_expr(&mut self, node: Node<'_>) -> Ptr<Expr<'db>> {
        let span = self.rel_span(node);
        let kind = match node.kind() {
            "block" => return self.lower_block(node),
            "integer_literal" => ExprKind::Literal(Literal::Int),
            "float_literal" => ExprKind::Literal(Literal::Float),
            "string_literal" | "raw_string_literal" => ExprKind::Literal(Literal::String),
            "char_literal" => ExprKind::Literal(Literal::Char),
            "boolean_literal" => ExprKind::Literal(Literal::Bool(self.node_text(node) == "true")),
            "identifier" | "scoped_identifier" | "self" => {
                ExprKind::Path(self.lower_path_ast(node))
            }
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
                    .map(|n| self.lower_type_ref_ast(n))
                    .unwrap_or_else(|| self.make_error_type_ref(node));
                let ty = self.stash.alloc(ty);
                ExprKind::Cast(expr, ty)
            }
            "struct_expression" => return self.lower_struct_lit(node),
            "range_expression" => {
                let children: Vec<_> = node.named_children(&mut node.walk()).collect();
                let (lo, hi) = match children.len() {
                    0 => (None, None),
                    1 => {
                        let c = self.lower_expr(children[0]);
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
                    .map(|n| self.lower_path_ast(n))
                    .unwrap_or_else(|| self.make_error_path(node));
                let args_node = node
                    .named_children(&mut node.walk())
                    .find(|c| c.kind() == "token_tree");
                let args = TokenTree::new(
                    self.db,
                    args_node
                        .map(|n| self.node_text(n))
                        .unwrap_or("")
                        .to_owned(),
                    args_node
                        .map(|n| self.rel_span(n))
                        .unwrap_or(self.rel_span(node)),
                );
                ExprKind::MacroCall(path, args)
            }
            "let_condition" => {
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
            "line_comment"
            | "block_comment"
            | "attribute_item"
            | "inner_attribute_item"
            | "empty_statement"
            | "use_declaration"
            | "const_item" => ExprKind::Missing,
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
        let span = self.rel_span(node);
        let mut stmts = Vec::new();
        let mut tail: Option<Ptr<Expr<'db>>> = None;
        let mut cursor = node.walk();
        let children: Vec<_> = node.named_children(&mut cursor).collect();

        for (i, child) in children.iter().enumerate() {
            let is_last = i == children.len() - 1;
            match child.kind() {
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
                            span: self.rel_span(*child),
                        });
                    }
                }
                _ if is_last => {
                    tail = Some(self.lower_expr(*child));
                }
                _ => {
                    let expr = self.lower_expr(*child);
                    stmts.push(Stmt {
                        kind: StmtKind::Expr(expr),
                        span: self.rel_span(*child),
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
        let ty = node.child_by_field_name("type").map(|n| {
            let ty_ref = self.lower_type_ref_ast(n);
            self.stash.alloc(ty_ref)
        });
        let init = node
            .child_by_field_name("value")
            .map(|n| self.lower_expr(n));
        Stmt {
            kind: StmtKind::Let(pat, ty, init),
            span: self.rel_span(node),
        }
    }

    fn lower_if(&mut self, node: Node<'_>) -> Ptr<Expr<'db>> {
        let span = self.rel_span(node);
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
        let span = self.rel_span(node);
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
                        .child_by_field_name("condition")
                        .map(|n| self.lower_expr(n));
                    let body = child
                        .child_by_field_name("value")
                        .map(|n| self.lower_expr(n))
                        .unwrap_or_else(|| self.missing_expr(child));
                    arms.push(MatchArm {
                        pat,
                        guard,
                        body,
                        span: self.rel_span(child),
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
        let span = self.rel_span(node);
        let mut params = Vec::new();
        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.named_children(&mut cursor) {
                let pat = self.lower_pat(child);
                params.push(ClosureParam {
                    pat,
                    ty: None,
                    span: self.rel_span(child),
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
        let span = self.rel_span(node);
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
        let span = self.rel_span(node);
        let path = node
            .child_by_field_name("name")
            .map(|n| self.lower_path_ast(n))
            .unwrap_or_else(|| self.make_error_path(node));
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
                        span: self.rel_span(child),
                    });
                } else if child.kind() == "shorthand_field_initializer" {
                    let name = self.name(child);
                    let path = self.lower_path_ast(child);
                    let value = self.stash.alloc(Expr {
                        kind: ExprKind::Path(path),
                        span: self.rel_span(child),
                    });
                    fields.push(FieldInit {
                        name,
                        value,
                        span: self.rel_span(child),
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

    // -- Patterns -------------------------------------------------------------

    fn lower_pat(&mut self, node: Node<'_>) -> Ptr<Pat<'db>> {
        let span = self.rel_span(node);
        let kind = match node.kind() {
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
                    .map(|n| self.lower_path_ast(n))
                    .unwrap_or_else(|| self.make_error_path(node));
                let pats = self.lower_pat_children(node);
                PatKind::TupleStruct(path, self.stash.alloc_slice(&pats))
            }
            "struct_pattern" => {
                let path = node
                    .child_by_field_name("type")
                    .map(|n| self.lower_path_ast(n))
                    .unwrap_or_else(|| self.make_error_path(node));
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
                                self.stash.alloc(Pat {
                                    kind: PatKind::Bind(name, Mutability::Shared),
                                    span: self.rel_span(child),
                                })
                            });
                        field_pats.push(FieldPat {
                            name,
                            pat,
                            span: self.rel_span(child),
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
            "scoped_identifier" | "scoped_type_identifier" => {
                PatKind::Path(self.lower_path_ast(node))
            }
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
            span: self.rel_span(node),
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
