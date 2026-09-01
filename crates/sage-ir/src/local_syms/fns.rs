use sage_stash::{Ptr, StashDirect, Stashed};

use crate::cst::fns::FnCst;
use crate::local_syms::LocalAssociatedOwner;
use crate::name::Name;
use crate::scope::ScopeSymbol;
use crate::span::AbsoluteSpan;
use crate::ty::{Binder, FnSig};
use crate::tytree::CheckedBody;

struct OpenedOwner<'db> {
    generics: Vec<crate::generic_param::GenericParam<'db>>,
    self_ty: Option<Ptr<crate::ty::Ty<'db>>>,
    where_clauses: Vec<crate::ty::WherePredicate<'db>>,
    eligibility: crate::ty::SolverEligibility,
}

fn open_owner<'db>(
    cx: &mut crate::check::Check<'_, 'db>,
    owner: Option<LocalAssociatedOwner<'db>>,
) -> OpenedOwner<'db> {
    use crate::generic_param::GenericParamKind;
    use crate::symbol::TraitSymbol;
    use crate::ty::{SolverEligibility, TraitRef, Ty, WherePredicate};

    match owner {
        None => OpenedOwner {
            generics: Vec::new(),
            self_ty: None,
            where_clauses: Vec::new(),
            eligibility: SolverEligibility::Eligible,
        },
        Some(LocalAssociatedOwner::Trait(local_trait)) => {
            let signature = local_trait.sig(cx.db);
            let binder = signature.copy_into(&mut cx.target_stash);
            let generics = cx.target_stash[binder.generics].to_vec();
            let self_ty = cx.target_stash.alloc(Ty::Param(binder.value.self_param));
            let mut where_clauses = cx.target_stash[binder.value.where_clauses].to_vec();
            let args: Vec<_> = generics
                .iter()
                .skip(1)
                .filter(|generic| generic.kind(cx.db) == GenericParamKind::Type)
                .map(|generic| cx.target_stash.alloc(Ty::Param(*generic)))
                .collect();
            let args = cx.target_stash.alloc_slice(&args);
            where_clauses.push(WherePredicate {
                self_ty,
                trait_ref: TraitRef {
                    trait_sym: TraitSymbol::Local(local_trait),
                    args,
                },
            });
            OpenedOwner {
                generics,
                self_ty: Some(self_ty),
                where_clauses,
                eligibility: binder.value.solver_eligibility,
            }
        }
        Some(LocalAssociatedOwner::Impl(local_impl)) => {
            let signature = local_impl.sig(cx.db);
            let binder = signature.copy_into(&mut cx.target_stash);
            let generics = cx.target_stash[binder.generics].to_vec();
            let mut where_clauses = cx.target_stash[binder.value.where_clauses].to_vec();
            if let Some(trait_ref) = binder.value.trait_ref {
                where_clauses.push(WherePredicate {
                    self_ty: binder.value.self_ty,
                    trait_ref,
                });
            }
            OpenedOwner {
                generics,
                self_ty: Some(binder.value.self_ty),
                where_clauses,
                eligibility: binder.value.solver_eligibility,
            }
        }
    }
}

// ANCHOR: architecture_local_function_symbol
#[salsa::tracked(debug)]
pub struct LocalFnSym<'db> {
    pub name: Name<'db>,
    pub scope: ScopeSymbol<'db>,
    pub owner: Option<LocalAssociatedOwner<'db>>,

    #[tracked]
    #[returns(ref)]
    pub cst: FnCst<'db>,

    #[tracked]
    pub span: AbsoluteSpan<'db>,
}
// ANCHOR_END: architecture_local_function_symbol

impl StashDirect for LocalFnSym<'_> {}

impl<'db> LocalFnSym<'db> {
    pub fn attrs(
        self,
        db: &'db dyn crate::Db,
    ) -> (
        &'db sage_stash::Stash,
        &'db [crate::cst::attrs::AttrCst<'db>],
    ) {
        let (stash, data) = self.cst(db).open_deref();
        (stash, &stash[data.attrs])
    }
}

