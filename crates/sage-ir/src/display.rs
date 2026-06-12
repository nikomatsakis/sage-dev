//! `Display` impls for IR types using salsa's attached database.
//!
//! These impls use `salsa::with_attached_database` to access the db,
//! so they work in `Debug`/`Display` contexts without passing `db` explicitly.
//! The database must be attached (it is during tracked function execution,
//! or call `db.attach(|| ...)` in tests).

use std::fmt;

use sage_stash::{Ptr, Stash, StashData};

use crate::item::*;
use crate::sig_ast::{PathAst, PathSegmentAst, TypeRefAst, TypeRefAstKind};
use crate::types::*;

fn with_db(f: impl FnOnce(&dyn salsa::Database) -> fmt::Result) -> fmt::Result {
    salsa::with_attached_database(f).unwrap_or_else(|| Ok(()))
}

// -- Item --

impl fmt::Display for ItemAst<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ItemAst::Function(v) => fmt::Display::fmt(v, f),
            ItemAst::Struct(v) => fmt::Display::fmt(v, f),
            ItemAst::Enum(v) => fmt::Display::fmt(v, f),
            ItemAst::Trait(v) => fmt::Display::fmt(v, f),
            ItemAst::Impl(v) => fmt::Display::fmt(v, f),
            ItemAst::TypeAlias(v) => fmt::Display::fmt(v, f),
            ItemAst::Const(v) => fmt::Display::fmt(v, f),
            ItemAst::Static(v) => fmt::Display::fmt(v, f),
            ItemAst::Mod(v) => fmt::Display::fmt(v, f),
            ItemAst::Use(v) => fmt::Display::fmt(v, f),
            ItemAst::MacroDef(v) => fmt::Display::fmt(v, f),
            ItemAst::MacroInvocation(v) => fmt::Display::fmt(v, f),
            ItemAst::Error(span) => write!(f, "{{error {}..{}}}", span.start, span.end),
        }
    }
}

// -- Function --

impl fmt::Display for FnAst<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            write_attrs(f, self.attrs(db))?;
            if self.is_async(db) {
                f.write_str("async ")?;
            }
            if self.is_unsafe(db) {
                f.write_str("unsafe ")?;
            }
            write!(f, "fn {}(", self.name(db).text(db))?;
            let sig = self.signature(db);
            let sig_stash = sig.stash();
            let sig_data = &sig_stash[*sig.root()];
            for (i, p) in sig_stash[sig_data.params].iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                if let Some(name) = p.name {
                    write!(f, "{}: ", name.text(db))?;
                }
                sig_stash[p.ty].pretty(f, sig_stash, 0)?;
            }
            f.write_str(")")?;
            if let Some(ret) = sig_data.ret_type {
                f.write_str(" -> ")?;
                sig_stash[ret].pretty(f, sig_stash, 0)?;
            }
            f.write_str(" ")?;
            let body = self.body(db);
            let stash = body.stash();
            let root = body.root();
            let body_data = &stash[*root];
            body_data.root.pretty(f, stash, 0)
        })
    }
}

// -- Struct --

impl fmt::Display for StructAst<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            write_attrs(f, self.attrs(db))?;
            writeln!(f, "struct {} {{", self.name(db).text(db))?;
            let sig = self.signature(db);
            let sig_stash = sig.stash();
            let sig_data = &sig_stash[*sig.root()];
            for field in &sig_stash[sig_data.fields] {
                write!(f, "  {}: ", field.name.text(db))?;
                sig_stash[field.ty].pretty(f, sig_stash, 0)?;
                writeln!(f)?;
            }
            f.write_str("}")
        })
    }
}

// -- Enum --

impl fmt::Display for EnumAst<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            write_attrs(f, self.attrs(db))?;
            writeln!(f, "enum {} {{", self.name(db).text(db))?;
            let sig = self.signature(db);
            let sig_stash = sig.stash();
            let sig_data = &sig_stash[*sig.root()];
            for v in &sig_stash[sig_data.variants] {
                let fields = &sig_stash[v.fields];
                if fields.is_empty() {
                    writeln!(f, "  {}", v.name.text(db))?;
                } else {
                    writeln!(f, "  {} {{", v.name.text(db))?;
                    for field in fields {
                        write!(f, "    {}: ", field.name.text(db))?;
                        sig_stash[field.ty].pretty(f, sig_stash, 0)?;
                        writeln!(f)?;
                    }
                    writeln!(f, "  }}")?;
                }
            }
            f.write_str("}")
        })
    }
}

// -- Trait --

impl fmt::Display for TraitAst<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            write_attrs(f, self.attrs(db))?;
            writeln!(f, "trait {} {{", self.name(db).text(db))?;
            for item in self.items(db) {
                writeln!(f, "  {item}")?;
            }
            f.write_str("}")
        })
    }
}

// -- Impl --

