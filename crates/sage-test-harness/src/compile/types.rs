//! Resolve semantic names while capturing queries; format only frozen data later.
// {anchor}`crates.sage_test_harness.compile_tests.resolved_symbols`
// {anchor}`crates.sage_test_harness.compile_tests.completed_types`
// {anchor}`crates.sage_test_harness.compile_tests.rendering`
// {anchor}`crates.sage_test_harness.compile_tests.query_capture`
use sage_ir::{
    Db,
    cst::Mutability,
    symbol::{Symbol, TypeAliasSymbol},
    ty::{AliasTy, Const, Ty},
};
use sage_stash::{Ptr, Slice, Stash};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum FrozenType {
    Name(String, Vec<Self>),
    Tuple(Vec<Self>),
    Ref(bool, Box<Self>),
    Slice(Box<Self>),
    Array(Box<Self>, String),
    Function(Vec<Self>, Box<Self>),
    Projection(Box<Self>, String, Vec<Self>, String, Vec<Self>),
    Opaque(Box<Self>),
    Missing,
}
impl FrozenType {
    pub fn read(db: &dyn Db, stash: &Stash, ty: Ptr<Ty<'_>>) -> Self {
        let args = |args: Slice<Ptr<Ty<'_>>>| {
            stash[args]
                .iter()
                .map(|ty| Self::read(db, stash, *ty))
                .collect()
        };
        let name = |symbol: Symbol<'_>| qualified_name(db, symbol);
        let alias_name = |alias: TypeAliasSymbol<'_>| name(alias.into());
        let primitive = |s: &str| Self::Name(s.into(), vec![]);
        match stash[ty] {
            Ty::Bool => primitive("bool"),
            Ty::Char => primitive("char"),
            Ty::Str => primitive("str"),
            Ty::Int(value) => primitive(match value {
                sage_ir::ty::IntTy::I8 => "i8",
                sage_ir::ty::IntTy::I16 => "i16",
                sage_ir::ty::IntTy::I32 => "i32",
                sage_ir::ty::IntTy::I64 => "i64",
                sage_ir::ty::IntTy::I128 => "i128",
                sage_ir::ty::IntTy::Isize => "isize",
            }),
            Ty::Uint(value) => primitive(match value {
                sage_ir::ty::UintTy::U8 => "u8",
                sage_ir::ty::UintTy::U16 => "u16",
                sage_ir::ty::UintTy::U32 => "u32",
                sage_ir::ty::UintTy::U64 => "u64",
                sage_ir::ty::UintTy::U128 => "u128",
                sage_ir::ty::UintTy::Usize => "usize",
            }),
            Ty::Float(value) => primitive(match value {
                sage_ir::ty::FloatTy::F32 => "f32",
                sage_ir::ty::FloatTy::F64 => "f64",
            }),
            Ty::Never => primitive("!"),
            Ty::Error(_) => primitive("<error>"),
            Ty::InferVar(_) => Self::Missing,
            Ty::Param(param) => param
                .name(db)
                .map_or(Self::Missing, |name| primitive(name.text(db))),
            Ty::Adt(symbol, parameters) => {
                name(symbol).map_or(Self::Missing, |name| Self::Name(name, args(parameters)))
            }
            Ty::Tuple(elements) => Self::Tuple(args(elements)),
            Ty::Ref(inner, mutability, _) => Self::Ref(
                mutability == Mutability::Mut,
                Box::new(Self::read(db, stash, inner)),
            ),
            Ty::Slice(inner) => Self::Slice(Box::new(Self::read(db, stash, inner))),
            Ty::Array(inner, length) => {
                let length = match length {
                    Const::Literal(n) => Some(n.to_string()),
                    Const::Param(p) => p.name(db).map(|n| n.text(db).clone()),
                    Const::Other(s) => name(s),
                };
                length.map_or(Self::Missing, |length| {
                    Self::Array(Box::new(Self::read(db, stash, inner)), length)
                })
            }
            Ty::FnPtr(parameters, result) => {
                Self::Function(args(parameters), Box::new(Self::read(db, stash, result)))
            }
            Ty::Alias(AliasTy::Named(alias)) => alias_name(alias.def)
                .map_or(Self::Missing, |name| Self::Name(name, args(alias.args))),
            Ty::Alias(AliasTy::Opaque(alias)) => alias_name(alias.def)
                .map_or(Self::Missing, |name| {
                    Self::Opaque(Box::new(Self::Name(name, args(alias.args))))
                }),
            Ty::Alias(AliasTy::Associated(alias)) => match (
                name(alias.trait_ref.trait_sym.into()),
                alias_name(alias.associated_ty),
            ) {
                (Some(trait_name), Some(alias_name)) => Self::Projection(
                    Box::new(Self::read(db, stash, alias.self_ty)),
                    trait_name,
                    args(alias.trait_ref.args),
                    alias_name,
                    args(alias.args),
                ),
                _ => Self::Missing,
            },
        }
    }
    pub fn complete(&self) -> bool {
        match self {
            Self::Missing => false,
            Self::Name(_, args) | Self::Tuple(args) => args.iter().all(Self::complete),
            Self::Ref(_, inner)
            | Self::Slice(inner)
            | Self::Array(inner, _)
            | Self::Opaque(inner) => inner.complete(),
            Self::Function(args, result) => args.iter().all(Self::complete) && result.complete(),
            Self::Projection(ty, _, a, _, b) => {
                ty.complete() && a.iter().all(Self::complete) && b.iter().all(Self::complete)
            }
        }
    }
}
fn list(f: &mut fmt::Formatter<'_>, args: &[FrozenType]) -> fmt::Result {
    for (index, arg) in args.iter().enumerate() {
        if index > 0 {
            write!(f, ", ")?;
        }
        write!(f, "{arg}")?;
    }
    Ok(())
}
fn named(f: &mut fmt::Formatter<'_>, name: &str, args: &[FrozenType]) -> fmt::Result {
    write!(f, "{name}")?;
    if !args.is_empty() {
        write!(f, "<")?;
        list(f, args)?;
        write!(f, ">")?;
    }
    Ok(())
}
impl fmt::Display for FrozenType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => Err(fmt::Error),
            Self::Name(name, args) => named(f, name, args),
            Self::Tuple(args) => {
                write!(f, "(")?;
                list(f, args)?;
                if args.len() == 1 {
                    write!(f, ",")?;
                }
                write!(f, ")")
            }
            Self::Ref(mutable, inner) => {
                write!(f, "&{}{inner}", if *mutable { "mut " } else { "" })
            }
            Self::Slice(inner) => write!(f, "[{inner}]"),
            Self::Array(inner, length) => write!(f, "[{inner}; {length}]"),
            Self::Function(args, result) => {
                write!(f, "fn(")?;
                list(f, args)?;
                write!(f, ") -> {result}")
            }
            Self::Opaque(inner) => write!(f, "opaque {inner}"),
            Self::Projection(ty, trait_name, args, alias, alias_args) => {
                write!(f, "<{ty} as ")?;
                named(f, trait_name, args)?;
                write!(f, ">::")?;
                named(f, alias, alias_args)
            }
        }
    }
}

