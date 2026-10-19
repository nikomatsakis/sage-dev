//! Freeze resolved identities before query capture ends; never print database IDs.
// {anchor}`crates.sage_test_harness.compile_tests.annotation_validation`
// {anchor}`crates.sage_test_harness.compile_tests.resolved_symbols`
// {anchor}`crates.sage_test_harness.compile_tests.query_capture`
use sage_ir::{
    Db,
    scope::ScopeSymbol,
    symbol::*,
    tytree::{LocalVar, PathResolution},
};

pub(super) fn resolution(
    db: &dyn Db,
    resolution: PathResolution<'_>,
    locals: &[LocalVar<'_>],
) -> Result<String, String> {
    match resolution {
        PathResolution::Def(symbol) => definition(db, symbol),
        PathResolution::Error(_) => Ok("<unresolved>".into()),
        PathResolution::Local(id) => {
            let local = locals.get(id.0 as usize).ok_or("missing local binding")?;
            let ordinal = locals
                .iter()
                .filter(|other| other.name == local.name && other.span.start < local.span.start)
                .count()
                + 1;
            if locals
                .iter()
                .filter(|other| other.name == local.name && other.span.start == local.span.start)
                .count()
                != 1
            {
                return Err("ambiguous local binding identity".into());
            }
            Ok(format!("local {}#{ordinal}", local.name.text(db)))
        }
    }
}
fn local_name(db: &dyn Db, scope: ScopeSymbol<'_>, name: &str) -> String {
    format!(
        "crate::{}",
        super::types::qualified_local_name(db, scope, name)
    )
}
fn definition(db: &dyn Db, symbol: Symbol<'_>) -> Result<String, String> {
    let named = |kind: &str, scope| {
        let name = symbol.name(db).ok_or("missing definition name")?.0;
        Ok(format!("{kind} {}", local_name(db, scope, name.text(db))))
    };
    let external = |kind: &str, s: SymExt<'_>| {
        db.tcx()
            .def_path(s.crate_num(db), s.def_index(db))
            .map(|path| format!("{kind} {path}"))
            .ok_or_else(|| "missing external definition path".into())
    };
    match symbol.data(db) {
        SymbolData::FnSymbol(FnSymbol::Local(s)) if s.owner(db).is_none() => {
            named("fn", s.scope(db))
        }
        SymbolData::StructSymbol(StructSymbol::Local(s)) => named("struct", s.scope(db)),
        SymbolData::EnumSymbol(EnumSymbol::Local(s)) => named("enum", s.scope(db)),
        SymbolData::ConstSymbol(ConstSymbol::Local(s)) if s.owner(db).is_none() => {
            named("const", s.scope(db))
        }
        SymbolData::StaticSymbol(StaticSymbol::Local(s)) => named("static", s.scope(db)),
        SymbolData::VariantSymbol(VariantSymbol::Local(s)) => Ok(format!(
            "variant {}::{}",
            local_name(
                db,
                s.parent_enum(db).scope(db),
                s.parent_enum(db).name(db).text(db)
            ),
            s.name(db).text(db)
        )),
        SymbolData::VariantCtorSymbol(VariantCtorSymbol::Local(s)) => {
            definition(db, VariantSymbol::Local(s.variant(db)).into())
        }
        SymbolData::FnSymbol(FnSymbol::Ext(s)) => external("fn", s),
        SymbolData::StructSymbol(StructSymbol::Ext(s)) => external("struct", s),
        SymbolData::EnumSymbol(EnumSymbol::Ext(s)) => external("enum", s),
        SymbolData::ConstSymbol(ConstSymbol::Ext(s)) => external("const", s),
        SymbolData::StaticSymbol(StaticSymbol::Ext(s)) => external("static", s),
        SymbolData::VariantSymbol(VariantSymbol::Ext(s)) => external("variant", s),
        SymbolData::VariantCtorSymbol(VariantCtorSymbol::Ext(s)) => external("variant", s),
        SymbolData::FnSymbol(FnSymbol::Local(_))
        | SymbolData::ConstSymbol(ConstSymbol::Local(_))
        | SymbolData::TraitSymbol(_)
        | SymbolData::TypeAliasSymbol(_)
        | SymbolData::ImplSymbol(_)
        | SymbolData::ModSymbol(_)
        | SymbolData::MacroDefSymbol(_)
        | SymbolData::UseSymbol(_)
        | SymbolData::IntrinsicTypeSymbol(_)
        | SymbolData::MacroInvocationSymbol(_) => Err("unsupported symbol identity".into()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[sage::test(crates.sage_test_harness.compile_tests.resolved_symbols)]
    fn missing_local_correspondence_is_not_an_unresolved_name() {
        use sage_ir::tytree::{LocalId, PathResolution};
        let db = sage_ir::db::Database::default();
        let error = super::resolution(&db, PathResolution::Local(LocalId(0)), &[]).unwrap_err();
        assert_eq!(error, "missing local binding");
    }
}
