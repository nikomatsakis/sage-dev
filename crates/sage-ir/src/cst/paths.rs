use sage_stash::{AllocStashData, Ptr, Slice};

use crate::check::Check;
use crate::cst::ty::TypeCst;
use crate::name::Name;
use crate::resolve::{Namespace, Resolution};
use crate::span::RelativeSpan;
use crate::ty::Ty;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum Path<'db> {
    /// Explicit anchor keyword like `self::foo::bar`.
    Anchored(PathAnchor<'db>, Slice<PathSegment<'db>>),
    /// Relative path like `foo::bar`.
    Relative(PathSegment<'db>, Slice<PathSegment<'db>>),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct PathSegment<'db> {
    pub name: Name<'db>,
    pub type_args: Slice<TypeCst<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct PathAnchor<'db> {
    pub kind: PathAnchorKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum PathAnchorKind<'db> {
    /// `::foo` — extern crate lookup. Parser consumes `::` + first ident together.
    ExternCrate(Name<'db>),
    /// `crate` — this crate's root module.
    CurrentCrate,
    /// `self`
    Self_,
    /// `$crate`
    DollarCrate,
    /// `super` — parent of the inner anchor. Inner is always rooted at Self_.
    /// `super::super` desugars to Super(→ Super(→ Self_)).
    Super(Ptr<PathAnchor<'db>>),
}

impl<'db> Path<'db> {
    pub fn anchor(self) -> Option<PathAnchor<'db>> {
        match self {
            Path::Anchored(a, _) => Some(a),
            Path::Relative(_, _) => None,
        }
    }

    /// Resolve this path to a `Resolution` — checks ribs first, then module scope.
    pub(crate) fn resolve(self, cx: &mut Check<'_, 'db>, ns: Namespace) -> Resolution<'db> {
        let results = cx.resolver.resolve_path(cx.source_stash, self, ns);
        results.into_iter().next().unwrap_or(Resolution::Error)
    }

    /// Collect all segment names in order (for module-level resolution).
    pub(crate) fn collect_names(self, cx: &Check<'_, 'db>) -> Vec<Name<'db>> {
        let mut names = Vec::new();
        match self {
            Path::Anchored(anchor, seg_slice) => {
                anchor.collect_names_into(cx, &mut names);
                let segs = &cx.source_stash[seg_slice];
                names.extend(segs.iter().map(|s| s.name));
            }
            Path::Relative(first, rest_slice) => {
                names.push(first.name);
                let rest = &cx.source_stash[rest_slice];
                names.extend(rest.iter().map(|s| s.name));
            }
        }
        names
    }

    /// Get the final segment (for type arg checking).
    pub(crate) fn final_segment(self, cx: &Check<'_, 'db>) -> PathSegment<'db> {
        match self {
            Path::Relative(first, rest) => cx.source_stash[rest].last().copied().unwrap_or(first),
            Path::Anchored(_, rest) => *cx.source_stash[rest]
                .last()
                .expect("anchored path with no segments"),
        }
    }
}

impl<'db> PathAnchor<'db> {
    fn collect_names_into(self, cx: &Check<'_, 'db>, out: &mut Vec<Name<'db>>) {
        match self.kind {
            PathAnchorKind::ExternCrate(name) => {
                out.push(name);
            }
            PathAnchorKind::CurrentCrate => {
                out.push(Name::new(cx.db, "crate".to_owned()));
            }
            PathAnchorKind::Self_ => {
                out.push(Name::new(cx.db, "self".to_owned()));
            }
            PathAnchorKind::DollarCrate => {
                out.push(Name::new(cx.db, "$crate".to_owned()));
            }
            PathAnchorKind::Super(inner) => {
                let inner_anchor = cx.source_stash[inner];
                inner_anchor.collect_names_into(cx, out);
                out.push(Name::new(cx.db, "super".to_owned()));
            }
        }
    }
}

impl<'db> PathSegment<'db> {
    pub(crate) fn check_type_args(&self, cx: &mut Check<'_, 'db>) -> Slice<Ptr<Ty<'db>>> {
        let src_args = &cx.source_stash[self.type_args];
        if src_args.is_empty() {
            return cx.target_stash.alloc_slice(&[]);
        }
        let tys: Vec<_> = src_args.iter().map(|a| a.check(cx)).collect();
        let ptrs: Vec<_> = tys.into_iter().map(|t| cx.target_stash.alloc(t)).collect();
        cx.target_stash.alloc_slice(&ptrs)
    }
}
