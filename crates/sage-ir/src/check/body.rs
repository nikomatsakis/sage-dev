use sage_stash::{Ptr, Stash, Stashed};

use crate::name::Name;
use crate::resolve::{Namespace, Resolver};
use crate::resolved::*;
use crate::ribs::{RibEntry, Ribs};
use crate::span::RelativeSpan;

pub struct BodyCheckCtx<'a, 'db> {
    pub resolver: Resolver<'db>,
    pub src: &'a Stash,
    pub out: Stash,
    pub locals: Vec<LocalVar<'db>>,
    pub ribs: Ribs<'db>,
}

impl<'a, 'db> BodyCheckCtx<'a, 'db> {
    pub fn new(src: &'a Stash, resolver: Resolver<'db>) -> Self {
        let mut ribs = Ribs::new();
        ribs.push_scope();
        Self {
            resolver,
            src,
            out: Stash::new(),
            locals: Vec::new(),
            ribs,
        }
    }

    pub fn add_binding(&mut self, name: Name<'db>, span: RelativeSpan) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(LocalVar { name, span });
        self.ribs.add(name, Namespace::Value, RibEntry::Local(id));
        id
    }

    pub fn finish(self, root: Ptr<CheckedExpr<'db>>, span: RelativeSpan) -> ResolvedBody<'db> {
        let mut out = self.out;
        let locals = out.alloc_slice(&self.locals);
        let body = out.alloc(CheckedBody { root, locals, span });
        Stashed::new(out, body)
    }

    pub fn resolve_path(
        &mut self,
        path: crate::cst::paths::PathCst<'db>,
        ns: Namespace,
    ) -> Res<'db> {
        let segments = &self.src[path.segments];
        if segments.is_empty() {
            return Res::Err;
        }

        let first = &segments[0];
        let rest = &segments[1..];

        if let Some(entry) = self.ribs.lookup(first.name, ns) {
            return match entry {
                RibEntry::Local(id) => {
                    if rest.is_empty() {
                        Res::Local(id)
                    } else {
                        Res::Err
                    }
                }
                RibEntry::Param(_) | RibEntry::SelfTy(_) => Res::Err,
                RibEntry::Sym(sym) => {
                    if rest.is_empty() {
                        Res::Def(sym)
                    } else {
                        let names: Vec<_> = segments.iter().map(|s| s.name).collect();
                        match self.resolver.resolve_segments(&names, ns) {
                            Ok(sym) => Res::Def(sym),
                            Err(_) => Res::Err,
                        }
                    }
                }
            };
        }

        let names: Vec<_> = segments.iter().map(|s| s.name).collect();
        match self.resolver.resolve_segments(&names, ns) {
            Ok(sym) => Res::Def(sym),
            Err(_) => Res::Err,
        }
    }
}
