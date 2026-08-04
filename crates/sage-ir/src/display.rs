//! Reusable type display: `TyDisplay` implements `fmt::Display` for a type.

use std::fmt;

use sage_stash::{Ptr, Stash};

use crate::ty::{AliasTy, TraitRef, Ty, WherePredicate};

/// Wrapper that implements `Display` for a stash-allocated type.
///
/// Usage: `format!("{}", TyDisplay::new(db, stash, ty))`
pub struct TyDisplay<'a, 'db> {
    db: &'db dyn crate::Db,
    stash: &'a Stash,
    ty: Ptr<Ty<'db>>,
}

impl<'a, 'db> TyDisplay<'a, 'db> {
    pub fn new(db: &'db dyn crate::Db, stash: &'a Stash, ty: Ptr<Ty<'db>>) -> Self {
        Self { db, stash, ty }
    }
}

impl fmt::Display for TyDisplay<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_ty(f, self.db, self.stash, self.ty)
    }
}

pub struct TraitRefDisplay<'a, 'db> {
    db: &'db dyn crate::Db,
    stash: &'a Stash,
    trait_ref: TraitRef<'db>,
}

impl<'a, 'db> TraitRefDisplay<'a, 'db> {
    pub fn new(db: &'db dyn crate::Db, stash: &'a Stash, trait_ref: TraitRef<'db>) -> Self {
        Self {
            db,
            stash,
            trait_ref,
        }
    }
}

impl fmt::Display for TraitRefDisplay<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::symbol::TraitSymbol;

        let name = match self.trait_ref.trait_sym {
            TraitSymbol::Local(local) => local.name(self.db).text(self.db),
            TraitSymbol::Ext(external) => external
                .name(self.db)
                .map_or("?", |(name, _)| name.text(self.db)),
        };
        f.write_str(name)?;
        if !self.stash[self.trait_ref.args].is_empty() {
            f.write_str("<")?;
            for (index, argument) in self.stash[self.trait_ref.args].iter().enumerate() {
                if index > 0 {
                    f.write_str(", ")?;
                }
                fmt_ty(f, self.db, self.stash, *argument)?;
            }
            f.write_str(">")?;
        }
        Ok(())
    }
}

pub struct WherePredicateDisplay<'a, 'db> {
    db: &'db dyn crate::Db,
    stash: &'a Stash,
    predicate: WherePredicate<'db>,
}

impl<'a, 'db> WherePredicateDisplay<'a, 'db> {
    pub fn new(db: &'db dyn crate::Db, stash: &'a Stash, predicate: WherePredicate<'db>) -> Self {
        Self {
            db,
            stash,
            predicate,
        }
    }
}

impl fmt::Display for WherePredicateDisplay<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_ty(f, self.db, self.stash, self.predicate.self_ty)?;
        f.write_str(": ")?;
        TraitRefDisplay::new(self.db, self.stash, self.predicate.trait_ref).fmt(f)
    }
}

fn fmt_ty(
    f: &mut fmt::Formatter<'_>,
    db: &dyn crate::Db,
    stash: &Stash,
    ty: Ptr<Ty<'_>>,
) -> fmt::Result {
    match stash[ty] {
        Ty::Bool => f.write_str("bool"),
        Ty::Char => f.write_str("char"),
        Ty::Int(i) => f.write_str(match i {
            crate::ty::IntTy::I8 => "i8",
            crate::ty::IntTy::I16 => "i16",
            crate::ty::IntTy::I32 => "i32",
            crate::ty::IntTy::I64 => "i64",
            crate::ty::IntTy::I128 => "i128",
            crate::ty::IntTy::Isize => "isize",
        }),
        Ty::Uint(u) => f.write_str(match u {
            crate::ty::UintTy::U8 => "u8",
            crate::ty::UintTy::U16 => "u16",
            crate::ty::UintTy::U32 => "u32",
            crate::ty::UintTy::U64 => "u64",
            crate::ty::UintTy::U128 => "u128",
            crate::ty::UintTy::Usize => "usize",
        }),
        Ty::Float(fl) => f.write_str(match fl {
            crate::ty::FloatTy::F32 => "f32",
            crate::ty::FloatTy::F64 => "f64",
        }),
        Ty::Str => f.write_str("str"),
        Ty::Never => f.write_str("!"),
        Ty::Error(_) => f.write_str("<error>"),
        Ty::InferVar(idx) => write!(f, "?{}", idx.0),
        Ty::Param(p) => {
            let name = p.name(db).map_or("?", |n| n.text(db));
            f.write_str(name)
        }
        Ty::Tuple(elems) => {
            f.write_str("(")?;
            for (i, elem) in stash[elems].iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                fmt_ty(f, db, stash, *elem)?;
            }
            f.write_str(")")
        }
        Ty::Ref(inner, m, _) => {
            match m {
                crate::cst::Mutability::Shared => f.write_str("&")?,
                crate::cst::Mutability::Mut => f.write_str("&mut ")?,
            }
            fmt_ty(f, db, stash, inner)
        }
        Ty::Adt(sym, args) => {
            let name = sym.name(db).map_or("?", |(n, _)| n.text(db));
            f.write_str(name)?;
            let type_args = &stash[args];
            if !type_args.is_empty() {
                f.write_str("<")?;
                for (i, arg) in type_args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    fmt_ty(f, db, stash, *arg)?;
                }
                f.write_str(">")?;
            }
            Ok(())
        }
        Ty::Alias(AliasTy::Named(alias)) => fmt_alias_name(f, db, alias.def, stash, alias.args),
        Ty::Alias(AliasTy::Associated(projection)) => {
            f.write_str("<")?;
            fmt_ty(f, db, stash, projection.self_ty)?;
            f.write_str(" as ")?;
            fmt::Display::fmt(&TraitRefDisplay::new(db, stash, projection.trait_ref), f)?;
            f.write_str(">::")?;
            fmt_alias_name(f, db, projection.associated_ty, stash, projection.args)
        }
        Ty::Alias(AliasTy::Opaque(alias)) => {
            f.write_str("opaque ")?;
            fmt_alias_name(f, db, alias.def, stash, alias.args)
        }
        Ty::Slice(inner) => {
            f.write_str("[")?;
            fmt_ty(f, db, stash, inner)?;
            f.write_str("]")
        }
        Ty::Array(inner, _) => {
            f.write_str("[")?;
            fmt_ty(f, db, stash, inner)?;
            f.write_str("; _]")
        }
        Ty::FnPtr(params, ret) => {
            f.write_str("fn(")?;
            for (i, p) in stash[params].iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                fmt_ty(f, db, stash, *p)?;
            }
            f.write_str(") -> ")?;
            fmt_ty(f, db, stash, ret)
        }
    }
}

fn fmt_alias_name(
    f: &mut fmt::Formatter<'_>,
    db: &dyn crate::Db,
    def: crate::symbol::TypeAliasSymbol<'_>,
    stash: &Stash,
    args: sage_stash::Slice<Ptr<Ty<'_>>>,
) -> fmt::Result {
    use crate::symbol::TypeAliasSymbol;

    let name = match def {
        TypeAliasSymbol::Local(local) => local.name(db).text(db),
        TypeAliasSymbol::Ext(external) => external.name(db).map_or("?", |(name, _)| name.text(db)),
    };
    f.write_str(name)?;
    if !stash[args].is_empty() {
        f.write_str("<")?;
        for (index, argument) in stash[args].iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            fmt_ty(f, db, stash, *argument)?;
        }
        f.write_str(">")?;
    }
    Ok(())
}
