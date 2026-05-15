use crate::Db;
use crate::body::*;
use crate::item::*;
use crate::name::Name;
use crate::source::SourceFile;
use crate::span::{AbsoluteSpan, ParseSource, RelativeSpan};
use crate::types::*;

use sage_stash::{Stash, Stashed};

const GEN_REL_SPAN: RelativeSpan = RelativeSpan { start: 0, end: 0 };

/// Generate impl blocks for a builtin derive.
#[salsa::tracked(returns(ref))]
pub fn expand_builtin<'db>(
    db: &'db dyn Db,
    derive_name: Name<'db>,
    item: ItemAst<'db>,
) -> Vec<ImplAst<'db>> {
    let name_text = derive_name.text(db);
    let item_name = crate::resolve::item_name(db, item)
        .map(|n| n.text(db).to_string())
        .unwrap_or_else(|| "?".into());
    db.log_query(format!("expand_builtin(\"{name_text}\", \"{item_name}\")"));
    match name_text.as_str() {
        "Debug" => expand_debug(db, item),
        "Clone" => expand_clone(db, item),
        "Default" => expand_default(db, item),
        // Marker traits — empty impl body
        "Copy" | "Eq" | "PartialEq" | "Hash" | "PartialOrd" | "Ord" => {
            expand_marker(db, derive_name, item)
        }
        _ => Vec::new(),
    }
}

/// `impl Debug for T { fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result { ... } }`
fn expand_debug<'db>(db: &'db dyn Db, item: ItemAst<'db>) -> Vec<ImplAst<'db>> {
    let (type_name, _fields) = match item_info(db, item) {
        Some(info) => info,
        None => return Vec::new(),
    };

    let trait_path = make_abs_path(db, &["std", "fmt", "Debug"]);
    let self_ty = make_type_path(db, &[type_name.text(db)]);

    // Build fmt method body: `f.debug_struct("TypeName").field("x", &self.x)...finish()`
    let mut stash = Stash::new();
    let body_expr = {
        // Missing body — we represent the body as a placeholder
        stash.alloc(Expr {
            kind: ExprKind::Missing,
            span: GEN_REL_SPAN,
        })
    };
    let body = stash.alloc(Body {
        root: body_expr,
        span: GEN_REL_SPAN,
    });
    let body = Stashed::new(stash, body);

    let fmt_fn = FnAst::new(
        db,
        Name::new(db, "fmt".to_owned()),
        Vec::new(), // attrs
        vec![
            Param::new(
                db,
                Some(Name::new(db, "self".to_owned())),
                make_ref_self(db),
                GEN_REL_SPAN,
            ),
            Param::new(
                db,
                Some(Name::new(db, "f".to_owned())),
                make_abs_type_path(db, &["std", "fmt", "Formatter"]),
                GEN_REL_SPAN,
            ),
        ],
        Some(make_abs_type_path(db, &["std", "fmt", "Result"])),
        false,
        false,
        body,
        gen_abs_span(db),
    );

    let impl_item = ImplAst::new(
        db,
        Vec::new(),
        self_ty,
        Some(trait_path),
        vec![ItemAst::Function(fmt_fn)],
        gen_abs_span(db),
    );
    vec![impl_item]
}

/// `impl Clone for T { fn clone(&self) -> Self { ... } }`
fn expand_clone<'db>(db: &'db dyn Db, item: ItemAst<'db>) -> Vec<ImplAst<'db>> {
    let (type_name, _fields) = match item_info(db, item) {
        Some(info) => info,
        None => return Vec::new(),
    };

    let trait_path = make_abs_path(db, &["core", "clone", "Clone"]);
    let self_ty = make_type_path(db, &[type_name.text(db)]);

    let mut stash = Stash::new();
    let body_expr = stash.alloc(Expr {
        kind: ExprKind::Missing,
        span: GEN_REL_SPAN,
    });
    let body = stash.alloc(Body {
        root: body_expr,
        span: GEN_REL_SPAN,
    });
    let body = Stashed::new(stash, body);

    let clone_fn = FnAst::new(
        db,
        Name::new(db, "clone".to_owned()),
        Vec::new(),
        vec![Param::new(
            db,
            Some(Name::new(db, "self".to_owned())),
            make_ref_self(db),
            GEN_REL_SPAN,
        )],
        Some(make_type_path(db, &["Self"])),
        false,
        false,
        body,
        gen_abs_span(db),
    );

    let impl_item = ImplAst::new(
        db,
        Vec::new(),
        self_ty,
        Some(trait_path),
        vec![ItemAst::Function(clone_fn)],
        gen_abs_span(db),
    );
    vec![impl_item]
}

