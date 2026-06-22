# `macro_rules!` Parsing, Matching, and Expansion in rustc

## 1. Syntax of `macro_rules!` Arms

### Grammar Overview

A `macro_rules!` definition consists of one or more rules, separated by semicolons. Each rule has two parts:

```
rule ::= matcher `=>` transcriber
```

The matcher and transcriber are each enclosed in a delimiter group (parens, brackets, or braces). The parser is in `compiler/rustc_expand/src/mbe/macro_rules.rs`, function `compile_declarative_macro`.

### Fragment Specifiers

Fragment specifiers are declared in the LHS (matcher/pattern) as `$name:kind`. The valid kinds are defined in `compiler/rustc_ast/src/token.rs` in the `NonterminalKind` enum and its `from_symbol` method:

| Specifier | Notes |
|-----------|-------|
| `item` | A full item (fn, struct, impl, etc.) |
| `block` | A braced block `{ ... }` |
| `stmt` | A statement |
| `pat` | Edition-dependent: `PatWithOr` in 2021+, `PatParam{inferred:true}` pre-2021 |
| `pat_param` | Explicit non-or patterns |
| `expr` | Edition-dependent: `Expr` in 2024+, `Expr2021{inferred:true}` pre-2024 |
| `expr_2021` | Explicit 2021 flavor |
| `ty` | A type |
| `ident` | A single identifier |
| `lifetime` | A lifetime `'a` |
| `literal` | A literal |
| `meta` | A meta item (attribute content) |
| `path` | A path like `foo::bar` |
| `vis` | A visibility qualifier (can be empty) |
| `guard` | Unstable: a match guard (`feature(macro_guard_matcher)`) |
| `tt` | A single token tree |

### Repetition Operators

Repetitions have the form `$( content ) sep? op` where:
- `content` is a sequence of token trees (possibly containing nested metavars and repetitions)
- `sep` is an optional separator token (any single token except `?`, `*`, `+`)
- `op` is a Kleene operator: `*` (zero or more), `+` (one or more), `?` (zero or one)

The `?` operator cannot take a separator. This is enforced in `parse_sep_and_kleene_op` in `compiler/rustc_expand/src/mbe/quoted.rs`.

### Internal Representation

The parsed macro body is represented by `mbe::TokenTree` variants defined in `compiler/rustc_expand/src/mbe.rs`:

```rust
enum TokenTree {
    Token(Token),                                       // literal token
    Delimited(DelimSpan, DelimSpacing, Delimited),      // {...}, (...), [...]
    Sequence(DelimSpan, SequenceRepetition),             // $(...)*
    MetaVar(Span, Ident),                               // $var (in RHS)
    MetaVarDecl { span, name, kind },                   // $var:kind (in LHS)
    MetaVarExpr(DelimSpan, MetaVarExpr),                // ${expr(...)} (in RHS)
}
```

The `SequenceRepetition` struct carries:
- `tts: Vec<TokenTree>` — the repeated content
- `separator: Option<Token>` — the separator
- `kleene: KleeneToken` — the operator and its span
- `num_captures: usize` — count of metavar declarations within

---

## 2. How Matching Works

### Algorithm Overview

The matcher is implemented in `compiler/rustc_expand/src/mbe/macro_parser.rs`. The file header describes the algorithm:

> This is an NFA-based parser, which calls out to the main Rust parser for named non-terminals (which it commits to fully when it hits one in a grammar). There's a set of current NFA threads and a set of next ones. Instead of NTs, we have a special case for Kleene star.

The matching proceeds as follows:

1. **Linearization**: Before matching, the `mbe::TokenTree` matcher is converted to a flat `Vec<MatcherLoc>` via `compute_locs`. This eliminates tree recursion and enables efficient index-based traversal.

2. **NFA Simulation**: The `TtParser` struct maintains three sets:
   - `cur_mps`: positions currently being processed (epsilon transitions)
   - `next_mps`: positions waiting on a specific token
   - `bb_mps`: positions waiting on a "black-box" (nonterminal) parse

