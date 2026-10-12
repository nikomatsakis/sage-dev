module crate::types;

// Sources: ty.rs, tytree/, scope.rs, span.rs, check/solve/ in crates/sage-ir/src.
#[rust(crate::name::Name)]
type Name """Interned spelling, not definition identity.""";
#[rust(crate::scope::ScopeSymbol)]
type Scope """Enclosing local module or crate scope.""";
#[rust(std::option::Option<crate::local_syms::LocalAssociatedOwner>)]
type Owner """Optional trait or impl owner of an associated definition.""";
#[rust(none, """Groups per-item CST types; there is no single Rust Syntax type.""")]
type Syntax """Self-contained item CST, including source-relative spans.""";
#[rust(crate::span::AbsoluteSpan)]
type Span """Absolute source or expansion provenance.""";
#[rust(none, """Review-only sum of the separate signature result types in ty.rs.""")]
enum Signature {
    """Review-only sum for grouped checking and metadata operations. Rust exposes
    these results separately; this enum is not a Rust runtime representation."""
    Function(Binder<FnSig>),
    Struct(Binder<StructSig>),
    Enum(Binder<EnumSig>),
    Trait(Binder<TraitSignatureData>),
    Impl(Binder<ImplSignatureData>),
    ExternalAdt(Binder<ExternalAdtSignatureData>),
}
#[rust(std::vec::Vec<crate::symbol::Symbol>)]
struct Items {
    """Review wrapper for ordered module/variant symbol lists; membership does not imply expansion completeness."""
    items: Vec<crate::symbols::Symbol>,
}
#[rust(none, """Groups fields, associated values, and declared types returned by distinct Rust operations.""")]
type Value """Checked associated value or field/member representation.""";
#[rust(none, """Groups resolved values and failure outcomes across resolver operations.""")]
type Resolution """Resolved symbol, ambiguity, failure, or incomplete lookup.""";
#[rust(none, """Groups lexical ribs, generic parameters, and solver assumptions; no single environment type.""")]
type Environment """Generic parameters, Self, assumptions, and lexical bindings.""";
#[rust(crate::check::infer_ctx::InferCtx)]
type InferenceState """Body-local variables, versioned constraints, obligations, tasks, and diagnostics.""";
#[rust(crate::check::solve::GoalQuery)]
type Goal """Canonical proof or normalization question and its environment.""";
#[rust(crate::check::solve::QueryResult)]
type Answer """Canonical solver response, including uncertainty and resource limits.""";
#[rust(none, """Groups LocalImplCandidates and ExternalImplCandidates with their completeness evidence.""")]
type Candidates """Candidate set with completeness evidence.""";
#[rust(none, """Groups the owned Raw metadata types supplied by TcxDb.""")]
type Metadata """Owned keyed facts supplied by an external provider.""";
#[rust(())]
type Unit """No semantic result.""";
#[rust(none, """Groups file and generated sources, their text, and module body provenance.""")]
type Source """Source text or generated-token source with provenance.""";
#[rust(none, """Groups external SymExt and scoped identity families.""")]
type Identity """External crate and definition indices, or a scoped identity.""";
#[rust(bool)]
type Boolean """Boolean flag.""";
#[rust(crate::scope::Edition)]
type Edition """Rust edition controlling syntax and resolution.""";
#[rust(std::option::Option<crate::scope::ScopeSymbol>)]
type OptionalScope """Optional parent scope; absent for the root module.""";

// Source: crates/sage-ir/src/ty.rs::Binder. Representation is deliberately opaque.
#[rust(crate::ty::Binder)]
type Binder<T> {
    """A value of T together with the generic parameters binding its references.
    Parameters have stable identities; opening an owner's binder reuses those
    identities before introducing an associated item's own parameters."""
}