impl fmt::Display for ImplAst<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            write_attrs(f, self.attrs(db))?;
            let sig = self.signature(db);
            let stash = sig.stash();
            let data = &stash[*sig.root()];
            f.write_str("impl ")?;
            if let Some(trait_path) = data.trait_path {
                stash[trait_path].pretty(f, stash, 0)?;
                f.write_str(" for ")?;
            }
            stash[data.self_ty].pretty(f, stash, 0)?;
            f.write_str(" {\n")?;
            for item in self.items(db) {
                writeln!(f, "  {item}")?;
            }
            f.write_str("}")
        })
    }
}

// -- TypeAlias --

impl fmt::Display for TypeAliasAst<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            write_attrs(f, self.attrs(db))?;
            write!(f, "type {}", self.name(db).text(db))?;
            let sig = self.signature(db);
            let stash = sig.stash();
            let data = &stash[*sig.root()];
            if let Some(ty) = data.ty {
                f.write_str(" = ")?;
                stash[ty].pretty(f, stash, 0)?;
            }
            Ok(())
        })
    }
}

// -- Const --

impl fmt::Display for ConstAst<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            write_attrs(f, self.attrs(db))?;
            write!(f, "const {}", self.name(db).text(db))?;
            let sig = self.signature(db);
            let stash = sig.stash();
            let data = &stash[*sig.root()];
            if let Some(ty) = data.ty {
                f.write_str(": ")?;
                stash[ty].pretty(f, stash, 0)?;
            }
            Ok(())
        })
    }
}

// -- Static --

impl fmt::Display for StaticAst<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            write_attrs(f, self.attrs(db))?;
            if self.is_mut(db) {
                f.write_str("static mut ")?;
            } else {
                f.write_str("static ")?;
            }
            write!(f, "{}", self.name(db).text(db))?;
            let sig = self.signature(db);
            let stash = sig.stash();
            let data = &stash[*sig.root()];
            if let Some(ty) = data.ty {
                f.write_str(": ")?;
                stash[ty].pretty(f, stash, 0)?;
            }
            Ok(())
        })
    }
}

// -- Mod --

impl fmt::Display for ModAst<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            write_attrs(f, self.attrs(db))?;
            match self.inline_unexpanded_items(db) {
                Some(items) => {
                    writeln!(f, "mod {} {{", self.name(db).text(db))?;
                    for item in items {
                        writeln!(f, "  {item}")?;
                    }
                    f.write_str("}")
                }
                None => write!(f, "mod {};", self.name(db).text(db)),
            }
        })
    }
}

// -- Use --

impl fmt::Display for UseGroupAst<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            write_attrs(f, self.attrs(db))?;
            let imports = self.imports(db);
            let stash = imports.stash();
            for (i, import) in stash[*imports.root()].iter().enumerate() {
                if i > 0 {
                    writeln!(f)?;
                }
                let segs = &stash[import.path];
                f.write_str("use ")?;
                for (j, seg) in segs.iter().enumerate() {
                    if j > 0 {
                        f.write_str("::")?;
                    }
                    let text = seg.text(db);
                    if text.is_empty() {
                        f.write_str("::")?;
                    } else {
                        f.write_str(text)?;
                    }
                }
                match import.kind {
                    UseKind::Named(name) => {
                        if segs.last().map(|s| s.text(db)) != Some(name.text(db)) {
                            write!(f, " as {}", name.text(db))?;
                        }
                    }
                    UseKind::Glob => f.write_str("::*")?,
                    UseKind::Unnamed => f.write_str(" as _")?,
                }
            }
            Ok(())
        })
    }
}

// -- MacroDef --

impl fmt::Display for MacroDefAst<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            let body = self.body_tokens(db);
            if body.is_empty() {
                write!(
                    f,
                    "macro_rules! {} {{ () => {{}} }}",
                    self.name(db).text(db)
                )
            } else {
                write!(
                    f,
                    "macro_rules! {} {{ () => {{ {} }} }}",
                    self.name(db).text(db),
                    body
                )
            }
        })
    }
}

// -- MacroInvocation --

impl fmt::Display for MacroInvocationAst<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            let segs = self.path(db);
            for (i, seg) in segs.iter().enumerate() {
                if i > 0 {
                    f.write_str("::")?;
                }
                f.write_str(seg.text(db))?;
            }
            f.write_str("!()")
        })
    }
}

// -- Attr --

impl fmt::Display for Attr<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| match self.kind(db) {
            AttrKind::DocComment => {
                let prefix = if self.is_inner(db) { "//!" } else { "///" };
                if let Some(args) = self.args(db) {
                    let text = args.text(db);
                    if text.is_empty() {
                        write!(f, "{prefix}")
                    } else {
                        write!(f, "{prefix} {text}")
                    }
                } else {
                    write!(f, "{prefix}")
                }
            }
            AttrKind::Normal => {
                if self.is_inner(db) {
                    f.write_str("#![")?;
                } else {
                    f.write_str("#[")?;
                }
                for (i, seg) in self.path(db).iter().enumerate() {
                    if i > 0 {
                        f.write_str("::")?;
                    }
                    f.write_str(seg.text(db))?;
                }
                if let Some(args) = self.args(db) {
                    write!(f, "{}", args.text(db))?;
                }
                f.write_str("]")
            }
        })
    }
}