3. **Token-at-a-time processing**: For each input token, `parse_tt_inner` processes all `cur_mps`. Each position either:
   - Matches a literal token and advances to `next_mps`
   - Enters/exits sequences via epsilon transitions (staying in `cur_mps`)
   - Requests a nonterminal parse (moves to `bb_mps`)
   - Reaches EOF (checks for completion)

4. **Nonterminal ("black-box") parsing**: When exactly one `bb_mps` entry remains and no `next_mps` entries exist, the actual Rust parser (`parser.parse_nonterminal(kind)`) is invoked to consume tokens for that fragment. If multiple `bb_mps` co-exist with `next_mps`, it's an ambiguity error.

5. **Arm selection**: The top-level matching is in `try_match_macro`. It tries each arm sequentially, returning the first `Success`. On `Failure`, it tries the next arm. On `Error` or `ErrorReported`, it stops early.

### The `MatcherLoc` Representation

The key insight is the linearized `MatcherLoc` enum:

```rust
enum MatcherLoc {
    Token { token: Token },
    Delimited,
    Sequence { op, num_metavar_decls, idx_first_after, next_metavar, seq_depth },
    SequenceKleeneOpNoSep { op, idx_first },
    SequenceSep { separator: Token },
    SequenceKleeneOpAfterSep { idx_first },
    MetaVarDecl { span, bind, kind, next_metavar, seq_depth },
    Eof,
}
```

### Token Comparison

Token matching uses `token_name_eq`, which performs an **unhygienic comparison** — it ignores `SyntaxContext` but compares names. Importantly, invisible delimiters (from metavar expansion) never compare equal to anything, which enforces the "forwarding a matched fragment" restriction.

### Sequence Handling

When a `Sequence` position is reached:
- Empty `MatchedSeq` vectors are installed for all metavars within
- For `*` or `?`, a "skip" position jumps past the sequence
- A "enter" position starts processing the sequence body
- At the end (`SequenceKleeneOpNoSep`), both "end sequence" and "repeat" positions are spawned
- For `?`, only "end" is spawned (no repeat)

### Named Matches

Results are stored as `NamedMatch`:
```rust
enum NamedMatch {
    MatchedSeq(Vec<NamedMatch>),     // from repetitions
    MatchedSingle(ParseNtResult),    // from a single fragment capture
}
```

This forms a tree whose nesting depth mirrors the repetition depth in the matcher.

---

## 3. Special Tokenizations

### `$crate`

When parsing the macro body in `compiler/rustc_expand/src/mbe/quoted.rs`:

```rust
if ident.name == kw::Crate && matches!(is_raw, IdentIsRaw::No) {
    TokenTree::token(token::Ident(kw::DollarCrate, is_raw), span)
}
```

`$crate` is converted to a special `token::Ident(kw::DollarCrate, ...)`. The actual crate name is resolved lazily via `update_dollar_crate_names` in `compiler/rustc_span/src/hygiene.rs`. Each `SyntaxContext` carries a `dollar_crate_name` field that gets filled by the resolver calling `resolve_crate_root` in `compiler/rustc_resolve/src/macros.rs`.

### Invisible Delimiters

When a captured fragment (except `tt`) is transcribed back, it is wrapped in `Delimiter::Invisible(InvisibleOrigin::MetaVar(kind))` (see `transcribe.rs`). This preserves parsing priorities — for example, a captured `$e:expr` that expands inside a larger expression retains its precedence boundaries.

The `InvisibleOrigin` and `MetaVarKind` enums are defined in `compiler/rustc_ast/src/token.rs`. The `skip()` method determines whether the parser ignores these delimiters: proc-macro invisible delimiters are skipped, but metavar ones are not (they participate in parsing).

### `$$` (Dollar-Dollar Escape)

In the RHS, `$$` produces a literal `$` token in the output. This is gated behind `feature(macro_metavar_expr)`.

### Metavar Expressions (`${...}`)

Defined in `compiler/rustc_expand/src/mbe/metavar_expr.rs`, the supported expressions are:

