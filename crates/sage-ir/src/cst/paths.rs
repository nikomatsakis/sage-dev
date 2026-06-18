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
    /// No explicit anchor — resolve relative to current scope.
    Relative,
    /// Explicit anchor keyword.
    Anchor(PathAnchor<'db>),
    /// A named segment with a prefix path.
    Segment(PathSegment<'db>),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct PathSegment<'db> {
    pub name: Name<'db>,
    pub prefix: Ptr<Path<'db>>,
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
    /// Resolve this path to a `Resolution` — checks ribs first, then module scope.
    pub(crate) fn resolve(self, cx: &mut Check<'_, 'db>, ns: Namespace) -> Resolution<'db> {
        match self {
            Path::Relative => Resolution::Error,
            Path::Anchor(_) => {
                // Standalone anchor (e.g. bare `self` as value) — module resolution.
                let names = self.collect_names(cx);
                match cx.resolver.resolve_segments(&names, ns) {
                    Ok(sym) => Resolution::Sym(sym),
                    Err(_) => Resolution::Error,
                }
            }
            Path::Segment(seg) => {
                let prefix = cx.src[seg.prefix];
                match prefix {
                    Path::Relative => {
                        // Single-segment path: check ribs first.
                        if let Some(entry) = cx.resolver.ribs.lookup(seg.name, ns) {
                            return match entry {
                                RibEntry::Param(param) => Resolution::Param(param),
                                RibEntry::Sym(sym) => Resolution::Sym(sym),
                                RibEntry::SelfTy(ty) => Resolution::SelfTy(ty),
                                RibEntry::Local(id) => Resolution::Local(id),
                            };
                        }
                        // Fall back to module-level resolution.
                        let names = vec![seg.name];
                        match cx.resolver.resolve_segments(&names, ns) {
                            Ok(sym) => Resolution::Sym(sym),
                            Err(_) => Resolution::Error,
                        }
                    }
                    _ => {
                        // Multi-segment or anchored path — module resolution.
                        let names = self.collect_names(cx);
                        match cx.resolver.resolve_segments(&names, ns) {
                            Ok(sym) => Resolution::Sym(sym),
                            Err(_) => Resolution::Error,
                        }
                    }
                }
            }
        }
    }

    /// Collect all segment names in order (for module-level resolution).
    fn collect_names(self, cx: &Check<'_, 'db>) -> Vec<Name<'db>> {
        let mut names = Vec::new();
        self.collect_names_into(cx, &mut names);
        names
    }

    fn collect_names_into(self, cx: &Check<'_, 'db>, out: &mut Vec<Name<'db>>) {
        match self {
            Path::Relative => {}
            Path::Anchor(anchor) => anchor.collect_names_into(cx, out),
            Path::Segment(seg) => {
                let prefix = cx.src[seg.prefix];
                prefix.collect_names_into(cx, out);
                out.push(seg.name);
            }
        }
    }

    /// Get the outermost segment (for type arg checking), if any.
    pub(crate) fn final_segment(self) -> Option<PathSegment<'db>> {
        match self {
            Path::Segment(seg) => Some(seg),
            _ => None,
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
