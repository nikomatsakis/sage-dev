module crate::types;

// Sources: ty.rs, tytree/, scope.rs, span.rs, check/solve/ in crates/sage-ir/src.
type Name """Interned spelling, not definition identity.""";
type Scope """Enclosing local module or crate scope.""";
type Owner """Optional trait or impl owner of an associated definition.""";
type Syntax """Self-contained item CST, including source-relative spans.""";
type Span """Absolute source or expansion provenance.""";
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
struct Items {
    """Review wrapper for ordered module/variant symbol lists; membership does not imply expansion completeness."""
    items: Vec<crate::symbols::Symbol>,
}
type Value """Checked associated value or field/member representation.""";
type Resolution """Resolved symbol, ambiguity, failure, or incomplete lookup.""";
type Environment """Generic parameters, Self, assumptions, and lexical bindings.""";
type InferenceState """Body-local variables, versioned constraints, obligations, tasks, and diagnostics.""";
type Goal """Canonical proof or normalization question and its environment.""";
type Answer """Canonical solver response, including uncertainty and resource limits.""";
type Candidates """Candidate set with completeness evidence.""";
type Metadata """Owned keyed facts supplied by an external provider.""";
type Unit """No semantic result.""";
type Source """Source text or generated-token source with provenance.""";
type Identity """External crate and definition indices, or a scoped identity.""";
type Boolean """Boolean flag.""";
type Edition """Rust edition controlling syntax and resolution.""";
type OptionalScope """Optional parent scope; absent for the root module.""";

// Source: crates/sage-ir/src/ty.rs::Binder. Representation is deliberately opaque.
type Binder<T> {
    """A value of T together with the generic parameters binding its references.
    Parameters have stable identities; opening an owner's binder reuses those
    identities before introducing an associated item's own parameters."""
}

// Source: crates/sage-ir/src/ty.rs::Ty. Keep the current type-only argument model.
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

enum IntTy { I8, I16, I32, I64, I128, Isize }
enum UintTy { U8, U16, U32, U64, U128, Usize }
enum FloatTy { F32, F64 }
// Source: crates/sage-ir/src/cst/mod.rs::Mutability.
enum Mutability { Shared, Mut }
enum Lifetime {
    """Meaningful lifetime reasoning is not implemented in the current compiler."""
    Dummy """Placeholder used for every represented lifetime.""",
}
enum Const {
    Literal(u64),
    Param("""A parameter whose kind must be Const.""" GenericParam),
    Other(crate::symbols::Symbol),
}

enum AliasTy {
    """Alias identity is retained until normalization is explicitly requested."""
    Named(NamedAliasTy),
    Associated(ProjectionTy),
    Opaque(OpaqueAliasTy),
}
struct NamedAliasTy {
    definition: TypeAliasSymbol,
    arguments: Vec<Ty>,
}
struct ProjectionTy {
    associated_ty: TypeAliasSymbol,
    self_ty: Ty,
    trait_ref: TraitRef,
    arguments: Vec<Ty>,
}
struct OpaqueAliasTy {
    definition: TypeAliasSymbol,
    arguments: Vec<Ty>,
}
// These identity families are intentionally described without storage layout.
type TypeAliasSymbol { """Kind-specific local or external type-alias identity; Rust symbol::TypeAliasSymbol.""" }
type ConstSymbol { """Kind-specific local or external constant identity; Rust symbol::ConstSymbol.""" }
type GenericParam {
    """Stable parameter identity: source-backed Ast, external Ext, or an AlphaEquiv
    placeholder. Its kind is Type, Lifetime, or Const. See generic_param.rs."""
}
type InferVarIndex { """Dense index identifying an inference variable inside its owning inference context.""" }
type ErrorReported { """Evidence that a diagnostic has already been recorded for an error-recovery value.""" }

// Source: crates/sage-ir/src/ty.rs::FnSig; no semantic fields are omitted.
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
struct CheckedReceiver {
    owner_self_ty: Ty,
    form: MethodReceiver,
}
enum MethodReceiver {
    Value { mutable_binding: bool },
    Ref { mutability: Mutability },
}
struct StructSig { parameter_env: CheckedParameterEnv }
struct StructFields {
    fields: Vec<FieldSig>,
    """Well-formedness predicates contributed by the declared field types."""
    parameter_env: CheckedParameterEnv,
}
struct FieldSig { name: Name, ty: Ty }
struct EnumSig {
    variants: Vec<VariantSig>,
    parameter_env: CheckedParameterEnv,
}
struct VariantSig { name: Name, fields: Vec<FieldSig> }
struct ExternalAdtSignatureData {
    """External nominal declaration facts; defaults align with the enclosing binder's parameters."""
    defaults: Vec<GenericDefault>,
    parameter_env: CheckedParameterEnv,
    ordinary_complete: bool,
    deferred_complete: bool,
}
enum GenericDefault { Absent, Type(Ty), Unsupported }
struct TraitRef {
    """Self is supplied separately from this trait reference."""
    trait_sym: crate::symbols::TraitSymbol,
    args: Vec<Ty>,
}
struct WherePredicate { self_ty: Ty, trait_ref: TraitRef }
enum SolverEligibility { Eligible, Unsupported }
struct CheckedParameterEnv {
    where_clauses: Vec<WherePredicate>,
    solver_eligibility: SolverEligibility,
}
struct TraitSignatureData {
    self_param: GenericParam,
    where_clauses: Vec<WherePredicate>,
    solver_eligibility: SolverEligibility,
    semantics: TraitSemantics,
}
enum TraitSemantics { Ordinary, Sized, MetaSized }
struct ImplSignatureData {
    trait_ref: Option<TraitRef>,
    self_ty: Ty,
    where_clauses: Vec<WherePredicate>,
    solver_eligibility: SolverEligibility,
}
enum TraitItemDef {
    Function(crate::symbols::FnSymbol),
    Type(TypeAliasSymbol),
    Const(ConstSymbol),
}

// Source: crates/sage-ir/src/tytree/mod.rs::CheckedBody.
struct Body {
    """CheckedBody, with storage erased. A recovery result can contain diagnosed error nodes."""
    body: TyBody,
    diagnostics: Vec<Diagnostic>,
}
type TyBody { """The completed elaborated expression tree described by tytree::TyBodyData. Successful output has no unresolved inference work.""" }
type Diagnostic { """A source-located diagnostic with severity, labels, and explanatory notes.""" }