// Source: crates/sage-ir/src/ty.rs::Ty. Keep the current type-only argument model.
#[rust(crate::ty::Ty)]
enum Ty {
    """A resolved Rust type. Recursive children are semantic values; storage is omitted."""
    Bool,
    Char,
    Int(IntTy),
    Uint(UintTy),
    Float(FloatTy),
    Str,
    Adt {
        """An ADT identity applied to type arguments. The current Rust representation
        carries Symbol and type arguments, not a general GenericArg list."""
        definition: crate::symbols::Symbol,
        arguments: Vec<Ty>,
    },
    Alias(AliasTy),
    Ref {
        referent: Ty,
        mutability: Mutability,
        lifetime: Lifetime,
    },
    Tuple(Vec<Ty>),
    Slice(Ty),
    Array(Ty, Const),
    FnPtr {
        inputs: Vec<Ty>,
        output: Ty,
    },
    Param(
        """A universal parameter whose kind must be Type."""
        GenericParam,
    ),
    InferVar(
        """A body-local existential variable. Must be resolved in a successful completed body."""
        InferVarIndex,
    ),
    Never,
    Error(ErrorReported),
}

#[rust(crate::ty::IntTy)]
enum IntTy { I8, I16, I32, I64, I128, Isize }
#[rust(crate::ty::UintTy)]
enum UintTy { U8, U16, U32, U64, U128, Usize }
#[rust(crate::ty::FloatTy)]
enum FloatTy { F32, F64 }
// Source: crates/sage-ir/src/cst/mod.rs::Mutability.
#[rust(crate::cst::Mutability)]
enum Mutability { Shared, Mut }
#[rust(crate::ty::Lifetime)]
enum Lifetime {
    """Meaningful lifetime reasoning is not implemented in the current compiler."""
    Dummy """Placeholder used for every represented lifetime.""",
}
#[rust(crate::ty::Const)]
enum Const {
    Literal(u64),
    Param("""A parameter whose kind must be Const.""" GenericParam),
    Other(crate::symbols::Symbol),
}

#[rust(crate::ty::AliasTy)]
enum AliasTy {
    """Alias identity is retained until normalization is explicitly requested."""
    Named(NamedAliasTy),
    Associated(ProjectionTy),
    Opaque(OpaqueAliasTy),
}
#[rust(crate::ty::NamedAliasTy)]
struct NamedAliasTy {
    definition: TypeAliasSymbol,
    arguments: Vec<Ty>,
}
#[rust(crate::ty::ProjectionTy)]
struct ProjectionTy {
    associated_ty: TypeAliasSymbol,
    self_ty: Ty,
    trait_ref: TraitRef,
    arguments: Vec<Ty>,
}
#[rust(crate::ty::OpaqueAliasTy)]
struct OpaqueAliasTy {
    definition: TypeAliasSymbol,
    arguments: Vec<Ty>,
}
// These identity families are intentionally described without storage layout.
#[rust(crate::symbol::TypeAliasSymbol)]
type TypeAliasSymbol { """Kind-specific local or external type-alias identity; Rust symbol::TypeAliasSymbol.""" }
#[rust(crate::symbol::ConstSymbol)]
type ConstSymbol { """Kind-specific local or external constant identity; Rust symbol::ConstSymbol.""" }
#[rust(crate::generic_param::GenericParam)]
type GenericParam {
    """Stable parameter identity: source-backed Ast, external Ext, or an AlphaEquiv
    placeholder. Its kind is Type, Lifetime, or Const. See generic_param.rs."""
}
#[rust(crate::ty::InferVarIndex)]
type InferVarIndex { """Dense index identifying an inference variable inside its owning inference context.""" }
#[rust(crate::diagnostic::ErrorReported)]
type ErrorReported { """Evidence that a diagnostic has already been recorded for an error-recovery value.""" }