fn write_attrs(f: &mut fmt::Formatter<'_>, attrs: &[Attr<'_>]) -> fmt::Result {
    for attr in attrs {
        writeln!(f, "{attr}")?;
    }
    Ok(())
}

// ===========================================================================
// PrettyPrint — trait for stash-allocated body types
// ===========================================================================

use crate::body::*;

/// Pretty-print a stash-allocated value. Takes the stash as context.
pub trait PrettyPrint<'db> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result;
}

/// Helper: write `indent * 2` spaces.
fn pad(f: &mut fmt::Formatter<'_>, indent: usize) -> fmt::Result {
    for _ in 0..indent {
        f.write_str("  ")?;
    }
    Ok(())
}

// -- Ptr<T> delegates to T --

impl<'db, T: StashData<'db> + PrettyPrint<'db>> PrettyPrint<'db> for Ptr<T> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result {
        s[*self].pretty(f, s, indent)
    }
}

// -- dump_function_body (public entry point) --

/// Dump a function including its body.
pub fn dump_function_body(f: &mut fmt::Formatter<'_>, func: FnAst<'_>) -> fmt::Result {
    with_db(|db| {
        fmt::Display::fmt(&func, f)?;
        f.write_str(" ")?;
        let body = func.body(db);
        let stash = body.stash();
        let root = body.root();
        let body_data = &stash[*root];
        body_data.root.pretty(f, stash, 0)
    })
}

// -- Expr --

impl<'db> PrettyPrint<'db> for Expr<'db> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result {
        self.kind.pretty(f, s, indent)
    }
}