#[salsa::tracked]
impl<'db> LocalFnSym<'db> {
    /// Computes the signature: generics, parameter types, return type.
    // ANCHOR: example_fn_sig
    #[salsa::tracked]
    pub fn sig(self, db: &'db dyn crate::Db) -> Stashed<Binder<'db, FnSig<'db>>> {
        use crate::check::Check;
        use crate::cst::fns::ReceiverCst;
        use crate::cst::generics::CheckGenerics;
        use crate::resolve::{Namespace, Resolution, Resolver};
        use crate::symbol::Symbol;
        use crate::ty::{CheckedReceiver, MethodReceiver, SolverEligibility};

        let (src, cst) = self.cst(db).open_deref();
        let mut cx = Check::new(db, src, Resolver::new(db, self.scope(db)));
        cx.current_sym = Some(crate::local_syms::LocalModItemSym::Function(self));

        let owner = open_owner(&mut cx, self.owner(db));
        if !owner.generics.is_empty() {
            cx.resolver
                .ribs
                .add_generic_params(db, owner.generics.iter().copied());
        }
        if let Some(self_ty) = owner.self_ty {
            cx.resolver.ribs.add(
                Name::new(db, "Self".to_owned()),
                Namespace::Type,
                Resolution::SelfTy(cx.target_stash[self_ty]),
            );
        }

        let parent: Symbol<'db> = self.into();
        let method_generics = cst.generics.check(db, &mut cx, parent);

        let mut receiver = None;
        let mut receiver_eligibility = SolverEligibility::Eligible;
        let mut param_tys = Vec::new();
        for parameter in &cx.source_stash[cst.params] {
            match parameter.receiver {
                None => {
                    let ty = cx.source_stash[parameter.ty].check(&mut cx);
                    param_tys.push(cx.target_stash.alloc(ty));
                }
                Some(ReceiverCst::Value { mutable_binding }) => {
                    if receiver.is_some() || owner.self_ty.is_none() {
                        receiver_eligibility = SolverEligibility::Unsupported;
                    } else {
                        receiver = Some(CheckedReceiver {
                            owner_self_ty: owner.self_ty.unwrap(),
                            form: MethodReceiver::Value { mutable_binding },
                        });
                    }
                }
                Some(ReceiverCst::Ref {
                    mutability,
                    lifetime: _,
                }) => {
                    if receiver.is_some() || owner.self_ty.is_none() {
                        receiver_eligibility = SolverEligibility::Unsupported;
                    } else {
                        receiver = Some(CheckedReceiver {
                            owner_self_ty: owner.self_ty.unwrap(),
                            form: MethodReceiver::Ref { mutability },
                        });
                    }
                }
                Some(ReceiverCst::Typed) => {
                    receiver_eligibility = SolverEligibility::Unsupported;
                    let ty = cx.source_stash[parameter.ty].check(&mut cx);
                    param_tys.push(cx.target_stash.alloc(ty));
                }
            }
        }
        let params = cx.target_stash.alloc_slice(&param_tys);

        let ret_ty = match cst.ret {
            Some(ret_ptr) => cx.source_stash[ret_ptr].check(&mut cx),
            None => {
                let unit = cx.target_stash.alloc_slice(&[]);
                crate::ty::Ty::Tuple(unit)
            }
        };
        let ret = cx.target_stash.alloc(ret_ty);

        let (method_where_clauses, method_eligibility) = crate::check::trait_env::lower_predicates(
            &mut cx,
            cst.generics,
            method_generics,
            cst.where_clauses,
        );
        let mut where_clauses = owner.where_clauses;
        where_clauses.extend_from_slice(&cx.target_stash[method_where_clauses]);
        let where_clauses = cx.target_stash.alloc_slice(&where_clauses);
        let solver_eligibility = owner.eligibility.and(method_eligibility);
        let parameter_env = cx.complete_parameter_env(where_clauses, solver_eligibility);
        let method_candidate_eligibility = if self.owner(db).is_some() && receiver.is_some() {
            parameter_env.solver_eligibility.and(receiver_eligibility)
        } else {
            SolverEligibility::Unsupported
        };

        let owner_generic_count = owner.generics.len() as u32;
        let mut generics = owner.generics;
        generics.extend_from_slice(&cx.target_stash[method_generics]);
        let generics = cx.target_stash.alloc_slice(&generics);
        let fn_sig = FnSig {
            owner_generic_count,
            owner_self_ty: owner.self_ty,
            receiver,
            params,
            ret,
            parameter_env,
            method_candidate_eligibility,
            const_call_complete: false,
        };
        let binder = Binder::new(fn_sig, generics);
        cx.finish(binder)
    }
    // ANCHOR_END: example_fn_sig

