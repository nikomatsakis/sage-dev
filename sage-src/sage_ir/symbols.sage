module crate::symbols;

// Sources: crates/sage-ir/src/symbol/mod.rs and local_syms/*.rs.
// signature is the logical spelling of Rust sig; these classes model enum dispatch.
// Fn/Trait/Impl methods use a uniform optional interface here: local results are
// lifted to Some, while unavailable external facts remain None. Rust local sig/items
// return the unwrapped result. Trait/impl membership retains its generic binder.

#[rust(crate::symbol::Symbol)]
abstract class Symbol {
    """Erased definition identity; specialized interfaces retain the definition kind."""
}

#[rust(crate::symbol::FnSymbol)]
abstract class FnSymbol extends crate::symbols::Symbol {
    """Kind-specific local-or-external semantic interface."""
    query signature() -> Option<crate::types::Binder<crate::types::FnSig>> {
        """Return this definition's checked interface. Local and external representations dispatch separately."""
        abstract;
    }
}

#[rust(crate::symbol::TraitSymbol)]
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

#[rust(crate::symbol::ImplSymbol)]
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

#[rust(crate::symbol::StructSymbol)]
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

#[rust(crate::symbol::EnumSymbol)]
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

#[rust(crate::local_syms::fns::LocalFnSym)]
class LocalFnSym extends crate::symbols::FnSymbol {
    """Source-backed definition. Identity detail here is a review abstraction; inspect the linked Rust declaration for the exact per-kind fields."""
    identity name: crate::types::Name;
    identity scope: crate::types::Scope;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
    identity owner: crate::types::Owner;
    override query signature() -> Option<crate::types::Binder<crate::types::FnSig>> {
        """Check the declaration in its defining scope; preserve generic identities, binder, predicates, and unsupported eligibility. For associated functions inherit the owner generics and Self before method generics. This models local_syms/fns.rs::sig."""
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
        """Import this signature, bind parameters and Self, and set the solver environment and return type. Check the body in a fresh InferCtx, finalize obligations, then finish the typed tree. A missing body produces the existing recovery representation."""
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

#[rust(crate::symbol::SymExt)]
class ExternalFn extends crate::symbols::FnSymbol {
    """Logical external branch of the kind wrapper; Rust uses SymExt rather than a separate struct for each kind."""
    identity definition: crate::types::Identity;
    override query signature() -> Option<crate::types::Binder<crate::types::FnSig>> {
        """Read one external signature fact and lower owned metadata into Sage types and binders."""
        envelope {
            invokes crate::metadata::ExternalFacts::signature();
            forbid transitive crate::symbols::LocalFnSym::body;
        }
    }
}

#[rust(crate::local_syms::traits::LocalTraitSym)]
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

#[rust(crate::symbol::SymExt)]
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

#[rust(crate::local_syms::impls::LocalImplSym)]
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

#[rust(crate::symbol::SymExt)]
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

#[rust(crate::local_syms::structs::LocalStructSym)]
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

#[rust(crate::symbol::SymExt)]
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

#[rust(crate::local_syms::enums::LocalEnumSym)]
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

#[rust(crate::symbol::SymExt)]
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

#[rust(crate::local_syms::type_aliases::LocalTypeAliasSym)]
class LocalTypeAliasSym extends crate::symbols::Symbol {
    """Source-backed identity and syntax. Semantic operations not uniformly available as symbol methods; inspect local_syms for current per-kind support."""
    identity name: crate::types::Name;
    identity scope: crate::types::Scope;
    identity owner: crate::types::Owner;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
}

#[rust(crate::local_syms::consts::LocalConstSym)]
class LocalConstSym extends crate::symbols::Symbol {
    """Source-backed identity and syntax. Semantic operations not uniformly available as symbol methods; inspect local_syms for current per-kind support."""
    identity name: crate::types::Name;
    identity scope: crate::types::Scope;
    identity owner: crate::types::Owner;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
}

#[rust(crate::local_syms::statics::LocalStaticSym)]
class LocalStaticSym extends crate::symbols::Symbol {
    """Source-backed identity and syntax. Semantic operations not uniformly available as symbol methods; inspect local_syms for current per-kind support."""
    identity name: crate::types::Name;
    identity scope: crate::types::Scope;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
}

#[rust(crate::local_syms::uses::LocalUseSym)]
class LocalUseSym extends crate::symbols::Symbol {
    """Source-backed identity and syntax. Semantic operations not uniformly available as symbol methods; inspect local_syms for current per-kind support."""
    identity scope: crate::types::Scope;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
}

#[rust(crate::local_syms::macro_defs::LocalMacroDefSym)]
class LocalMacroDefSym extends crate::symbols::Symbol {
    """Source-backed identity and syntax. Semantic operations not uniformly available as symbol methods; inspect local_syms for current per-kind support."""
    identity name: crate::types::Name;
    identity scope: crate::types::Scope;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
}

#[rust(crate::local_syms::macro_invocations::LocalMacroInvocationSym)]
class LocalMacroInvocationSym extends crate::symbols::Symbol {
    """Source-backed identity and syntax. Semantic operations not uniformly available as symbol methods; inspect local_syms for current per-kind support."""
    identity scope: crate::types::Scope;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
}

// Sources: local_syms/enums.rs::LocalVariantSym and LocalVariantCtorSym.
#[rust(crate::local_syms::enums::LocalVariantSym)]
class LocalVariantSym extends crate::symbols::Symbol {
    """Enum variant identity is its parent enum and name; syntax, span, and tuple shape are tracked detail."""
    identity name: crate::types::Name;
    identity parent_enum: crate::symbols::LocalEnumSym;
    input syntax: crate::types::Syntax;
    input span: crate::types::Span;
    input is_tuple: crate::types::Boolean;
}

#[rust(crate::local_syms::enums::LocalVariantCtorSym)]
class LocalVariantCtorSym extends crate::symbols::Symbol {
    """Tuple variant constructor identity is distinct from the type-namespace variant."""
    identity name: crate::types::Name;
    identity variant: crate::symbols::LocalVariantSym;
}