| Expression | Meaning |
|-----------|---------|
| `${count($var)}` | Number of repetitions of `$var` at current depth |
| `${count($var, depth)}` | Count at given nesting depth |
| `${index(depth)}` | Current repetition index at given depth (0 = innermost) |
| `${len(depth)}` | Total length of repetition at given depth |
| `${ignore($var)}` | Reference `$var` to satisfy repetition constraints without emitting it |
| `${concat($a, $b, ...)}` | Concatenate identifiers/literals into a new identifier (separate feature gate) |

During transcription (in `transcribe_metavar_expr` in `transcribe.rs`), these produce literal integer tokens or identifiers.

---

## 4. Follow-Set Rules

The follow-set validation ensures forward-compatibility: if the language evolves to allow new syntax for a fragment specifier, existing macros won't break. The implementation is in `check_matcher_core` and `is_in_follow` in `macro_rules.rs`.

### The Algorithm

1. `FirstSets` is precomputed for the matcher, mapping each sequence to its FIRST set.
2. For each token in the matcher, the SUFFIX (what can follow it) is computed.
3. If the token is a `MetaVarDecl`, each possible next token in SUFFIX is checked against `is_in_follow`.

### Follow-Set Restrictions

| Fragment | Allowed Followers |
|----------|-------------------|
| `item` | Anything (always ends with `}` or `;`) |
| `block` | Anything (bounded by braces) |
| `stmt`, `expr` | `=>`, `,`, `;` |
| `pat` (pre-2021) | `=>`, `,`, `=`, `\|`, `if`, `in` |
| `pat` (2021+) | `=>`, `,`, `=`, `if`, `in` (no `\|`) |
| `guard` | `=>`, `,`, `{` |
| `ty`, `path` | `{`, `[`, `=>`, `,`, `>`, `=`, `:`, `;`, `\|`, `as`, `where`, and `$:block` |
| `ident`, `lifetime` | Anything (single token) |
| `literal` | Anything (one or two tokens) |
| `meta`, `tt` | Anything (bounded by delimiters) |
| `vis` | `,`, any ident (except `priv`), or anything that can begin a type |

**Key design principle**: Closing delimiters (`)`, `]`, `}`) are always allowed as followers for any fragment, because they represent matched delimiter pairs which are structurally guaranteed.

From `frag_can_be_followed_by_any`, fragments that consume at most one token tree (`item`, `block`, `ident`, `literal`, `meta`, `lifetime`, `tt`) can be followed by anything.

---

## 5. Hygiene

### Core Model

The hygiene system is in `compiler/rustc_span/src/hygiene.rs`. It implements a model inspired by "Macros That Work Together" (Flatt et al., 2012).

### `SyntaxContext`

A `SyntaxContext` is a 32-bit index into a global table. Each entry represents a chain of "marks", where each mark is a `(ExpnId, Transparency)` pair. The data structure:

```rust
struct SyntaxContextData {
    outer_expn: ExpnId,
    outer_transparency: Transparency,
    parent: SyntaxContext,
    opaque: SyntaxContext,
    opaque_and_semiopaque: SyntaxContext,
    dollar_crate_name: Symbol,
}
```

### Transparency Levels

```rust
pub enum Transparency {
    Transparent,    // always resolved at call-site
    SemiOpaque,     // local vars at def-site, everything else at call-site
    Opaque,         // always resolved at definition-site
}
```

**`macro_rules!` uses `SemiOpaque`** by default:
```rust
pub fn fallback(macro_rules: bool) -> Self {
    if macro_rules { Transparency::SemiOpaque } else { Transparency::Opaque }
}
```

This means:
- Local variables and labels introduced by a `macro_rules!` expansion are resolved at the macro's definition site (hygienic).
- All other names (types, functions, modules) are resolved at the call site (transparent to the user).

`macro` (decl macro 2.0) uses `Opaque` by default — everything is hygienic.

### `apply_mark`

The `apply_mark` method is the core operation. When transcribing a macro, every output span gets a mark applied via the `Marker` struct in `transcribe.rs`:

```rust
span.map_ctxt(|ctxt| ctxt.apply_mark(self.expand_id.to_expn_id(), self.transparency))
```