    /// Resolves and type-checks the function body in a single walk.
    // ANCHOR: example_fn_body
    #[salsa::tracked(returns(ref))]
    pub fn body(self, db: &'db dyn crate::Db) -> CheckedBody<'db> {
        use crate::check::infer_ctx::{ErrorContext, InferCtx, Scope};
        use crate::local_syms::LocalModItemSym;
        use crate::resolve::Resolver;
        use crate::ty::BinderExt;

        let sig = self.sig(db);
        let (src, cst) = self.cst(db).open_deref();

        let current_sym = LocalModItemSym::Function(self);
        let mut cx = InferCtx::new(db, src, Some(current_sym));
        let mut scope = Scope::new(Resolver::new(db, self.scope(db)));

        // Bring generics into scope.
        scope
            .resolver
            .ribs
            .add_generic_params(db, sig.iter_symbols());

        // Import the signature's param/return types into the body stash.
        let imported = cx.import_fn_sig(&sig);
        if let Some(owner_self_ty) = imported.owner_self_ty {
            scope.resolver.ribs.add(
                crate::name::Name::new(db, "Self".to_owned()),
                crate::resolve::Namespace::Type,
                crate::resolve::Resolution::SelfTy(cx.stash()[owner_self_ty]),
            );
        }
        cx.set_solver_environment(imported.parameter_env);
        let ret_span = cst.ret.map(|r| src[r].span);
        cx.set_ret_ty(imported.ret, ret_span);

        // Bind function parameters as locals with their declared types.
        let ordinary_params = cx.stash()[imported.params].to_vec();
        let mut ordinary_params = ordinary_params.into_iter();
        let mut body_params = Vec::new();
        for parameter in &src[cst.params] {
            let ty = match parameter.receiver {
                None | Some(crate::cst::fns::ReceiverCst::Typed) => ordinary_params
                    .next()
                    .expect("checked ordinary parameter must have a type"),
                Some(
                    crate::cst::fns::ReceiverCst::Value { .. }
                    | crate::cst::fns::ReceiverCst::Ref { .. },
                ) => match imported.receiver {
                    Some(receiver) => match receiver.form {
                        crate::ty::MethodReceiver::Value { .. } => receiver.owner_self_ty,
                        crate::ty::MethodReceiver::Ref { mutability } => {
                            cx.alloc_ty(crate::ty::Ty::Ref(
                                receiver.owner_self_ty,
                                mutability,
                                crate::ty::Lifetime::Dummy,
                            ))
                        }
                    },
                    None => {
                        let error = cx.record(crate::diagnostic::Diagnostic::error(
                            cx.span(parameter.span),
                            "receiver has no associated owner",
                        ));
                        cx.alloc_ty(crate::ty::Ty::Error(error))
                    }
                },
            };
            body_params.push(ty);
        }
        let body_params = cx.stash_mut().alloc_slice(&body_params);
        scope.bind_params(&cx, body_params, cst.params);

        // Walk the body CST: resolve names + infer types → TyExpr.
        let body_expr = cx.block_on_body(async {
            let expr = match cst.body {
                Some(body_ptr) => src[body_ptr].check_with(&cx, &scope).await,
                None => {
                    let ty = cx.alloc_ty(crate::ty::Ty::Never);
                    cx.alloc_expr(crate::tytree::TyExprData::Missing, ty, cst.span)
                }
            };

            // Constrain body type against declared return type.
            let body_ty = cx.stash()[expr].ty;
            let body_span = cx.stash()[expr].span;
            if let Err(e) = cx.require_coerce(body_ty, imported.ret, body_span) {
                let e = if let Some(ret_ptr) = cst.ret {
                    let ret_span = src[ret_ptr].span;
                    e.with_context(ErrorContext::ReturnType { ret_span })
                } else {
                    e
                };
                cx.catch(e);
            }
            expr
        });

        // Resolve remaining inference variables.
        cx.finalize();

        cx.finish(body_expr, cst.span)
    }
    // ANCHOR_END: example_fn_body
}
