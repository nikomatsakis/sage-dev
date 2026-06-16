use sage_stash::{Ptr, Stash, Stashed};

use crate::cst::consts::{ConstCst, ConstCstData};
use crate::cst::enums::{EnumCstData, VariantCst};
use crate::cst::fns::{FnCstData, ParamCst};
use crate::cst::impls::ImplCstData;
use crate::cst::statics::{StaticCst, StaticCstData};
use crate::cst::structs::{FieldCst, StructCstData};
use crate::cst::traits::{TraitCstData, TraitItemCst};
use crate::cst::type_aliases::{TypeAliasCst, TypeAliasCstData};
use crate::local_syms::LocalModItemSym;
use crate::local_syms::consts::LocalConstSym;
use crate::local_syms::enums::LocalEnumSym;
use crate::local_syms::fns::LocalFnSym;
use crate::local_syms::impls::LocalImplSym;
use crate::local_syms::macro_defs::LocalMacroDefSym;
use crate::local_syms::macro_invocations::LocalMacroInvocationSym;
use crate::local_syms::mods::{LocalModSym, ModBodySource, unexpanded_items};
use crate::local_syms::statics::LocalStaticSym;
use crate::local_syms::structs::LocalStructSym;
use crate::local_syms::traits::LocalTraitSym;
use crate::local_syms::type_aliases::LocalTypeAliasSym;
use crate::local_syms::uses::LocalUseSym;
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::RelativeSpan;
use crate::ts_helpers;
use crate::types::{UseImportAst, UseKind};

use super::Parser;
use super::util::{absolute_span, item_start, node_name};

impl<'a, 'db> Parser<'a, 'db> {
    // -----------------------------------------------------------------------
    // Functions
    // -----------------------------------------------------------------------

    pub(super) fn parse_fn(
        &self,
        node: tree_sitter::Node<'a>,
        pending_attrs: &[tree_sitter::Node<'a>],
    ) -> LocalModItemSym<'db> {
        let mut stash = Stash::new();
        let start = item_start(node, pending_attrs);
        let name = node_name(self.db, node, self.text);
        let attrs = self.parse_attr_nodes(&mut stash, pending_attrs, start);
        let generics = self.parse_generics(&mut stash, node, start);
        let params = self.parse_fn_params(&mut stash, node, start);
        let ret = self.parse_return_type(&mut stash, node, start);
        let body = node
            .child_by_field_name("body")
            .map(|n| self.parse_expr(&mut stash, n, start));
        let where_clauses = self.parse_where_clauses(&mut stash, node, start);

        let span = RelativeSpan {
            start: 0,
            end: node.end_byte() as u32 - start,
        };
        let cst_data = FnCstData {
            attrs,
            name,
            generics,
            params,
            ret,
            body,
            where_clauses,
            span,
        };
        let root = stash.alloc(cst_data);
        let cst = Stashed::new(stash, root);
        let abs_span = absolute_span(self.source, node, start);

        LocalModItemSym::Function(LocalFnSym::new(self.db, name, self.scope, cst, abs_span))
    }

    fn parse_fn_params(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> sage_stash::Slice<ParamCst<'db>> {
        let params_node = match node.child_by_field_name("parameters") {
            Some(n) => n,
            None => return stash.alloc_slice(&[]),
        };

        let mut params = Vec::new();
        let mut cursor = params_node.walk();
        for child in params_node.children(&mut cursor) {
            match child.kind() {
                "parameter" => {
                    let p_span = RelativeSpan {
                        start: child.start_byte() as u32 - item_start,
                        end: child.end_byte() as u32 - item_start,
                    };
                    let name = child.child_by_field_name("pattern").and_then(|pat| {
                        if pat.kind() == "identifier" {
                            Some(Name::new(self.db, self.text[pat.byte_range()].to_owned()))
                        } else {
                            None
                        }
                    });
                    let ty = child
                        .child_by_field_name("type")
                        .map(|n| self.parse_type(stash, n, item_start))
                        .unwrap_or_else(|| {
                            stash.alloc(crate::cst::ty::TypeCst {
                                kind: crate::cst::ty::TypeCstKind::Error,
                                span: p_span,
                            })
                        });
                    params.push(ParamCst {
                        name,
                        ty,
                        span: p_span,
                    });
                }
                "self_parameter" => {
                    let p_span = RelativeSpan {
                        start: child.start_byte() as u32 - item_start,
                        end: child.end_byte() as u32 - item_start,
                    };
                    let name = Some(Name::new(self.db, "self".to_owned()));
                    let ty = stash.alloc(crate::cst::ty::TypeCst {
                        kind: crate::cst::ty::TypeCstKind::Infer,
                        span: p_span,
                    });
                    params.push(ParamCst {
                        name,
                        ty,
                        span: p_span,
                    });
                }
                _ => {}
            }
        }
        stash.alloc_slice(&params)
    }

