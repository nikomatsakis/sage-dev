module crate::language;

// Sources: crates/sage-meta/src/{lib,parser,validate}.rs.
#[rust(crate::Document)]
type Document """Typed AST with source byte ranges.""";
#[rust(std::vec::Vec<crate::Diagnostic>)]
type Diagnostics """Name, inheritance, manifest, or dependency-envelope errors.""";
#[rust(none, """Groups the parse and validate free functions, not a Rust type.""")]
class Language {
    """Architecture-language tooling."""
    fn parse() -> crate::language::Document {
        """Lex triple-quoted prose separately from structural syntax and parse declarations and envelopes into an AST. Reject unrecognized syntax."""
        envelope {
        }
    }
    fn validate() -> crate::language::Diagnostics {
        """Resolve crate/module/type/method paths, validate inheritance, conservatively expand virtual calls, and search for paths violating transitive prohibitions. This validates the declared model, not Rust conformance."""
        envelope {
        }
    }
}

