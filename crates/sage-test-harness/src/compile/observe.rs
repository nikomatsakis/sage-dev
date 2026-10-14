// {anchor}`crates.sage_test_harness.compile_tests.annotation_validation`
// {anchor}`crates.sage_test_harness.compile_tests.compiler_conditions`
// {anchor}`crates.sage_test_harness.compile_tests.fixtures`
use super::types::FrozenType;
use super::{BTreeSet, Kind, Marker, Output};
use sage_host::driver::SageContext;
use sage_ir::tytree::{TyExpr, TyExprData as E, TyStmtKind};
use sage_ir::{
    Db,
    diagnostic::{Diagnostic, DiagnosticSnapshot, Severity, SignatureDiagnostic, Span},
    local_syms::{LocalAssociatedOwner, LocalModItemSym},
    span::ParseSource,
    symbol::*,
};
use sage_stash::{Ptr, Stash};

pub(super) fn collect(
    context: &SageContext<'_>,
    markers: &[Marker],
    queries: bool,
) -> Result<Output, String> {
    let db = context.db;
    let root = match context.root {
        ModSymbol::Local(root) => root,
        ModSymbol::Ext(_) => unreachable!("fixture root is local"),
    };
    let checked = check_crate(db, root);
    let mut diagnostics = checked.diagnostics.clone();
    diagnostics.extend(
        check_crate::accumulated::<SignatureDiagnostic>(db, root)
            .into_iter()
            .map(|diag| diag.0.clone()),
    );
    for diagnostic in &mut diagnostics {
        diagnostic.prefix_paths("src/");
    }
    for marker in markers {
        if !checked.files.contains(&marker.file) {
            return Err(format!(
                "{}: annotation outside the selected crate",
                marker.file
            ));
        }
    }
    let events = db.take_inspection_log();
    match_diagnostics(markers, &diagnostics)?;
    // {anchor}`crates.sage_test_harness.compile_tests.grouped_observations`
    // {anchor}`crates.sage_test_harness.compile_tests.resolved_symbols`
    let targets: BTreeSet<_> = markers
        .iter()
        .filter(|m| matches!(m.kind, Kind::Type | Kind::Symbol))
        .map(|m| (m.file.clone(), m.line))
        .collect();
    let mut sorted_types: Vec<_> = checked.types.iter().collect();
    sorted_types.sort_by_key(|row| (&row.file, row.end, std::cmp::Reverse(row.start)));
    let mut observations = String::new();
    for (file, line) in targets {
        let wants = |kind| {
            markers
                .iter()
                .any(|m| m.file == file && m.line == line && m.kind == kind)
        };
        let types = wants(Kind::Type);
        let symbols = wants(Kind::Symbol);
        let mut found_type = false;
        let mut found_symbol = false;
        for row in sorted_types
            .iter()
            .copied()
            .filter(|row| row.file == file && row.line == line)
        {
            let symbol = symbols && row.kind == "path";
            if !types && !symbol {
                continue;
            }
            if !row.valid || (types && !row.ty.complete()) {
                return Err(format!(
                    "{file}:{line}: missing, ambiguous, or unresolved type observation for {}",
                    row.source
                ));
            }
            observations.push_str(&format!(
                "{}: {} `{}`\n",
                row.file,
                row.kind,
                compact(&row.source)
            ));
            if symbol {
                let identity = row
                    .symbol
                    .as_ref()
                    .map_err(|error| format!("{file}:{line}: {error} for {}", row.source))?;
                observations.push_str(&format!("  symbol: {identity}\n"));
                found_symbol = true;
            }
            if types {
                observations.push_str(&format!("  type: {}\n", row.ty));
                found_type = true;
            }
        }
        if types && !found_type {
            return Err(format!(
                "{file}:{line}: no expression with a completed type starts on this line"
            ));
        }
        if symbols && !found_symbol {
            return Err(format!(
                "{file}:{line}: no path expression starts on this line"
            ));
        }
    }
    // {anchor}`crates.sage_test_harness.compile_tests.query_capture`
    // {anchor}`crates.sage_test_harness.compile_tests.query_projection`
    let queries = if queries {
        let trace = sage_host::trace::snapshot_trace(events);
        serde_json::to_string_pretty(&trace).map_err(|e| e.to_string())? + "\n"
    } else {
        String::new()
    };
    Ok(Output {
        observations,
        diagnostics: diagnostics
            .iter()
            .map(DiagnosticSnapshot::render)
            .collect::<Vec<_>>()
            .join("\n"),
        queries,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TypeRow {
    start: u32,
    end: u32,
    span: sage_ir::span::RelativeSpan,
    valid: bool,
    file: String,
    line: usize,
    kind: String,
    source: String,
    ty: FrozenType,
    symbol: Result<String, String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Default)]
struct Checked {
    diagnostics: Vec<DiagnosticSnapshot>,
    types: Vec<TypeRow>,
    files: BTreeSet<String>,
}

// {anchor}`crates.sage_test_harness.compile_tests.checking_scope`<<
#[salsa::tracked]
fn check_crate<'db>(db: &'db dyn Db, root: sage_ir::local_syms::mods::LocalModSym<'db>) -> Checked {
    let mut checked = Checked::default();
    walk_module(db, root, &mut checked);
    checked
}

fn walk_module<'db>(
    db: &'db dyn Db,
    module: sage_ir::local_syms::mods::LocalModSym<'db>,
    checked: &mut Checked,
) {
    if let Some(file) = module.file(db) {
        checked
            .files
            .insert(sage_ir::source::normalize_path(&format!(
                "src/{}",
                file.path(db)
            )));
    }
    for item in module.unexpanded_items(db) {
        if let LocalModItemSym::Error(span) = item {
            checked.diagnostics.push(
                Diagnostic::error(Span::Absolute(*span), "unrecognized or unsupported item")
                    .snapshot(db),
            );
        }
    }
    if !sage_ir::local_syms::mods::module_expansion_complete_for_symbol_listing(db, module) {
        checked.diagnostics.push(
            Diagnostic::error(
                Span::Absolute(module.span(db)),
                "module expansion is incomplete",
            )
            .snapshot(db),
        );
    }
    for symbol in ModSymbol::Local(module).expanded_module_items(db) {
        walk_symbol(db, *symbol, checked);
    }
}
fn walk_symbol<'db>(db: &'db dyn Db, symbol: Symbol<'db>, checked: &mut Checked) {
    match symbol.data(db) {
        SymbolData::ModSymbol(ModSymbol::Local(module)) => walk_module(db, module, checked),
        SymbolData::FnSymbol(FnSymbol::Local(function)) => {
            function.sig(db);
            let (source_stash, cst) = function.cst(db).open_deref();
            if cst.body.is_some() {
                let result = function.body(db);
                checked
                    .diagnostics
                    .extend(result.diagnostics.iter().map(|d| d.snapshot(db)));
                let (stash, body) = result.body.open_deref();
                let mut typed = Vec::new();
                walk_expr(
                    db,
                    stash,
                    body.root,
                    function.span(db),
                    &stash[body.locals],
                    &mut typed,
                );
                let mut source = Vec::new();
                super::source::walk(source_stash, source_stash[cst.body.unwrap()], &mut source);
                for expression in source {
                    let candidates: Vec<_> = typed
                        .iter()
                        .filter(|row| row.span == expression.span && row.kind != "coercion")
                        .collect();
                    let matching: Vec<_> = candidates
                        .iter()
                        .copied()
                        .filter(|row| row.kind == expression.kind)
                        .collect();
                    let candidate = if matching.len() == 1 {
                        matching.first().copied()
                    } else if candidates.len() == 1 {
                        candidates.first().copied()
                    } else {
                        None
                    };
                    if let Some(candidate) = candidate {
                        let mut row = candidate.clone();
                        row.kind = expression.kind.into();
                        row.valid &= expression.kind != "unresolved";
                        checked.types.push(row);
                        checked.types.extend(
                            typed
                                .iter()
                                .filter(|row| row.span == expression.span && row.kind == "coercion")
                                .cloned(),
                        );
                    } else if let ParseSource::SourceFile(file) = function.span(db).source {
                        let span = function.span(db).resolve(expression.span);
                        let text = file.text(db);
                        checked.types.push(TypeRow {
                            start: span.start,
                            end: span.end,
                            span: expression.span,
                            valid: false,
                            file: sage_ir::source::normalize_path(&format!(
                                "src/{}",
                                file.path(db)
                            )),
                            line: text.as_bytes()[..span.start as usize]
                                .iter()
                                .filter(|&&b| b == b'\n')
                                .count()
                                + 1,
                            kind: expression.kind.into(),
                            source: text[span.start as usize..span.end as usize].into(),
                            ty: FrozenType::Missing,
                            symbol: Err("missing semantic correspondence".into()),
                        });
                    }
                }
            }
        }
        SymbolData::StructSymbol(StructSymbol::Local(s)) => {
            s.sig(db);
            s.fields(db);
        }
        SymbolData::EnumSymbol(EnumSymbol::Local(s)) => {
            s.sig(db);
        }
        SymbolData::TraitSymbol(TraitSymbol::Local(s)) => {
            s.sig(db);
            walk_associated(db, LocalAssociatedOwner::Trait(s), checked);
        }
        SymbolData::ImplSymbol(ImplSymbol::Local(s)) => {
            s.sig(db);
            walk_associated(db, LocalAssociatedOwner::Impl(s), checked);
        }
        SymbolData::ConstSymbol(ConstSymbol::Local(s)) => {
            unsupported(db, LocalModItemSym::Const(s), checked)
        }
        SymbolData::StaticSymbol(StaticSymbol::Local(s)) => {
            unsupported(db, LocalModItemSym::Static(s), checked)
        }
        SymbolData::TypeAliasSymbol(TypeAliasSymbol::Local(s)) => {
            unsupported(db, LocalModItemSym::TypeAlias(s), checked)
        }
        SymbolData::FnSymbol(FnSymbol::Ext(_))
        | SymbolData::ModSymbol(ModSymbol::Ext(_))
        | SymbolData::StructSymbol(StructSymbol::Ext(_))
        | SymbolData::EnumSymbol(EnumSymbol::Ext(_))
        | SymbolData::TraitSymbol(TraitSymbol::Ext(_))
        | SymbolData::ImplSymbol(ImplSymbol::Ext(_))
        | SymbolData::ConstSymbol(ConstSymbol::Ext(_))
        | SymbolData::StaticSymbol(StaticSymbol::Ext(_))
        | SymbolData::TypeAliasSymbol(TypeAliasSymbol::Ext(_))
        | SymbolData::VariantSymbol(_)
        | SymbolData::VariantCtorSymbol(_)
        | SymbolData::MacroDefSymbol(_)
        | SymbolData::UseSymbol(UseSymbol::Ext(_))
        | SymbolData::IntrinsicTypeSymbol(_) => {}
        SymbolData::UseSymbol(UseSymbol::Local(import)) => {
            checked
                .diagnostics
                .extend(import.diagnostics(db).iter().map(|d| d.snapshot(db)));
        }
        SymbolData::MacroInvocationSymbol(invocation) => {
            unsupported(db, LocalModItemSym::MacroInvocation(invocation), checked)
        }
    }
}
fn walk_associated<'db>(db: &'db dyn Db, owner: LocalAssociatedOwner<'db>, checked: &mut Checked) {
    for item in sage_ir::local_syms::associated::local_associated_items(db, owner) {
        match item {
            LocalModItemSym::Error(span) => checked.diagnostics.push(
                Diagnostic::error(Span::Absolute(*span), "unsupported associated item")
                    .snapshot(db),
            ),
            LocalModItemSym::Function(_)
            | LocalModItemSym::Struct(_)
            | LocalModItemSym::Enum(_)
            | LocalModItemSym::Trait(_)
            | LocalModItemSym::Impl(_)
            | LocalModItemSym::TypeAlias(_)
            | LocalModItemSym::Const(_)
            | LocalModItemSym::Static(_)
            | LocalModItemSym::Mod(_)
            | LocalModItemSym::Use(_)
            | LocalModItemSym::MacroDef(_)
            | LocalModItemSym::MacroInvocation(_) => walk_symbol(db, (*item).into(), checked),
        }
    }
}
fn unsupported<'db>(db: &'db dyn Db, item: LocalModItemSym<'db>, checked: &mut Checked) {
    checked.diagnostics.push(
        Diagnostic::error(
            Span::Symbol(item),
            "whole-crate checking for this item is not supported",
        )
        .snapshot(db),
    );
}
// >>