    fn parse_return_type(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Option<Ptr<crate::cst::ty::TypeCst<'db>>> {
        node.child_by_field_name("return_type")
            .map(|n| self.parse_type(stash, n, item_start))
    }

    // -----------------------------------------------------------------------
    // Structs
    // -----------------------------------------------------------------------

    pub(super) fn parse_struct(
        &self,
        node: tree_sitter::Node<'a>,
        pending_attrs: &[tree_sitter::Node<'a>],
    ) -> LocalModItemSym<'db> {
        let mut stash = Stash::new();
        let start = item_start(node, pending_attrs);
        let name = node_name(self.db, node, self.text);
        let attrs = self.parse_attr_nodes(&mut stash, pending_attrs, start);
        let generics = self.parse_generics(&mut stash, node, start);
        let fields = self.parse_struct_fields(&mut stash, node, start);
        let where_clauses = self.parse_where_clauses(&mut stash, node, start);

        let span = RelativeSpan {
            start: 0,
            end: node.end_byte() as u32 - start,
        };
        let cst_data = StructCstData {
            attrs,
            name,
            generics,
            fields,
            where_clauses,
            span,
        };
        let root = stash.alloc(cst_data);
        let cst = Stashed::new(stash, root);
        let abs_span = absolute_span(self.source, node, start);

        LocalModItemSym::Struct(LocalStructSym::new(
            self.db, name, self.scope, cst, abs_span,
        ))
    }

    fn parse_struct_fields(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> sage_stash::Slice<FieldCst<'db>> {
        let body = match node.child_by_field_name("body") {
            Some(n) => n,
            None => return stash.alloc_slice(&[]),
        };

        match body.kind() {
            "field_declaration_list" => {
                let mut fields = Vec::new();
                let mut cursor = body.walk();
                for child in body.children(&mut cursor) {
                    if child.kind() == "field_declaration" {
                        let f_span = RelativeSpan {
                            start: child.start_byte() as u32 - item_start,
                            end: child.end_byte() as u32 - item_start,
                        };
                        let name = child
                            .child_by_field_name("name")
                            .map(|n| Name::new(self.db, self.text[n.byte_range()].to_owned()))
                            .unwrap_or_else(|| Name::new(self.db, "_".to_owned()));
                        let ty = child
                            .child_by_field_name("type")
                            .map(|n| self.parse_type(stash, n, item_start))
                            .unwrap_or_else(|| {
                                stash.alloc(crate::cst::ty::TypeCst {
                                    kind: crate::cst::ty::TypeCstKind::Error,
                                    span: f_span,
                                })
                            });
                        fields.push(FieldCst {
                            name,
                            ty,
                            span: f_span,
                        });
                    }
                }
                stash.alloc_slice(&fields)
            }
            "ordered_field_declaration_list" => {
                let mut fields = Vec::new();
                let mut idx = 0u32;
                let mut cursor = body.walk();
                for child in body.children(&mut cursor) {
                    if child.is_named()
                        && child.kind() != ","
                        && child.kind() != "visibility_modifier"
                    {
                        let f_span = RelativeSpan {
                            start: child.start_byte() as u32 - item_start,
                            end: child.end_byte() as u32 - item_start,
                        };
                        let name = Name::new(self.db, idx.to_string());
                        let ty = self.parse_type(stash, child, item_start);
                        fields.push(FieldCst {
                            name,
                            ty,
                            span: f_span,
                        });
                        idx += 1;
                    }
                }
                stash.alloc_slice(&fields)
            }
            _ => stash.alloc_slice(&[]),
        }
    }

