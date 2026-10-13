module crate::symbols;

// Sources: crates/sage-ir/src/symbol/mod.rs and local_syms/*.rs.
// signature is the logical spelling of Rust sig; these classes model enum dispatch.
// Fn/Trait/Impl methods use a uniform optional interface here: local results are
// lifted to Some, while unavailable external facts remain None. Rust local sig/items
// return the unwrapped result. Trait/impl membership retains its generic binder.

abstract class Symbol {
    """
    Erased definition identity; specialized interfaces retain the definition kind.

    {anchor}`definition_identity`
    Resolution may use names, paths, scopes, and namespaces to discover a definition, but every successful downstream semantic reference records the resulting symbol. Renaming an import or choosing another path to the same definition must not mint a different definition identity.

    {anchor}`kind_wrappers`
    Symbol supports heterogeneous membership and resolution results. Operations whose meaning requires a definition kind accept the corresponding kind-specific wrapper; conversion into Symbol preserves identity, checked recovery accepts the right kind, and recovery of a wrong kind fails.
    """
}

abstract class FnSymbol extends crate::symbols::Symbol {
    """Kind-specific local-or-external semantic interface."""
    query signature() -> Option<crate::types::Binder<crate::types::FnSig>> {
        """Return this definition's checked interface. Local and external representations dispatch separately."""
        abstract;
    }
}

abstract class TraitSymbol extends crate::symbols::Symbol {
    """Kind-specific local-or-external semantic interface."""
    query signature() -> Option<crate::types::Binder<crate::types::TraitSignatureData>> {
        """Return this definition's checked interface. Local and external representations dispatch separately."""
        abstract;
    }
    query items() -> Option<crate::types::Binder<Vec<crate::types::TraitItemDef>>> {
        """Return member identities or kind-specific member descriptions."""
        abstract;
    }
}

abstract class ImplSymbol extends crate::symbols::Symbol {
    """Kind-specific local-or-external semantic interface."""
    query signature() -> Option<crate::types::Binder<crate::types::ImplSignatureData>> {
        """Return this definition's checked interface. Local and external representations dispatch separately."""
        abstract;
    }
    query items() -> Option<crate::types::Binder<Vec<crate::types::TraitItemDef>>> {
        """Return member identities or kind-specific member descriptions."""
        abstract;
    }
}

abstract class StructSymbol extends crate::symbols::Symbol {
    """Kind-specific local-or-external semantic interface."""
    query signature() -> crate::types::Signature {
        """Return this definition's checked interface. Local and external representations dispatch separately."""
        abstract;
    }
    query value() -> crate::types::Value {
        """Return fields, alias value, or declared value type as appropriate to this kind."""
        abstract;
    }
}

abstract class EnumSymbol extends crate::symbols::Symbol {
    """Kind-specific local-or-external semantic interface."""
    query signature() -> crate::types::Signature {
        """Return this definition's checked interface. Local and external representations dispatch separately."""
        abstract;
    }
    query items() -> crate::types::Items {
        """Return member identities or kind-specific member descriptions."""
        abstract;
    }
}