// {anchor}`crates.sage_test_harness.compile_tests.starting_expressions`
// {anchor}`crates.sage_test_harness.compile_tests.completed_types`<<
fn walk_expr<'db>(
    db: &'db dyn Db,
    stash: &Stash,
    expression: Ptr<TyExpr<'db>>,
    base: sage_ir::span::AbsoluteSpan<'db>,
    locals: &[sage_ir::tytree::LocalVar<'db>],
    rows: &mut Vec<TypeRow>,
) {
    let expr = stash[expression];
    let mut children = Vec::new();
    let mut symbol = Err("missing path resolution".into());
    let kind = match expr.data {
        E::Literal(_) => "literal",
        E::Path(res) => {
            symbol = super::symbols::resolution(db, res, locals);
            "path"
        }
        E::Block(statements, tail) => {
            for statement in &stash[statements] {
                match statement.kind {
                    TyStmtKind::Expr(e) => children.push(e),
                    TyStmtKind::Let(_, _, e) => children.extend(e),
                }
            }
            children.extend(tail);
            "block"
        }
        E::Call(callee, args) => {
            children.push(callee);
            children.extend_from_slice(&stash[args]);
            "call"
        }
        E::ResolvedCall(_, args) => {
            children.extend_from_slice(&stash[args]);
            "resolved call"
        }
        E::MethodCall(receiver, _, args) => {
            children.push(receiver);
            children.extend_from_slice(&stash[args]);
            "method call"
        }
        E::Field(e, _) => {
            children.push(e);
            "field"
        }
        E::Binary(a, _, b) => {
            children.extend([a, b]);
            "binary"
        }
        E::Unary(_, e) => {
            children.push(e);
            "unary"
        }
        E::Deref(e) => {
            children.push(e);
            "deref"
        }
        E::Ref(e, _) => {
            children.push(e);
            "reference"
        }
        E::NeverToAny(e) => {
            children.push(e);
            "coercion"
        }
        E::If(a, b, c) => {
            children.extend([a, b]);
            children.extend(c);
            "if"
        }
        E::IfLet(_, a, b, c) => {
            children.extend([a, b]);
            children.extend(c);
            "if let"
        }
        E::Match(e, arms) => {
            children.push(e);
            for arm in &stash[arms] {
                children.extend(arm.guard);
                children.push(arm.body);
            }
            "match"
        }
        E::Loop(e) => {
            children.push(e);
            "loop"
        }
        E::While(a, b) | E::WhileLet(_, a, b) | E::For(_, a, b) => {
            children.extend([a, b]);
            "loop"
        }
        E::Break(e) => {
            children.extend(e);
            "break"
        }
        E::Continue => "continue",
        E::Return(e) => {
            children.extend(e);
            "return"
        }
        E::Assign(a, b) => {
            children.extend([a, b]);
            "assignment"
        }
        E::Await(e) => {
            children.push(e);
            "await"
        }
        E::Try(e) => {
            children.push(e);
            "try"
        }
        E::Closure(_, e) => {
            children.push(e);
            "closure"
        }
        E::Tuple(es) => {
            children.extend_from_slice(&stash[es]);
            "tuple"
        }
        E::Array(es) => {
            children.extend_from_slice(&stash[es]);
            "array"
        }
        E::Index(a, b) => {
            children.extend([a, b]);
            "index"
        }
        E::Cast(e, _) => {
            children.push(e);
            "cast"
        }
        E::StructLit(_, fields) => {
            children.extend(stash[fields].iter().map(|f| f.value));
            "struct"
        }
        E::Range(a, b) => {
            children.extend(a);
            children.extend(b);
            "range"
        }
        E::MacroCall(_, _) => "macro",
        E::Error(_) => {
            symbol = Ok("<unresolved>".into());
            "error"
        }
        E::Missing | E::Unresolved(_) => "unresolved",
    };
    for child in children {
        walk_expr(db, stash, child, base, locals, rows);
    }
    let span = base.resolve(expr.span);
    let ParseSource::SourceFile(file) = span.source else {
        return;
    };
    let text = file.text(db);
    let Some(source) = text.get(span.start as usize..span.end as usize) else {
        return;
    };
    let line = text.as_bytes()[..span.start as usize]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1;
    rows.push(TypeRow {
        start: span.start,
        end: span.end,
        span: expr.span,
        valid: kind != "unresolved",
        file: sage_ir::source::normalize_path(&format!("src/{}", file.path(db))),
        line,
        kind: kind.into(),
        source: source.into(),
        ty: FrozenType::read(db, stash, expr.ty),
        symbol,
    });
}
// >>

