use sage_stash::{AllocStashData, Ptr, Slice};

use crate::cst::ty::TypeCst;
use crate::name::Name;
use crate::resolve::Namespace;
use crate::ribs::RibEntry;
use crate::check::CstLowerCtx;
use crate::span::RelativeSpan;
use crate::symbol::Symbol;
use crate::ty::Ty;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct PathCst<'db> {
    pub segments: Slice<PathSegmentCst<'db>>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct PathSegmentCst<'db> {
    pub name: Name<'db>,
    pub type_args: Slice<TypeCst<'db>>,
    pub span: RelativeSpan,
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

impl<'db> PathCst<'db> {
    /// Resolve this path to a `Resolution` — checks ribs first, then module scope.
    pub(crate) fn resolve(self, cx: &mut CstLowerCtx<'_, 'db>, ns: Namespace) -> Resolution<'db> {
        let segments = &cx.src[self.segments];
        if segments.is_empty() {
            return Resolution::Error;
        }

        let first = &segments[0];
        let rest = &segments[1..];

        // Check ribs (generics, locals, Self).
        if let Some(entry) = cx.resolver.ribs.lookup(first.name, ns) {
            if rest.is_empty() {
                return match entry {
                    RibEntry::Param(param) => Resolution::Param(param),
                    RibEntry::Sym(sym) => Resolution::Sym(sym),
                    RibEntry::SelfTy(ty) => Resolution::SelfTy(ty),
                    RibEntry::Local(id) => Resolution::Local(id),
                };
            }
            // Multi-segment paths starting with a rib entry (e.g. T::Assoc)
            // are not yet supported.
            return Resolution::Error;
        }

        // Fall back to module-level resolution.
        let names: Vec<_> = segments.iter().map(|s| s.name).collect();
        match cx.resolver.resolve_segments(&names, ns) {
            Ok(sym) => Resolution::Sym(sym),
            Err(_) => Resolution::Error,
        }
    }
}

impl<'db> PathSegmentCst<'db> {
    pub(crate) fn check_type_args(
        &self,
        cx: &mut CstLowerCtx<'_, 'db>,
    ) -> Slice<Ptr<Ty<'db>>> {
        let src_args = &cx.src[self.type_args];
        if src_args.is_empty() {
            return cx.dst.alloc_slice(&[]);
        }
        let tys: Vec<_> = src_args.iter().map(|a| a.check(cx)).collect();
        let ptrs: Vec<_> = tys.into_iter().map(|t| cx.dst.alloc(t)).collect();
        cx.dst.alloc_slice(&ptrs)
    }
}