class LocalFnSym extends crate::symbols::FnSymbol {
    """
    Source-backed definition. Identity detail here is a review abstraction; inspect the linked Rust declaration for the exact per-kind fields.

    {anchor}`local_identity`
    A local symbol's identity fields identify its definition. Independently tracked CST, spans, signatures, members, and bodies do not become part of that identity. Detail-only changes preserve the symbol, and consumers of unchanged sibling facts remain reusable; genuine identity changes are distinguishable.

    {anchor}`associated_identity`
    A trait or impl function, type, or constant has its own stable symbol tied to its associated owner. Repeated requests and movement among siblings preserve that owner and the associated item identity.
    """
    identity name: crate::types::Name;
    identity scope: crate::types::Scope;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
    identity owner: crate::types::Owner;
    override query signature() -> Option<crate::types::Binder<crate::types::FnSig>> {
        """
    Check the declaration in its defining scope; preserve generic identities, binder, predicates, and unsupported eligibility. For associated functions inherit the owner generics and Self before method generics. This models local_syms/fns.rs::sig.

    {anchor}`associated_interfaces`
    Checking an associated signature reuses its owner interface and never reads a sibling associated body.
    """
        envelope {
            reads self.syntax;
            reads self.scope;
            creates check: crate::checking::Check;
            invokes crate::checking::Check::signature();
            forbid transitive crate::symbols::LocalFnSym::body;
            reads self.owner;
            invokes crate::symbols::TraitSymbol::signature();
            invokes crate::symbols::ImplSymbol::signature();
        }
    }
    query body() -> crate::types::Body {
        """
    Import this signature, bind parameters and Self, and set the solver environment and return type. Check the body in a fresh InferCtx, finalize obligations, then finish the typed tree. A missing body produces the existing recovery representation.

    {anchor}`body_interfaces`
    Checking and elaborating one function may read its own source and signature plus demanded signatures, fields, associated items, impl headers, metadata facts, and solver results. It must not read a callee or sibling body; a callee-body-only edit must not change or reexecute the caller's completed body.
    """
        envelope {
            reads self.syntax;
            reads self.scope;
            invokes self.signature();
            creates resolver: crate::resolution::Resolver;
            creates inference: crate::inference::InferCtx;
            invokes crate::checking::BodyCheck::check();
            invokes crate::inference::InferCtx::finalize();
            invokes crate::inference::InferCtx::finish();
            forbid transitive crate::symbols::LocalFnSym::body;
        }
    }
}

class ExternalFn extends crate::symbols::FnSymbol {
    """
    Logical external branch of the kind wrapper; Rust uses SymExt rather than a separate struct for each kind.

    {anchor}`external_identity`
    An external symbol identifies a dependency definition by crate number and definition index, independently of the metadata facts requested about it. Metadata adapters supply one consistent definition kind for that identity.

    {anchor}`external_fact_queries`
    Sage imports external facts through owned, definition-keyed queries. A metadata provider neither resolves Sage-local names nor performs Sage trait proof or normalization; those operations execute on the imported Sage values.
    """
    identity definition: crate::types::Identity;
    override query signature() -> Option<crate::types::Binder<crate::types::FnSig>> {
        """Read one external signature fact and lower owned metadata into Sage types and binders."""
        envelope {
            invokes crate::metadata::ExternalFacts::signature();
            forbid transitive crate::symbols::LocalFnSym::body;
        }
    }
}

class LocalTraitSym extends crate::symbols::TraitSymbol {
    """Source-backed definition. Identity detail here is a review abstraction; inspect the linked Rust declaration for the exact per-kind fields."""
    identity name: crate::types::Name;
    identity scope: crate::types::Scope;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
    override query signature() -> Option<crate::types::Binder<crate::types::TraitSignatureData>> {
        """Check the declaration in its defining scope; preserve generic identities, binder, predicates, and unsupported eligibility."""
        envelope {
            reads self.syntax;
            reads self.scope;
            creates check: crate::checking::Check;
            invokes crate::checking::Check::signature();
            forbid transitive crate::symbols::LocalFnSym::body;
        }
    }
    override query items() -> Option<crate::types::Binder<Vec<crate::types::TraitItemDef>>> {
        """Read member syntax and produce stable member identities. Trait/impl semantic items also open the owner signature. Enum variants have their own tracked symbols. Aggregate and keyed associated lookups share producers."""
        envelope {
            reads self.syntax;
            invokes self.signature();
        }
    }
}

class ExternalTrait extends crate::symbols::TraitSymbol {
    """Logical external branch of the kind wrapper; Rust uses SymExt rather than a separate struct for each kind."""
    identity definition: crate::types::Identity;
    override query signature() -> Option<crate::types::Binder<crate::types::TraitSignatureData>> {
        """Read one external signature fact and lower owned metadata into Sage types and binders."""
        envelope {
            invokes crate::metadata::ExternalFacts::signature();
            forbid transitive crate::symbols::LocalFnSym::body;
        }
    }
    override query items() -> Option<crate::types::Binder<Vec<crate::types::TraitItemDef>>> {
        """Import external member metadata into typed symbol identities."""
        envelope {
            invokes crate::metadata::ExternalFacts::items();
        }
    }
}