impl<'db> PrettyPrint<'db> for ExprKind<'db> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result {
        match self {
            ExprKind::Block(stmts, tail) => {
                writeln!(f, "{{")?;
                for stmt in &s[*stmts] {
                    stmt.pretty(f, s, indent + 1)?;
                }
                if let Some(tail) = tail {
                    pad(f, indent + 1)?;
                    tail.pretty(f, s, indent + 1)?;
                    writeln!(f)?;
                }
                pad(f, indent)?;
                f.write_str("}")
            }
            ExprKind::Literal(lit) => write!(f, "{lit:?}"),
            ExprKind::Path(p) => p.pretty(f, s, indent),
            ExprKind::Call(func, args) => {
                func.pretty(f, s, indent)?;
                f.write_str("(")?;
                for (i, arg) in s[*args].iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    arg.pretty(f, s, indent)?;
                }
                f.write_str(")")
            }
            ExprKind::MethodCall(obj, name, args) => with_db(|db| {
                obj.pretty(f, s, indent)?;
                write!(f, ".{}", name.text(db))?;
                f.write_str("(")?;
                for (i, arg) in s[*args].iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    arg.pretty(f, s, indent)?;
                }
                f.write_str(")")
            }),
            ExprKind::Field(obj, name) => with_db(|db| {
                obj.pretty(f, s, indent)?;
                write!(f, ".{}", name.text(db))
            }),
            ExprKind::Binary(lhs, op, rhs) => {
                lhs.pretty(f, s, indent)?;
                write!(f, " {op:?} ")?;
                rhs.pretty(f, s, indent)
            }
            ExprKind::Unary(op, operand) => {
                write!(f, "{op:?}")?;
                operand.pretty(f, s, indent)
            }
            ExprKind::Ref(inner, m) => {
                match m {
                    Mutability::Shared => f.write_str("&")?,
                    Mutability::Mut => f.write_str("&mut ")?,
                }
                inner.pretty(f, s, indent)
            }
            ExprKind::If(cond, then, else_) => {
                f.write_str("if ")?;
                cond.pretty(f, s, indent)?;
                f.write_str(" ")?;
                then.pretty(f, s, indent)?;
                if let Some(e) = else_ {
                    f.write_str(" else ")?;
                    e.pretty(f, s, indent)?;
                }
                Ok(())
            }
            ExprKind::Match(scrutinee, arms) => {
                f.write_str("match ")?;
                scrutinee.pretty(f, s, indent)?;
                writeln!(f, " {{")?;
                for arm in &s[*arms] {
                    arm.pretty(f, s, indent + 1)?;
                }
                pad(f, indent)?;
                f.write_str("}")
            }
            ExprKind::Loop(body) => {
                f.write_str("loop ")?;
                body.pretty(f, s, indent)
            }
            ExprKind::While(cond, body) => {
                f.write_str("while ")?;
                cond.pretty(f, s, indent)?;
                f.write_str(" ")?;
                body.pretty(f, s, indent)
            }
            ExprKind::For(pat, iter, body) => {
                f.write_str("for ")?;
                pat.pretty(f, s, indent)?;
                f.write_str(" in ")?;
                iter.pretty(f, s, indent)?;
                f.write_str(" ")?;
                body.pretty(f, s, indent)
            }
            ExprKind::Break(val) => {
                f.write_str("break")?;
                if let Some(v) = val {
                    f.write_str(" ")?;
                    v.pretty(f, s, indent)?;
                }
                Ok(())
            }
            ExprKind::Continue => f.write_str("continue"),
            ExprKind::Return(val) => {
                f.write_str("return")?;
                if let Some(v) = val {
                    f.write_str(" ")?;
                    v.pretty(f, s, indent)?;
                }
                Ok(())
            }
            ExprKind::Assign(lhs, rhs) => {
                lhs.pretty(f, s, indent)?;
                f.write_str(" = ")?;
                rhs.pretty(f, s, indent)
            }
            ExprKind::Await(inner) => {
                inner.pretty(f, s, indent)?;
                f.write_str(".await")
            }
            ExprKind::Try(inner) => {
                inner.pretty(f, s, indent)?;
                f.write_str("?")
            }
            ExprKind::Closure(params, body) => {
                f.write_str("|")?;
                for (i, p) in s[*params].iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    p.pretty(f, s, indent)?;
                }
                f.write_str("| ")?;
                body.pretty(f, s, indent)
            }
            ExprKind::Tuple(elems) => {
                f.write_str("(")?;
                for (i, e) in s[*elems].iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    e.pretty(f, s, indent)?;
                }
                f.write_str(")")
            }
            ExprKind::Array(elems) => {
                f.write_str("[")?;
                for (i, e) in s[*elems].iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    e.pretty(f, s, indent)?;
                }
                f.write_str("]")
            }
            ExprKind::Index(obj, idx) => {
                obj.pretty(f, s, indent)?;
                f.write_str("[")?;
                idx.pretty(f, s, indent)?;
                f.write_str("]")
            }
            ExprKind::Cast(expr, ty) => {
                expr.pretty(f, s, indent)?;
                f.write_str(" as ")?;
                ty.pretty(f, s, indent)
            }
            ExprKind::StructLit(path, fields) => with_db(|db| {
                path.pretty(f, s, indent)?;
                f.write_str(" {")?;
                for (i, fi) in s[*fields].iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, " {}: ", fi.name.text(db))?;
                    fi.value.pretty(f, s, indent)?;
                }
                f.write_str(" }")
            }),
            ExprKind::Range(lo, hi) => {
                if let Some(lo) = lo {
                    lo.pretty(f, s, indent)?;
                }
                f.write_str("..")?;
                if let Some(hi) = hi {
                    hi.pretty(f, s, indent)?;
                }
                Ok(())
            }
            ExprKind::MacroCall(path, args) => with_db(|db| {
                path.pretty(f, s, indent)?;
                write!(f, "!{}", args.text(db))
            }),
            ExprKind::IfLet(pat, scrutinee, then, else_) => {
                f.write_str("if let ")?;
                pat.pretty(f, s, indent)?;
                f.write_str(" = ")?;
                scrutinee.pretty(f, s, indent)?;
                f.write_str(" ")?;
                then.pretty(f, s, indent)?;
                if let Some(e) = else_ {
                    f.write_str(" else ")?;
                    e.pretty(f, s, indent)?;
                }
                Ok(())
            }
            ExprKind::WhileLet(pat, scrutinee, body) => {
                f.write_str("while let ")?;
                pat.pretty(f, s, indent)?;
                f.write_str(" = ")?;
                scrutinee.pretty(f, s, indent)?;
                f.write_str(" ")?;
                body.pretty(f, s, indent)
            }
            ExprKind::Missing => f.write_str("{missing}"),
        }
    }
}

// -- Stmt --

impl<'db> PrettyPrint<'db> for Stmt<'db> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result {
        pad(f, indent)?;
        match &self.kind {
            StmtKind::Let(pat, ty, init) => {
                f.write_str("let ")?;
                pat.pretty(f, s, indent)?;
                if let Some(ty) = ty {
                    f.write_str(": ")?;
                    ty.pretty(f, s, indent)?;
                }
                if let Some(init) = init {
                    f.write_str(" = ")?;
                    init.pretty(f, s, indent)?;
                }
                writeln!(f, ";")
            }
            StmtKind::Expr(expr) => {
                expr.pretty(f, s, indent)?;
                writeln!(f, ";")
            }
        }
    }
}

// -- Pat --

impl<'db> PrettyPrint<'db> for Pat<'db> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result {
        self.kind.pretty(f, s, indent)
    }
}

