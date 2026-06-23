use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════
// Core data structures — generic over Def
// ═══════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(serialize = "Def: Serialize", deserialize = "Def: Deserialize<'de>"))]
pub struct Crate<Def> {
    pub root: Module<Def>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(serialize = "Def: Serialize", deserialize = "Def: Deserialize<'de>"))]
pub struct Module<Def> {
    pub def: Def,
    pub name: String,
    pub items: Vec<Item<Def>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(bound(serialize = "Def: Serialize", deserialize = "Def: Deserialize<'de>"))]
pub enum Item<Def> {
    Fn(FnItem<Def>),
    Struct(StructItem<Def>),
    Mod(Module<Def>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(serialize = "Def: Serialize", deserialize = "Def: Deserialize<'de>"))]
pub struct FnItem<Def> {
    pub def: Def,
    pub name: String,
    pub params: Vec<Param<Def>>,
    pub return_ty: Type<Def>,
    pub body: Option<Expr<Def>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(serialize = "Def: Serialize", deserialize = "Def: Deserialize<'de>"))]
pub struct Param<Def> {
    pub name: String,
    pub ty: Type<Def>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(serialize = "Def: Serialize", deserialize = "Def: Deserialize<'de>"))]
pub struct StructItem<Def> {
    pub def: Def,
    pub name: String,
    pub fields: Vec<FieldDef<Def>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(serialize = "Def: Serialize", deserialize = "Def: Deserialize<'de>"))]
pub struct FieldDef<Def> {
    pub name: String,
    pub ty: Type<Def>,
}

// ═══════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(bound(serialize = "Def: Serialize", deserialize = "Def: Deserialize<'de>"))]
pub enum Type<Def> {
    Primitive(String),
    Def {
        target: Def,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        type_args: Vec<Type<Def>>,
    },
    Ref {
        mutable: bool,
        ty: Box<Type<Def>>,
    },
    Unit,
    Tuple(Vec<Type<Def>>),
}

