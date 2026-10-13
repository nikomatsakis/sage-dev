//! Final lowering from transient body-checking state into completed Typed IR.

use rustc_hash::{FxHashMap, FxHashSet};
use sage_stash::{Ptr, Slice, Stash, Stashed};

use crate::ty::{AliasTy, NamedAliasTy, OpaqueAliasTy, ProjectionTy, TraitRef, Ty};
use crate::tytree::{
    CallDispatch, ExprSlot, LocalVar, ResolvedCallTarget, TyBody, TyBodyData, TyClosureParam,
    TyExpr, TyExprData, TyFieldInit, TyFieldPat, TyMatchArm, TyPat, TyPatKind, TyStmt, TyStmtKind,
};

use super::infer::egraph::VersionedEGraph;
use super::infer::version::Version;

/// Resolve every inference edge and rebuild the completed body in a fresh,
/// append-only stash.
// ANCHOR: finalize_typed_body_into_fresh_stash
pub(super) fn finalize_typed_body<'db>(
    source: &Stash,
    egraph: &VersionedEGraph<'db>,
    root: Ptr<TyExpr<'db>>,
    expr_slots: &[Ptr<TyExpr<'db>>],
    locals: &[LocalVar<'db>],
    span: crate::span::RelativeSpan,
) -> TyBody<'db> {
    let mut target = Stash::new();
    let mut finalizer = BodyFinalizer {
        source,
        target: &mut target,
        egraph,
        expr_slots,
        copied_types: FxHashMap::default(),
        active_types: FxHashSet::default(),
        copied_exprs: FxHashMap::default(),
        active_exprs: FxHashSet::default(),
        copied_patterns: FxHashMap::default(),
    };

    let root = finalizer.copy_expr(root);
    let locals = finalizer.target.alloc_slice(locals);
    let body = finalizer.target.alloc(TyBodyData { root, locals, span });
    Stashed::new(target, body)
}
// ANCHOR_END: finalize_typed_body_into_fresh_stash

struct BodyFinalizer<'source, 'target, 'db> {
    source: &'source Stash,
    target: &'target mut Stash,
    egraph: &'source VersionedEGraph<'db>,
    expr_slots: &'source [Ptr<TyExpr<'db>>],
    copied_types: FxHashMap<Ptr<Ty<'db>>, Ptr<Ty<'db>>>,
    active_types: FxHashSet<Ptr<Ty<'db>>>,
    copied_exprs: FxHashMap<Ptr<TyExpr<'db>>, Ptr<TyExpr<'db>>>,
    active_exprs: FxHashSet<Ptr<TyExpr<'db>>>,
    copied_patterns: FxHashMap<Ptr<TyPat<'db>>, Ptr<TyPat<'db>>>,
}