    // -----------------------------------------------------------------------
    // Enums
    // -----------------------------------------------------------------------

    pub(super) fn parse_enum(
        &self,
        node: tree_sitter::Node<'a>,
        pending_attrs: &[tree_sitter::Node<'a>],
    ) -> LocalModItemSym<'db> {
        let mut stash = Stash::new();
        let start = item_start(node, pending_attrs);
        let name = node_name(self.db, node, self.text);
        let attrs = self.parse_attr_nodes(&mut stash, pending_attrs, start);
        let generics = self.parse_generics(&mut stash, node, start);
        let where_clauses = self.parse_where_clauses(&mut stash, node, start);

        let mut variants = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if child.kind() == "enum_variant" {
                    variants.push(self.parse_enum_variant(&mut stash, child, start));
                }
            }
        }

        let span = RelativeSpan {
            start: 0,
            end: node.end_byte() as u32 - start,
        };
        let cst_data = EnumCstData {
            attrs,
            name,
            generics,
            variants: stash.alloc_slice(&variants),
            where_clauses,
            span,
        };
        let root = stash.alloc(cst_data);
        let cst = Stashed::new(stash, root);
        let abs_span = absolute_span(self.source, node, start);

        LocalModItemSym::Enum(LocalEnumSym::new(self.db, name, self.scope, cst, abs_span))
    }

    fn parse_enum_variant(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> VariantCst<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };
        let name = node
            .child_by_field_name("name")
            .map(|n| Name::new(self.db, self.text[n.byte_range()].to_owned()))
            .unwrap_or_else(|| Name::new(self.db, "_".to_owned()));

        let mut fields = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            match body.kind() {
                "field_declaration_list" => {
                    let mut cursor = body.walk();
                    for child in body.children(&mut cursor) {
                        if child.kind() == "field_declaration" {
                            let f_span = RelativeSpan {
                                start: child.start_byte() as u32 - item_start,
                                end: child.end_byte() as u32 - item_start,
                            };
                            let f_name = child
                                .child_by_field_name("name")
                                .map(|n| Name::new(self.db, self.text[n.byte_range()].to_owned()))
                                .unwrap_or_else(|| Name::new(self.db, "_".to_owned()));
                            let ty = child
                                .child_by_field_name("type")
                                .map(|n| self.parse_type(stash, n, item_start))
                                .unwrap_or_else(|| {
                                    stash.alloc(crate::cst::ty::TypeCst {
                                        kind: crate::cst::ty::TypeCstKind::Error,
                                        span: f_span,
                                    })
                                });
                            fields.push(FieldCst {
                                name: f_name,
                                ty,
                                span: f_span,
                            });
                        }
                    }
                }
                "ordered_field_declaration_list" => {
                    let mut idx = 0u32;
                    let mut cursor = body.walk();
                    for child in body.children(&mut cursor) {
                        if child.is_named() && child.kind() != "," {
                            let f_span = RelativeSpan {
                                start: child.start_byte() as u32 - item_start,
                                end: child.end_byte() as u32 - item_start,
                            };
                            let f_name = Name::new(self.db, idx.to_string());
                            let ty = self.parse_type(stash, child, item_start);
                            fields.push(FieldCst {
                                name: f_name,
                                ty,
                                span: f_span,
                            });
                            idx += 1;
                        }
                    }
                }
                _ => {}
            }
        }

        VariantCst {
            name,
            fields: stash.alloc_slice(&fields),
            discriminant: None,
            span,
        }
    }

    // -----------------------------------------------------------------------
    // Traits
    // -----------------------------------------------------------------------

    pub(super) fn parse_trait(
        &self,
        node: tree_sitter::Node<'a>,
        pending_attrs: &[tree_sitter::Node<'a>],
    ) -> LocalModItemSym<'db> {
        let mut stash = Stash::new();
        let start = item_start(node, pending_attrs);
        let name = node_name(self.db, node, self.text);
        let attrs = self.parse_attr_nodes(&mut stash, pending_attrs, start);
        let generics = self.parse_generics(&mut stash, node, start);
        let where_clauses = self.parse_where_clauses(&mut stash, node, start);
        let items = self.parse_trait_body(&mut stash, node, start);

        let span = RelativeSpan {
            start: 0,
            end: node.end_byte() as u32 - start,
        };
        let cst_data = TraitCstData {
            attrs,
            name,
            generics,
            where_clauses,
            items,
            span,
        };
        let root = stash.alloc(cst_data);
        let cst = Stashed::new(stash, root);
        let abs_span = absolute_span(self.source, node, start);

        LocalModItemSym::Trait(LocalTraitSym::new(self.db, name, self.scope, cst, abs_span))
    }

    fn parse_trait_body(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> sage_stash::Slice<TraitItemCst<'db>> {
        let body = match node.child_by_field_name("body") {
            Some(n) => n,
            None => return stash.alloc_slice(&[]),
        };

        let mut items = Vec::new();
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            match child.kind() {
                "function_item" | "function_signature_item" => {
                    let fn_data = self.parse_fn_cst_data(stash, child, item_start);
                    let ptr = stash.alloc(fn_data);
                    items.push(TraitItemCst::Fn(ptr));
                }
                "associated_type" | "type_item" => {
                    let ta_data = self.parse_type_alias_cst_data(stash, child, item_start);
                    let ptr = stash.alloc(ta_data);
                    items.push(TraitItemCst::Type(ptr));
                }
                "const_item" => {
                    let c_data = self.parse_const_cst_data(stash, child, item_start);
                    let ptr = stash.alloc(c_data);
                    items.push(TraitItemCst::Const(ptr));
                }
                _ => {}
            }
        }
        stash.alloc_slice(&items)
    }

    // -----------------------------------------------------------------------
    // Impls
    // -----------------------------------------------------------------------

    pub(super) fn parse_impl(
        &self,
        node: tree_sitter::Node<'a>,
        pending_attrs: &[tree_sitter::Node<'a>],
    ) -> LocalModItemSym<'db> {
        let mut stash = Stash::new();
        let start = item_start(node, pending_attrs);
        let attrs = self.parse_attr_nodes(&mut stash, pending_attrs, start);
        let generics = self.parse_generics(&mut stash, node, start);
        let where_clauses = self.parse_where_clauses(&mut stash, node, start);

        let self_ty = node
            .child_by_field_name("type")
            .map(|n| self.parse_type(&mut stash, n, start))
            .unwrap_or_else(|| {
                let s = RelativeSpan {
                    start: 0,
                    end: node.end_byte() as u32 - start,
                };
                stash.alloc(crate::cst::ty::TypeCst {
                    kind: crate::cst::ty::TypeCstKind::Error,
                    span: s,
                })
            });

        let trait_path = node
            .child_by_field_name("trait")
            .map(|n| self.parse_path_from_type_node(&mut stash, n, start));

        let items = self.parse_impl_body(&mut stash, node, start);

        let span = RelativeSpan {
            start: 0,
            end: node.end_byte() as u32 - start,
        };
        let cst_data = ImplCstData {
            attrs,
            generics,
            self_ty,
            trait_path,
            where_clauses,
            items,
            span,
        };
        let root = stash.alloc(cst_data);
        let cst = Stashed::new(stash, root);
        let abs_span = absolute_span(self.source, node, start);

        LocalModItemSym::Impl(LocalImplSym::new(self.db, self.scope, cst, abs_span))
    }

    fn parse_impl_body(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> sage_stash::Slice<TraitItemCst<'db>> {
        let body = match node.child_by_field_name("body") {
            Some(n) => n,
            None => return stash.alloc_slice(&[]),
        };

        let mut items = Vec::new();
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            match child.kind() {
                "function_item" | "function_signature_item" => {
                    let fn_data = self.parse_fn_cst_data(stash, child, item_start);
                    let ptr = stash.alloc(fn_data);
                    items.push(TraitItemCst::Fn(ptr));
                }
                "associated_type" | "type_item" => {
                    let ta_data = self.parse_type_alias_cst_data(stash, child, item_start);
                    let ptr = stash.alloc(ta_data);
                    items.push(TraitItemCst::Type(ptr));
                }
                "const_item" => {
                    let c_data = self.parse_const_cst_data(stash, child, item_start);
                    let ptr = stash.alloc(c_data);
                    items.push(TraitItemCst::Const(ptr));
                }
                _ => {}
            }
        }
        stash.alloc_slice(&items)
    }

    // -----------------------------------------------------------------------
    // Modules
    // -----------------------------------------------------------------------

    pub(super) fn parse_mod(
        &self,
        node: tree_sitter::Node<'a>,
        pending_attrs: &[tree_sitter::Node<'a>],
    ) -> LocalModItemSym<'db> {
        let start = item_start(node, pending_attrs);
        let name = node_name(self.db, node, self.text);
        let abs_span = absolute_span(self.source, node, start);

        let mut attr_stash = Stash::new();
        let outer_attrs = self.parse_attr_nodes(&mut attr_stash, pending_attrs, start);
        let attrs_cst = Stashed::new(attr_stash, outer_attrs);

        if let Some(body) = node.child_by_field_name("body") {
            // Inline mod
            let mod_sym = LocalModSym::new(
                self.db,
                name,
                Some(self.scope),
                ModBodySource::Inline,
                attrs_cst,
                abs_span,
            );

            let child_scope = ScopeSymbol::Module(mod_sym, self.scope.source_root(self.db));
            let child_parser = Parser {
                db: self.db,
                source: self.source,
                scope: child_scope,
                text: self.text,
            };
            let children = child_parser.parse_item_list(body);
            unexpanded_items::specify(self.db, mod_sym, children);

            LocalModItemSym::Mod(mod_sym)
        } else {
            // File-backed mod — file resolution deferred to `unexpanded_items`.
            // We need the SourceFile. Look it up from the source root.
            let source_root = self.scope.source_root(self.db);
            let file = self.resolve_mod_file(name, source_root);
            let body_source = match file {
                Some(f) => ModBodySource::File(f),
                None => ModBodySource::Inline, // fallback, will produce empty items
            };

            let mod_sym = LocalModSym::new(
                self.db,
                name,
                Some(self.scope),
                body_source,
                attrs_cst,
                abs_span,
            );
            LocalModItemSym::Mod(mod_sym)
        }
    }

    fn resolve_mod_file(
        &self,
        name: Name<'db>,
        source_root: crate::resolve::SourceRoot,
    ) -> Option<crate::source::SourceFile> {
        // Determine parent dir from current source file
        let parent_file = match self.source {
            crate::span::ParseSource::SourceFile(f) => f,
            _ => return None,
        };
        let parent_path = parent_file.path(self.db);
        let mod_name = name.text(self.db);
        let parent_dir = parent_dir_for(parent_path);

        let candidates = [
            format!("{parent_dir}{mod_name}.rs"),
            format!("{parent_dir}{mod_name}/mod.rs"),
        ];

        for candidate in &candidates {
            for file in source_root.files(self.db) {
                if file.path(self.db) == candidate.as_str() {
                    return Some(*file);
                }
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // Const
    // -----------------------------------------------------------------------

    pub(super) fn parse_const(
        &self,
        node: tree_sitter::Node<'a>,
        pending_attrs: &[tree_sitter::Node<'a>],
    ) -> LocalModItemSym<'db> {
        let mut stash = Stash::new();
        let start = item_start(node, pending_attrs);
        let name = node_name(self.db, node, self.text);
        let attrs = self.parse_attr_nodes(&mut stash, pending_attrs, start);

        let ty = node
            .child_by_field_name("type")
            .map(|n| self.parse_type(&mut stash, n, start));
        let value = node
            .child_by_field_name("value")
            .map(|n| self.parse_expr(&mut stash, n, start));

        let span = RelativeSpan {
            start: 0,
            end: node.end_byte() as u32 - start,
        };
        let cst_data = ConstCstData {
            attrs,
            name,
            ty,
            value,
            span,
        };
        let root = stash.alloc(cst_data);
        let cst: ConstCst = Stashed::new(stash, root);
        let abs_span = absolute_span(self.source, node, start);

        LocalModItemSym::Const(LocalConstSym::new(self.db, name, self.scope, cst, abs_span))
    }

    // -----------------------------------------------------------------------
    // Static
    // -----------------------------------------------------------------------

    pub(super) fn parse_static(
        &self,
        node: tree_sitter::Node<'a>,
        pending_attrs: &[tree_sitter::Node<'a>],
    ) -> LocalModItemSym<'db> {
        let mut stash = Stash::new();
        let start = item_start(node, pending_attrs);
        let name = node_name(self.db, node, self.text);
        let attrs = self.parse_attr_nodes(&mut stash, pending_attrs, start);

        let is_mut = node
            .children(&mut node.walk())
            .any(|c| c.kind() == "mutable_specifier");
        let ty = node
            .child_by_field_name("type")
            .map(|n| self.parse_type(&mut stash, n, start));
        let value = node
            .child_by_field_name("value")
            .map(|n| self.parse_expr(&mut stash, n, start));

        let span = RelativeSpan {
            start: 0,
            end: node.end_byte() as u32 - start,
        };
        let cst_data = StaticCstData {
            attrs,
            name,
            is_mut,
            ty,
            value,
            span,
        };
        let root = stash.alloc(cst_data);
        let cst: StaticCst = Stashed::new(stash, root);
        let abs_span = absolute_span(self.source, node, start);

        LocalModItemSym::Static(LocalStaticSym::new(
            self.db, name, self.scope, cst, abs_span,
        ))
    }

    // -----------------------------------------------------------------------
    // Type Alias
    // -----------------------------------------------------------------------

    pub(super) fn parse_type_alias(
        &self,
        node: tree_sitter::Node<'a>,
        pending_attrs: &[tree_sitter::Node<'a>],
    ) -> LocalModItemSym<'db> {
        let mut stash = Stash::new();
        let start = item_start(node, pending_attrs);
        let name = node_name(self.db, node, self.text);
        let attrs = self.parse_attr_nodes(&mut stash, pending_attrs, start);
        let generics = self.parse_generics(&mut stash, node, start);
        let where_clauses = self.parse_where_clauses(&mut stash, node, start);
        let ty = node
            .child_by_field_name("type")
            .map(|n| self.parse_type(&mut stash, n, start));

        let span = RelativeSpan {
            start: 0,
            end: node.end_byte() as u32 - start,
        };
        let cst_data = TypeAliasCstData {
            attrs,
            name,
            generics,
            ty,
            where_clauses,
            span,
        };
        let root = stash.alloc(cst_data);
        let cst: TypeAliasCst = Stashed::new(stash, root);
        let abs_span = absolute_span(self.source, node, start);

        LocalModItemSym::TypeAlias(LocalTypeAliasSym::new(
            self.db, name, self.scope, cst, abs_span,
        ))
    }

    // -----------------------------------------------------------------------
    // Use declarations
    // -----------------------------------------------------------------------

    pub(super) fn parse_use(
        &self,
        node: tree_sitter::Node<'a>,
        pending_attrs: &[tree_sitter::Node<'a>],
    ) -> LocalModItemSym<'db> {
        let start = item_start(node, pending_attrs);
        let abs_span = absolute_span(self.source, node, start);

        let mut stash = Stash::new();
        let mut imports = Vec::new();

        if let Some(arg) = node.child_by_field_name("argument") {
            self.collect_use_tree(&mut stash, arg, start, &mut Vec::new(), &mut imports);
        }

        let imports_slice = stash.alloc_slice(&imports);
        let use_imports = Stashed::new(stash, imports_slice);

        LocalModItemSym::Use(LocalUseSym::new(self.db, self.scope, use_imports, abs_span))
    }

    fn collect_use_tree(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
        prefix: &mut Vec<Name<'db>>,
        out: &mut Vec<UseImportAst<'db>>,
    ) {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };

        match node.kind() {
            "identifier" | "self" | "crate" | "super" => {
                let name = Name::new(self.db, self.text[node.byte_range()].to_owned());
                prefix.push(name);
                let path = stash.alloc_slice(prefix);
                out.push(UseImportAst {
                    path,
                    kind: UseKind::Named(name),
                    span,
                });
                prefix.pop();
            }
            "scoped_identifier" => {
                if let Some(path_node) = node.child_by_field_name("path") {
                    self.push_use_path_prefix(path_node, prefix);
                }
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = Name::new(self.db, self.text[name_node.byte_range()].to_owned());
                    prefix.push(name);
                    let path = stash.alloc_slice(prefix);
                    out.push(UseImportAst {
                        path,
                        kind: UseKind::Named(name),
                        span,
                    });
                    prefix.pop();
                }
                // Clean up prefix pushed by push_use_path_prefix
                if node.child_by_field_name("path").is_some() {
                    self.pop_use_path_prefix(node.child_by_field_name("path").unwrap(), prefix);
                }
            }
            "use_as_clause" => {
                // `foo as bar` or `foo as _`
                let mut cursor = node.walk();
                let children: Vec<_> = node.children(&mut cursor).collect();
                if children.len() >= 3 {
                    let source = children[0];
                    let alias = children[children.len() - 1];
                    self.push_use_path_prefix(source, prefix);
                    let source_name = Name::new(self.db, self.text[source.byte_range()].to_owned());
                    prefix.push(source_name);
                    let path = stash.alloc_slice(prefix);
                    prefix.pop();
                    self.pop_use_path_prefix(source, prefix);

                    let alias_text = &self.text[alias.byte_range()];
                    let kind = if alias_text == "_" {
                        UseKind::Unnamed
                    } else {
                        UseKind::Named(Name::new(self.db, alias_text.to_owned()))
                    };
                    out.push(UseImportAst { path, kind, span });
                }
            }
            "use_list" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.is_named() && child.kind() != "," {
                        self.collect_use_tree(stash, child, item_start, prefix, out);
                    }
                }
            }
            "scoped_use_list" => {
                if let Some(path_node) = node.child_by_field_name("path") {
                    self.push_use_path_prefix(path_node, prefix);
                }
                if let Some(list) = node.child_by_field_name("list") {
                    self.collect_use_tree(stash, list, item_start, prefix, out);
                }
                if let Some(path_node) = node.child_by_field_name("path") {
                    self.pop_use_path_prefix(path_node, prefix);
                }
            }
            "use_wildcard" => {
                // `*` or `path::*`
                if let Some(path_node) = node.child_by_field_name("path") {
                    self.push_use_path_prefix(path_node, prefix);
                }
                let path = stash.alloc_slice(prefix);
                out.push(UseImportAst {
                    path,
                    kind: UseKind::Glob,
                    span,
                });
                if let Some(path_node) = node.child_by_field_name("path") {
                    self.pop_use_path_prefix(path_node, prefix);
                }
            }
            _ => {}
        }
    }

    fn push_use_path_prefix(&self, node: tree_sitter::Node<'a>, prefix: &mut Vec<Name<'db>>) {
        match node.kind() {
            "identifier" | "self" | "crate" | "super" => {
                prefix.push(Name::new(self.db, self.text[node.byte_range()].to_owned()));
            }
            "scoped_identifier" => {
                if let Some(path) = node.child_by_field_name("path") {
                    self.push_use_path_prefix(path, prefix);
                }
                if let Some(name) = node.child_by_field_name("name") {
                    prefix.push(Name::new(self.db, self.text[name.byte_range()].to_owned()));
                }
            }
            _ => {
                prefix.push(Name::new(self.db, self.text[node.byte_range()].to_owned()));
            }
        }
    }

    fn pop_use_path_prefix(&self, node: tree_sitter::Node<'a>, prefix: &mut Vec<Name<'db>>) {
        match node.kind() {
            "identifier" | "self" | "crate" | "super" => {
                prefix.pop();
            }
            "scoped_identifier" => {
                if node.child_by_field_name("name").is_some() {
                    prefix.pop();
                }
                if let Some(path) = node.child_by_field_name("path") {
                    self.pop_use_path_prefix(path, prefix);
                }
            }
            _ => {
                prefix.pop();
            }
        }
    }

    // -----------------------------------------------------------------------
    // Macro def
    // -----------------------------------------------------------------------

    pub(super) fn parse_macro_def(
        &self,
        node: tree_sitter::Node<'a>,
        pending_attrs: &[tree_sitter::Node<'a>],
    ) -> LocalModItemSym<'db> {
        let start = item_start(node, pending_attrs);
        let name = node_name(self.db, node, self.text);
        let abs_span = absolute_span(self.source, node, start);
        let body_tokens = ts_helpers::extract_macro_body_tokens(node, self.text);

        LocalModItemSym::MacroDef(LocalMacroDefSym::new(
            self.db,
            name,
            self.scope,
            body_tokens,
            abs_span,
        ))
    }

    // -----------------------------------------------------------------------
    // Macro invocation
    // -----------------------------------------------------------------------

    pub(super) fn try_parse_macro_invocation(
        &self,
        node: tree_sitter::Node<'a>,
        pending_attrs: &[tree_sitter::Node<'a>],
    ) -> Option<LocalModItemSym<'db>> {
        let mut cursor = node.walk();
        let macro_node = node
            .children(&mut cursor)
            .find(|c| c.kind() == "macro_invocation")?;
        Some(self.parse_macro_invocation_node(macro_node, pending_attrs))
    }

    pub(super) fn parse_macro_invocation_node(
        &self,
        node: tree_sitter::Node<'a>,
        pending_attrs: &[tree_sitter::Node<'a>],
    ) -> LocalModItemSym<'db> {
        let start = item_start(node, pending_attrs);
        let abs_span = absolute_span(self.source, node, start);

        let macro_name_node = node.child_by_field_name("macro").unwrap_or(node);
        let path = ts_helpers::collect_macro_path_segments(self.db, macro_name_node, self.text);
        let input_tokens = ts_helpers::extract_macro_invocation_tokens(node, self.text);

        LocalModItemSym::MacroInvocation(LocalMacroInvocationSym::new(
            self.db,
            self.scope,
            path,
            input_tokens,
            abs_span,
        ))
    }

    // -----------------------------------------------------------------------
    // Shared CST data helpers (used by trait/impl bodies)
    // -----------------------------------------------------------------------

    fn parse_fn_cst_data(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> FnCstData<'db> {
        let name = node
            .child_by_field_name("name")
            .map(|n| Name::new(self.db, self.text[n.byte_range()].to_owned()))
            .unwrap_or_else(|| Name::new(self.db, "_".to_owned()));
        let attrs = stash.alloc_slice(&[]);
        let generics = self.parse_generics(stash, node, item_start);
        let params = self.parse_fn_params(stash, node, item_start);
        let ret = self.parse_return_type(stash, node, item_start);
        let body = node
            .child_by_field_name("body")
            .map(|n| self.parse_expr(stash, n, item_start));
        let where_clauses = self.parse_where_clauses(stash, node, item_start);
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };

        FnCstData {
            attrs,
            name,
            generics,
            params,
            ret,
            body,
            where_clauses,
            span,
        }
    }

    fn parse_type_alias_cst_data(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> TypeAliasCstData<'db> {
        let name = node
            .child_by_field_name("name")
            .map(|n| Name::new(self.db, self.text[n.byte_range()].to_owned()))
            .unwrap_or_else(|| Name::new(self.db, "_".to_owned()));
        let attrs = stash.alloc_slice(&[]);
        let generics = self.parse_generics(stash, node, item_start);
        let ty = node
            .child_by_field_name("type")
            .map(|n| self.parse_type(stash, n, item_start));
        let where_clauses = self.parse_where_clauses(stash, node, item_start);
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };

        TypeAliasCstData {
            attrs,
            name,
            generics,
            ty,
            where_clauses,
            span,
        }
    }

    fn parse_const_cst_data(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ConstCstData<'db> {
        let name = node
            .child_by_field_name("name")
            .map(|n| Name::new(self.db, self.text[n.byte_range()].to_owned()))
            .unwrap_or_else(|| Name::new(self.db, "_".to_owned()));
        let attrs = stash.alloc_slice(&[]);
        let ty = node
            .child_by_field_name("type")
            .map(|n| self.parse_type(stash, n, item_start));
        let value = node
            .child_by_field_name("value")
            .map(|n| self.parse_expr(stash, n, item_start));
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };

        ConstCstData {
            attrs,
            name,
            ty,
            value,
            span,
        }
    }
}

fn parent_dir_for(path: &str) -> String {
    if path == "lib.rs" || path == "main.rs" {
        return String::new();
    }
    if let Some(prefix) = path.strip_suffix("/mod.rs") {
        return format!("{prefix}/");
    }
    if let Some(stem) = path.strip_suffix(".rs") {
        return format!("{stem}/");
    }
    String::new()
}