fn qualified_name(db: &dyn Db, symbol: Symbol<'_>) -> Option<String> {
    use sage_ir::symbol::*;
    let scope = match symbol.data(db) {
        SymbolData::StructSymbol(StructSymbol::Local(s)) => s.scope(db),
        SymbolData::EnumSymbol(EnumSymbol::Local(s)) => s.scope(db),
        SymbolData::TraitSymbol(TraitSymbol::Local(s)) => s.scope(db),
        SymbolData::TypeAliasSymbol(TypeAliasSymbol::Local(s)) => s.scope(db),
        SymbolData::ConstSymbol(ConstSymbol::Local(s)) => s.scope(db),
        SymbolData::StaticSymbol(StaticSymbol::Local(s)) => s.scope(db),
        SymbolData::StructSymbol(StructSymbol::Ext(s))
        | SymbolData::EnumSymbol(EnumSymbol::Ext(s))
        | SymbolData::TraitSymbol(TraitSymbol::Ext(s))
        | SymbolData::TypeAliasSymbol(TypeAliasSymbol::Ext(s))
        | SymbolData::ConstSymbol(ConstSymbol::Ext(s))
        | SymbolData::StaticSymbol(StaticSymbol::Ext(s)) => {
            return db.tcx().def_path(s.crate_num(db), s.def_index(db));
        }
        SymbolData::FnSymbol(_)
        | SymbolData::VariantSymbol(_)
        | SymbolData::VariantCtorSymbol(_)
        | SymbolData::ImplSymbol(_)
        | SymbolData::ModSymbol(_)
        | SymbolData::MacroDefSymbol(_)
        | SymbolData::UseSymbol(_)
        | SymbolData::IntrinsicTypeSymbol(_)
        | SymbolData::MacroInvocationSymbol(_) => return None,
    };
    Some(qualified_local_name(db, scope, symbol.name(db)?.0.text(db)))
}

pub(super) fn qualified_local_name(
    db: &dyn Db,
    scope: sage_ir::scope::ScopeSymbol<'_>,
    name: &str,
) -> String {
    let mut parts = vec![name.to_owned()];
    let mut module = scope.module(db);
    while let Some(parent) = module.parent(db) {
        parts.push(module.name(db).text(db).clone());
        module = parent.module(db);
    }
    parts.reverse();
    parts.join("::")
}