impl<'db> PrettyPrint<'db> for PatKind<'db> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result {
        match self {
            PatKind::Wildcard => f.write_str("_"),
            PatKind::Bind(name, m) => with_db(|db| {
                if matches!(m, Mutability::Mut) {
                    f.write_str("mut ")?;
                }
                f.write_str(name.text(db))
            }),
            PatKind::Path(p) => p.pretty(f, s, indent),
            PatKind::Tuple(pats) => {
                f.write_str("(")?;
                for (i, p) in s[*pats].iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    p.pretty(f, s, indent)?;
                }
                f.write_str(")")
            }
            PatKind::Struct(path, fields) => with_db(|db| {
                path.pretty(f, s, indent)?;
                f.write_str(" {")?;
                for (i, fp) in s[*fields].iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, " {}: ", fp.name.text(db))?;
                    fp.pat.pretty(f, s, indent)?;
                }
                f.write_str(" }")
            }),
            PatKind::TupleStruct(path, pats) => {
                path.pretty(f, s, indent)?;
                f.write_str("(")?;
                for (i, p) in s[*pats].iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    p.pretty(f, s, indent)?;
                }
                f.write_str(")")
            }
            PatKind::Ref(inner, m) => {
                match m {
                    Mutability::Shared => f.write_str("&")?,
                    Mutability::Mut => f.write_str("&mut ")?,
                }
                inner.pretty(f, s, indent)
            }
            PatKind::Literal(lit) => write!(f, "{lit:?}"),
            PatKind::Or(pats) => {
                for (i, p) in s[*pats].iter().enumerate() {
                    if i > 0 {
                        f.write_str(" | ")?;
                    }
                    p.pretty(f, s, indent)?;
                }
                Ok(())
            }
            PatKind::Rest => f.write_str(".."),
            PatKind::Missing => f.write_str("{missing}"),
        }
    }
}

// -- MatchArm --

impl<'db> PrettyPrint<'db> for MatchArm<'db> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result {
        pad(f, indent)?;
        self.pat.pretty(f, s, indent)?;
        f.write_str(" => ")?;
        self.body.pretty(f, s, indent)?;
        writeln!(f)
    }
}

// -- ClosureParam --

impl<'db> PrettyPrint<'db> for ClosureParam<'db> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result {
        self.pat.pretty(f, s, indent)
    }
}

// ===========================================================================
// PrettyPrint — stash-allocated type refs and paths
// ===========================================================================

impl<'db> PrettyPrint<'db> for TypeRefAst<'db> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result {
        match self.kind {
            TypeRefAstKind::Path(p) => s[p].pretty(f, s, indent),
            TypeRefAstKind::Reference(inner, Mutability::Shared) => {
                f.write_str("&")?;
                s[inner].pretty(f, s, indent)
            }
            TypeRefAstKind::Reference(inner, Mutability::Mut) => {
                f.write_str("&mut ")?;
                s[inner].pretty(f, s, indent)
            }
            TypeRefAstKind::Slice(inner) => {
                f.write_str("[")?;
                s[inner].pretty(f, s, indent)?;
                f.write_str("]")
            }
            TypeRefAstKind::Array(inner) => {
                f.write_str("[")?;
                s[inner].pretty(f, s, indent)?;
                f.write_str("; _]")
            }
            TypeRefAstKind::Tuple(elems) => {
                f.write_str("(")?;
                for (i, elem) in s[elems].iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    elem.pretty(f, s, indent)?;
                }
                f.write_str(")")
            }
            TypeRefAstKind::Never => f.write_str("!"),
            TypeRefAstKind::Infer => f.write_str("_"),
            TypeRefAstKind::Error => f.write_str("{error}"),
        }
    }
}

impl<'db> PrettyPrint<'db> for PathAst<'db> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result {
        with_db(|db| {
            for (i, seg) in s[self.segments].iter().enumerate() {
                if i > 0 {
                    f.write_str("::")?;
                }
                f.write_str(seg.name.text(db))?;
                let type_args = &s[seg.type_args];
                if !type_args.is_empty() {
                    f.write_str("<")?;
                    for (j, arg) in type_args.iter().enumerate() {
                        if j > 0 {
                            f.write_str(", ")?;
                        }
                        arg.pretty(f, s, indent)?;
                    }
                    f.write_str(">")?;
                }
            }
            Ok(())
        })
    }
}

impl<'db> PrettyPrint<'db> for PathSegmentAst<'db> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result {
        with_db(|db| {
            f.write_str(self.name.text(db))?;
            let type_args = &s[self.type_args];
            if !type_args.is_empty() {
                f.write_str("<")?;
                for (i, arg) in type_args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    arg.pretty(f, s, indent)?;
                }
                f.write_str(">")?;
            }
            Ok(())
        })
    }
}

// ===========================================================================
// PrettyPrint — resolved body types
// ===========================================================================

use crate::resolved::*;
use crate::symbol::SymbolData;

std::thread_local! {
    static DISPLAY_TCX: std::cell::Cell<[usize; 2]> =
        const { std::cell::Cell::new([0; 2]) };
}

fn set_display_tcx(tcx: &dyn crate::tcx::TcxDb) {
    let fat: *const dyn crate::tcx::TcxDb = tcx;
    // SAFETY: *const dyn Trait is two usizes (data + vtable)
    let raw: [usize; 2] = unsafe { std::mem::transmute(fat) };
    DISPLAY_TCX.set(raw);
}