// {anchor}`crates.sage_test_harness.compile_tests.rendering`<<
fn compact(source: &str) -> String {
    let tree = super::annotations::tree(source);
    let mut ranges = Vec::new();
    fn leaves(node: tree_sitter::Node<'_>, source: &str, ranges: &mut Vec<std::ops::Range<usize>>) {
        if node.kind() == "line_comment" && source[node.byte_range()].starts_with("//#") {
            return;
        }
        if node.child_count() == 0
            || matches!(
                node.kind(),
                "string_literal" | "raw_string_literal" | "char_literal"
            )
        {
            ranges.push(node.byte_range());
            return;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            leaves(child, source, ranges);
        }
    }
    leaves(tree.root_node(), source, &mut ranges);
    let mut out = String::new();
    let mut end = 0;
    for range in ranges {
        if !out.is_empty() && range.start > end {
            out.push(' ');
        }
        out.push_str(
            &source[range.clone()]
                .replace('\\', "\\\\")
                .replace('`', "\\`")
                .replace('\n', "\\n")
                .replace('\r', "\\r"),
        );
        end = range.end;
    }
    out
}
// >>

// {anchor}`crates.sage_test_harness.compile_tests.diagnostic_locations`
// {anchor}`crates.sage_test_harness.compile_tests.diagnostic_counts`<<
fn match_diagnostics(markers: &[Marker], diagnostics: &[DiagnosticSnapshot]) -> Result<(), String> {
    let markers: Vec<_> = markers
        .iter()
        .filter(|m| m.active && matches!(m.kind, Kind::Error | Kind::Warning))
        .collect();
    let edges: Vec<Vec<_>> = markers
        .iter()
        .map(|marker| {
            diagnostics
                .iter()
                .enumerate()
                .filter_map(|(i, diag)| {
                    let severity = match marker.kind {
                        Kind::Error => Severity::Error,
                        Kind::Warning => Severity::Warning,
                        Kind::Type | Kind::Symbol => unreachable!(),
                    };
                    (diag.severity == severity
                        && diag.primary.iter().any(|location| {
                            location.path == marker.file && location.lines().contains(&marker.line)
                        }))
                    .then_some(i)
                })
                .collect()
        })
        .collect();
    fn augment(
        marker: usize,
        edges: &[Vec<usize>],
        assigned: &mut [Option<usize>],
        seen: &mut BTreeSet<usize>,
    ) -> bool {
        for &diag in &edges[marker] {
            if !seen.insert(diag) {
                continue;
            }
            if assigned[diag].is_none_or(|previous| augment(previous, edges, assigned, seen)) {
                assigned[diag] = Some(marker);
                return true;
            }
        }
        false
    }
    let mut assigned = vec![None; diagnostics.len()];
    let matched =
        (0..markers.len()).all(|m| augment(m, &edges, &mut assigned, &mut BTreeSet::new()));
    if !matched || assigned.iter().any(Option::is_none) {
        return Err(format!(
            "diagnostic markers do not match: {} markers, {} diagnostics\n{}",
            markers.len(),
            diagnostics.len(),
            diagnostics
                .iter()
                .map(DiagnosticSnapshot::render)
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    Ok(())
}
// >>

#[cfg(test)]
mod tests {
    use super::*;
    use sage_ir::diagnostic::{LabelStyle, SourceLocation};
    fn diagnostic(first: usize, last: usize, severity: Severity) -> DiagnosticSnapshot {
        let source = "a\nb\nc\n".to_owned();
        DiagnosticSnapshot {
            severity,
            primary: vec![SourceLocation {
                path: "src/lib.rs".into(),
                source,
                start: (first - 1) * 2,
                end: (last - 1) * 2 + 1,
            }],
            message: "test diagnostic".into(),
            labels: vec![],
            notes: vec![],
        }
    }
    fn marker(line: usize, kind: Kind) -> Marker {
        Marker {
            file: "src/lib.rs".into(),
            line,
            kind,
            active: true,
        }
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.diagnostic_counts)]
    fn overlapping_locations_require_global_matching_and_exact_counts() {
        let diagnostics = [
            diagnostic(1, 2, Severity::Error),
            diagnostic(1, 1, Severity::Error),
        ];
        let markers = [marker(1, Kind::Error), marker(2, Kind::Error)];
        assert!(match_diagnostics(&markers, &diagnostics).is_ok());
        assert!(match_diagnostics(&markers[..1], &diagnostics).is_err());
        assert!(match_diagnostics(&markers, &diagnostics[..1]).is_err());
        assert!(
            match_diagnostics(
                &[marker(1, Kind::Warning), marker(2, Kind::Error)],
                &diagnostics
            )
            .is_err()
        );
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.diagnostic_locations)]
    fn labels_and_notes_do_not_supply_matching_locations() {
        let mut diagnostic = diagnostic(1, 1, Severity::Error);
        let other = self::diagnostic(2, 2, Severity::Warning);
        diagnostic.labels.push((
            other.primary.clone(),
            "related declaration".into(),
            LabelStyle::Primary,
        ));
        diagnostic.notes.push(other);
        assert!(match_diagnostics(&[marker(1, Kind::Error)], &[diagnostic.clone()]).is_ok());
        assert!(match_diagnostics(&[marker(2, Kind::Error)], &[diagnostic.clone()]).is_err());
        let rendered = diagnostic.render();
        assert!(rendered.contains("related declaration"));
        assert!(rendered.contains("warning"));
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.diagnostic_locations)]
    fn empty_spans_match_positions_including_eof() {
        let mut diagnostic = diagnostic(1, 1, Severity::Error);
        diagnostic.primary[0].start = 2;
        diagnostic.primary[0].end = 2;
        assert!(match_diagnostics(&[marker(2, Kind::Error)], &[diagnostic.clone()]).is_ok());
        diagnostic.primary[0].start = 6;
        diagnostic.primary[0].end = 6;
        assert!(match_diagnostics(&[marker(4, Kind::Error)], &[diagnostic]).is_ok());
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.rendering)]
    fn compact_descriptions_preserve_token_and_literal_contents() {
        assert_eq!(
            compact("call(\n  22, //# ty\n  true,\n)"),
            "call( 22, true, )"
        );
        assert_eq!(compact("\"a  b\\nc\""), "\"a  b\\\\nc\"");
        assert_eq!(compact("r#\"first\nsecond\"#"), "r#\"first\\nsecond\"#");
        assert_eq!(compact("left    + right"), "left + right");
    }

    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.completed_types)]
    fn nested_inference_variables_are_not_completed_observations() {
        use sage_ir::ty::{InferVarIndex, Ty};
        let mut stash = Stash::new();
        let unresolved = stash.alloc(Ty::InferVar(InferVarIndex(0)));
        let tuple = stash.alloc_slice(&[unresolved]);
        let tuple = stash.alloc(Ty::Tuple(tuple));
        let db = sage_ir::db::Database::default();
        assert!(!FrozenType::read(&db, &stash, tuple).complete());
        let boolean = stash.alloc(Ty::Bool);
        assert!(FrozenType::read(&db, &stash, boolean).complete());
    }
}
