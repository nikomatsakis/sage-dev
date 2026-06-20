use sage_stash::{AllocStashData, Ptr, Slice};

use crate::check::Check;
use crate::cst::ty::TypeCst;
use crate::name::Name;
use crate::resolve::Namespace;
use crate::ribs::RibEntry;
use crate::span::RelativeSpan;
use crate::symbol::Symbol;
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

/// The result of resolving a path in scope.
#[derive(Copy, Clone, Debug)]
pub enum Resolution<'db> {
    /// A generic parameter.
    Param(crate::generic_param::GenericParam<'db>),
    /// A module-level symbol (struct, fn, enum, etc).
    Sym(Symbol<'db>),
    /// The `Self` type in an impl/trait context.
    SelfTy(Ty<'db>),
    /// A local variable binding.
    Local(crate::tytree::LocalId),
    /// Resolution failed.
    Error,
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
        match self {
            Path::Relative(first, rest_slice) => {
                let rest = &cx.src[rest_slice];
                if rest.is_empty() {
                    if let Some(entry) = cx.resolver.ribs.lookup(first.name, ns) {
                        return match entry {
                            RibEntry::Param(param) => Resolution::Param(param),
                            RibEntry::Sym(sym) => Resolution::Sym(sym),
                            RibEntry::SelfTy(ty) => Resolution::SelfTy(ty),
                            RibEntry::Local(id) => Resolution::Local(id),
                        };
                    }
                }
                let mut names = vec![first.name];
                names.extend(rest.iter().map(|s| s.name));
                match cx.resolver.resolve_segments(&names, ns) {
                    Ok(sym) => Resolution::Sym(sym),
                    Err(_) => Resolution::Error,
                }
            }
            Path::Anchored(anchor, seg_slice) => {
                let segs = &cx.src[seg_slice];
                let mut names = Vec::new();
                anchor.collect_names_into(cx, &mut names);
                names.extend(segs.iter().map(|s| s.name));
                match cx.resolver.resolve_segments(&names, ns) {
                    Ok(sym) => Resolution::Sym(sym),
                    Err(_) => Resolution::Error,
                }
            }
        }
    }

    /// Collect all segment names in order (for module-level resolution).
    pub(crate) fn collect_names(self, cx: &Check<'_, 'db>) -> Vec<Name<'db>> {
        let mut names = Vec::new();
        match self {
            Path::Anchored(anchor, seg_slice) => {
                anchor.collect_names_into(cx, &mut names);
                let segs = &cx.src[seg_slice];
                names.extend(segs.iter().map(|s| s.name));
            }
            Path::Relative(first, rest_slice) => {
                names.push(first.name);
                let rest = &cx.src[rest_slice];
                names.extend(rest.iter().map(|s| s.name));
            }
        }
        names
    }

    /// Get the final segment (for type arg checking).
    pub(crate) fn final_segment(self, cx: &Check<'_, 'db>) -> PathSegment<'db> {
        match self {
            Path::Relative(first, rest) => cx.src[rest].last().copied().unwrap_or(first),
            Path::Anchored(_, rest) => {
                *cx.src[rest].last().expect("anchored path with no segments")
            }
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
                let inner_anchor = cx.src[inner];
                inner_anchor.collect_names_into(cx, out);
                out.push(Name::new(cx.db, "super".to_owned()));
            }
        }
    }
}

impl<'db> PathSegment<'db> {
    pub(crate) fn check_type_args(&self, cx: &mut Check<'_, 'db>) -> Slice<Ptr<Ty<'db>>> {
        let src_args = &cx.src[self.type_args];
        if src_args.is_empty() {
            return cx.target_stash.alloc_slice(&[]);
        }
        let tys: Vec<_> = src_args.iter().map(|a| a.check(cx)).collect();
        let ptrs: Vec<_> = tys.into_iter().map(|t| cx.target_stash.alloc(t)).collect();
        cx.target_stash.alloc_slice(&ptrs)
    }
}
