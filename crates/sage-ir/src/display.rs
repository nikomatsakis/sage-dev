//! Reusable type display: `TyDisplay` implements `fmt::Display` for a type.

use std::fmt;

use sage_stash::{Ptr, Stash};

use crate::ty::Ty;

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
        Ty::Error => f.write_str("<error>"),
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
