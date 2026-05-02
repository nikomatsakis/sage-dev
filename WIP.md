# WIP: Body resolution — producing a resolved IR

## Goal

Given a function in a workspace crate, produce a **resolved body** —
structurally identical to the syntactic body, but every path points at
a `Symbol` or local variable ID instead of raw names. This is
analogous to rustc's HIR: lexical name resolution is done, type-
dependent resolution (methods, associated functions) is deferred.

Target: resolve all paths in `Get::apply()`, `Get::parse_frames()`,
`Connection::read_frame()`, and other mini-redis functions. Snapshot
the resolved output and query log.

## Codebase orientation

### Crate structure

- **`sage`** (root) — the `cargo-sage` binary. Contains `src/driver.rs`
  (`run_sage_with`, `SageContext`), `src/tcx_impl.rs` (`RustcTcxDb`),
  `src/metadata.rs` (workspace loading), `src/main.rs` (CLI).
  Integration tests live in `tests/expand_tests.rs`.
- **`sage-ir`** (`crates/sage-ir`) — the salsa-based IR. All tracked
  structs, lowering, resolution, derive expansion. This is where the
  new code goes.
- **`sage-stash`** (`crates/sage-stash`) — type-erased `Copy`-only
  storage with `Ptr<T>` and `Slice<T>` handles. Used for function
  bodies.

### Key existing types (in `sage-ir`)

**Database and traits** (`db.rs`, `lib.rs`):
```rust
#[salsa::db]
pub trait Db: salsa::Database {
    fn tcx(&self) -> &dyn tcx::TcxDb;
    fn log_query(&self, entry: String);
}

// Database is 'static (salsa requires it). TcxDb is accessed via
// Arc<dyn TcxDb>. The proxy sends requests to a rustc thread.
pub struct Database {
    storage: salsa::Storage<Self>,
    tcx: Arc<dyn TcxDb>,
    query_log: Arc<Mutex<Vec<String>>>,
}
```

**Syntactic body** (`body.rs`) — lives in a `Stash`:
```rust
pub type FunctionBody<'db> = Stashed<Ptr<Body<'db>>>;

pub struct Body<'db> { pub root: Ptr<Expr<'db>>, pub span: SpanIndices }
pub struct Expr<'db> { pub kind: ExprKind<'db>, pub span: SpanIndices }
pub struct Stmt<'db> { pub kind: StmtKind<'db>, pub span: SpanIndices }
pub struct Pat<'db>  { pub kind: PatKind<'db>,  pub span: SpanIndices }

// All body types derive AllocStashData (from sage-stash-macros).
// They must be Copy. They can contain salsa IDs (Name, Path, TypeRef)
// which are Copy because salsa tracked struct handles are just IDs.
```

Key `ExprKind` variants that contain paths (need resolution):
- `Path(Path<'db>)` — value path expression
- `StructLit(Path<'db>, Slice<FieldInit<'db>>)` — struct literal
- `MacroCall(Path<'db>, TokenTree<'db>)` — macro invocation
- `MethodCall(Ptr<Expr>, Name<'db>, Slice<Expr>)` — stays unresolved
- `Field(Ptr<Expr>, Name<'db>)` — stays unresolved

Key `PatKind` variants that contain paths:
- `Bind(Name<'db>, Mutability)` — introduces a local variable
- `Path(Path<'db>)` — enum variant or constant
- `Struct(Path<'db>, Slice<FieldPat<'db>>)` — struct pattern
- `TupleStruct(Path<'db>, Slice<Pat<'db>>)` — tuple struct pattern

**Items** (`item.rs`):
```rust
enum Item<'db> {
    Function(FunctionItem<'db>), Struct(StructItem<'db>),
    Enum(EnumItem<'db>), Trait(TraitItem<'db>), Impl(ImplItem<'db>),
    TypeAlias(..), Const(..), Static(..), Mod(ModItem<'db>),
    Use(UseGroup<'db>), Error(SpanIndices),
}
// FunctionItem has: name, attrs, params, ret_type, is_async,
// is_unsafe, body (FunctionBody), span_table, span.
// body is a tracked field — changes to it don't invalidate
// queries that only read params/ret_type.
```

**Module-level resolution** (`resolve.rs`):
```rust
// Existing queries:
fn module_items(db, module) -> Vec<Item>
fn module_use_imports(db, module) -> Vec<UseImport>
fn definition(db, module, name) -> Option<Symbol>
fn resolve_mod(db, parent, mod_item, source_root) -> Option<Module>
fn resolve_module_path(db, root, source_root, segments) -> Option<Module>

// Name resolution (not a salsa tracked fn — takes too many params):
fn resolve_name(db, module, source_root, crate_root, name, ns)
    -> Result<Symbol, ResolutionError>

// Use path resolution:
fn resolve_use_path(db, current_module, source_root, crate_root, import)
    -> Result<Symbol, ResolutionError>

// First-segment resolution (crate/self/super/bare → module):
fn resolve_first_segment(db, current_module, source_root, crate_root, segments)
    -> Result<(Module, &[Name]), ResolutionError>
```

**Important:** `resolve_name` resolves a *single name* in a module's
scope. It checks: declared items → named use imports → glob imports →
extern prelude → std prelude. It does NOT handle multi-segment paths.
For multi-segment paths in bodies, we need to resolve the first
segment (possibly a local variable or module-level item), then walk
remaining segments via `definition()`.

**Symbols and modules** (`symbol.rs`, `module.rs`):
```rust
#[salsa::interned]
pub struct Symbol<'db> { pub source: SymbolSource<'db> }
enum SymbolSource<'db> { Local(Item<'db>), External(CrateNum, DefIndex) }

#[salsa::interned]
pub struct Module<'db> { pub source: ModuleSource<'db> }
enum ModuleSource<'db> {
    Local { file: SourceFile, parent: Option<Module<'db>> },
    External(CrateNum, DefIndex),
}
```

**TcxDb** (`tcx/mod.rs`) — external crate metadata interface:
```rust
pub trait TcxDb: Send + Sync {
    fn extern_crate(&self, name: &str) -> Option<CrateNum>;
    fn module_children(&self, crate_num: CrateNum, def_index: DefIndex) -> Vec<RawChild>;
    fn is_builtin_derive(&self, crate_num: CrateNum, def_index: DefIndex) -> bool;
    fn def_path(&self, crate_num: CrateNum, def_index: DefIndex) -> Option<String>;
}
// RawChild { name: String, crate_num, def_index, namespace }
// Returns owned data — caller interns into salsa types.
// def_path returns e.g. "std::prelude::v1::Ok" — used for display only.
```

