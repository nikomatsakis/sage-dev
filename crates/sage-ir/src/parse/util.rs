use crate::Db;
use crate::name::Name;
use crate::span::{AbsoluteSpan, ParseSource};

pub(super) fn node_field_text<'a>(
    node: tree_sitter::Node<'a>,
    field: &str,
    text: &'a str,
) -> Option<&'a str> {
    node.child_by_field_name(field)
        .map(|n| &text[n.byte_range()])
}

pub(super) fn node_name<'db>(
    db: &'db dyn Db,
    node: tree_sitter::Node<'_>,
    text: &str,
) -> Name<'db> {
    let name_text = node_field_text(node, "name", text).unwrap_or("_");
    Name::new(db, name_text.to_owned())
}

pub(super) fn item_start(
    node: tree_sitter::Node<'_>,
    pending_attrs: &[tree_sitter::Node<'_>],
) -> u32 {
    pending_attrs
        .first()
        .map_or(node.start_byte(), |a| a.start_byte()) as u32
}


pub(super) fn absolute_span<'db>(
    source: ParseSource<'db>,
    node: tree_sitter::Node<'_>,
    item_start: u32,
) -> AbsoluteSpan<'db> {
    AbsoluteSpan {
        source,
        start: item_start,
        end: node.end_byte() as u32,
    }
}
