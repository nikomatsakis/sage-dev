use sage_stash::Ptr;

use crate::cst::expr::BinaryOp;
use crate::diagnostic::{Diagnostic, ErrorReported};
use crate::generic_param::GenericParamKind;
use crate::span::RelativeSpan;
use crate::ty::{
    AliasTy, FnSig, MethodReceiver, ProjectionTy, SolverEligibility, TraitRef, Ty, WherePredicate,
};
use crate::tytree::{CallDispatch, ResolvedCallTarget, TyExpr, TyExprData};
use crate::well_known::{OperatorMethod, OperatorMethodSymbols, operator_method};

use super::infer::version::Version;
use super::infer_ctx::{InferCtx, Scope};

/// Type-check one binary operation and construct its transient typed form.
///
/// Lazy boolean operators remain control-flow nodes. Every overloadable
/// operator is represented by a provisional resolved call whose expression
/// slot is replaced with an error node if its retained proof later fails.
pub(crate) fn check_binary_operator<'db>(
    cx: &InferCtx<'_, 'db>,
    scope: &Scope<'db>,
    operation: BinaryOp,
    left: Ptr<TyExpr<'db>>,
    right: Ptr<TyExpr<'db>>,
    span: RelativeSpan,
) -> (TyExprData<'db>, Ptr<Ty<'db>>) {
    let left_ty = cx.stash()[left].ty;
    let right_ty = cx.stash()[right].ty;
    if let Some(error) = cx.error_in_ty(left_ty).or_else(|| cx.error_in_ty(right_ty)) {
        return propagated_error(cx, error);
    }

    if matches!(operation, BinaryOp::And | BinaryOp::Or) {
        return check_lazy_boolean(cx, operation, left, right);
    }

    let Some(method_kind) = operator_method_for(operation) else {
        unreachable!("lazy operators returned before overloadable operator lookup")
    };
    let Some(symbols) = operator_method(cx.db, scope.resolver.local_crate(), method_kind) else {
        return diagnosed_error(
            cx,
            span,
            "the operator trait method is unavailable from core metadata",
        );
    };
    let Some(signature) = symbols.function.sig(cx.db) else {
        return diagnosed_error(cx, span, "the operator method signature is unavailable");
    };
    let Some(instantiated) =
        instantiate_operator_signature(cx, symbols, &signature, left_ty, right_ty)
    else {
        return diagnosed_error(cx, span, "the operator method signature is unsupported");
    };
    let InstantiatedOperatorSignature {
        signature,
        owner_predicate,
        additional_requirements,
    } = instantiated;

    let trait_ref = owner_predicate.trait_ref;
    let return_ty = cx.stash()[signature.ret];
    let result_relation = match return_ty {
        Ty::Alias(AliasTy::Associated(projection))
            if projection_matches(cx, projection, symbols, left_ty, right_ty) =>
        {
            Some(projection)
        }
        Ty::Bool => None,
        Ty::Alias(_)
        | Ty::Char
        | Ty::Int(_)
        | Ty::Uint(_)
        | Ty::Float(_)
        | Ty::Str
        | Ty::Adt(_, _)
        | Ty::Ref(_, _, _)
        | Ty::Tuple(_)
        | Ty::Slice(_)
        | Ty::Array(_, _)
        | Ty::FnPtr(_, _)
        | Ty::Param(_)
        | Ty::InferVar(_)
        | Ty::Never
        | Ty::Error(_) => {
            return diagnosed_error(cx, span, "the operator result signature is unsupported");
        }
    };
    let left_span = cx.stash()[left].span;
    let receiver = cx.adjust_method_receiver(
        left,
        left_ty,
        signature
            .receiver
            .expect("operator receiver was validated")
            .form,
        left_span,
    );
    let parameter = cx.stash()[signature.params][0];
    let Some(argument) = adjust_argument(cx, right, right_ty, parameter) else {
        return diagnosed_error(
            cx,
            span,
            "the operands do not match the operator method signature",
        );
    };
    let (slot, result_ty) = cx.alloc_expr_slot();
    if result_relation.is_none() {
        cx.assume_eq(result_ty, signature.ret);
    }

    let owner_type_args = cx.stash_mut().alloc_slice(&[left_ty, right_ty]);
    let method_type_args = cx.stash_mut().alloc_slice(&[]);
    let arguments = cx.stash_mut().alloc_slice(&[receiver, argument]);
    let call = cx.alloc_expr(
        TyExprData::ResolvedCall(
            ResolvedCallTarget {
                function: symbols.function,
                dispatch: CallDispatch::StaticTrait {
                    self_ty: left_ty,
                    trait_ref,
                },
                owner_type_args,
                method_type_args,
            },
            arguments,
        ),
        result_ty,
        span,
    );
    cx.fill_expr_slot(slot, call);

    let watched_types = vec![left_ty, right_ty, result_ty];
    if let Some(projection) = result_relation {
        cx.submit_operator_alias_relation(
            AliasTy::Associated(projection),
            result_ty,
            span,
            slot,
            watched_types.clone(),
        );
    } else {
        cx.submit_operator_trait_obligation(left_ty, trait_ref, span, slot, watched_types.clone());
    }
    for requirement in additional_requirements {
        cx.submit_operator_trait_obligation(
            requirement.self_ty,
            requirement.trait_ref,
            span,
            slot,
            watched_types.clone(),
        );
    }
    (TyExprData::Unresolved(slot), result_ty)
}