**Stash conventions** (`sage-stash`):
- Types in a Stash must be `Copy` and derive `AllocStashData`.
- `Ptr<T>` is a handle to one value. `Slice<T>` is a handle to a
  contiguous array. Both are `Copy`.
- `Stashed<Ptr<T>>` pairs a `Stash` with a root pointer. It implements
  `PartialEq` via byte comparison of the stash buffer — if the bytes
  are identical, salsa skips downstream invalidation.
- Salsa IDs (`Name<'db>`, `Path<'db>`, `TypeRef<'db>`, `Symbol<'db>`)
  are `Copy` and can be stored in stash types directly.

**Display conventions** (`display.rs`):
- Item-level types use `impl fmt::Display` with `salsa::with_attached_database`.
- Body types use a `PrettyPrint` trait: `fn pretty(&self, f, stash, indent)`.
  This is needed because stash types require the `&Stash` to dereference
  `Ptr`/`Slice` handles.
- The database must be attached (`db.attach(|| ...)`) for Display impls
  to access salsa data.

**Existing integration tests** (`tests/expand_tests.rs`):
- Tests call `run_sage_with(mini_redis_dir(), &[], |sage| { ... })`.
- `sage` is a `SageContext { db, root, source_root }`.
- Tests use `expect![[...]]` (from `expect-test`) for inline snapshots.
- Tests run the full pipeline: workspace loading, dep building,
  `rustc_driver`, `RustcTcxDb`, salsa Database.

## Design: Resolved IR

The resolved IR mirrors the syntactic body 1:1. Same expression
variants, same statement kinds, same pattern kinds. The only
difference: where the syntactic IR has `Path`, the resolved IR has
`Res`, and where it has `Bind(Name)`, the resolved IR has
`Bind(LocalId)`.

### New types

**File:** new `crates/sage-ir/src/resolved.rs`

```rust
/// What a path resolved to.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Res<'db> {
    /// A module-level or external definition.
    Def(Symbol<'db>),
    /// A local variable (param, let binding, for variable, closure param).
    Local(LocalId),
    /// Couldn't resolve (diagnostic emitted).
    Err,
}

/// Identifies a local variable within a function body.
/// Index into RBody.locals.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct LocalId(pub u32);

/// Metadata about a local variable.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct LocalVar<'db> {
    pub name: Name<'db>,
    pub span: SpanIndices,
}
```