// Source: crates/sage-ir/src/ty.rs::FnSig; no semantic fields are omitted.
#[rust(crate::ty::FnSig)]
struct FnSig {
    """Checked function interface; generic parameters are supplied by its enclosing Binder."""
    """Leading binder parameters inherited from the trait or impl owner."""
    owner_generic_count: u32,
    owner_self_ty: Option<Ty>,
    receiver: Option<CheckedReceiver>,
    params: Vec<Ty>,
    ret: Ty,
    parameter_env: CheckedParameterEnv,
    method_candidate_eligibility: SolverEligibility,
    """Whether const-only call conditions are fully represented."""
    const_call_complete: bool,
}
#[rust(crate::ty::CheckedReceiver)]
struct CheckedReceiver {
    owner_self_ty: Ty,
    form: MethodReceiver,
}
#[rust(crate::ty::MethodReceiver)]
enum MethodReceiver {
    Value { mutable_binding: bool },
    Ref { mutability: Mutability },
}
#[rust(crate::ty::StructSig)]
struct StructSig { parameter_env: CheckedParameterEnv }
#[rust(crate::ty::StructFields)]
struct StructFields {
    fields: Vec<FieldSig>,
    """Well-formedness predicates contributed by the declared field types."""
    parameter_env: CheckedParameterEnv,
}
#[rust(crate::ty::FieldSig)]
struct FieldSig { name: Name, ty: Ty }
#[rust(crate::ty::EnumSig)]
struct EnumSig {
    variants: Vec<VariantSig>,
    parameter_env: CheckedParameterEnv,
}
#[rust(crate::ty::VariantSig)]
struct VariantSig { name: Name, fields: Vec<FieldSig> }
#[rust(crate::ty::ExternalAdtSignatureData)]
struct ExternalAdtSignatureData {
    """External nominal declaration facts; defaults align with the enclosing binder's parameters."""
    defaults: Vec<GenericDefault>,
    parameter_env: CheckedParameterEnv,
    ordinary_complete: bool,
    deferred_complete: bool,
}
#[rust(crate::ty::GenericDefault)]
enum GenericDefault { Absent, Type(Ty), Unsupported }
#[rust(crate::ty::TraitRef)]
struct TraitRef {
    """Self is supplied separately from this trait reference."""
    trait_sym: crate::symbols::TraitSymbol,
    args: Vec<Ty>,
}
#[rust(crate::ty::WherePredicate)]
struct WherePredicate { self_ty: Ty, trait_ref: TraitRef }
#[rust(crate::ty::SolverEligibility)]
enum SolverEligibility { Eligible, Unsupported }
#[rust(crate::ty::CheckedParameterEnv)]
struct CheckedParameterEnv {
    where_clauses: Vec<WherePredicate>,
    solver_eligibility: SolverEligibility,
}
#[rust(crate::ty::TraitSignatureData)]
struct TraitSignatureData {
    self_param: GenericParam,
    where_clauses: Vec<WherePredicate>,
    solver_eligibility: SolverEligibility,
    semantics: TraitSemantics,
}
#[rust(crate::ty::TraitSemantics)]
enum TraitSemantics { Ordinary, Sized, MetaSized }
#[rust(crate::ty::ImplSignatureData)]
struct ImplSignatureData {
    trait_ref: Option<TraitRef>,
    self_ty: Ty,
    where_clauses: Vec<WherePredicate>,
    solver_eligibility: SolverEligibility,
}
#[rust(crate::ty::TraitItemDef)]
enum TraitItemDef {
    Function(crate::symbols::FnSymbol),
    Type(TypeAliasSymbol),
    Const(ConstSymbol),
}

// Source: crates/sage-ir/src/tytree/mod.rs::CheckedBody.
#[rust(crate::tytree::CheckedBody)]
struct Body {
    """CheckedBody, with storage erased. A recovery result can contain diagnosed error nodes."""
    body: TyBody,
    diagnostics: Vec<Diagnostic>,
}
#[rust(crate::tytree::TyBody)]
type TyBody { """The completed elaborated expression tree described by tytree::TyBodyData. Successful output has no unresolved inference work.""" }
#[rust(crate::diagnostic::Diagnostic)]
type Diagnostic { """A source-located diagnostic with severity, labels, and explanatory notes.""" }
