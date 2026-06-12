use crate::Db;
use crate::body_resolve::resolve_body;
use crate::infer::check::type_check_body;
use crate::item::FnAst;
use crate::resolved::ResolvedBody;
use crate::scope::ScopeSymbol;
use crate::sig_lower::fn_signature;
use crate::symbol::FnSymbol;

/// The combined output of body resolution and type checking for a function.
pub struct TypedBody<'db> {
    pub body: ResolvedBody<'db>,
    pub errors: Vec<String>,
}

impl<'db> TypedBody<'db> {
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

impl PartialEq for TypedBody<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.body == other.body && self.errors == other.errors
    }
}

impl Eq for TypedBody<'_> {}

impl std::hash::Hash for TypedBody<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.body.hash(state);
        self.errors.hash(state);
    }
}

// Safety: salsa guarantees `old_pointer` is valid and aligned (it points into
// the tracked-function memo slot). Our `PartialEq` correctly reflects semantic
// equality (body fingerprint + rendered error strings).
unsafe impl salsa::Update for TypedBody<'_> {
    unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
        let old = unsafe { &*old_pointer };
        if *old == new_value {
            false
        } else {
            unsafe { *old_pointer = new_value };
            true
        }
    }
}

/// Memoized query that resolves and type-checks a function body.
#[salsa::tracked(returns(ref))]
pub fn fn_body<'db>(
    db: &'db dyn Db,
    function: FnAst<'db>,
    scope: ScopeSymbol<'db>,
) -> TypedBody<'db> {
    let body = resolve_body(db, function, scope);
    let fn_sym = FnSymbol::local(function, scope);
    let sig = fn_signature(db, fn_sym, scope);
    let result = type_check_body(db, &body, sig, scope);
    let errors = result.render_errors(db);

    TypedBody { body, errors }
}

impl<'db> FnSymbol<'db> {
    pub fn body(self, db: &'db dyn Db) -> &'db TypedBody<'db> {
        let scope = self
            .scope()
            .expect("FnSymbol::body requires a scoped symbol");
        let ast = self
            .as_ast()
            .expect("FnSymbol::body requires a local symbol");
        fn_body(db, ast, scope)
    }
}
