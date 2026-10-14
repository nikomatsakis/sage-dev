// {anchor}`crates.sage_test_harness.compile_tests.starting_expressions`
use sage_ir::cst::expr::{ExprCst, ExprCstKind as C, StmtCstKind};
use sage_ir::span::RelativeSpan;
use sage_stash::Stash;

pub(super) struct SourceExpression {
    pub span: RelativeSpan,
    pub kind: &'static str,
}

pub(super) fn walk(stash: &Stash, expr: ExprCst<'_>, rows: &mut Vec<SourceExpression>) {
    let mut children = Vec::new();
    let kind = match expr.kind {
        C::Literal(_) => "literal",
        C::Path(_) => "path",
        C::Block(statements, tail) => {
            for statement in &stash[statements] {
                match statement.kind {
                    StmtCstKind::Expr(e) => children.push(stash[e]),
                    StmtCstKind::Let(_, _, e) => children.extend(e.map(|e| stash[e])),
                }
            }
            children.extend(tail.map(|e| stash[e]));
            "block"
        }
        C::Call(callee, args) => {
            children.push(stash[callee]);
            children.extend_from_slice(&stash[args]);
            "call"
        }
        C::MethodCall(receiver, _, args) => {
            children.push(stash[receiver]);
            children.extend_from_slice(&stash[args]);
            "method call"
        }
        C::Field(e, _) => {
            children.push(stash[e]);
            "field"
        }
        C::Binary(a, _, b) => {
            children.extend([stash[a], stash[b]]);
            "binary"
        }
        C::Unary(_, e) => {
            children.push(stash[e]);
            "unary"
        }
        C::Ref(e, _) => {
            children.push(stash[e]);
            "reference"
        }
        C::If(a, b, c) => {
            children.extend([stash[a], stash[b]]);
            children.extend(c.map(|e| stash[e]));
            "if"
        }
        C::IfLet(_, a, b, c) => {
            children.extend([stash[a], stash[b]]);
            children.extend(c.map(|e| stash[e]));
            "if let"
        }
        C::Match(e, arms) => {
            children.push(stash[e]);
            for arm in &stash[arms] {
                children.extend(arm.guard.map(|e| stash[e]));
                children.push(stash[arm.body]);
            }
            "match"
        }
        C::Loop(e) => {
            children.push(stash[e]);
            "loop"
        }
        C::While(a, b) | C::WhileLet(_, a, b) | C::For(_, a, b) => {
            children.extend([stash[a], stash[b]]);
            "loop"
        }
        C::Break(e) => {
            children.extend(e.map(|e| stash[e]));
            "break"
        }
        C::Continue => "continue",
        C::Return(e) => {
            children.extend(e.map(|e| stash[e]));
            "return"
        }
        C::Assign(a, b) => {
            children.extend([stash[a], stash[b]]);
            "assignment"
        }
        C::Await(e) => {
            children.push(stash[e]);
            "await"
        }
        C::Try(e) => {
            children.push(stash[e]);
            "try"
        }
        C::Closure(_, e) => {
            children.push(stash[e]);
            "closure"
        }
        C::Tuple(es) => {
            children.extend_from_slice(&stash[es]);
            "tuple"
        }
        C::Array(es) => {
            children.extend_from_slice(&stash[es]);
            "array"
        }
        C::Index(a, b) => {
            children.extend([stash[a], stash[b]]);
            "index"
        }
        C::Cast(e, _) => {
            children.push(stash[e]);
            "cast"
        }
        C::StructLit(_, fields) => {
            children.extend(stash[fields].iter().map(|f| stash[f.value]));
            "struct"
        }
        C::Range(a, b) => {
            children.extend(a.map(|e| stash[e]));
            children.extend(b.map(|e| stash[e]));
            "range"
        }
        C::Missing => "unresolved",
    };
    for child in children {
        walk(stash, child, rows);
    }
    rows.push(SourceExpression {
        span: expr.span,
        kind,
    });
}