/// `impl Default for T { fn default() -> Self { ... } }`
fn expand_default<'db>(db: &'db dyn Db, item: ItemAst<'db>) -> Vec<ImplAst<'db>> {
    let (type_name, _fields) = match item_info(db, item) {
        Some(info) => info,
        None => return Vec::new(),
    };

    let trait_path = make_abs_path(db, &["core", "default", "Default"]);
    let self_ty = make_type_path(db, &[type_name.text(db)]);

    let mut stash = Stash::new();
    let body_expr = stash.alloc(Expr {
        kind: ExprKind::Missing,
        span: GEN_REL_SPAN,
    });
    let body = stash.alloc(Body {
        root: body_expr,
        span: GEN_REL_SPAN,
    });
    let body = Stashed::new(stash, body);

    let default_fn = FnAst::new(
        db,
        Name::new(db, "default".to_owned()),
        Vec::new(),
        Vec::new(),
        Some(make_type_path(db, &["Self"])),
        false,
        false,
        body,
        gen_abs_span(db),
    );

    let impl_item = ImplAst::new(
        db,
        Vec::new(),
        self_ty,
        Some(trait_path),
        vec![ItemAst::Function(default_fn)],
        gen_abs_span(db),
    );
    vec![impl_item]
}

/// Marker trait — empty impl body.
fn expand_marker<'db>(
    db: &'db dyn Db,
    derive_name: Name<'db>,
    item: ItemAst<'db>,
) -> Vec<ImplAst<'db>> {
    let (type_name, _fields) = match item_info(db, item) {
        Some(info) => info,
        None => return Vec::new(),
    };

    let trait_path = match derive_name.text(db).as_str() {
        "Copy" => make_abs_path(db, &["core", "marker", "Copy"]),
        "Eq" => make_abs_path(db, &["core", "cmp", "Eq"]),
        "PartialEq" => make_abs_path(db, &["core", "cmp", "PartialEq"]),
        "Hash" => make_abs_path(db, &["core", "hash", "Hash"]),
        "PartialOrd" => make_abs_path(db, &["core", "cmp", "PartialOrd"]),
        "Ord" => make_abs_path(db, &["core", "cmp", "Ord"]),
        _ => make_path(db, &[derive_name.text(db)]),
    };
    let self_ty = make_type_path(db, &[type_name.text(db)]);

    let impl_item = ImplAst::new(
        db,
        Vec::new(),
        self_ty,
        Some(trait_path),
        Vec::new(),
        gen_abs_span(db),
    );
    vec![impl_item]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract type name and fields from a struct or enum item.
fn item_info<'db>(db: &'db dyn Db, item: ItemAst<'db>) -> Option<(Name<'db>, Vec<FieldDef<'db>>)> {
    match item {
        ItemAst::Struct(s) => Some((s.name(db), s.fields(db).to_vec())),
        ItemAst::Enum(e) => Some((e.name(db), Vec::new())),
        _ => None,
    }
}

fn make_path<'db>(db: &'db dyn Db, segments: &[&str]) -> Path<'db> {
    Path::new(
        db,
        segments
            .iter()
            .map(|s| Name::new(db, (*s).to_owned()))
            .collect(),
        GEN_REL_SPAN,
    )
}

/// Make an absolute path (`::foo::bar`) — prepends the empty sentinel segment.
fn make_abs_path<'db>(db: &'db dyn Db, segments: &[&str]) -> Path<'db> {
    let mut segs = vec![Name::new(db, String::new())];
    segs.extend(segments.iter().map(|s| Name::new(db, (*s).to_owned())));
    Path::new(db, segs, GEN_REL_SPAN)
}

fn make_type_path<'db>(db: &'db dyn Db, segments: &[&str]) -> TypeRef<'db> {
    TypeRef::new(db, TypeRefKind::Path(make_path(db, segments)), GEN_REL_SPAN)
}

fn make_abs_type_path<'db>(db: &'db dyn Db, segments: &[&str]) -> TypeRef<'db> {
    TypeRef::new(
        db,
        TypeRefKind::Path(make_abs_path(db, segments)),
        GEN_REL_SPAN,
    )
}

fn make_ref_self<'db>(db: &'db dyn Db) -> TypeRef<'db> {
    let self_ty = make_type_path(db, &["Self"]);
    TypeRef::new(
        db,
        TypeRefKind::Reference(self_ty, Mutability::Shared),
        GEN_REL_SPAN,
    )
}

fn gen_abs_span<'db>(db: &'db dyn Db) -> AbsoluteSpan<'db> {
    AbsoluteSpan {
        source: ParseSource::SourceFile(gen_source_file(db)),
        start: 0,
        end: 0,
    }
}

fn gen_source_file<'db>(db: &'db dyn Db) -> SourceFile {
    SourceFile::new(db, "<generated>".to_owned(), String::new())
}