// ═══════════════════════════════════════════════════════════════════════
// Expressions and statements
// ═══════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(bound(serialize = "Def: Serialize", deserialize = "Def: Deserialize<'de>"))]
pub enum Expr<Def> {
    Local {
        name: String,
        index: u32,
    },
    Literal {
        kind: LiteralKind,
        value: String,
    },
    BinaryOp {
        op: BinOp,
        lhs: Box<Expr<Def>>,
        rhs: Box<Expr<Def>>,
        ty: Type<Def>,
    },
    Call {
        target: Def,
        args: Vec<Expr<Def>>,
        ty: Type<Def>,
    },
    StructLit {
        target: Def,
        fields: Vec<FieldExpr<Def>>,
        ty: Type<Def>,
    },
    Field {
        expr: Box<Expr<Def>>,
        field_name: String,
        ty: Type<Def>,
    },
    Block {
        stmts: Vec<Stmt<Def>>,
        tail: Option<Box<Expr<Def>>>,
        ty: Type<Def>,
    },
    Deref {
        expr: Box<Expr<Def>>,
        ty: Type<Def>,
    },
    Ref {
        mutable: bool,
        expr: Box<Expr<Def>>,
        ty: Type<Def>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(bound(serialize = "Def: Serialize", deserialize = "Def: Deserialize<'de>"))]
pub enum Stmt<Def> {
    Let {
        name: String,
        index: u32,
        ty: Type<Def>,
        init: Option<Expr<Def>>,
    },
    Expr(Expr<Def>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(serialize = "Def: Serialize", deserialize = "Def: Deserialize<'de>"))]
pub struct FieldExpr<Def> {
    pub name: String,
    pub value: Expr<Def>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiteralKind {
    Int,
    Float,
    Bool,
    Char,
    Str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

// ═══════════════════════════════════════════════════════════════════════
// Normalization — map Def to a comparable form
// ═══════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalizedDef {
    Local(u32),
    External(DefPath),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefPath {
    pub krate: String,
    pub segments: Vec<DefPathSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefPathSegment {
    pub kind: DefKind,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefKind {
    Mod,
    Fn,
    Struct,
    Enum,
    Trait,
    Impl,
    TypeAlias,
    Const,
    Static,
}

// ═══════════════════════════════════════════════════════════════════════
// Generic map operation
// ═══════════════════════════════════════════════════════════════════════

impl<Def> Crate<Def> {
    pub fn map<Def2>(self, mut f: impl FnMut(Def) -> Def2) -> Crate<Def2> {
        Crate {
            root: self.root.map(&mut f),
        }
    }
}

impl<Def> Module<Def> {
    pub fn map<Def2>(self, f: &mut impl FnMut(Def) -> Def2) -> Module<Def2> {
        Module {
            def: f(self.def),
            name: self.name,
            items: self.items.into_iter().map(|item| item.map(f)).collect(),
        }
    }
}

impl<Def> Item<Def> {
    pub fn map<Def2>(self, f: &mut impl FnMut(Def) -> Def2) -> Item<Def2> {
        match self {
            Item::Fn(item) => Item::Fn(item.map(f)),
            Item::Struct(item) => Item::Struct(item.map(f)),
            Item::Mod(module) => Item::Mod(module.map(f)),
        }
    }
}

impl<Def> FnItem<Def> {
    pub fn map<Def2>(self, f: &mut impl FnMut(Def) -> Def2) -> FnItem<Def2> {
        FnItem {
            def: f(self.def),
            name: self.name,
            params: self.params.into_iter().map(|p| p.map(f)).collect(),
            return_ty: self.return_ty.map(f),
            body: self.body.map(|e| e.map(f)),
        }
    }
}

impl<Def> Param<Def> {
    pub fn map<Def2>(self, f: &mut impl FnMut(Def) -> Def2) -> Param<Def2> {
        Param {
            name: self.name,
            ty: self.ty.map(f),
        }
    }
}

impl<Def> StructItem<Def> {
    pub fn map<Def2>(self, f: &mut impl FnMut(Def) -> Def2) -> StructItem<Def2> {
        StructItem {
            def: f(self.def),
            name: self.name,
            fields: self.fields.into_iter().map(|fd| fd.map(f)).collect(),
        }
    }
}

impl<Def> FieldDef<Def> {
    pub fn map<Def2>(self, f: &mut impl FnMut(Def) -> Def2) -> FieldDef<Def2> {
        FieldDef {
            name: self.name,
            ty: self.ty.map(f),
        }
    }
}

impl<Def> Type<Def> {
    pub fn map<Def2>(self, f: &mut impl FnMut(Def) -> Def2) -> Type<Def2> {
        match self {
            Type::Primitive(s) => Type::Primitive(s),
            Type::Def { target, type_args } => Type::Def {
                target: f(target),
                type_args: type_args.into_iter().map(|t| t.map(f)).collect(),
            },
            Type::Ref { mutable, ty } => Type::Ref {
                mutable,
                ty: Box::new((*ty).map(f)),
            },
            Type::Unit => Type::Unit,
            Type::Tuple(tys) => Type::Tuple(tys.into_iter().map(|t| t.map(f)).collect()),
        }
    }
}

impl<Def> Expr<Def> {
    pub fn map<Def2>(self, f: &mut impl FnMut(Def) -> Def2) -> Expr<Def2> {
        match self {
            Expr::Local { name, index } => Expr::Local { name, index },
            Expr::Literal { kind, value } => Expr::Literal { kind, value },
            Expr::BinaryOp { op, lhs, rhs, ty } => Expr::BinaryOp {
                op,
                lhs: Box::new((*lhs).map(f)),
                rhs: Box::new((*rhs).map(f)),
                ty: ty.map(f),
            },
            Expr::Call { target, args, ty } => Expr::Call {
                target: f(target),
                args: args.into_iter().map(|a| a.map(f)).collect(),
                ty: ty.map(f),
            },
            Expr::StructLit { target, fields, ty } => Expr::StructLit {
                target: f(target),
                fields: fields.into_iter().map(|fe| fe.map(f)).collect(),
                ty: ty.map(f),
            },
            Expr::Field {
                expr,
                field_name,
                ty,
            } => Expr::Field {
                expr: Box::new((*expr).map(f)),
                field_name,
                ty: ty.map(f),
            },
            Expr::Block { stmts, tail, ty } => Expr::Block {
                stmts: stmts.into_iter().map(|s| s.map(f)).collect(),
                tail: tail.map(|e| Box::new((*e).map(f))),
                ty: ty.map(f),
            },
            Expr::Deref { expr, ty } => Expr::Deref {
                expr: Box::new((*expr).map(f)),
                ty: ty.map(f),
            },
            Expr::Ref { mutable, expr, ty } => Expr::Ref {
                mutable,
                expr: Box::new((*expr).map(f)),
                ty: ty.map(f),
            },
        }
    }
}

impl<Def> Stmt<Def> {
    pub fn map<Def2>(self, f: &mut impl FnMut(Def) -> Def2) -> Stmt<Def2> {
        match self {
            Stmt::Let {
                name,
                index,
                ty,
                init,
            } => Stmt::Let {
                name,
                index,
                ty: ty.map(f),
                init: init.map(|e| e.map(f)),
            },
            Stmt::Expr(e) => Stmt::Expr(e.map(f)),
        }
    }
}

impl<Def> FieldExpr<Def> {
    pub fn map<Def2>(self, f: &mut impl FnMut(Def) -> Def2) -> FieldExpr<Def2> {
        FieldExpr {
            name: self.name,
            value: self.value.map(f),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_json() {
        let krate: Crate<String> = Crate {
            root: Module {
                def: "root".to_string(),
                name: "".to_string(),
                items: vec![
                    Item::Fn(FnItem {
                        def: "identity".to_string(),
                        name: "identity".to_string(),
                        params: vec![Param {
                            name: "x".to_string(),
                            ty: Type::Primitive("u32".to_string()),
                        }],
                        return_ty: Type::Primitive("u32".to_string()),
                        body: None,
                    }),
                    Item::Struct(StructItem {
                        def: "Point".to_string(),
                        name: "Point".to_string(),
                        fields: vec![
                            FieldDef {
                                name: "x".to_string(),
                                ty: Type::Primitive("u32".to_string()),
                            },
                            FieldDef {
                                name: "y".to_string(),
                                ty: Type::Primitive("u32".to_string()),
                            },
                        ],
                    }),
                ],
            },
        };

        let json = serde_json::to_string_pretty(&krate).unwrap();
        let deserialized: Crate<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(krate, deserialized);
    }

    #[test]
    fn map_replaces_all_defs() {
        let krate: Crate<u32> = Crate {
            root: Module {
                def: 0,
                name: "".to_string(),
                items: vec![Item::Fn(FnItem {
                    def: 1,
                    name: "foo".to_string(),
                    params: vec![Param {
                        name: "x".to_string(),
                        ty: Type::Def {
                            target: 2,
                            type_args: vec![],
                        },
                    }],
                    return_ty: Type::Unit,
                    body: Some(Expr::Call {
                        target: 3,
                        args: vec![Expr::Local {
                            name: "x".to_string(),
                            index: 0,
                        }],
                        ty: Type::Unit,
                    }),
                })],
            },
        };

        let mut seen = Vec::new();
        let mapped = krate.map(|d| {
            seen.push(d);
            format!("def_{}", seen.len() - 1)
        });

        assert_eq!(seen, vec![0, 1, 2, 3]);
        assert_eq!(mapped.root.def, "def_0");
        match &mapped.root.items[0] {
            Item::Fn(f) => {
                assert_eq!(f.def, "def_1");
                match &f.params[0].ty {
                    Type::Def { target, .. } => assert_eq!(target, "def_2"),
                    _ => panic!("expected Def type"),
                }
                match &f.body {
                    Some(Expr::Call { target, .. }) => assert_eq!(target, "def_3"),
                    _ => panic!("expected Call expr"),
                }
            }
            _ => panic!("expected Fn item"),
        }
    }

    #[test]
    fn map_visits_struct_fields() {
        let krate: Crate<u32> = Crate {
            root: Module {
                def: 0,
                name: "".to_string(),
                items: vec![Item::Struct(StructItem {
                    def: 1,
                    name: "S".to_string(),
                    fields: vec![FieldDef {
                        name: "f".to_string(),
                        ty: Type::Def {
                            target: 2,
                            type_args: vec![Type::Def {
                                target: 3,
                                type_args: vec![],
                            }],
                        },
                    }],
                })],
            },
        };

        let mut seen = Vec::new();
        let _ = krate.map(|d| {
            seen.push(d);
            d
        });
        assert_eq!(seen, vec![0, 1, 2, 3]);
    }

    #[test]
    fn map_visits_block_stmts_and_tail() {
        let krate: Crate<u32> = Crate {
            root: Module {
                def: 0,
                name: "".to_string(),
                items: vec![Item::Fn(FnItem {
                    def: 1,
                    name: "f".to_string(),
                    params: vec![],
                    return_ty: Type::Unit,
                    body: Some(Expr::Block {
                        stmts: vec![
                            Stmt::Let {
                                name: "x".to_string(),
                                index: 0,
                                ty: Type::Def {
                                    target: 2,
                                    type_args: vec![],
                                },
                                init: Some(Expr::Call {
                                    target: 3,
                                    args: vec![],
                                    ty: Type::Unit,
                                }),
                            },
                            Stmt::Expr(Expr::Call {
                                target: 4,
                                args: vec![],
                                ty: Type::Unit,
                            }),
                        ],
                        tail: Some(Box::new(Expr::StructLit {
                            target: 5,
                            fields: vec![FieldExpr {
                                name: "a".to_string(),
                                value: Expr::Call {
                                    target: 6,
                                    args: vec![],
                                    ty: Type::Unit,
                                },
                            }],
                            ty: Type::Def {
                                target: 7,
                                type_args: vec![],
                            },
                        })),
                        ty: Type::Def {
                            target: 8,
                            type_args: vec![],
                        },
                    }),
                })],
            },
        };

        let mut seen = Vec::new();
        let _ = krate.map(|d| {
            seen.push(d);
            d
        });
        assert_eq!(seen, vec![0, 1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn map_visits_ref_deref_and_tuple() {
        let krate: Crate<u32> = Crate {
            root: Module {
                def: 0,
                name: "".to_string(),
                items: vec![Item::Fn(FnItem {
                    def: 1,
                    name: "f".to_string(),
                    params: vec![Param {
                        name: "x".to_string(),
                        ty: Type::Ref {
                            mutable: false,
                            ty: Box::new(Type::Def {
                                target: 2,
                                type_args: vec![],
                            }),
                        },
                    }],
                    return_ty: Type::Tuple(vec![
                        Type::Def {
                            target: 3,
                            type_args: vec![],
                        },
                        Type::Primitive("u32".to_string()),
                    ]),
                    body: Some(Expr::Ref {
                        mutable: true,
                        expr: Box::new(Expr::Deref {
                            expr: Box::new(Expr::Local {
                                name: "x".to_string(),
                                index: 0,
                            }),
                            ty: Type::Def {
                                target: 4,
                                type_args: vec![],
                            },
                        }),
                        ty: Type::Ref {
                            mutable: true,
                            ty: Box::new(Type::Def {
                                target: 5,
                                type_args: vec![],
                            }),
                        },
                    }),
                })],
            },
        };

        let mut seen = Vec::new();
        let _ = krate.map(|d| {
            seen.push(d);
            d
        });
        assert_eq!(seen, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn round_trip_json_with_body() {
        let krate: Crate<String> = Crate {
            root: Module {
                def: "root".to_string(),
                name: "".to_string(),
                items: vec![Item::Fn(FnItem {
                    def: "add".to_string(),
                    name: "add".to_string(),
                    params: vec![
                        Param {
                            name: "a".to_string(),
                            ty: Type::Primitive("u32".to_string()),
                        },
                        Param {
                            name: "b".to_string(),
                            ty: Type::Primitive("u32".to_string()),
                        },
                    ],
                    return_ty: Type::Primitive("u32".to_string()),
                    body: Some(Expr::Block {
                        stmts: vec![],
                        tail: Some(Box::new(Expr::BinaryOp {
                            op: BinOp::Add,
                            lhs: Box::new(Expr::Local {
                                name: "a".to_string(),
                                index: 0,
                            }),
                            rhs: Box::new(Expr::Local {
                                name: "b".to_string(),
                                index: 1,
                            }),
                            ty: Type::Primitive("u32".to_string()),
                        })),
                        ty: Type::Primitive("u32".to_string()),
                    }),
                })],
            },
        };

        let json = serde_json::to_string_pretty(&krate).unwrap();
        let deserialized: Crate<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(krate, deserialized);
    }
}