The algorithm for `apply_mark`:
- For `Opaque`: simply allocate a new context extending the parent.
- For `SemiOpaque`/`Transparent`: normalize the call site context and create the mark relative to it. This ensures that for `macro_rules`, the semi-opaque mark interacts correctly with the call-site's existing context.

### Resolution (`adjust`)

The `adjust` method is used during name resolution. It strips marks from a syntax context until the expansion is a descendant of the context's outermost expansion. This is how the resolver determines whether an identifier "belongs" to the current scope.

---

## 6. Scoping Rules

The macro scoping implementation is in `compiler/rustc_resolve/src/`.

### Textual (Lexical) Scoping for `macro_rules!`

`macro_rules!` macros use a **textual scope** that is NOT module-based. Key structure from `compiler/rustc_resolve/src/macros.rs`:

```rust
pub(crate) enum MacroRulesScope<'ra> {
    Empty,
    Def(&'ra MacroRulesDecl<'ra>),
    Invocation(LocalExpnId),
}
```

Each `ParentScope` carries a `macro_rules: MacroRulesScopeRef` that forms a linked list. When a `macro_rules!` is defined, its scope extends from that point to the end of the enclosing block/module. This is implemented in `define_macro` in `compiler/rustc_resolve/src/build_reduced_graph.rs`:

- **Textual order matters**: A new `MacroRulesScope::Def` is linked to the previous scope. Items defined *after* the macro can use it; items defined *before* cannot.
- The scope is stored in `macro_rules_scopes` and propagated as subsequent items are processed.

### `#[macro_use]` on Modules

When a child module has `#[macro_use]`, its `macro_rules!` scope escapes to the parent. This means the parent's subsequent code sees `macro_rules` from the child module.

### `#[macro_export]`

A `macro_rules!` with `#[macro_export]` is placed at crate root scope. This makes it available as a module item at crate scope, allowing path-based access.

### `#[macro_use] extern crate`

All exported macros from the external crate are imported into the `macro_use_prelude`, which is a crate-level flat namespace checked during macro resolution.

### Path-Based Macros (Rust 2018+)

`macro` items (not `macro_rules!`) are defined as normal module items with standard visibility. They use `DefKind::Macro` and are placed into the module's namespace. They follow normal path resolution (`foo::bar!`) and respect `pub`/`pub(crate)` visibility.

Non-`#[macro_export]` `macro_rules!` cannot be accessed by path (they are only in textual scope). `#[macro_export]` ones can be accessed as `crate_name::macro_name!` in Rust 2018+.

---

## 7. Cross-Crate Encoding

### What Gets Serialized

From `compiler/rustc_metadata/src/rmeta/encoder.rs`, the `encode_info_for_macro` function:

```rust
fn encode_info_for_macro(&mut self, def_id: LocalDefId) {
    let (_, macro_def, _) = tcx.hir_expect_item(def_id).expect_macro();
    self.tables.is_macro_rules.set(def_id.local_def_index, macro_def.macro_rules);
    record!(self.tables.macro_definition[def_id.to_def_id()] <- &*macro_def.body);
}
```

