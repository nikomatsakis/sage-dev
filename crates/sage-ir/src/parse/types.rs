use sage_stash::{Ptr, Stash};

use crate::cst::ty::{TypeCst, TypeCstKind};
use crate::span::RelativeSpan;
use crate::types::Mutability;

use super::Parser;

impl<'a, 'db> Parser<'a, 'db> {
    pub(super) fn parse_type(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Ptr<TypeCst<'db>> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };

        let kind = match node.kind() {
            "type_identifier" | "primitive_type" | "identifier" => {
                let path = self.parse_path_from_type_node(stash, node, item_start);
                TypeCstKind::Path(path)
            }
            "scoped_type_identifier" | "generic_type" => {
                let path = self.parse_path_from_type_node(stash, node, item_start);
                TypeCstKind::Path(path)
            }
            "reference_type" => self.parse_reference_type(stash, node, item_start),
            "tuple_type" => self.parse_tuple_type(stash, node, item_start),
            "array_type" => {
                if let Some(elem) = node.child_by_field_name("element") {
                    let inner = self.parse_type(stash, elem, item_start);
                    TypeCstKind::Array(inner)
                } else {
                    TypeCstKind::Error
                }
            }
            "unit_type" => TypeCstKind::Tuple(stash.alloc_slice(&[])),
            "never_type" => TypeCstKind::Never,
            "function_type" => self.parse_fn_type(stash, node, item_start),
            "bounded_type" | "removed_trait_bound" => {
                // For now, treat bounded types as error
                TypeCstKind::Error
            }
            "macro_invocation" => TypeCstKind::Error,
            _ => TypeCstKind::Error,
        };

        stash.alloc(TypeCst { kind, span })
    }

    fn parse_reference_type(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> TypeCstKind<'db> {
        let mut mutability = Mutability::Shared;
        let mut inner_node = None;
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            match child.kind() {
                "mutable_specifier" => mutability = Mutability::Mut,
                "&" | "lifetime" => {}
                _ => {
                    inner_node = Some(child);
                }
            }
        }

        match inner_node {
            Some(inner) => {
                let inner_ptr = self.parse_type(stash, inner, item_start);
                TypeCstKind::Reference(inner_ptr, mutability)
            }
            None => TypeCstKind::Error,
        }
    }

    fn parse_tuple_type(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> TypeCstKind<'db> {
        let mut elements = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.is_named() && child.kind() != "," {
                let ty = self.parse_type(stash, child, item_start);
                elements.push(stash[ty]);
            }
        }
        TypeCstKind::Tuple(stash.alloc_slice(&elements))
    }

    fn parse_fn_type(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> TypeCstKind<'db> {
        let mut params = Vec::new();
        let mut ret = None;

        if let Some(parameters) = node.child_by_field_name("parameters") {
            let mut cursor = parameters.walk();
            for child in parameters.children(&mut cursor) {
                if child.is_named() && child.kind() != "," {
                    let ty = self.parse_type(stash, child, item_start);
                    params.push(stash[ty]);
                }
            }
        }

        if let Some(return_type) = node.child_by_field_name("return_type") {
            let ty = self.parse_type(stash, return_type, item_start);
            ret = Some(ty);
        }

        TypeCstKind::Fn(stash.alloc_slice(&params), ret)
    }
}