fn check_lazy_boolean<'db>(
    cx: &InferCtx<'_, 'db>,
    operation: BinaryOp,
    left: Ptr<TyExpr<'db>>,
    right: Ptr<TyExpr<'db>>,
) -> (TyExprData<'db>, Ptr<Ty<'db>>) {
    let bool_ty = cx.alloc_ty(Ty::Bool);
    let mut first_error = None;
    for operand in [left, right] {
        let operand = cx.stash()[operand];
        if let Err(error) = cx.require_eq(operand.ty, bool_ty, operand.span) {
            let error = cx.catch(error);
            first_error.get_or_insert(error);
        }
    }
    if let Some(error) = first_error {
        propagated_error(cx, error)
    } else {
        (TyExprData::Binary(left, operation, right), bool_ty)
    }
}

struct InstantiatedOperatorSignature<'db> {
    signature: FnSig<'db>,
    owner_predicate: WherePredicate<'db>,
    additional_requirements: Vec<WherePredicate<'db>>,
}

fn instantiate_operator_signature<'db>(
    cx: &InferCtx<'_, 'db>,
    symbols: OperatorMethodSymbols<'db>,
    signature: &sage_stash::Stashed<crate::ty::Binder<'db, FnSig<'db>>>,
    left_ty: Ptr<Ty<'db>>,
    right_ty: Ptr<Ty<'db>>,
) -> Option<InstantiatedOperatorSignature<'db>> {
    let binder = signature.root();
    let source = signature.stash();
    let generics = &source[binder.generics];
    if binder.value.owner_generic_count != 2
        || generics.len() != 2
        || generics
            .iter()
            .any(|generic| generic.kind(cx.db) != GenericParamKind::Type)
        || binder.value.method_candidate_eligibility != SolverEligibility::Eligible
    {
        return None;
    }

    let arguments = {
        let stash = cx.stash();
        vec![stash[left_ty], stash[right_ty]]
    };
    let instantiated =
        crate::ty_fold::instantiate_fn_sig(cx.db, source, &mut cx.stash_mut(), &binder, arguments);
    let receiver = instantiated.receiver?;
    let owner_self_ty = instantiated.owner_self_ty?;
    let parameters = cx.stash()[instantiated.params].to_vec();
    let predicates = cx.stash()[instantiated.parameter_env.where_clauses].to_vec();
    if instantiated.parameter_env.solver_eligibility != SolverEligibility::Eligible
        || matches!(
            receiver.form,
            MethodReceiver::Value {
                mutable_binding: true
            }
        )
        || parameters.len() != 1
        || !probe_equalities(
            cx,
            &[
                (receiver.owner_self_ty, owner_self_ty),
                (owner_self_ty, left_ty),
            ],
        )
    {
        return None;
    }
    let mut owner_predicates = predicates
        .iter()
        .copied()
        .filter(|predicate| predicate_matches(cx, *predicate, symbols, left_ty, right_ty));
    let owner_predicate = owner_predicates.next()?;
    if owner_predicates.next().is_some() {
        return None;
    }
    let additional_requirements = predicates
        .into_iter()
        .filter(|predicate| *predicate != owner_predicate)
        .collect();
    Some(InstantiatedOperatorSignature {
        signature: instantiated,
        owner_predicate,
        additional_requirements,
    })
}

fn predicate_matches<'db>(
    cx: &InferCtx<'_, 'db>,
    predicate: WherePredicate<'db>,
    symbols: OperatorMethodSymbols<'db>,
    left_ty: Ptr<Ty<'db>>,
    right_ty: Ptr<Ty<'db>>,
) -> bool {
    operator_trait_ref_matches(
        cx,
        predicate.self_ty,
        predicate.trait_ref,
        symbols,
        left_ty,
        right_ty,
    )
}

fn projection_matches<'db>(
    cx: &InferCtx<'_, 'db>,
    projection: ProjectionTy<'db>,
    symbols: OperatorMethodSymbols<'db>,
    left_ty: Ptr<Ty<'db>>,
    right_ty: Ptr<Ty<'db>>,
) -> bool {
    if !cx.stash()[projection.args].is_empty() {
        return false;
    }
    operator_trait_ref_matches(
        cx,
        projection.self_ty,
        projection.trait_ref,
        symbols,
        left_ty,
        right_ty,
    )
}

fn operator_trait_ref_matches<'db>(
    cx: &InferCtx<'_, 'db>,
    self_ty: Ptr<Ty<'db>>,
    trait_ref: TraitRef<'db>,
    symbols: OperatorMethodSymbols<'db>,
    left_ty: Ptr<Ty<'db>>,
    right_ty: Ptr<Ty<'db>>,
) -> bool {
    if trait_ref.trait_sym != symbols.trait_symbol {
        return false;
    }
    let arguments = cx.stash()[trait_ref.args].to_vec();
    if let [argument] = arguments.as_slice() {
        probe_equalities(cx, &[(self_ty, left_ty), (*argument, right_ty)])
    } else {
        false
    }
}

