//! `Display` impls for IR types using salsa's attached database.
//!
//! These impls use `salsa::with_attached_database` to access the db,
//! so they work in `Debug`/`Display` contexts without passing `db` explicitly.
//! The database must be attached (it is during tracked function execution,
//! or call `db.attach(|| ...)` in tests).

use std::fmt;

use crate::item::*;
use crate::types::*;

fn with_db(f: impl FnOnce(&dyn salsa::Database) -> fmt::Result) -> fmt::Result {
    salsa::with_attached_database(f).unwrap_or_else(|| Ok(()))
}

// -- Item --

impl fmt::Display for Item<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Item::Function(v) => fmt::Display::fmt(v, f),
            Item::Struct(v) => fmt::Display::fmt(v, f),
            Item::Enum(v) => fmt::Display::fmt(v, f),
            Item::Trait(v) => fmt::Display::fmt(v, f),
            Item::Impl(v) => fmt::Display::fmt(v, f),
            Item::TypeAlias(v) => fmt::Display::fmt(v, f),
            Item::Const(v) => fmt::Display::fmt(v, f),
            Item::Static(v) => fmt::Display::fmt(v, f),
            Item::Mod(v) => fmt::Display::fmt(v, f),
            Item::Use(v) => fmt::Display::fmt(v, f),
            Item::Error(span) => write!(f, "{{error {}..{}}}", span.start, span.end),
        }
    }
}

// -- Function --

impl fmt::Display for FunctionItem<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            if self.is_async(db) {
                f.write_str("async ")?;
            }
            if self.is_unsafe(db) {
                f.write_str("unsafe ")?;
            }
            write!(f, "fn {}(", self.name(db).text(db))?;
            for (i, p) in self.params(db).iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                fmt::Display::fmt(p, f)?;
            }
            f.write_str(")")?;
            if let Some(ret) = self.ret_type(db) {
                write!(f, " -> {ret}")?;
            }
            Ok(())
        })
    }
}

// -- Struct --

impl fmt::Display for StructItem<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            writeln!(f, "struct {} {{", self.name(db).text(db))?;
            for field in self.fields(db) {
                writeln!(f, "  {field}")?;
            }
            f.write_str("}")
        })
    }
}

// -- Enum --

impl fmt::Display for EnumItem<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            writeln!(f, "enum {} {{", self.name(db).text(db))?;
            for v in self.variants(db) {
                let fields = v.fields(db);
                if fields.is_empty() {
                    writeln!(f, "  {}", v.name(db).text(db))?;
                } else {
                    writeln!(f, "  {} {{", v.name(db).text(db))?;
                    for field in fields {
                        writeln!(f, "    {field}")?;
                    }
                    writeln!(f, "  }}")?;
                }
            }
            f.write_str("}")
        })
    }
}

// -- Trait --

impl fmt::Display for TraitItem<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            writeln!(f, "trait {} {{", self.name(db).text(db))?;
            for item in self.items(db) {
                writeln!(f, "  {item}")?;
            }
            f.write_str("}")
        })
    }
}

// -- Impl --

impl fmt::Display for ImplItem<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            if let Some(trait_path) = self.trait_path(db) {
                write!(f, "impl {trait_path} for {} {{", self.self_ty(db))?;
            } else {
                write!(f, "impl {} {{", self.self_ty(db))?;
            }
            f.write_str("\n")?;
            for item in self.items(db) {
                writeln!(f, "  {item}")?;
            }
            f.write_str("}")
        })
    }
}

// -- TypeAlias --

impl fmt::Display for TypeAliasItem<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            write!(f, "type {}", self.name(db).text(db))?;
            if let Some(ty) = self.ty(db) {
                write!(f, " = {ty}")?;
            }
            Ok(())
        })
    }
}

// -- Const --

impl fmt::Display for ConstItem<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            write!(f, "const {}", self.name(db).text(db))?;
            if let Some(ty) = self.ty(db) {
                write!(f, ": {ty}")?;
            }
            Ok(())
        })
    }
}

// -- Static --

impl fmt::Display for StaticItem<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            if self.is_mut(db) {
                f.write_str("static mut ")?;
            } else {
                f.write_str("static ")?;
            }
            write!(f, "{}", self.name(db).text(db))?;
            if let Some(ty) = self.ty(db) {
                write!(f, ": {ty}")?;
            }
            Ok(())
        })
    }
}

// -- Mod --

impl fmt::Display for ModItem<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| match self.items(db) {
            Some(items) => {
                writeln!(f, "mod {} {{", self.name(db).text(db))?;
                for item in items {
                    writeln!(f, "  {item}")?;
                }
                f.write_str("}")
            }
            None => write!(f, "mod {};", self.name(db).text(db)),
        })
    }
}

// -- Use --

impl fmt::Display for UseItem<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            write!(f, "use {}", self.path(db))?;
            if let Some(alias) = self.alias(db) {
                write!(f, " as {}", alias.text(db))?;
            }
            Ok(())
        })
    }
}

// -- TypeRef --

impl fmt::Display for TypeRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| match self.kind(db) {
            TypeRefKind::Path(p) => write!(f, "{p}"),
            TypeRefKind::Reference(inner, Mutability::Shared) => write!(f, "&{inner}"),
            TypeRefKind::Reference(inner, Mutability::Mut) => write!(f, "&mut {inner}"),
            TypeRefKind::Slice(inner) => write!(f, "[{inner}]"),
            TypeRefKind::Array(inner) => write!(f, "[{inner}; _]"),
            TypeRefKind::Tuple(tup) => {
                f.write_str("(")?;
                for (i, elem) in tup.elements(db).iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{elem}")?;
                }
                f.write_str(")")
            }
            TypeRefKind::Never => f.write_str("!"),
            TypeRefKind::Infer => f.write_str("_"),
            TypeRefKind::Error => f.write_str("{error}"),
        })
    }
}

// -- Path --

impl fmt::Display for Path<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            for (i, seg) in self.segments(db).iter().enumerate() {
                if i > 0 {
                    f.write_str("::")?;
                }
                f.write_str(seg.text(db))?;
            }
            Ok(())
        })
    }
}

// -- Param --

impl fmt::Display for Param<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            if let Some(name) = self.name(db) {
                write!(f, "{}: {}", name.text(db), self.ty(db))
            } else {
                write!(f, "{}", self.ty(db))
            }
        })
    }
}

// -- FieldDef --

impl fmt::Display for FieldDef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| write!(f, "{}: {}", self.name(db).text(db), self.ty(db)))
    }
}

// -- VariantDef --

impl fmt::Display for VariantDef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        with_db(|db| {
            let fields = self.fields(db);
            if fields.is_empty() {
                write!(f, "{}", self.name(db).text(db))
            } else {
                writeln!(f, "{} {{", self.name(db).text(db))?;
                for field in fields {
                    writeln!(f, "  {field}")?;
                }
                f.write_str("}")
            }
        })
    }
}