impl<'db> BodyFinalizer<'_, '_, 'db> {
    fn copy_ty(&mut self, source: Ptr<Ty<'db>>) -> Ptr<Ty<'db>> {
        let source = self.egraph.find(Version::ROOT, source);
        if let Some(&target) = self.copied_types.get(&source) {
            return target;
        }
        assert!(
            self.active_types.insert(source),
            "recursive type escaped the inference occurs check"
        );

        let data = match self.source[source] {
            Ty::Bool => Ty::Bool,
            Ty::Char => Ty::Char,
            Ty::Int(value) => Ty::Int(value),
            Ty::Uint(value) => Ty::Uint(value),
            Ty::Float(value) => Ty::Float(value),
            Ty::Str => Ty::Str,
            Ty::Adt(symbol, arguments) => Ty::Adt(symbol, self.copy_ty_slice(arguments)),
            Ty::Alias(alias) => Ty::Alias(self.copy_alias(alias)),
            Ty::Ref(referent, mutability, lifetime) => {
                Ty::Ref(self.copy_ty(referent), mutability, lifetime)
            }
            Ty::Tuple(elements) => Ty::Tuple(self.copy_ty_slice(elements)),
            Ty::Slice(element) => Ty::Slice(self.copy_ty(element)),
            Ty::Array(element, length) => Ty::Array(self.copy_ty(element), length),
            Ty::FnPtr(parameters, return_ty) => {
                Ty::FnPtr(self.copy_ty_slice(parameters), self.copy_ty(return_ty))
            }
            Ty::Param(parameter) => Ty::Param(parameter),
            Ty::InferVar(variable) => {
                panic!("unresolved inference variable {variable:?} escaped body finalization")
            }
            Ty::Never => Ty::Never,
            Ty::Error(error) => Ty::Error(error),
        };
        let target = self.target.alloc(data);
        assert!(self.active_types.remove(&source));
        self.copied_types.insert(source, target);
        target
    }

    fn copy_alias(&mut self, alias: AliasTy<'db>) -> AliasTy<'db> {
        match alias {
            AliasTy::Named(NamedAliasTy { def, args }) => AliasTy::Named(NamedAliasTy {
                def,
                args: self.copy_ty_slice(args),
            }),
            AliasTy::Associated(ProjectionTy {
                associated_ty,
                self_ty,
                trait_ref,
                args,
            }) => AliasTy::Associated(ProjectionTy {
                associated_ty,
                self_ty: self.copy_ty(self_ty),
                trait_ref: self.copy_trait_ref(trait_ref),
                args: self.copy_ty_slice(args),
            }),
            AliasTy::Opaque(OpaqueAliasTy { def, args }) => AliasTy::Opaque(OpaqueAliasTy {
                def,
                args: self.copy_ty_slice(args),
            }),
        }
    }

    fn copy_trait_ref(&mut self, trait_ref: TraitRef<'db>) -> TraitRef<'db> {
        let TraitRef { trait_sym, args } = trait_ref;
        TraitRef {
            trait_sym,
            args: self.copy_ty_slice(args),
        }
    }

    fn copy_ty_slice(&mut self, source: Slice<Ptr<Ty<'db>>>) -> Slice<Ptr<Ty<'db>>> {
        let source_values = self.source[source].to_vec();
        let target_values: Vec<_> = source_values
            .into_iter()
            .map(|value| self.copy_ty(value))
            .collect();
        self.target.alloc_slice(&target_values)
    }

    fn copy_expr(&mut self, source: Ptr<TyExpr<'db>>) -> Ptr<TyExpr<'db>> {
        if let Some(&target) = self.copied_exprs.get(&source) {
            return target;
        }
        let TyExpr { data, ty, span } = self.source[source];
        match data {
            TyExprData::Unresolved(ExprSlot(slot)) => {
                assert!(
                    self.active_exprs.insert(source),
                    "recursive expression slot escaped body checking"
                );
                let resolved = self.expr_slots[slot as usize];
                let target = self.copy_expr(resolved);
                assert!(self.active_exprs.remove(&source));
                self.copied_exprs.insert(source, target);
                return target;
            }
            TyExprData::Literal(_)
            | TyExprData::Path(_)
            | TyExprData::Block(_, _)
            | TyExprData::Call(_, _)
            | TyExprData::ResolvedCall(_, _)
            | TyExprData::MethodCall(_, _, _)
            | TyExprData::Field(_, _)
            | TyExprData::Binary(_, _, _)
            | TyExprData::Unary(_, _)
            | TyExprData::Deref(_)
            | TyExprData::Ref(_, _)
            | TyExprData::NeverToAny(_)
            | TyExprData::If(_, _, _)
            | TyExprData::IfLet(_, _, _, _)
            | TyExprData::Match(_, _)
            | TyExprData::Loop(_)
            | TyExprData::While(_, _)
            | TyExprData::WhileLet(_, _, _)
            | TyExprData::For(_, _, _)
            | TyExprData::Break(_)
            | TyExprData::Continue
            | TyExprData::Return(_)
            | TyExprData::Assign(_, _)
            | TyExprData::Await(_)
            | TyExprData::Try(_)
            | TyExprData::Closure(_, _)
            | TyExprData::Tuple(_)
            | TyExprData::Array(_)
            | TyExprData::Index(_, _)
            | TyExprData::Cast(_, _)
            | TyExprData::StructLit(_, _)
            | TyExprData::Range(_, _)
            | TyExprData::MacroCall(_, _)
            | TyExprData::Error(_)
            | TyExprData::Missing => {}
        }
        let ty = self.copy_ty(ty);
        let data = self.copy_expr_data(data, ty);
        let target = self.target.alloc(TyExpr { data, ty, span });
        self.copied_exprs.insert(source, target);
        target
    }

    fn copy_expr_data(
        &mut self,
        data: TyExprData<'db>,
        finalized_ty: Ptr<Ty<'db>>,
    ) -> TyExprData<'db> {
        match data {
            TyExprData::Literal(literal) => TyExprData::Literal(literal),
            TyExprData::Path(resolution) => TyExprData::Path(resolution),
            TyExprData::Block(statements, tail) => TyExprData::Block(
                self.copy_stmt_slice(statements),
                tail.map(|tail| self.copy_expr(tail)),
            ),
            TyExprData::Call(callee, arguments) => {
                TyExprData::Call(self.copy_expr(callee), self.copy_expr_slice(arguments))
            }
            TyExprData::ResolvedCall(target, arguments) => TyExprData::ResolvedCall(
                self.copy_call_target(target),
                self.copy_expr_slice(arguments),
            ),
            TyExprData::MethodCall(receiver, name, arguments) => TyExprData::MethodCall(
                self.copy_expr(receiver),
                name,
                self.copy_expr_slice(arguments),
            ),
            TyExprData::Field(owner, field) => TyExprData::Field(self.copy_expr(owner), field),
            TyExprData::Binary(left, operation, right) => {
                TyExprData::Binary(self.copy_expr(left), operation, self.copy_expr(right))
            }
            TyExprData::Unary(operation, operand) => {
                TyExprData::Unary(operation, self.copy_expr(operand))
            }
            TyExprData::Deref(operand) => TyExprData::Deref(self.copy_expr(operand)),
            TyExprData::Ref(operand, mutability) => {
                TyExprData::Ref(self.copy_expr(operand), mutability)
            }
            TyExprData::NeverToAny(operand) => {
                let operand = self.copy_expr(operand);
                if matches!(self.target[finalized_ty], Ty::Never) {
                    self.target[operand].data
                } else {
                    TyExprData::NeverToAny(operand)
                }
            }
            TyExprData::If(condition, if_true, if_false) => TyExprData::If(
                self.copy_expr(condition),
                self.copy_expr(if_true),
                if_false.map(|expr| self.copy_expr(expr)),
            ),
            TyExprData::IfLet(pattern, scrutinee, if_true, if_false) => TyExprData::IfLet(
                self.copy_pattern(pattern),
                self.copy_expr(scrutinee),
                self.copy_expr(if_true),
                if_false.map(|expr| self.copy_expr(expr)),
            ),
            TyExprData::Match(scrutinee, arms) => {
                TyExprData::Match(self.copy_expr(scrutinee), self.copy_match_arm_slice(arms))
            }
            TyExprData::Loop(body) => TyExprData::Loop(self.copy_expr(body)),
            TyExprData::While(condition, body) => {
                TyExprData::While(self.copy_expr(condition), self.copy_expr(body))
            }
            TyExprData::WhileLet(pattern, scrutinee, body) => TyExprData::WhileLet(
                self.copy_pattern(pattern),
                self.copy_expr(scrutinee),
                self.copy_expr(body),
            ),
            TyExprData::For(pattern, iterator, body) => TyExprData::For(
                self.copy_pattern(pattern),
                self.copy_expr(iterator),
                self.copy_expr(body),
            ),
            TyExprData::Break(value) => TyExprData::Break(value.map(|value| self.copy_expr(value))),
            TyExprData::Continue => TyExprData::Continue,
            TyExprData::Return(value) => {
                TyExprData::Return(value.map(|value| self.copy_expr(value)))
            }
            TyExprData::Assign(place, value) => {
                TyExprData::Assign(self.copy_expr(place), self.copy_expr(value))
            }
            TyExprData::Await(future) => TyExprData::Await(self.copy_expr(future)),
            TyExprData::Try(value) => TyExprData::Try(self.copy_expr(value)),
            TyExprData::Closure(parameters, body) => TyExprData::Closure(
                self.copy_closure_param_slice(parameters),
                self.copy_expr(body),
            ),
            TyExprData::Tuple(elements) => TyExprData::Tuple(self.copy_expr_slice(elements)),
            TyExprData::Array(elements) => TyExprData::Array(self.copy_expr_slice(elements)),
            TyExprData::Index(container, index) => {
                TyExprData::Index(self.copy_expr(container), self.copy_expr(index))
            }
            TyExprData::Cast(value, ty) => {
                TyExprData::Cast(self.copy_expr(value), self.copy_ty(ty))
            }
            TyExprData::StructLit(resolution, fields) => {
                TyExprData::StructLit(resolution, self.copy_field_init_slice(fields))
            }
            TyExprData::Range(start, end) => TyExprData::Range(
                start.map(|expr| self.copy_expr(expr)),
                end.map(|expr| self.copy_expr(expr)),
            ),
            TyExprData::MacroCall(resolution, tokens) => TyExprData::MacroCall(resolution, tokens),
            TyExprData::Error(error) => TyExprData::Error(error),
            TyExprData::Unresolved(_) => {
                unreachable!("expression slots are resolved before copying expression data")
            }
            TyExprData::Missing => TyExprData::Missing,
        }
    }

    fn copy_call_target(&mut self, target: ResolvedCallTarget<'db>) -> ResolvedCallTarget<'db> {
        let ResolvedCallTarget {
            function,
            dispatch,
            owner_type_args,
            method_type_args,
        } = target;
        let dispatch = match dispatch {
            CallDispatch::Direct => CallDispatch::Direct,
            CallDispatch::StaticTrait { self_ty, trait_ref } => CallDispatch::StaticTrait {
                self_ty: self.copy_ty(self_ty),
                trait_ref: self.copy_trait_ref(trait_ref),
            },
        };
        ResolvedCallTarget {
            function,
            dispatch,
            owner_type_args: self.copy_ty_slice(owner_type_args),
            method_type_args: self.copy_ty_slice(method_type_args),
        }
    }

    fn copy_expr_slice(&mut self, source: Slice<Ptr<TyExpr<'db>>>) -> Slice<Ptr<TyExpr<'db>>> {
        let source_values = self.source[source].to_vec();
        let target_values: Vec<_> = source_values
            .into_iter()
            .map(|value| self.copy_expr(value))
            .collect();
        self.target.alloc_slice(&target_values)
    }

    fn copy_stmt_slice(&mut self, source: Slice<TyStmt<'db>>) -> Slice<TyStmt<'db>> {
        let source_values = self.source[source].to_vec();
        let target_values: Vec<_> = source_values
            .into_iter()
            .map(|statement| self.copy_stmt(statement))
            .collect();
        self.target.alloc_slice(&target_values)
    }

    fn copy_stmt(&mut self, statement: TyStmt<'db>) -> TyStmt<'db> {
        let TyStmt { kind, span } = statement;
        let kind = match kind {
            TyStmtKind::Let(pattern, annotation, initializer) => TyStmtKind::Let(
                self.copy_pattern(pattern),
                annotation.map(|ty| self.copy_ty(ty)),
                initializer.map(|expr| self.copy_expr(expr)),
            ),
            TyStmtKind::Expr(expr) => TyStmtKind::Expr(self.copy_expr(expr)),
        };
        TyStmt { kind, span }
    }

    fn copy_pattern(&mut self, source: Ptr<TyPat<'db>>) -> Ptr<TyPat<'db>> {
        if let Some(&target) = self.copied_patterns.get(&source) {
            return target;
        }
        let TyPat { kind, ty, span } = self.source[source];
        let kind = self.copy_pattern_kind(kind);
        let ty = self.copy_ty(ty);
        let target = self.target.alloc(TyPat { kind, ty, span });
        self.copied_patterns.insert(source, target);
        target
    }

    fn copy_pattern_kind(&mut self, kind: TyPatKind<'db>) -> TyPatKind<'db> {
        match kind {
            TyPatKind::Wildcard => TyPatKind::Wildcard,
            TyPatKind::Bind(local, mutability) => TyPatKind::Bind(local, mutability),
            TyPatKind::Path(resolution) => TyPatKind::Path(resolution),
            TyPatKind::Tuple(patterns) => TyPatKind::Tuple(self.copy_pattern_slice(patterns)),
            TyPatKind::Struct(resolution, fields) => {
                TyPatKind::Struct(resolution, self.copy_field_pattern_slice(fields))
            }
            TyPatKind::TupleStruct(resolution, patterns) => {
                TyPatKind::TupleStruct(resolution, self.copy_pattern_slice(patterns))
            }
            TyPatKind::Ref(pattern, mutability) => {
                TyPatKind::Ref(self.copy_pattern(pattern), mutability)
            }
            TyPatKind::Literal(literal) => TyPatKind::Literal(literal),
            TyPatKind::Or(patterns) => TyPatKind::Or(self.copy_pattern_slice(patterns)),
            TyPatKind::Rest => TyPatKind::Rest,
            TyPatKind::Missing => TyPatKind::Missing,
        }
    }

    fn copy_pattern_slice(&mut self, source: Slice<Ptr<TyPat<'db>>>) -> Slice<Ptr<TyPat<'db>>> {
        let source_values = self.source[source].to_vec();
        let target_values: Vec<_> = source_values
            .into_iter()
            .map(|value| self.copy_pattern(value))
            .collect();
        self.target.alloc_slice(&target_values)
    }

    fn copy_field_init_slice(
        &mut self,
        source: Slice<TyFieldInit<'db>>,
    ) -> Slice<TyFieldInit<'db>> {
        let source_values = self.source[source].to_vec();
        let target_values: Vec<_> = source_values
            .into_iter()
            .map(|field| {
                let TyFieldInit { name, value, span } = field;
                TyFieldInit {
                    name,
                    value: self.copy_expr(value),
                    span,
                }
            })
            .collect();
        self.target.alloc_slice(&target_values)
    }

    fn copy_field_pattern_slice(
        &mut self,
        source: Slice<TyFieldPat<'db>>,
    ) -> Slice<TyFieldPat<'db>> {
        let source_values = self.source[source].to_vec();
        let target_values: Vec<_> = source_values
            .into_iter()
            .map(|field| {
                let TyFieldPat { name, pat, span } = field;
                TyFieldPat {
                    name,
                    pat: self.copy_pattern(pat),
                    span,
                }
            })
            .collect();
        self.target.alloc_slice(&target_values)
    }

    fn copy_match_arm_slice(&mut self, source: Slice<TyMatchArm<'db>>) -> Slice<TyMatchArm<'db>> {
        let source_values = self.source[source].to_vec();
        let target_values: Vec<_> = source_values
            .into_iter()
            .map(|arm| {
                let TyMatchArm {
                    pat,
                    guard,
                    body,
                    span,
                } = arm;
                TyMatchArm {
                    pat: self.copy_pattern(pat),
                    guard: guard.map(|expr| self.copy_expr(expr)),
                    body: self.copy_expr(body),
                    span,
                }
            })
            .collect();
        self.target.alloc_slice(&target_values)
    }

    fn copy_closure_param_slice(
        &mut self,
        source: Slice<TyClosureParam<'db>>,
    ) -> Slice<TyClosureParam<'db>> {
        let source_values = self.source[source].to_vec();
        let target_values: Vec<_> = source_values
            .into_iter()
            .map(|parameter| {
                let TyClosureParam { pat, ty, span } = parameter;
                TyClosureParam {
                    pat: self.copy_pattern(pat),
                    ty: self.copy_ty(ty),
                    span,
                }
            })
            .collect();
        self.target.alloc_slice(&target_values)
    }
}

#[cfg(test)]
mod tests {
    use super::super::infer::unify::try_unify;
    use super::super::infer::version::Universe;
    use super::*;
    use crate::span::RelativeSpan;
    use crate::ty::{InferVarIndex, IntTy};
    use crate::tytree::TyExprData;

    #[test]
    #[sage::test(crates.sage_ir.inference.body_local_quiescence)]
    #[sage::test(crates.sage_ir.checking.owned_products)]
    #[sage::test(crates.sage_ir.checking.body.elaboration)]
    fn body_finalization_resolves_types_into_a_fresh_stash() {
        let mut source = Stash::new();
        let mut egraph = VersionedEGraph::new();
        let variable = egraph.alloc_var(Version::ROOT, Universe::ROOT);
        let inference_ty = source.alloc(Ty::InferVar(variable));
        let resolved_ty = source.alloc(Ty::Int(IntTy::I32));
        try_unify(
            &mut egraph,
            &mut source,
            Version::ROOT,
            inference_ty,
            resolved_ty,
        )
        .unwrap();
        let tuple_elements = source.alloc_slice(&[inference_ty]);
        let tuple_ty = source.alloc(Ty::Tuple(tuple_elements));

        let span = RelativeSpan { start: 0, end: 0 };
        let root = source.alloc(TyExpr {
            data: TyExprData::Missing,
            ty: tuple_ty,
            span,
        });
        let body = finalize_typed_body(&source, &egraph, root, &[], &[], span);

        assert_eq!(source[inference_ty], Ty::InferVar(InferVarIndex(0)));
        let (target, body_root) = body.open_deref();
        let Ty::Tuple(elements) = target[target[body_root.root].ty] else {
            panic!("completed expression should retain its tuple type")
        };
        let [element] = target[elements] else {
            panic!("completed tuple should have one element")
        };
        assert_eq!(target[element], Ty::Int(IntTy::I32));
    }

    #[test]
    #[sage::test(crates.sage_stash.storage.handle_ownership)]
    #[sage::test(crates.sage_stash.storage.immutable_entries)]
    fn finalized_nested_body_owns_resolved_and_imported_types_after_sources_drop() {
        use crate::cst::expr::Literal;

        let mut imported_stash = Stash::new();
        let imported_bool = imported_stash.alloc(Ty::Bool);
        let imported = Stashed::new(imported_stash, imported_bool);
        let mut source = Stash::new();
        source.alloc(123_u64);
        let boolean = imported.copy_into(&mut source);
        let mut egraph = VersionedEGraph::new();
        let variable = egraph.alloc_var(Version::ROOT, Universe::ROOT);
        let inference = source.alloc(Ty::InferVar(variable));
        try_unify(&mut egraph, &mut source, Version::ROOT, inference, boolean).unwrap();
        let span = RelativeSpan { start: 0, end: 4 };
        let child = source.alloc(TyExpr {
            data: TyExprData::Literal(Literal::Bool(true)),
            ty: inference,
            span,
        });
        let elements = source.alloc_slice(&[child, child]);
        let types = source.alloc_slice(&[inference, boolean]);
        let tuple = source.alloc(Ty::Tuple(types));
        let root = source.alloc(TyExpr {
            data: TyExprData::Tuple(elements),
            ty: tuple,
            span,
        });
        let completed = finalize_typed_body(&source, &egraph, root, &[], &[], span);
        assert_eq!(source[inference], Ty::InferVar(variable));
        assert_eq!(source[types], [inference, boolean]);
        assert_eq!(source[elements], [child, child]);
        assert_eq!(imported.root_deref(), Ty::Bool);
        drop(egraph);
        drop(source);
        drop(imported);

        let (
            target,
            TyBodyData {
                root,
                locals,
                span: completed_span,
            },
        ) = completed.open_deref();
        assert_eq!(completed_span, span);
        assert!(target[locals].is_empty());
        let TyExpr {
            data,
            ty,
            span: root_span,
        } = target[root];
        assert_eq!(root_span, span);
        let Ty::Tuple(types) = target[ty] else {
            panic!("expected tuple type")
        };
        assert_eq!(target[types].len(), 2);
        for ty in &target[types] {
            assert_eq!(target[*ty], Ty::Bool);
        }
        let TyExprData::Tuple(elements) = data else {
            panic!("expected tuple expression")
        };
        assert_eq!(target[elements].len(), 2);
        assert_eq!(target[elements][0], target[elements][1]);
        for child in &target[elements] {
            let TyExpr {
                data,
                ty,
                span: child_span,
            } = target[*child];
            assert_eq!(data, TyExprData::Literal(Literal::Bool(true)));
            assert_eq!(target[ty], Ty::Bool);
            assert_eq!(child_span, span);
        }
    }
}