fn clear_display_tcx() {
    DISPLAY_TCX.set([0; 2]);
}

fn with_display_tcx<R>(f: impl FnOnce(&dyn crate::tcx::TcxDb) -> R) -> Option<R> {
    let raw = DISPLAY_TCX.get();
    if raw == [0; 2] {
        return None;
    }
    // SAFETY: pointer is valid for the duration of pretty_print_resolved
    let fat: *const dyn crate::tcx::TcxDb = unsafe { std::mem::transmute(raw) };
    Some(f(unsafe { &*fat }))
}

/// Helper to format a Res.
fn fmt_res(f: &mut fmt::Formatter<'_>, res: &Res<'_>) -> fmt::Result {
    with_db(|db| match res {
        Res::Def(sym) => {
            if let Some(ext) = sym.as_ext() {
                let path =
                    with_display_tcx(|tcx| tcx.def_path(ext.crate_num, ext.def_index)).flatten();
                return match path {
                    Some(p) => write!(f, "<ext {p}>"),
                    None => write!(f, "<ext {}:{}>", ext.crate_num.0, ext.def_index.0),
                };
            }
            match sym.data() {
                SymbolData::Fn(s) => write!(f, "<def {}>", s.as_ast().unwrap().name(db).text(db)),
                SymbolData::Struct(s) => {
                    write!(f, "<def {}>", s.as_ast().unwrap().name(db).text(db))
                }
                SymbolData::TupleStructCtor(s) => {
                    write!(f, "<ctor {}>", s.as_ast().unwrap().name(db).text(db))
                }
                SymbolData::Enum(s) => {
                    write!(f, "<def {}>", s.as_ast().unwrap().name(db).text(db))
                }
                SymbolData::Trait(s) => {
                    write!(f, "<def {}>", s.as_ast().unwrap().name(db).text(db))
                }
                SymbolData::TypeAlias(s) => {
                    write!(f, "<def {}>", s.as_ast().unwrap().name(db).text(db))
                }
                SymbolData::Const(s) => {
                    write!(f, "<def {}>", s.as_ast().unwrap().name(db).text(db))
                }
                SymbolData::Static(s) => {
                    write!(f, "<def {}>", s.as_ast().unwrap().name(db).text(db))
                }
                SymbolData::Mod(m) => match m.data() {
                    crate::module::ModSymbolData::Ast(a) => {
                        write!(f, "<def {}>", a.name(db).text(db))
                    }
                    crate::module::ModSymbolData::Ext(_) => unreachable!(),
                },
                SymbolData::Impl(_) => write!(f, "<def ?>"),
                SymbolData::MacroDef(_) => write!(f, "<def ?>"),
                SymbolData::Use(_) => write!(f, "<def ?>"),
                SymbolData::MacroInvocation(_) => write!(f, "<def ?>"),
                SymbolData::GenericParam(p) => match p.name(db) {
                    Some(n) => write!(f, "<param {}>", n.text(db)),
                    None => write!(f, "<param ?>"),
                },
                SymbolData::Intrinsic(i) => write!(f, "<intrinsic {i:?}>"),
                SymbolData::Error(_) => write!(f, "<def ?>"),
                SymbolData::Unknown(_) => unreachable!(),
            }
        }
        Res::Local(id) => write!(f, "<local:{}>", id.0),
        Res::Err => f.write_str("<unresolved>"),
    })
}

/// Pretty-print a resolved body to a string.
/// The database must be attached before calling this.
pub fn pretty_print_resolved(tcx: &dyn crate::tcx::TcxDb, resolved: &ResolvedBody<'_>) -> String {
    struct Wrapper<'db>(&'db ResolvedBody<'db>);
    impl fmt::Display for Wrapper<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let stash = self.0.stash();
            let body = &stash[*self.0.root()];
            let root = &stash[body.root];

            // Print locals table
            let locals = &stash[body.locals];
            writeln!(f, "locals:")?;
            for (i, local) in locals.iter().enumerate() {
                with_db(|db| writeln!(f, "  {i}: {}", local.name.text(db)))?;
            }

            // Print body
            root.pretty(f, stash, 0)?;
            writeln!(f)
        }
    }
    // SAFETY: we clear the pointer before returning, so it never outlives `tcx`
    set_display_tcx(tcx);
    let result = format!("{}", Wrapper(resolved));
    clear_display_tcx();
    result
}

impl<'db> PrettyPrint<'db> for CheckedExpr<'db> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result {
        self.kind.pretty(f, s, indent)
    }
}