fn probe_equalities<'db>(
    cx: &InferCtx<'_, 'db>,
    equalities: &[(Ptr<Ty<'db>>, Ptr<Ty<'db>>)],
) -> bool {
    let transaction = cx.branch_from(Version::ROOT);
    let result = equalities
        .iter()
        .all(|(left, right)| cx.try_eq_in(transaction, *left, *right));
    cx.discard_branch(transaction);
    result
}

fn commit_equality<'db>(cx: &InferCtx<'_, 'db>, left: Ptr<Ty<'db>>, right: Ptr<Ty<'db>>) -> bool {
    let transaction = cx.branch_from(Version::ROOT);
    if cx.try_eq_in(transaction, left, right) {
        cx.commit_branch(transaction);
        true
    } else {
        cx.discard_branch(transaction);
        false
    }
}

fn adjust_argument<'db>(
    cx: &InferCtx<'_, 'db>,
    expression: Ptr<TyExpr<'db>>,
    ty: Ptr<Ty<'db>>,
    parameter: Ptr<Ty<'db>>,
) -> Option<Ptr<TyExpr<'db>>> {
    if commit_equality(cx, ty, parameter) {
        return Some(expression);
    }
    let parameter = cx.find(parameter);
    let Ty::Ref(inner, mutability, _) = cx.stash()[parameter] else {
        return None;
    };
    if !commit_equality(cx, ty, inner) {
        return None;
    }
    let span = cx.stash()[expression].span;
    Some(cx.alloc_expr(TyExprData::Ref(expression, mutability), parameter, span))
}

fn propagated_error<'db>(
    cx: &InferCtx<'_, 'db>,
    error: ErrorReported,
) -> (TyExprData<'db>, Ptr<Ty<'db>>) {
    let ty = cx.alloc_ty(Ty::Error(error));
    (TyExprData::Error(error), ty)
}

fn diagnosed_error<'db>(
    cx: &InferCtx<'_, 'db>,
    span: RelativeSpan,
    message: &'static str,
) -> (TyExprData<'db>, Ptr<Ty<'db>>) {
    let error = cx.record(Diagnostic::error(cx.span(span), message).label(cx.span(span), message));
    propagated_error(cx, error)
}

fn operator_method_for(operation: BinaryOp) -> Option<OperatorMethod> {
    match operation {
        BinaryOp::Add => Some(OperatorMethod::Add),
        BinaryOp::Sub => Some(OperatorMethod::Sub),
        BinaryOp::Mul => Some(OperatorMethod::Mul),
        BinaryOp::Div => Some(OperatorMethod::Div),
        BinaryOp::Rem => Some(OperatorMethod::Rem),
        BinaryOp::BitAnd => Some(OperatorMethod::BitAnd),
        BinaryOp::BitOr => Some(OperatorMethod::BitOr),
        BinaryOp::BitXor => Some(OperatorMethod::BitXor),
        BinaryOp::Shl => Some(OperatorMethod::Shl),
        BinaryOp::Shr => Some(OperatorMethod::Shr),
        BinaryOp::Eq => Some(OperatorMethod::Eq),
        BinaryOp::Ne => Some(OperatorMethod::Ne),
        BinaryOp::Lt => Some(OperatorMethod::Lt),
        BinaryOp::Le => Some(OperatorMethod::Le),
        BinaryOp::Gt => Some(OperatorMethod::Gt),
        BinaryOp::Ge => Some(OperatorMethod::Ge),
        BinaryOp::And | BinaryOp::Or => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_overloadable_binary_token_maps_to_its_method() {
        let cases = [
            (BinaryOp::Add, OperatorMethod::Add),
            (BinaryOp::Sub, OperatorMethod::Sub),
            (BinaryOp::Mul, OperatorMethod::Mul),
            (BinaryOp::Div, OperatorMethod::Div),
            (BinaryOp::Rem, OperatorMethod::Rem),
            (BinaryOp::BitAnd, OperatorMethod::BitAnd),
            (BinaryOp::BitOr, OperatorMethod::BitOr),
            (BinaryOp::BitXor, OperatorMethod::BitXor),
            (BinaryOp::Shl, OperatorMethod::Shl),
            (BinaryOp::Shr, OperatorMethod::Shr),
            (BinaryOp::Eq, OperatorMethod::Eq),
            (BinaryOp::Ne, OperatorMethod::Ne),
            (BinaryOp::Lt, OperatorMethod::Lt),
            (BinaryOp::Le, OperatorMethod::Le),
            (BinaryOp::Gt, OperatorMethod::Gt),
            (BinaryOp::Ge, OperatorMethod::Ge),
        ];
        for (operation, method) in cases {
            assert_eq!(operator_method_for(operation), Some(method));
        }
        assert_eq!(operator_method_for(BinaryOp::And), None);
        assert_eq!(operator_method_for(BinaryOp::Or), None);
    }
}