Two things are stored:
1. **`is_macro_rules`**: A boolean flag (whether it's `macro_rules!` vs `macro`)
2. **`macro_definition`**: The full `ast::DelimArgs` — the raw token stream body of the macro

### What is NOT Stored

The **compiled form** (the `Vec<MacroRule>`, `Vec<MatcherLoc>`, etc.) is NOT serialized. Only the raw token stream (the body as written) is stored. When a foreign crate's macro is loaded, it is **recompiled from tokens** using `compile_declarative_macro`.

### Loading Process

From `compiler/rustc_metadata/src/rmeta/decoder/cstore_impl.rs`:

```rust
pub fn load_macro_untracked(&self, tcx: TyCtxt<'_>, id: DefId) -> LoadedMacro {
    if cdata.root.is_proc_macro_crate() {
        LoadedMacro::ProcMacro(cdata.load_proc_macro(tcx, id.index))
    } else {
        LoadedMacro::MacroDef {
            def: cdata.get_macro(tcx, id.index),
            ident: cdata.item_ident(tcx, id.index),
            attrs: cdata.get_item_attrs(tcx, id.index).collect(),
            span: cdata.get_span(tcx, id.index),
            edition: cdata.root.edition,
        }
    }
}
```

The `get_macro` decoder reconstructs `ast::MacroDef`:
```rust
fn get_macro(&self, tcx: TyCtxt<'_>, id: DefIndex) -> ast::MacroDef {
    let macro_rules = self.root.tables.is_macro_rules.get(self, id);
    let body = self.root.tables.macro_definition.get(self, id).unwrap().decode((self, tcx));
    ast::MacroDef { macro_rules, body: Box::new(body), eii_declaration: None }
}
```

### Hygiene Encoding

Hygiene data (syntax contexts and expansion data) is encoded separately by `encode_hygiene`. This writes out:
- `SyntaxContextData` for each used syntax context
- `ExpnData` for each local expansion
- `ExpnHash` for each local expansion

This ensures that when a macro from crate A is expanded in crate B, the hygiene chain is preserved — the syntax contexts reference the original expansion IDs from crate A.

### Design Tradeoffs

**Why store tokens, not compiled matchers?**

1. **Edition sensitivity**: Fragment specifiers like `pat` and `expr` have edition-dependent behavior. Storing raw tokens allows recompilation with the correct edition semantics.
2. **Simplicity and correctness**: The compiled form (`MatcherLoc`) contains indices that are internal to the compilation process. Serializing it would require stable ABI guarantees on internal structures.
3. **Incremental compilation**: Token streams are stable across compiler versions. Internal representations may change.
4. **Size**: Token streams are relatively compact. The compilation cost is paid once per crate load.

**Why serialize `DelimArgs` specifically?**

`ast::DelimArgs` is the outer delimited body of the macro (`{ ... }` for `macro_rules! foo { ... }`). It contains the raw `TokenStream` which can be fed back to `compile_declarative_macro` with the appropriate edition, features, and node ID.

---

## Summary of Key Files

| File | Role |
|------|------|
| `compiler/rustc_expand/src/mbe.rs` | Module root; defines `TokenTree`, `SequenceRepetition`, `KleeneOp` |
| `compiler/rustc_expand/src/mbe/quoted.rs` | Parses raw token streams into `mbe::TokenTree` (LHS patterns and RHS bodies) |
| `compiler/rustc_expand/src/mbe/macro_rules.rs` | Compiles macro definitions; follow-set checking; orchestrates matching |
| `compiler/rustc_expand/src/mbe/macro_parser.rs` | NFA-based matcher; `MatcherLoc`, `TtParser`, `NamedMatch` |
| `compiler/rustc_expand/src/mbe/transcribe.rs` | Expands matched results into output token stream |
| `compiler/rustc_expand/src/mbe/metavar_expr.rs` | Parses `${count()}`, `${index()}`, etc. |
| `compiler/rustc_expand/src/mbe/macro_check.rs` | Static validation of metavar usage consistency |
| `compiler/rustc_ast/src/token.rs` | `NonterminalKind`, `MetaVarKind`, `InvisibleOrigin` definitions |
| `compiler/rustc_span/src/hygiene.rs` | `SyntaxContext`, `ExpnId`, `Transparency`, `apply_mark`, `adjust` |
| `compiler/rustc_resolve/src/macros.rs` | `MacroRulesScope`, resolver integration, `$crate` resolution |
| `compiler/rustc_resolve/src/build_reduced_graph.rs` | `define_macro`, `#[macro_export]`, `#[macro_use]` handling |
| `compiler/rustc_metadata/src/rmeta/encoder.rs` | `encode_info_for_macro` — serializes token body |
| `compiler/rustc_metadata/src/rmeta/decoder.rs` | `get_macro` — deserializes for cross-crate use |
| `compiler/rustc_metadata/src/rmeta/decoder/cstore_impl.rs` | `load_macro_untracked` — entry point for loading foreign macros |
| `compiler/rustc_metadata/src/rmeta/mod.rs` | Table schema: `is_macro_rules`, `macro_definition` |