class LocalImplSym extends crate::symbols::ImplSymbol {
    """Source-backed definition. Identity detail here is a review abstraction; inspect the linked Rust declaration for the exact per-kind fields."""
    identity scope: crate::types::Scope;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
    override query signature() -> Option<crate::types::Binder<crate::types::ImplSignatureData>> {
        """Check the declaration in its defining scope; preserve generic identities, binder, predicates, and unsupported eligibility."""
        envelope {
            reads self.syntax;
            reads self.scope;
            creates check: crate::checking::Check;
            invokes crate::checking::Check::signature();
            forbid transitive crate::symbols::LocalFnSym::body;
        }
    }
    override query items() -> Option<crate::types::Binder<Vec<crate::types::TraitItemDef>>> {
        """Read member syntax and produce stable member identities. Trait/impl semantic items also open the owner signature. Enum variants have their own tracked symbols. Aggregate and keyed associated lookups share producers."""
        envelope {
            reads self.syntax;
            invokes self.signature();
        }
    }
    query associated_type_value() -> crate::types::Value {
        """Find the keyed associated type item and lower only its value under the impl binder; do not enumerate sibling values."""
        envelope {
            reads self.syntax;
            invokes self.signature();
            invokes crate::checking::Check::signature();
        }
    }
}

class ExternalImpl extends crate::symbols::ImplSymbol {
    """Logical external branch of the kind wrapper; Rust uses SymExt rather than a separate struct for each kind."""
    identity definition: crate::types::Identity;
    override query signature() -> Option<crate::types::Binder<crate::types::ImplSignatureData>> {
        """Read one external signature fact and lower owned metadata into Sage types and binders."""
        envelope {
            invokes crate::metadata::ExternalFacts::signature();
            forbid transitive crate::symbols::LocalFnSym::body;
        }
    }
    override query items() -> Option<crate::types::Binder<Vec<crate::types::TraitItemDef>>> {
        """Import external member metadata into typed symbol identities."""
        envelope {
            invokes crate::metadata::ExternalFacts::items();
        }
    }
}

class LocalStructSym extends crate::symbols::StructSymbol {
    """Source-backed definition. Identity detail here is a review abstraction; inspect the linked Rust declaration for the exact per-kind fields."""
    identity name: crate::types::Name;
    identity scope: crate::types::Scope;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
    override query signature() -> crate::types::Signature {
        """Check the declaration in its defining scope; preserve generic identities, binder, predicates, and unsupported eligibility."""
        envelope {
            reads self.syntax;
            reads self.scope;
            creates check: crate::checking::Check;
            invokes crate::checking::Check::signature();
            forbid transitive crate::symbols::LocalFnSym::body;
        }
    }
    override query value() -> crate::types::Value {
        """Check the kind-specific field types, alias value, or declared type in its binder; this is an architectural grouping of the narrower Rust operations."""
        envelope {
            reads self.syntax;
            invokes self.signature();
            invokes crate::checking::Check::signature();
        }
    }
}

class ExternalStruct extends crate::symbols::StructSymbol {
    """Logical external branch of the kind wrapper; Rust uses SymExt rather than a separate struct for each kind."""
    identity definition: crate::types::Identity;
    override query signature() -> crate::types::Signature {
        """Read one external signature fact and lower owned metadata into Sage types and binders."""
        envelope {
            invokes crate::metadata::ExternalFacts::signature();
            forbid transitive crate::symbols::LocalFnSym::body;
        }
    }
    override query value() -> crate::types::Value {
        """Import the represented external value or type fact. Unsupported metadata remains unavailable rather than a semantic proof."""
        envelope {
            invokes crate::metadata::ExternalFacts::value();
        }
    }
}