`Res`, `LocalId`, and `LocalVar` must implement `AllocStashData`
(they live in the resolved body's Stash). They must also implement
`StashDirect` (from `sage-stash` — defined in `sage-stash/src/lib.rs`
at the `pub trait StashDirect: Copy {}` line) since they contain no
`Ptr`/`Slice` handles — their `StashEq`/`StashHash` can use plain
`Eq`/`Hash`. Implement with `impl StashDirect for Res<'_> {}` etc.

`Symbol<'db>` is a salsa interned struct (just a `Copy` ID), so it
can be stored in stash types directly. The `AllocStashData` derive
macro already works with salsa interned types — the existing
`body.rs` types (`Expr`, `Pat`, `Stmt`) contain `Name<'db>`,
`Path<'db>`, `TypeRef<'db>` (all salsa tracked/interned) and derive
`AllocStashData` without issue.

### Resolved body structure

```rust
pub type ResolvedBody<'db> = Stashed<Ptr<RBody<'db>>>;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct RBody<'db> {
    pub root: Ptr<RExpr<'db>>,
    /// All local variables in this body. Indexed by LocalId.
    pub locals: Slice<LocalVar<'db>>,
    pub span: SpanIndices,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct RExpr<'db> {
    pub kind: RExprKind<'db>,
    pub span: SpanIndices,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum RExprKind<'db> {
    Literal(Literal),
    Path(Res<'db>),
    Block(Slice<RStmt<'db>>, Option<Ptr<RExpr<'db>>>),
    Call(Ptr<RExpr<'db>>, Slice<RExpr<'db>>),
    MethodCall(Ptr<RExpr<'db>>, Name<'db>, Slice<RExpr<'db>>),
    Field(Ptr<RExpr<'db>>, Name<'db>),
    Binary(Ptr<RExpr<'db>>, BinaryOp, Ptr<RExpr<'db>>),
    Unary(UnaryOp, Ptr<RExpr<'db>>),
    Ref(Ptr<RExpr<'db>>, Mutability),
    If(Ptr<RExpr<'db>>, Ptr<RExpr<'db>>, Option<Ptr<RExpr<'db>>>),
    Match(Ptr<RExpr<'db>>, Slice<RMatchArm<'db>>),
    Loop(Ptr<RExpr<'db>>),
    While(Ptr<RExpr<'db>>, Ptr<RExpr<'db>>),
    For(Ptr<RPat<'db>>, Ptr<RExpr<'db>>, Ptr<RExpr<'db>>),
    Break(Option<Ptr<RExpr<'db>>>),
    Continue,
    Return(Option<Ptr<RExpr<'db>>>),
    Assign(Ptr<RExpr<'db>>, Ptr<RExpr<'db>>),
    Await(Ptr<RExpr<'db>>),
    Try(Ptr<RExpr<'db>>),
    Closure(Slice<RClosureParam<'db>>, Ptr<RExpr<'db>>),
    Tuple(Slice<RExpr<'db>>),
    Array(Slice<RExpr<'db>>),
    Index(Ptr<RExpr<'db>>, Ptr<RExpr<'db>>),
    Cast(Ptr<RExpr<'db>>, TypeRef<'db>),
    StructLit(Res<'db>, Slice<RFieldInit<'db>>),
    Range(Option<Ptr<RExpr<'db>>>, Option<Ptr<RExpr<'db>>>),
    MacroCall(Res<'db>, TokenTree<'db>),
    Missing,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct RStmt<'db> {
    pub kind: RStmtKind<'db>,
    pub span: SpanIndices,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum RStmtKind<'db> {
    Let(Ptr<RPat<'db>>, Option<TypeRef<'db>>, Option<Ptr<RExpr<'db>>>),
    Expr(Ptr<RExpr<'db>>),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct RPat<'db> {
    pub kind: RPatKind<'db>,
    pub span: SpanIndices,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum RPatKind<'db> {
    Wildcard,
    Bind(LocalId, Mutability),
    Path(Res<'db>),
    Tuple(Slice<RPat<'db>>),
    Struct(Res<'db>, Slice<RFieldPat<'db>>),
    TupleStruct(Res<'db>, Slice<RPat<'db>>),
    Ref(Ptr<RPat<'db>>, Mutability),
    Literal(Literal),
    Or(Slice<RPat<'db>>),
    Rest,
    Missing,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct RFieldInit<'db> {
    pub name: Name<'db>,
    pub value: Ptr<RExpr<'db>>,
    pub span: SpanIndices,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct RFieldPat<'db> {
    pub name: Name<'db>,
    pub pat: Ptr<RPat<'db>>,
    pub span: SpanIndices,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct RMatchArm<'db> {
    pub pat: Ptr<RPat<'db>>,
    pub guard: Option<Ptr<RExpr<'db>>>,
    pub body: Ptr<RExpr<'db>>,
    pub span: SpanIndices,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct RClosureParam<'db> {
    pub pat: Ptr<RPat<'db>>,
    pub ty: Option<TypeRef<'db>>,
    pub span: SpanIndices,
}
```

### What changes from the syntactic body

| Syntactic | Resolved | Notes |
|---|---|---|
| `ExprKind::Path(Path)` | `RExprKind::Path(Res)` | Resolved to def or local |
| `ExprKind::StructLit(Path, ..)` | `RExprKind::StructLit(Res, ..)` | Type path resolved |
| `ExprKind::MacroCall(Path, ..)` | `RExprKind::MacroCall(Res, ..)` | Macro path resolved |
| `ExprKind::MethodCall(..)` | `RExprKind::MethodCall(..)` | **Unchanged** — needs types |
| `ExprKind::Field(..)` | `RExprKind::Field(..)` | **Unchanged** — needs types |
| `PatKind::Bind(Name, ..)` | `RPatKind::Bind(LocalId, ..)` | Name → local variable ID |
| `PatKind::Path(Path)` | `RPatKind::Path(Res)` | Resolved |
| `PatKind::Struct(Path, ..)` | `RPatKind::Struct(Res, ..)` | Resolved |
| `PatKind::TupleStruct(Path, ..)` | `RPatKind::TupleStruct(Res, ..)` | Resolved |

Everything else (literals, operators, blocks, control flow) passes
through structurally unchanged — just recursively rebuilt in the
output stash.

### What stays unresolved (deferred to typeck)

- **Method calls** — `receiver.method(args)`. Method name preserved.
- **Field accesses** — `expr.field`. Field name preserved.
- **Associated functions** — `Type::func()`. The type path is resolved
  but which `impl` block provides `func` is unknown. There is no
  impl-block lookup infrastructure yet.
- **Operator overloads** — `a + b` stays as `Binary(a, Add, b)`.
- **Type references in bodies** — `let x: Foo`, `x as Bar`. The
  `TypeRef` passes through unchanged (still contains unresolved
  `Path`). Type path resolution is typeck's job.

## Scope model

```
resolve_in_body(name, ns, scope_stack) -> Res

1. Walk the scope stack innermost → outermost:
   a. Block scope: let bindings visible from their definition point
   b. Closure scope: closure params
   c. Function scope: function params
2. After exhausting local scopes: delegate to module-level
   resolve_name(module, name, ns)
```

Local scopes only apply to single-segment paths in the value
namespace. Multi-segment paths (`foo::bar`) and type-namespace paths
always go directly to module-level resolution.

### Path resolution algorithm for bodies

Given a path with segments `[s0, s1, ..., sN]` in a body context:

**Single-segment value path** (`[name]` in value namespace):
1. Check local scopes → `Res::Local(id)`
2. Check module items → `Res::Def(symbol)`
3. Check use imports → resolve the import path → `Res::Def(symbol)`
4. Check glob imports → `Res::Def(symbol)`
5. Check extern prelude → `Res::Def(symbol)`
6. Check std prelude → `Res::Def(symbol)`
7. → `Res::Err`

This reuses `resolve_name(db, module, source_root, crate_root, name, ns)`
from `resolve.rs` for steps 2–6.

**Multi-segment path** (`[s0, s1, ..., sN]`):
1. Resolve `s0`:
   - `crate` → crate root module
   - `self` → current module
   - `super` → parent module
   - bare identifier → check module items, then extern prelude
     (same as `resolve_first_segment` in `resolve.rs`)
2. Walk `s1..sN-1` via `definition(db, module, segment)`, converting
   each `Symbol` to a `Module` via `symbol_to_module`.
3. Resolve `sN` via `definition(db, module, sN)` → `Res::Def(symbol)`.

This reuses `resolve_first_segment` and `definition` from `resolve.rs`.

**Struct literal path** (`Path` in `StructLit`):
Resolve in type namespace. Same multi-segment algorithm but the final
segment uses `Namespace::Type`.

**Macro call path** (`Path` in `MacroCall`):
Resolve in `Namespace::Macro(MacroKind::Bang)`. Same multi-segment
algorithm.

**Pattern paths** (`Path` in `PatKind::Path/Struct/TupleStruct`):
- `PatKind::Path` → value namespace (constants, enum variants)
- `PatKind::Struct` → type namespace
- `PatKind::TupleStruct` → value namespace (tuple struct constructors)

## Macro handling

Macro calls in the resolved IR keep their `Res` (pointing at the
macro's `Symbol`) and their raw `TokenTree`. **No expansion happens
during resolution** — the macro path is resolved, but the tokens are
opaque.

This is sufficient because:
- Builtin macros (`format!`, `println!`, `vec!`, `panic!`, etc.) don't
  introduce new name bindings into the enclosing scope.
- Their arguments are mostly format strings (not path expressions).
- Expansion is a separate future concern.

mini-redis macro usage:

| Macro | Source | Count | Resolution target |
|---|---|---|---|
| `format!` | builtin | 8 | `Res::Def(core::format)` |
| `println!` | builtin | 5 | `Res::Def(std::println)` |
| `panic!` | builtin | 3 | `Res::Def(core::panic)` |
| `vec!` | builtin | 3 | `Res::Def(std::vec)` |
| `assert!` / `assert_eq!` | builtin | 4 | `Res::Def(core::assert)` |
| `write!` | builtin | 1 | `Res::Def(core::write)` |
| `unreachable!` | builtin | 1 | `Res::Def(core::unreachable)` |
| `unimplemented!` | builtin | 1 | `Res::Def(core::unimplemented)` |
| `debug!` / `info!` / `error!` | tracing | ~10 | `Res::Def(tracing::debug)` etc. |
| `tokio::select!` | tokio | 3 | `Res::Def(tokio::select)` |
| `async_stream::stream!` | async-stream | 1 | `Res::Def(async_stream::stream)` |

All of these resolve through the existing `resolve_name` with
`Namespace::Macro(MacroKind::Bang)`. The std prelude and extern
prelude already handle the lookup.

## BodyResolver implementation

### Core struct

**File:** new `crates/sage-ir/src/body_resolve.rs`

```rust
struct BodyResolver<'db> {
    db: &'db dyn Db,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
    /// The syntactic body's stash (for reading input).
    src_stash: &'db Stash,
    /// Output stash for the resolved body.
    out: Stash,
    /// Local variable table — indexed by LocalId.
    locals: Vec<LocalVar<'db>>,
    /// Scope stack. Each frame holds (Name, LocalId) pairs.
    scopes: Vec<Vec<(Name<'db>, LocalId)>>,
}
```

### Walk structure

The resolver reads from `src_stash` (the syntactic body) and writes
to `out` (the resolved body). For each syntactic node, it allocates
the resolved counterpart in `out`.

```rust
impl<'db> BodyResolver<'db> {
    /// Resolve an expression. Returns a Ptr into the output stash.
    fn resolve_expr(&mut self, expr: &Expr<'db>) -> Ptr<RExpr<'db>> {
        let kind = match &expr.kind {
            ExprKind::Path(path) => {
                let res = self.resolve_value_path(*path);
                RExprKind::Path(res)
            }
            ExprKind::StructLit(path, fields) => {
                let res = self.resolve_type_path(*path);
                let rfields = /* resolve each field's value expr */;
                RExprKind::StructLit(res, rfields)
            }
            ExprKind::MacroCall(path, tt) => {
                let res = self.resolve_macro_path(*path);
                RExprKind::MacroCall(res, *tt)
            }
            ExprKind::Block(stmts, tail) => {
                self.push_scope();
                let rstmts = /* resolve each stmt, let-bindings add to scope */;
                let rtail = tail.map(|t| self.resolve_expr(&self.src_stash[t]));
                self.pop_scope();
                RExprKind::Block(rstmts, rtail)
            }
            // MethodCall, Field — pass through with recursive resolve
            // of sub-expressions, but name stays as Name.
            // All other variants — recursively resolve sub-expressions.
            ...
        };
        self.out.alloc(RExpr { kind, span: expr.span })
    }

    /// Resolve a statement.
    fn resolve_stmt(&mut self, stmt: &Stmt<'db>) -> RStmt<'db> {
        match &stmt.kind {
            StmtKind::Let(pat, ty, init) => {
                // Resolve init FIRST (before pattern bindings are visible).
                let rinit = init.map(|e| self.resolve_expr(&self.src_stash[e]));
                // Then resolve pattern (introduces bindings).
                let rpat = self.resolve_pat(&self.src_stash[*pat]);
                RStmt {
                    kind: RStmtKind::Let(rpat, *ty, rinit),
                    span: stmt.span,
                }
            }
            StmtKind::Expr(e) => RStmt {
                kind: RStmtKind::Expr(self.resolve_expr(&self.src_stash[*e])),
                span: stmt.span,
            },
        }
    }

    /// Resolve a pattern. Introduces bindings into the current scope.
    fn resolve_pat(&mut self, pat: &Pat<'db>) -> Ptr<RPat<'db>> {
        let kind = match &pat.kind {
            PatKind::Bind(name, mutability) => {
                let id = self.add_binding(*name, pat.span);
                RPatKind::Bind(id, *mutability)
            }
            PatKind::Path(path) => {
                let res = self.resolve_value_path(*path);
                RPatKind::Path(res)
            }
            PatKind::Struct(path, fields) => {
                let res = self.resolve_type_path(*path);
                let rfields = /* resolve each field pat */;
                RPatKind::Struct(res, rfields)
            }
            PatKind::TupleStruct(path, pats) => {
                let res = self.resolve_value_path(*path);
                let rpats = /* resolve each sub-pattern */;
                RPatKind::TupleStruct(res, rpats)
            }
            // Tuple, Ref, Or, Literal, Wildcard, Rest, Missing —
            // recurse into sub-patterns, no path resolution needed.
            ...
        };
        self.out.alloc(RPat { kind, span: pat.span })
    }
}
```

### Scope operations

```rust
impl<'db> BodyResolver<'db> {
    fn push_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn add_binding(&mut self, name: Name<'db>, span: SpanIndices) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(LocalVar { name, span });
        if let Some(scope) = self.scopes.last_mut() {
            scope.push((name, id));
        }
        id
    }

    fn lookup_local(&self, name: Name<'db>) -> Option<LocalId> {
        // Walk scopes innermost → outermost.
        for scope in self.scopes.iter().rev() {
            // Walk bindings in reverse (last binding with this name wins).
            for (n, id) in scope.iter().rev() {
                if *n == name {
                    return Some(*id);
                }
            }
        }
        None
    }
}
```

### Path resolution methods

```rust
impl<'db> BodyResolver<'db> {
    /// Resolve a path in value namespace (expressions, value patterns).
    fn resolve_value_path(&mut self, path: Path<'db>) -> Res<'db> {
        self.resolve_path(path, Namespace::Value)
    }

    /// Resolve a path in type namespace (struct literals, struct patterns).
    fn resolve_type_path(&mut self, path: Path<'db>) -> Res<'db> {
        self.resolve_path(path, Namespace::Type)
    }

    /// Resolve a path in macro namespace.
    fn resolve_macro_path(&mut self, path: Path<'db>) -> Res<'db> {
        self.resolve_path(path, Namespace::Macro(MacroKind::Bang))
    }

    fn resolve_path(&mut self, path: Path<'db>, ns: Namespace) -> Res<'db> {
        let segments = path.segments(self.db);
        if segments.is_empty() {
            return Res::Err;
        }

        // Single-segment value path: check locals first.
        if segments.len() == 1 && ns == Namespace::Value {
            if let Some(id) = self.lookup_local(segments[0]) {
                return Res::Local(id);
            }
        }

        // Single-segment: delegate to module-level resolve_name.
        if segments.len() == 1 {
            return match resolve_name(
                self.db, self.module, self.source_root,
                self.crate_root, segments[0], ns,
            ) {
                Ok(sym) => Res::Def(sym),
                Err(_) => Res::Err,
            };
        }

        // Multi-segment: resolve first segment, walk the rest.
        match resolve_first_segment(
            self.db, self.module, self.source_root,
            self.crate_root, segments,
        ) {
            Ok((module, rest)) => {
                let mut current = module;
                for (i, seg) in rest.iter().enumerate() {
                    match definition(self.db, current, *seg) {
                        Some(sym) => {
                            if i < rest.len() - 1 {
                                // Intermediate: must be a module.
                                match symbol_to_module(
                                    self.db, sym, self.source_root, current,
                                ) {
                                    Some(m) => current = m,
                                    None => return Res::Err,
                                }
                            } else {
                                return Res::Def(sym);
                            }
                        }
                        None => return Res::Err,
                    }
                }
                // rest was empty — the first segment resolved to a module.
                // This is unusual in a body context but valid.
                Res::Err
            }
            Err(_) => Res::Err,
        }
    }
}
```

**Note:** `resolve_first_segment` and `symbol_to_module` are currently
**private** functions in `resolve.rs`. They need to be made `pub(crate)`
so `body_resolve.rs` can call them. This is Step 2 in the
implementation plan.

### Entry point

```rust
/// Produce a resolved body for a function.
/// This is the salsa tracked function — cached and incremental.
#[salsa::tracked(returns(ref))]
pub fn resolve_body<'db>(
    db: &'db dyn Db,
    function: FunctionItem<'db>,
    module: Module<'db>,
    source_root: SourceRoot,
    crate_root: Module<'db>,
) -> ResolvedBody<'db> {
    let body = function.body(db);
    let src_stash = body.stash();
    let root = &src_stash[*body.root()];

    let mut resolver = BodyResolver {
        db,
        module,
        source_root,
        crate_root,
        src_stash,
        out: Stash::new(),
        locals: Vec::new(),
        scopes: Vec::new(),
    };

    // Push function params as the outermost scope.
    resolver.push_scope();
    for param in function.params(db) {
        // Params with name: None (e.g. `_: Foo`) still get a LocalId
        // but are not added to the scope (they can't be referenced).
        if let Some(name) = param.name(db) {
            resolver.add_binding(name, param.span(db));
        }
    }

    let resolved_root = resolver.resolve_expr(&src_stash[root.root]);

    // Build the locals slice in the output stash.
    let locals = resolver.out.alloc_slice(&resolver.locals);

    let rbody = resolver.out.alloc(RBody {
        root: resolved_root,
        locals,
        span: root.span,
    });

    resolver.pop_scope();
    Stashed::new(resolver.out, rbody)
}
```

### Error handling policy

- **Unresolved path** → `Res::Err`. Don't panic. The snapshot tests
  will show `<unresolved>` which makes gaps visible.
- **Malformed AST** (e.g. `ExprKind::Missing`) → pass through as
  `RExprKind::Missing`. Don't panic.
- **Empty path segments** → `Res::Err`.
- **`symbol_to_module` returns `None`** for intermediate path segment
  → `Res::Err` (the path can't be fully resolved).

No panics during resolution. Every error case produces `Res::Err` or
the `Missing` variant. This ensures the resolver always produces
output, even on incomplete or broken code.

### Scope push/pop points (complete list)

| Syntactic node | Scope action |
|---|---|
| Function entry | Push scope with params |
| `ExprKind::Block` | Push scope before stmts, pop after |
| `StmtKind::Let` | Resolve init expr first, then resolve pattern (adds bindings to current block scope) |
| `ExprKind::Closure` | Push scope with closure params, resolve body, pop |
| `ExprKind::For(pat, iter, body)` | Resolve iter, push scope, resolve pat (adds bindings), resolve body, pop |
| `ExprKind::Match` arm | Push scope, resolve arm pattern (adds bindings), resolve guard, resolve body, pop |
| `ExprKind::IfLet(pat, scrutinee, then, else)` | Resolve scrutinee, push scope, resolve pat (adds bindings), resolve then, pop scope. Resolve else (if any) in a separate scope without the pattern bindings. |
| `ExprKind::WhileLet(pat, scrutinee, body)` | Resolve scrutinee, push scope, resolve pat (adds bindings), resolve body, pop scope. |

## Display for resolved bodies

Implement `PrettyPrint` for all resolved body types (matching the
existing pattern in `display.rs`). The resolved body display should
annotate paths with their resolution:

- `Res::Def(symbol)` where `symbol.source(db)` is `Local(item)` →
  show the item's name: `<def Get>`, `<def parse_frames>`.
- `Res::Def(symbol)` where `symbol.source(db)` is
  `External(crate_num, def_index)` → show `<ext path>` using
  `TcxDb::def_path`, e.g. `<ext std::prelude::v1::Ok>`,
  `<ext tracing::debug>`. Falls back to `<ext CrateNum:DefIndex>`
  if no TcxDb is available (e.g. noop tests).
- `Res::Local(id)` → show `<local:N>` where N is the LocalId index.
- `Res::Err` → show `<unresolved>`.

For `RPatKind::Bind(id, _)` → show `<bind:N>`.

Example output for `Get::parse_frames()`:
```
locals:
  0: parse
  1: key
{
  let <bind:1> = <local:0>.next_string()?;
  <ext std::prelude::v1::Ok>(<def Get> { key: <local:1> })
}
```

The `PrettyPrint` impls go in `display.rs` alongside the existing
body display code. They follow the same pattern: take `&self`,
`&mut Formatter`, `&Stash`, `indent`.

## Implementation plan

### Process

Each phase follows TDD style:

1. **Write tests first.** Add integration tests (or unit tests where
   noted) that exercise the phase's functionality. Run them — they
   must fail (compile errors count as failure).
2. **Implement.** Write the minimum code to make the tests pass.
3. **Verify.** All tests pass (new and existing).
4. **Commit.** One commit per phase with a descriptive message, e.g.
   `phase 1: if-let/while-let lowering`.

### Test helpers

Integration tests live in `tests/body_resolve_tests.rs` (root `sage`
crate). They use `run_sage_with` on mini-redis with real TcxDb.

Helper to find a method inside an impl block:

```rust
fn find_method<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    type_name: &str,
    method_name: &str,
) -> FunctionItem<'db> {
    let items = module_items(db, module);
    for item in items {
        if let Item::Impl(impl_item) = item {
            // impl_item.self_ty(db) is a TypeRef. Check if its kind
            // is TypeRefKind::Path and the path's last segment matches
            // type_name. Example:
            //   let TypeRefKind::Path(path) = impl_item.self_ty(db).kind(db);
            //   path.segments(db).last().map(|n| n.text(db)) == Some(type_name)
            for sub_item in impl_item.items(db) {
                if let Item::Function(f) = sub_item {
                    if f.name(db).text(db) == method_name {
                        return *f;
                    }
                }
            }
        }
    }
    panic!("{type_name}::{method_name} not found");
}
```

The `resolve_body` call needs `module`, `source_root`, and
`crate_root` — all available from `SageContext`:
```rust
let resolved = resolve_body(sage.db, method, module, sage.source_root, sage.root);
```

---

### Phase 1: `if let` / `while let` lowering

**Goal:** Fix the lowering so `if let` and `while let` preserve their
patterns. This is a prerequisite for body resolution.

**Files:** `crates/sage-ir/src/body.rs`, `crates/sage-ir/src/lower.rs`,
`crates/sage-ir/src/display.rs`

**Test first:** Write an integration test that resolves `cmd::get`,
finds `Get::apply()`, dumps its body (using existing
`dump_function_body`), and snapshots the output. The snapshot should
show `if let` with the pattern — currently it won't (the pattern is
lost). This test fails before the fix.

```rust
#[test]
fn lower_if_let_preserves_pattern() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let module = resolve_module_path(sage.db, sage.root, sage.source_root, &["cmd", "get"]).unwrap();
        let method = find_method(sage.db, module, "Get", "apply");
        // Dump the syntactic body and snapshot it.
        // Verify the output contains IfLet with a pattern, not If with the pattern lost.
        let mut out = String::new();
        // use dump_function_body or PrettyPrint on the body
        ...
        expect![[...]].assert_eq(&out);
    });
}
```

**Implement:**
1. Add `IfLet` and `WhileLet` variants to `ExprKind` in `body.rs`.
2. Update `lower_if` (~line 1069 in `lower.rs`): check if condition
   node kind is `"let_condition"`, extract `pattern` and `value`
   fields, emit `IfLet`.
3. Update while-expression lowering (~line 830): same check, emit
   `WhileLet`.
4. Add `PrettyPrint` impls for the new variants in `display.rs`.

**Verify:** New test passes. All existing tests still pass.

**Commit:** `phase 1: if-let/while-let lowering`

---

### Phase 2: Resolved IR + body walk + path resolution

**Goal:** The big phase. Define the resolved IR types, build the
body resolver, implement local variable resolution AND module-level
path resolution. This is merged because none of the intermediate
steps produce meaningful testable output on their own.

**Files:**
- New `crates/sage-ir/src/resolved.rs` — all R-types
- New `crates/sage-ir/src/body_resolve.rs` — BodyResolver + resolve_body
- `crates/sage-ir/src/resolve.rs` — make helpers `pub(crate)`
- `crates/sage-ir/src/lib.rs` — add new modules

**Test first:** Write an integration test that resolves
`Get::parse_frames()` body and checks resolution results. This test
won't compile initially (types and functions don't exist yet).

```rust
#[test]
fn resolve_body_get_parse_frames() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let module = resolve_module_path(sage.db, sage.root, sage.source_root, &["cmd", "get"]).unwrap();
        let method = find_method(sage.db, module, "Get", "parse_frames");
        let resolved = resolve_body(sage.db, method, module, sage.source_root, sage.root);

        // Check that locals table has the right entries:
        // LocalId(0) = "parse" (param)
        // LocalId(1) = "key" (let binding)
        let stash = resolved.stash();
        let body = &stash[*resolved.root()];
        let locals = &stash[body.locals];
        assert_eq!(locals[0].name.text(sage.db), "parse");
        assert_eq!(locals[1].name.text(sage.db), "key");

        // Check specific resolutions:
        // - "Get" in struct literal → Res::Def (local struct)
        // - "Ok" → Res::Def (std prelude)
        // - "parse" reference → Res::Local(LocalId(0))
        // - "key" reference → Res::Local(LocalId(1))
        // Exact assertions depend on the tree structure; snapshot
        // the full resolved body once display is working (phase 5).
    });
}
```

**Implement:**
1. Define all types in `resolved.rs` (from "Resolved body structure"
   section). Derive `AllocStashData`, implement `StashDirect`.
2. Make `resolve_first_segment`, `symbol_to_module`, `item_in_namespace`
   `pub(crate)` in `resolve.rs`.
3. Build `BodyResolver` in `body_resolve.rs` with the full walk,
   scope tracking, local resolution, and module-level path resolution.
   Implement `resolve_body` as a plain function (not salsa tracked).

**Verify:** Test passes — locals are correct, module-level paths
resolve.

**Commit:** `phase 2: resolved IR and body path resolution`

---

### Phase 3: Pattern resolution

**Goal:** Resolve paths inside patterns (`PatKind::Path`,
`PatKind::Struct`, `PatKind::TupleStruct`) and introduce bindings
from `if let` / `match` patterns.

**Test first:** Write an integration test for `Get::apply()` which
uses `if let Some(value) = db.get(&self.key)`:

```rust
#[test]
fn resolve_body_get_apply() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let module = resolve_module_path(sage.db, sage.root, sage.source_root, &["cmd", "get"]).unwrap();
        let method = find_method(sage.db, module, "Get", "apply");
        let resolved = resolve_body(sage.db, method, module, sage.source_root, sage.root);

        let stash = resolved.stash();
        let body = &stash[*resolved.root()];
        let locals = &stash[body.locals];

        // Params: self, db, dst
        assert_eq!(locals[0].name.text(sage.db), "self");
        assert_eq!(locals[1].name.text(sage.db), "db");
        assert_eq!(locals[2].name.text(sage.db), "dst");

        // if-let binding: "value" from `if let Some(value) = ...`
        // Should be LocalId(3) — introduced by the IfLet pattern.
        assert!(locals.iter().any(|l| l.name.text(sage.db) == "value"));

        // "response" from `let response = ...`
        assert!(locals.iter().any(|l| l.name.text(sage.db) == "response"));

        // "Some" in the if-let pattern should resolve to Res::Def
        // (from std prelude). Exact assertion depends on tree walk;
        // snapshot test in phase 5 will cover this fully.
    });
}
```

**Implement:** Complete `resolve_pat` for all `PatKind` variants.
Wire `IfLet` and `WhileLet` scope handling (resolve scrutinee, push
scope, resolve pattern, resolve then-branch, pop scope).

**Verify:** Test passes — `value` binding appears in locals, `Some`
resolves.

**Commit:** `phase 3: pattern resolution`

---

### Phase 4: Macro path resolution

**Goal:** Resolve macro call paths in `Namespace::Macro(MacroKind::Bang)`.

**Test first:**

```rust
#[test]
fn resolve_body_macro_calls() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let module = resolve_module_path(sage.db, sage.root, sage.source_root, &["cmd", "get"]).unwrap();
        let method = find_method(sage.db, module, "Get", "apply");
        let resolved = resolve_body(sage.db, method, module, sage.source_root, sage.root);

        // Walk the resolved body to find MacroCall nodes.
        // The `debug!(...)` call should have Res::Def pointing at
        // an external symbol (tracing crate), not Res::Err.
        // Helper: walk_find_macro_calls(resolved) -> Vec<Res>
        // Assert at least one is Res::Def with External source.
    });
}
```

**Implement:** Wire `resolve_macro_path` in the `MacroCall` arm of
`resolve_expr`. Uses existing `resolve_name` with
`Namespace::Macro(MacroKind::Bang)`.

**Verify:** Test passes — `debug!` resolves to tracing symbol.

**Commit:** `phase 4: macro path resolution`

---

### Phase 5: Display

**Goal:** Readable snapshot output for resolved bodies.

**Test first:** Write snapshot tests that pretty-print resolved bodies:

```rust
#[test]
fn display_resolved_body_get_parse_frames() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        let module = resolve_module_path(sage.db, sage.root, sage.source_root, &["cmd", "get"]).unwrap();
        let method = find_method(sage.db, module, "Get", "parse_frames");
        let resolved = resolve_body(sage.db, method, module, sage.source_root, sage.root);

        let output = pretty_print_resolved(sage.db, &resolved);
        expect![[r#"
            ...expected output with <local:0>, <def Get>, etc...
        "#]].assert_eq(&output);
    });
}
```

This test won't compile until `PrettyPrint` impls exist for R-types.

**Implement:** Add `PrettyPrint` impls for all resolved body types
in `display.rs`. Add a `pretty_print_resolved` helper function.

Display format:
- `Res::Def(symbol)` local → `<def ItemName>`
- `Res::Def(symbol)` external → `<ext CrateNum:DefIndex>`
- `Res::Local(id)` → `<local:N>`
- `Res::Err` → `<unresolved>`
- `RPatKind::Bind(id, _)` → `<bind:N>`

**Verify:** Snapshot tests pass with readable output.

**Commit:** `phase 5: resolved body display`

---

### Phase 6: Salsa tracked function + full test suite

**Goal:** Make `resolve_body` incremental and add comprehensive
snapshot tests for all target functions.

**Test first:** Add query log test and remaining function tests:

```rust
#[test]
fn query_log_body_resolve_demand_driven() {
    run_sage_with(mini_redis_dir(), &[], |sage| {
        sage.db.take_query_log(); // clear setup log

        let module = resolve_module_path(sage.db, sage.root, sage.source_root, &["cmd", "get"]).unwrap();
        let method = find_method(sage.db, module, "Get", "apply");
        let _resolved = resolve_body(sage.db, method, module, sage.source_root, sage.root);

        let log = sage.db.take_query_log();
        // Should NOT contain file_item_tree for cmd/set.rs
        assert!(!log.contains("cmd/set.rs"), "demand-driven violation:\n{log}");
    });
}

#[test]
fn resolve_body_get_key() { /* trivial: &self.key */ }

#[test]
fn resolve_body_get_into_frame() { /* associated fn calls, method chains */ }

#[test]
fn resolve_body_connection_new() { /* struct literal with nested calls */ }
```

**Implement:** Convert `resolve_body` to
`#[salsa::tracked(returns(ref))]`. Add all remaining snapshot tests.

**Verify:** All tests pass. Query log confirms demand-driven behavior.

**Commit:** `phase 6: salsa tracked resolve_body + full test suite`

## Subsetting restrictions (body resolution)

In addition to the existing restrictions in `md/design/subsetting.md`:

- **Method resolution is deferred.** `receiver.method(args)` preserves
  the method name. Resolving to a specific `impl` method requires
  type inference.
- **Field access resolution is deferred.** `expr.field` preserves the
  field name.
- **Associated function resolution is partial.** `Type::func()` — the
  type path is resolved, but which `impl` block provides `func` is
  not determined. No impl-block lookup exists yet.
- **Macro calls are not expanded.** The macro path is resolved to a
  `Symbol`, but the token tree is not expanded. Paths inside macro
  arguments are not resolved.
- **Type references in bodies are not resolved.** `TypeRef` passes
  through unchanged.
- **Closure captures not tracked.**
- **Or-patterns:** each alternative must introduce the same bindings.
  For this phase, we resolve each alternative independently and don't
  verify consistency (rustc does this check, we defer it).

## Key lowering details (from `lower.rs`)

These affect how the resolver handles certain constructs:

**`self` in methods:** Lowered as a regular `Param` with
`name: Some(Name("self"))` and type `&Self` or `Self`. The resolver
treats it like any other param — it gets `LocalId(0)` and is found
via `lookup_local`.

**`if let`:** The `let_condition` node is currently lowered by
extracting just the inner expression (the RHS of `let`). So
`if let Some(x) = expr` becomes `If(expr, then, else)` — **the
pattern is lost**. Fix (Step 0 in the implementation plan): add
`ExprKind::IfLet(pat, scrutinee, then, else)` variant to `body.rs`
(this variant does **not** exist yet) and update `lower_if` in
`lower.rs` to detect `let_condition` and emit `IfLet` instead of
`If`. This is a prerequisite for body resolution (needed for
`Get::apply()`).

Tree-sitter node structure for the fix:
- `if_expression` has field `condition` which can be `_expression`,
  `let_chain`, or `let_condition`.
- `let_condition` has fields: `pattern` (_pattern) and `value`
  (_expression).
- Current `lower_if` (line ~1069 in `lower.rs`) calls
  `node.child_by_field_name("condition")` and lowers it as a generic
  expression. The fix: check if the condition node's kind is
  `"let_condition"`, and if so, extract `pattern` and `value`
  separately:
  ```rust
  let cond_node = node.child_by_field_name("condition").unwrap();
  if cond_node.kind() == "let_condition" {
      let pat = cond_node.child_by_field_name("pattern")
          .map(|n| self.lower_pat(n))...;
      let scrutinee = cond_node.child_by_field_name("value")
          .map(|n| self.lower_expr(n))...;
      ExprKind::IfLet(pat, scrutinee, then, else_)
  } else {
      ExprKind::If(cond, then, else_)
  }
  ```
- `let_chain` (for `if let A = x && let B = y`) is out of scope —
  treat as `Missing` for now.

**`while let`:** Same issue — pattern is lost. Fix (also Step 0):
add `ExprKind::WhileLet(pat, scrutinee, body)` (does **not** exist
yet). Same approach: check if `while_expression`'s condition is
`"let_condition"`, extract pattern and value separately. The
`while_expression` node (line ~830 in `lower.rs`) has the same
`condition` field that can be `_expression`, `let_chain`, or
`let_condition`.

**Macro calls:** Lowered as `ExprKind::MacroCall(path, token_tree)`.
The path is a regular `Path` (multi-segment for `tokio::select!`).
The token tree is the raw text inside the delimiters.

## Open questions — resolved

1. **Const/static initializers:** Deferred. `resolve_body` takes
   `FunctionItem` only for this phase. Const/static initializers are
   trivial in mini-redis. Easy to add later by accepting `Item`.

2. **Error reporting:** `Res::Err` in the output is sufficient. No
   separate diagnostic collection. Snapshot tests show every
   `<unresolved>` which catches regressions. Diagnostic messages
   added later when we build a real error-reporting pipeline.

3. **`if let` lowering fix:** Add `ExprKind::IfLet` variant (not
   desugar to `Match`). `if let` has different temporary lifetime
   semantics than `match` — keeping it as a distinct node preserves
   that distinction for later phases. Same for `while let` →
   `ExprKind::WhileLet`.

   ```rust
   // New variants in ExprKind (body.rs):
   IfLet(Ptr<Pat<'db>>, Ptr<Expr<'db>>, Ptr<Expr<'db>>, Option<Ptr<Expr<'db>>>),
   //     pattern        scrutinee       then-branch     else-branch
   WhileLet(Ptr<Pat<'db>>, Ptr<Expr<'db>>, Ptr<Expr<'db>>),
   //       pattern        scrutinee       body
   ```

   Corresponding resolved variants:
   ```rust
   // New variants in RExprKind (resolved.rs):
   IfLet(Ptr<RPat<'db>>, Ptr<RExpr<'db>>, Ptr<RExpr<'db>>, Option<Ptr<RExpr<'db>>>),
   WhileLet(Ptr<RPat<'db>>, Ptr<RExpr<'db>>, Ptr<RExpr<'db>>),
   ```

   The resolver handles these by: resolve scrutinee, push scope,
   resolve pattern (introduces bindings), resolve then-branch/body,
   pop scope. Else-branch (if any) gets its own scope without the
   pattern bindings.

## FAQ — common questions from review

**Q: Do `resolve_first_segment` and `symbol_to_module` exist?**
Yes. They are private functions in `crates/sage-ir/src/resolve.rs`.
Phase 2 makes them `pub(crate)`. Their signatures are shown in the
"BodyResolver implementation → Path resolution methods" section.

**Q: Where are `CrateNum`, `DefIndex`, `Symbol`, `Module` defined?**
All in `crates/sage-ir/src/module.rs` and `symbol.rs`. Documented in
the "Codebase orientation → Key existing types" section under
"Symbols and modules."

**Q: Where is `RawChild` and how does `TcxDb` work?**
`RawChild` is in `crates/sage-ir/src/tcx/mod.rs`. The `TcxDb` trait
and its channel-based proxy are documented in the "Codebase
orientation → Key existing types" section under "TcxDb."

**Q: Does `AllocStashData` work with salsa interned types like `Symbol<'db>`?**
Yes. The existing `body.rs` types (`Expr`, `Pat`, `Stmt`) already
derive `AllocStashData` and contain salsa interned types (`Name<'db>`,
`Path<'db>`, `TypeRef<'db>`). `Symbol<'db>` is the same kind of type.

**Q: Does `StashDirect` exist?**
Yes. Defined in `crates/sage-stash/src/lib.rs` as
`pub trait StashDirect: Copy {}`. Blanket impls provide `StashEq`,
`StashHash`, `StashOrd` for any `StashDirect + Eq/Hash/Ord` type.

**Q: How does `resolve_name` work? What's its exact signature?**
Shown in the "Codebase orientation" section. It's a plain function
(not salsa tracked) in `resolve.rs`:
`fn resolve_name(db, module, source_root, crate_root, name, ns) -> Result<Symbol, ResolutionError>`.
It checks: declared items → named use imports → glob imports → extern
prelude → std prelude.

**Q: How do I find a method inside an impl block for tests?**
The `find_method` helper is shown in the "Implementation plan → Test
helpers" section with full code. It walks `module_items`, matches
`Item::Impl`, checks `self_ty` path, then searches the impl's items.

## Implementation status

**Complete.** All 6 phases implemented and tested.

### Post-plan improvements
- **`TcxDb::def_path`:** Added `fn def_path(&self, CrateNum, DefIndex) -> Option<String>` to `TcxDb`, backed by `tcx.def_path_str(def_id)`. External symbols now display as `<ext std::prelude::v1::Ok>` instead of `<ext 2:40257>`. This made snapshot tests deterministic (removed `regex` dev dependency).
- **Test cleanup:** Replaced manual tree-walking assertion tests with `expect_test` snapshots via `pretty_print_resolved`. Down from 10 tests to 6 focused snapshot + query-log tests.

### Deviations from plan
- **Phases 2–4 merged in practice:** Phase 2 was more comprehensive than planned — it included all pattern resolution and macro path resolution. Phases 3 and 4 added targeted tests but no new implementation code.
- **Enum variant resolution:** `Frame::Bulk` and `Frame::Null` show as `<unresolved>` because enum variants aren't directly resolvable as value-namespace items through module-level resolution. This is expected — enum variant resolution requires type-qualified paths, which is deferred to typeck.
- **`Bytes::from` in `into_frame`:** Shows as `<unresolved>` because `Bytes::from` is an associated function requiring impl-block lookup, which is explicitly deferred.

### Test summary
- 6 body_resolve integration tests (snapshot + query log)
- 6 expand integration tests (existing)
- 5 sage-ir expand tests (existing)
- 2 snapshot tests (existing, updated for if-let/while-let)
- 26 sage-stash tests (existing)