impl<'db> PrettyPrint<'db> for CheckedExprKind<'db> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result {
        match self {
            CheckedExprKind::Block(stmts, tail) => {
                writeln!(f, "{{")?;
                for stmt in &s[*stmts] {
                    stmt.pretty(f, s, indent + 1)?;
                }
                if let Some(tail) = tail {
                    pad(f, indent + 1)?;
                    tail.pretty(f, s, indent + 1)?;
                    writeln!(f)?;
                }
                pad(f, indent)?;
                f.write_str("}")
            }
            CheckedExprKind::Literal(lit) => write!(f, "{lit:?}"),
            CheckedExprKind::Path(res) => fmt_res(f, res),
            CheckedExprKind::Call(func, args) => {
                func.pretty(f, s, indent)?;
                f.write_str("(")?;
                for (i, arg) in s[*args].iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    arg.pretty(f, s, indent)?;
                }
                f.write_str(")")
            }
            CheckedExprKind::MethodCall(obj, name, args) => with_db(|db| {
                obj.pretty(f, s, indent)?;
                write!(f, ".{}", name.text(db))?;
                f.write_str("(")?;
                for (i, arg) in s[*args].iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    arg.pretty(f, s, indent)?;
                }
                f.write_str(")")
            }),
            CheckedExprKind::Field(obj, name) => with_db(|db| {
                obj.pretty(f, s, indent)?;
                write!(f, ".{}", name.text(db))
            }),
            CheckedExprKind::Binary(lhs, op, rhs) => {
                lhs.pretty(f, s, indent)?;
                write!(f, " {op:?} ")?;
                rhs.pretty(f, s, indent)
            }
            CheckedExprKind::Unary(op, operand) => {
                write!(f, "{op:?}")?;
                operand.pretty(f, s, indent)
            }
            CheckedExprKind::Ref(inner, m) => {
                match m {
                    Mutability::Shared => f.write_str("&")?,
                    Mutability::Mut => f.write_str("&mut ")?,
                }
                inner.pretty(f, s, indent)
            }
            CheckedExprKind::If(cond, then, else_) => {
                f.write_str("if ")?;
                cond.pretty(f, s, indent)?;
                f.write_str(" ")?;
                then.pretty(f, s, indent)?;
                if let Some(e) = else_ {
                    f.write_str(" else ")?;
                    e.pretty(f, s, indent)?;
                }
                Ok(())
            }
            CheckedExprKind::IfLet(pat, scrutinee, then, else_) => {
                f.write_str("if let ")?;
                pat.pretty(f, s, indent)?;
                f.write_str(" = ")?;
                scrutinee.pretty(f, s, indent)?;
                f.write_str(" ")?;
                then.pretty(f, s, indent)?;
                if let Some(e) = else_ {
                    f.write_str(" else ")?;
                    e.pretty(f, s, indent)?;
                }
                Ok(())
            }
            CheckedExprKind::Match(scrutinee, arms) => {
                f.write_str("match ")?;
                scrutinee.pretty(f, s, indent)?;
                writeln!(f, " {{")?;
                for arm in &s[*arms] {
                    arm.pretty(f, s, indent + 1)?;
                }
                pad(f, indent)?;
                f.write_str("}")
            }
            CheckedExprKind::Loop(body) => {
                f.write_str("loop ")?;
                body.pretty(f, s, indent)
            }
            CheckedExprKind::While(cond, body) => {
                f.write_str("while ")?;
                cond.pretty(f, s, indent)?;
                f.write_str(" ")?;
                body.pretty(f, s, indent)
            }
            CheckedExprKind::WhileLet(pat, scrutinee, body) => {
                f.write_str("while let ")?;
                pat.pretty(f, s, indent)?;
                f.write_str(" = ")?;
                scrutinee.pretty(f, s, indent)?;
                f.write_str(" ")?;
                body.pretty(f, s, indent)
            }
            CheckedExprKind::For(pat, iter, body) => {
                f.write_str("for ")?;
                pat.pretty(f, s, indent)?;
                f.write_str(" in ")?;
                iter.pretty(f, s, indent)?;
                f.write_str(" ")?;
                body.pretty(f, s, indent)
            }
            CheckedExprKind::Break(val) => {
                f.write_str("break")?;
                if let Some(v) = val {
                    f.write_str(" ")?;
                    v.pretty(f, s, indent)?;
                }
                Ok(())
            }
            CheckedExprKind::Continue => f.write_str("continue"),
            CheckedExprKind::Return(val) => {
                f.write_str("return")?;
                if let Some(v) = val {
                    f.write_str(" ")?;
                    v.pretty(f, s, indent)?;
                }
                Ok(())
            }
            CheckedExprKind::Assign(lhs, rhs) => {
                lhs.pretty(f, s, indent)?;
                f.write_str(" = ")?;
                rhs.pretty(f, s, indent)
            }
            CheckedExprKind::Await(inner) => {
                inner.pretty(f, s, indent)?;
                f.write_str(".await")
            }
            CheckedExprKind::Try(inner) => {
                inner.pretty(f, s, indent)?;
                f.write_str("?")
            }
            CheckedExprKind::Closure(params, body) => {
                f.write_str("|")?;
                for (i, p) in s[*params].iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    p.pretty(f, s, indent)?;
                }
                f.write_str("| ")?;
                body.pretty(f, s, indent)
            }
            CheckedExprKind::Tuple(elems) => {
                f.write_str("(")?;
                for (i, e) in s[*elems].iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    e.pretty(f, s, indent)?;
                }
                f.write_str(")")
            }
            CheckedExprKind::Array(elems) => {
                f.write_str("[")?;
                for (i, e) in s[*elems].iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    e.pretty(f, s, indent)?;
                }
                f.write_str("]")
            }
            CheckedExprKind::Index(obj, idx) => {
                obj.pretty(f, s, indent)?;
                f.write_str("[")?;
                idx.pretty(f, s, indent)?;
                f.write_str("]")
            }
            CheckedExprKind::Cast(expr, ty) => {
                expr.pretty(f, s, indent)?;
                f.write_str(" as ")?;
                ty.pretty(f, s, indent)
            }
            CheckedExprKind::StructLit(res, fields) => with_db(|db| {
                fmt_res(f, res)?;
                f.write_str(" {")?;
                for (i, fi) in s[*fields].iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, " {}: ", fi.name.text(db))?;
                    fi.value.pretty(f, s, indent)?;
                }
                f.write_str(" }")
            }),
            CheckedExprKind::Range(lo, hi) => {
                if let Some(lo) = lo {
                    lo.pretty(f, s, indent)?;
                }
                f.write_str("..")?;
                if let Some(hi) = hi {
                    hi.pretty(f, s, indent)?;
                }
                Ok(())
            }
            CheckedExprKind::MacroCall(res, args) => with_db(|db| {
                fmt_res(f, res)?;
                write!(f, "!{}", args.text(db))
            }),
            CheckedExprKind::Missing => f.write_str("{missing}"),
        }
    }
}