class LocalEnumSym extends crate::symbols::EnumSymbol {
    """Source-backed definition. Identity detail here is a review abstraction; inspect the linked Rust declaration for the exact per-kind fields."""
    identity name: crate::types::Name;
    identity scope: crate::types::Scope;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
    override query signature() -> crate::types::Signature {
        """Check the declaration in its defining scope; preserve generic identities, binder, predicates, and unsupported eligibility."""
        envelope {
            reads self.syntax;
            reads self.scope;
            creates check: crate::checking::Check;
            invokes crate::checking::Check::signature();
            forbid transitive crate::symbols::LocalFnSym::body;
        }
    }
    override query items() -> crate::types::Items {
        """Read member syntax and produce stable member identities. Trait/impl semantic items also open the owner signature. Enum variants have their own tracked symbols. Aggregate and keyed associated lookups share producers."""
        envelope {
            reads self.syntax;
        }
    }
}

class ExternalEnum extends crate::symbols::EnumSymbol {
    """Logical external branch of the kind wrapper; Rust uses SymExt rather than a separate struct for each kind."""
    identity definition: crate::types::Identity;
    override query signature() -> crate::types::Signature {
        """Read one external signature fact and lower owned metadata into Sage types and binders."""
        envelope {
            invokes crate::metadata::ExternalFacts::signature();
            forbid transitive crate::symbols::LocalFnSym::body;
        }
    }
    override query items() -> crate::types::Items {
        """Import external member metadata into typed symbol identities."""
        envelope {
            invokes crate::metadata::ExternalFacts::items();
        }
    }
}

class LocalTypeAliasSym extends crate::symbols::Symbol {
    """Source-backed identity and syntax. Semantic operations not uniformly available as symbol methods; inspect local_syms for current per-kind support."""
    identity name: crate::types::Name;
    identity scope: crate::types::Scope;
    identity owner: crate::types::Owner;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
}

class LocalConstSym extends crate::symbols::Symbol {
    """Source-backed identity and syntax. Semantic operations not uniformly available as symbol methods; inspect local_syms for current per-kind support."""
    identity name: crate::types::Name;
    identity scope: crate::types::Scope;
    identity owner: crate::types::Owner;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
}

class LocalStaticSym extends crate::symbols::Symbol {
    """Source-backed identity and syntax. Semantic operations not uniformly available as symbol methods; inspect local_syms for current per-kind support."""
    identity name: crate::types::Name;
    identity scope: crate::types::Scope;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
}

class LocalUseSym extends crate::symbols::Symbol {
    """Source-backed identity and syntax. Semantic operations not uniformly available as symbol methods; inspect local_syms for current per-kind support."""
    identity scope: crate::types::Scope;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
}

class LocalMacroDefSym extends crate::symbols::Symbol {
    """Source-backed identity and syntax. Semantic operations not uniformly available as symbol methods; inspect local_syms for current per-kind support."""
    identity name: crate::types::Name;
    identity scope: crate::types::Scope;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
}

class LocalMacroInvocationSym extends crate::symbols::Symbol {
    """Source-backed identity and syntax. Semantic operations not uniformly available as symbol methods; inspect local_syms for current per-kind support."""
    identity scope: crate::types::Scope;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
}

// Sources: local_syms/enums.rs::LocalVariantSym and LocalVariantCtorSym.
class LocalVariantSym extends crate::symbols::Symbol {
    """Enum variant identity is its parent enum and name; syntax, span, and tuple shape are tracked detail."""
    identity name: crate::types::Name;
    identity parent_enum: crate::symbols::LocalEnumSym;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
    input is_tuple: crate::types::Boolean;
}

class LocalVariantCtorSym extends crate::symbols::Symbol {
    """Tuple variant constructor identity is distinct from the type-namespace variant."""
    identity name: crate::types::Name;
    identity variant: crate::symbols::LocalVariantSym;
}