impl<'db> PrettyPrint<'db> for CheckedStmt<'db> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result {
        pad(f, indent)?;
        match &self.kind {
            CheckedStmtKind::Let(pat, ty, init) => {
                f.write_str("let ")?;
                pat.pretty(f, s, indent)?;
                if let Some(ty) = ty {
                    f.write_str(": ")?;
                    ty.pretty(f, s, indent)?;
                }
                if let Some(init) = init {
                    f.write_str(" = ")?;
                    init.pretty(f, s, indent)?;
                }
                writeln!(f, ";")
            }
            CheckedStmtKind::Expr(expr) => {
                expr.pretty(f, s, indent)?;
                writeln!(f, ";")
            }
        }
    }
}

impl<'db> PrettyPrint<'db> for CheckedPat<'db> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result {
        self.kind.pretty(f, s, indent)
    }
}

impl<'db> PrettyPrint<'db> for CheckedPatKind<'db> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result {
        match self {
            CheckedPatKind::Wildcard => f.write_str("_"),
            CheckedPatKind::Bind(id, m) => {
                if matches!(m, Mutability::Mut) {
                    f.write_str("mut ")?;
                }
                write!(f, "<bind:{}>", id.0)
            }
            CheckedPatKind::Path(res) => fmt_res(f, res),
            CheckedPatKind::Tuple(pats) => {
                f.write_str("(")?;
                for (i, p) in s[*pats].iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    p.pretty(f, s, indent)?;
                }
                f.write_str(")")
            }
            CheckedPatKind::Struct(res, fields) => with_db(|db| {
                fmt_res(f, res)?;
                f.write_str(" {")?;
                for (i, fp) in s[*fields].iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, " {}: ", fp.name.text(db))?;
                    fp.pat.pretty(f, s, indent)?;
                }
                f.write_str(" }")
            }),
            CheckedPatKind::TupleStruct(res, pats) => {
                fmt_res(f, res)?;
                f.write_str("(")?;
                for (i, p) in s[*pats].iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    p.pretty(f, s, indent)?;
                }
                f.write_str(")")
            }
            CheckedPatKind::Ref(inner, m) => {
                match m {
                    Mutability::Shared => f.write_str("&")?,
                    Mutability::Mut => f.write_str("&mut ")?,
                }
                inner.pretty(f, s, indent)
            }
            CheckedPatKind::Literal(lit) => write!(f, "{lit:?}"),
            CheckedPatKind::Or(pats) => {
                for (i, p) in s[*pats].iter().enumerate() {
                    if i > 0 {
                        f.write_str(" | ")?;
                    }
                    p.pretty(f, s, indent)?;
                }
                Ok(())
            }
            CheckedPatKind::Rest => f.write_str(".."),
            CheckedPatKind::Missing => f.write_str("{missing}"),
        }
    }
}

impl<'db> PrettyPrint<'db> for CheckedMatchArm<'db> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result {
        pad(f, indent)?;
        self.pat.pretty(f, s, indent)?;
        f.write_str(" => ")?;
        self.body.pretty(f, s, indent)?;
        writeln!(f)
    }
}

impl<'db> PrettyPrint<'db> for CheckedClosureParam<'db> {
    fn pretty(&self, f: &mut fmt::Formatter<'_>, s: &Stash, indent: usize) -> fmt::Result {
        self.pat.pretty(f, s, indent)
    }
}
