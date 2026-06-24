use sage_stash::{Ptr, Stash};

use crate::cst::Mutability;
use crate::cst::expr::*;
use crate::name::Name;
use crate::span::RelativeSpan;

use super::Parser;

impl<'a, 'db> Parser<'a, 'db> {
    pub(super) fn parse_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Ptr<ExprCst<'db>> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };

        let kind = match node.kind() {
            "integer_literal" => {
                let text = self.text[node.byte_range()].to_owned();
                ExprCstKind::Literal(Literal::Int(Name::new(self.db, text)))
            }
            "float_literal" => {
                let text = self.text[node.byte_range()].to_owned();
                ExprCstKind::Literal(Literal::Float(Name::new(self.db, text)))
            }
            "string_literal" | "raw_string_literal" => {
                let text = self.text[node.byte_range()].to_owned();
                ExprCstKind::Literal(Literal::String(Name::new(self.db, text)))
            }
            "char_literal" => {
                let text = self.text[node.byte_range()].to_owned();
                ExprCstKind::Literal(Literal::Char(Name::new(self.db, text)))
            }
            "boolean_literal" => {
                let val = &self.text[node.byte_range()] == "true";
                ExprCstKind::Literal(Literal::Bool(val))
            }
            "identifier" | "scoped_identifier" | "self" => {
                let path = self.parse_path(stash, node, item_start);
                ExprCstKind::Path(path)
            }
            "block" => self.parse_block_expr(stash, node, item_start),
            "call_expression" => self.parse_call_expr(stash, node, item_start),
            "field_expression" => self.parse_field_expr(stash, node, item_start),
            "method_call_expression" => self.parse_method_call_expr(stash, node, item_start),
            "binary_expression" => self.parse_binary_expr(stash, node, item_start),
            "unary_expression" => self.parse_unary_expr(stash, node, item_start),
            "reference_expression" => self.parse_ref_expr(stash, node, item_start),
            "if_expression" => self.parse_if_expr(stash, node, item_start),
            "match_expression" => self.parse_match_expr(stash, node, item_start),
            "loop_expression" => self.parse_loop_expr(stash, node, item_start),
            "while_expression" => self.parse_while_expr(stash, node, item_start),
            "for_expression" => self.parse_for_expr(stash, node, item_start),
            "break_expression" => {
                let val = node
                    .child_by_field_name("value")
                    .map(|n| self.parse_expr(stash, n, item_start));
                ExprCstKind::Break(val)
            }
            "continue_expression" => ExprCstKind::Continue,
            "return_expression" => {
                let val = node
                    .child_by_field_name("value")
                    .or_else(|| {
                        let mut c = node.walk();
                        node.children(&mut c)
                            .find(|ch| ch.is_named() && ch.kind() != "return")
                    })
                    .map(|n| self.parse_expr(stash, n, item_start));
                ExprCstKind::Return(val)
            }
            "assignment_expression" => self.parse_assign_expr(stash, node, item_start),
            "await_expression" => {
                let inner = node
                    .child(0)
                    .map(|n| self.parse_expr(stash, n, item_start))
                    .unwrap_or_else(|| {
                        stash.alloc(ExprCst {
                            kind: ExprCstKind::Missing,
                            span,
                        })
                    });
                ExprCstKind::Await(inner)
            }
            "try_expression" => {
                let inner = node
                    .child(0)
                    .map(|n| self.parse_expr(stash, n, item_start))
                    .unwrap_or_else(|| {
                        stash.alloc(ExprCst {
                            kind: ExprCstKind::Missing,
                            span,
                        })
                    });
                ExprCstKind::Try(inner)
            }
            "closure_expression" => self.parse_closure_expr(stash, node, item_start),
            "tuple_expression" | "parenthesized_expression" => {
                self.parse_tuple_expr(stash, node, item_start)
            }
            "array_expression" => self.parse_array_expr(stash, node, item_start),
            "index_expression" => self.parse_index_expr(stash, node, item_start),
            "type_cast_expression" => self.parse_cast_expr(stash, node, item_start),
            "struct_expression" => self.parse_struct_lit_expr(stash, node, item_start),
            "range_expression" => self.parse_range_expr(stash, node, item_start),
            "let_condition" | "if_let_expression" => {
                // if-let handled in parse_if_expr; fallback
                ExprCstKind::Missing
            }
            "unit_expression" => ExprCstKind::Tuple(stash.alloc_slice(&[])),
            _ => ExprCstKind::Missing,
        };

        stash.alloc(ExprCst { kind, span })
    }

    fn parse_block_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let mut stmts = Vec::new();
        let mut tail: Option<Ptr<ExprCst<'db>>> = None;
        let mut cursor = node.walk();

        let children: Vec<_> = node.children(&mut cursor).collect();
        let last_expr_idx = children
            .iter()
            .rposition(|c| c.kind() == "expression_statement");
        let has_trailing_semi = last_expr_idx.is_some_and(|i| {
            children[i + 1..]
                .iter()
                .any(|c| c.kind() == "empty_statement" || c.kind() == ";")
        });

        for (i, child) in children.iter().enumerate() {
            match child.kind() {
                "let_declaration" => {
                    stmts.push(self.parse_let_stmt(stash, *child, item_start));
                }
                "expression_statement" => {
                    if let Some(expr_node) = child.child(0) {
                        if expr_node.kind() != ";" {
                            if Some(i) == last_expr_idx && !has_trailing_semi {
                                tail = Some(self.parse_expr(stash, expr_node, item_start));
                            } else {
                                let expr = self.parse_expr(stash, expr_node, item_start);
                                let stmt_span = RelativeSpan {
                                    start: child.start_byte() as u32 - item_start,
                                    end: child.end_byte() as u32 - item_start,
                                };
                                stmts.push(StmtCst {
                                    kind: StmtCstKind::Expr(expr),
                                    span: stmt_span,
                                });
                            }
                        }
                    }
                }
                "{" | "}" | "empty_statement" => {}
                _ if child.is_named() => {
                    tail = Some(self.parse_expr(stash, *child, item_start));
                }
                _ => {}
            }
        }

        let stmts_slice = stash.alloc_slice(&stmts);
        ExprCstKind::Block(stmts_slice, tail)
    }

    fn parse_let_stmt(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> StmtCst<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };

        let pat = node
            .child_by_field_name("pattern")
            .map(|n| self.parse_pat(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(PatCst {
                    kind: PatCstKind::Missing,
                    span,
                })
            });

        let ty_ann = node
            .child_by_field_name("type")
            .map(|n| self.parse_type(stash, n, item_start));

        let init = node
            .child_by_field_name("value")
            .map(|n| self.parse_expr(stash, n, item_start));

        StmtCst {
            kind: StmtCstKind::Let(pat, ty_ann, init),
            span,
        }
    }

    fn parse_call_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };
        let func = node
            .child_by_field_name("function")
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });

        let mut args = Vec::new();
        if let Some(arg_list) = node.child_by_field_name("arguments") {
            let mut cursor = arg_list.walk();
            for child in arg_list.children(&mut cursor) {
                if child.is_named() && child.kind() != "," {
                    let arg = self.parse_expr(stash, child, item_start);
                    args.push(stash[arg]);
                }
            }
        }
        ExprCstKind::Call(func, stash.alloc_slice(&args))
    }

    fn parse_field_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };
        let obj = node
            .child_by_field_name("value")
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });
        let field_name = node
            .child_by_field_name("field")
            .map(|n| Name::new(self.db, self.text[n.byte_range()].to_owned()))
            .unwrap_or_else(|| Name::new(self.db, "_".to_owned()));
        ExprCstKind::Field(obj, field_name)
    }

    fn parse_method_call_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };
        let obj = node
            .child_by_field_name("value")
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });
        let method = node
            .child_by_field_name("name")
            .map(|n| Name::new(self.db, self.text[n.byte_range()].to_owned()))
            .unwrap_or_else(|| Name::new(self.db, "_".to_owned()));
        let mut args = Vec::new();
        if let Some(arg_list) = node.child_by_field_name("arguments") {
            let mut cursor = arg_list.walk();
            for child in arg_list.children(&mut cursor) {
                if child.is_named() && child.kind() != "," {
                    let arg = self.parse_expr(stash, child, item_start);
                    args.push(stash[arg]);
                }
            }
        }
        ExprCstKind::MethodCall(obj, method, stash.alloc_slice(&args))
    }

    fn parse_binary_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };
        let lhs = node
            .child_by_field_name("left")
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });
        let rhs = node
            .child_by_field_name("right")
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });

        let op_text = node
            .child_by_field_name("operator")
            .map(|n| &self.text[n.byte_range()])
            .unwrap_or("");

        let op = match op_text {
            "+" => BinaryOp::Add,
            "-" => BinaryOp::Sub,
            "*" => BinaryOp::Mul,
            "/" => BinaryOp::Div,
            "%" => BinaryOp::Rem,
            "&&" => BinaryOp::And,
            "||" => BinaryOp::Or,
            "&" => BinaryOp::BitAnd,
            "|" => BinaryOp::BitOr,
            "^" => BinaryOp::BitXor,
            "<<" => BinaryOp::Shl,
            ">>" => BinaryOp::Shr,
            "==" => BinaryOp::Eq,
            "!=" => BinaryOp::Ne,
            "<" => BinaryOp::Lt,
            "<=" => BinaryOp::Le,
            ">" => BinaryOp::Gt,
            ">=" => BinaryOp::Ge,
            _ => BinaryOp::Add,
        };

        ExprCstKind::Binary(lhs, op, rhs)
    }

    fn parse_unary_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };
        let mut op = UnaryOp::Neg;
        let mut operand_node = None;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "!" => op = UnaryOp::Not,
                "-" => op = UnaryOp::Neg,
                "*" => op = UnaryOp::Deref,
                _ if child.is_named() => operand_node = Some(child),
                _ => {}
            }
        }
        let operand = operand_node
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });
        ExprCstKind::Unary(op, operand)
    }

    fn parse_ref_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };
        let mut mutability = Mutability::Shared;
        let mut inner_node = None;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "mutable_specifier" => mutability = Mutability::Mut,
                "&" => {}
                _ if child.is_named() => inner_node = Some(child),
                _ => {}
            }
        }
        let inner = inner_node
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });
        ExprCstKind::Ref(inner, mutability)
    }

    fn parse_if_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };
        let cond = node
            .child_by_field_name("condition")
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });
        let then = node
            .child_by_field_name("consequence")
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });
        let else_ = node
            .child_by_field_name("alternative")
            .map(|n| self.parse_expr(stash, n, item_start));

        ExprCstKind::If(cond, then, else_)
    }

    fn parse_match_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };
        let scrutinee = node
            .child_by_field_name("value")
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });

        let mut arms = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if child.kind() == "match_arm" {
                    arms.push(self.parse_match_arm(stash, child, item_start));
                }
            }
        }

        ExprCstKind::Match(scrutinee, stash.alloc_slice(&arms))
    }

    fn parse_match_arm(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> MatchArmCst<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };
        let pat = node
            .child_by_field_name("pattern")
            .map(|n| self.parse_pat(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(PatCst {
                    kind: PatCstKind::Missing,
                    span,
                })
            });
        let guard = node
            .child_by_field_name("guard")
            .and_then(|g| g.child_by_field_name("condition"))
            .map(|n| self.parse_expr(stash, n, item_start));
        let body = node
            .child_by_field_name("value")
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });

        MatchArmCst {
            pat,
            guard,
            body,
            span,
        }
    }

    fn parse_loop_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };
        let body = node
            .child_by_field_name("body")
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });
        ExprCstKind::Loop(body)
    }

    fn parse_while_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };
        let cond = node
            .child_by_field_name("condition")
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });
        let body = node
            .child_by_field_name("body")
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });
        ExprCstKind::While(cond, body)
    }

    fn parse_for_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };
        let pat = node
            .child_by_field_name("pattern")
            .map(|n| self.parse_pat(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(PatCst {
                    kind: PatCstKind::Missing,
                    span,
                })
            });
        let iter = node
            .child_by_field_name("value")
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });
        let body = node
            .child_by_field_name("body")
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });
        ExprCstKind::For(pat, iter, body)
    }

    fn parse_assign_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };
        let lhs = node
            .child_by_field_name("left")
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });
        let rhs = node
            .child_by_field_name("right")
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });
        ExprCstKind::Assign(lhs, rhs)
    }

    fn parse_closure_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };
        let mut params = Vec::new();
        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                if child.kind() == "closure_parameter" || child.kind() == "parameter" {
                    let p_span = RelativeSpan {
                        start: child.start_byte() as u32 - item_start,
                        end: child.end_byte() as u32 - item_start,
                    };
                    let pat = child
                        .child_by_field_name("pattern")
                        .or_else(|| child.child(0).filter(|c| c.is_named()))
                        .map(|n| self.parse_pat(stash, n, item_start))
                        .unwrap_or_else(|| {
                            stash.alloc(PatCst {
                                kind: PatCstKind::Missing,
                                span: p_span,
                            })
                        });
                    let ty = child
                        .child_by_field_name("type")
                        .map(|n| self.parse_type(stash, n, item_start));
                    params.push(ClosureParamCst {
                        pat,
                        ty,
                        span: p_span,
                    });
                }
            }
        }
        let body = node
            .child_by_field_name("body")
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });
        ExprCstKind::Closure(stash.alloc_slice(&params), body)
    }

    fn parse_tuple_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let mut elems = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.is_named() && child.kind() != "," {
                let e = self.parse_expr(stash, child, item_start);
                elems.push(stash[e]);
            }
        }
        if elems.len() == 1 && node.kind() == "parenthesized_expression" {
            // Parenthesized single expression, not a tuple — return the inner expr.
            // We already allocated it though, so re-wrap.
            let inner = stash.alloc(elems[0]);
            return stash[inner].kind;
        }
        ExprCstKind::Tuple(stash.alloc_slice(&elems))
    }

    fn parse_array_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let mut elems = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.is_named() && child.kind() != "," {
                let e = self.parse_expr(stash, child, item_start);
                elems.push(stash[e]);
            }
        }
        ExprCstKind::Array(stash.alloc_slice(&elems))
    }

    fn parse_index_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };
        let obj = node
            .child(0)
            .filter(|c| c.is_named())
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });
        let idx = node
            .child(2)
            .filter(|c| c.is_named())
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });
        ExprCstKind::Index(obj, idx)
    }

    fn parse_cast_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };
        let expr = node
            .child_by_field_name("value")
            .map(|n| self.parse_expr(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(ExprCst {
                    kind: ExprCstKind::Missing,
                    span,
                })
            });
        let ty = node
            .child_by_field_name("type")
            .map(|n| self.parse_type(stash, n, item_start))
            .unwrap_or_else(|| {
                stash.alloc(crate::cst::ty::TypeCst {
                    kind: crate::cst::ty::TypeCstKind::Error,
                    span,
                })
            });
        ExprCstKind::Cast(expr, ty)
    }

    fn parse_struct_lit_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let name_node = node.child_by_field_name("name");
        let path = name_node
            .map(|n| self.parse_path(stash, n, item_start))
            .unwrap_or_else(|| self.parse_path(stash, node, item_start));

        let mut fields = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if child.kind() == "field_initializer" {
                    let f_span = RelativeSpan {
                        start: child.start_byte() as u32 - item_start,
                        end: child.end_byte() as u32 - item_start,
                    };
                    let name = child
                        .child_by_field_name("field")
                        .or_else(|| child.child_by_field_name("name"))
                        .map(|n| Name::new(self.db, self.text[n.byte_range()].to_owned()))
                        .unwrap_or_else(|| Name::new(self.db, "_".to_owned()));
                    let value = child
                        .child_by_field_name("value")
                        .map(|n| self.parse_expr(stash, n, item_start))
                        .unwrap_or_else(|| {
                            // Shorthand `Foo { x }` — value is the same name as path
                            let p = self.parse_path(stash, child, item_start);
                            stash.alloc(ExprCst {
                                kind: ExprCstKind::Path(p),
                                span: f_span,
                            })
                        });
                    fields.push(FieldInitCst {
                        name,
                        value,
                        span: f_span,
                    });
                } else if child.kind() == "shorthand_field_initializer" {
                    let f_span = RelativeSpan {
                        start: child.start_byte() as u32 - item_start,
                        end: child.end_byte() as u32 - item_start,
                    };
                    let name = Name::new(self.db, self.text[child.byte_range()].to_owned());
                    let p = self.parse_path(stash, child, item_start);
                    let value = stash.alloc(ExprCst {
                        kind: ExprCstKind::Path(p),
                        span: f_span,
                    });
                    fields.push(FieldInitCst {
                        name,
                        value,
                        span: f_span,
                    });
                }
            }
        }

        ExprCstKind::StructLit(path, stash.alloc_slice(&fields))
    }

    fn parse_range_expr(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> ExprCstKind<'db> {
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        let mut lo = None;
        let mut hi = None;
        let mut saw_dots = false;
        for child in &children {
            if child.kind().contains("..") {
                saw_dots = true;
            } else if child.is_named() {
                if !saw_dots {
                    lo = Some(self.parse_expr(stash, *child, item_start));
                } else {
                    hi = Some(self.parse_expr(stash, *child, item_start));
                }
            }
        }
        ExprCstKind::Range(lo, hi)
    }

    pub(super) fn parse_pat(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> Ptr<PatCst<'db>> {
        let span = RelativeSpan {
            start: node.start_byte() as u32 - item_start,
            end: node.end_byte() as u32 - item_start,
        };

        let kind = match node.kind() {
            "_" => PatCstKind::Wildcard,
            "identifier" => {
                let name = Name::new(self.db, self.text[node.byte_range()].to_owned());
                PatCstKind::Bind(name, Mutability::Shared)
            }
            "mut_pattern" => {
                let inner = node.child(1).filter(|c| c.is_named());
                match inner {
                    Some(n) => {
                        let name = Name::new(self.db, self.text[n.byte_range()].to_owned());
                        PatCstKind::Bind(name, Mutability::Mut)
                    }
                    None => PatCstKind::Missing,
                }
            }
            "scoped_identifier" => {
                let path = self.parse_path(stash, node, item_start);
                PatCstKind::Path(path)
            }
            "tuple_pattern" => {
                let mut pats = Vec::new();
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.is_named() && child.kind() != "," {
                        let p = self.parse_pat(stash, child, item_start);
                        pats.push(stash[p]);
                    }
                }
                PatCstKind::Tuple(stash.alloc_slice(&pats))
            }
            "struct_pattern" => self.parse_struct_pat(stash, node, item_start),
            "tuple_struct_pattern" => self.parse_tuple_struct_pat(stash, node, item_start),
            "reference_pattern" => {
                let mut mutability = Mutability::Shared;
                let mut inner_node = None;
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    match child.kind() {
                        "mutable_specifier" => mutability = Mutability::Mut,
                        "&" => {}
                        _ if child.is_named() => inner_node = Some(child),
                        _ => {}
                    }
                }
                match inner_node {
                    Some(n) => {
                        let inner = self.parse_pat(stash, n, item_start);
                        PatCstKind::Ref(inner, mutability)
                    }
                    None => PatCstKind::Missing,
                }
            }
            "or_pattern" => {
                let mut pats = Vec::new();
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.is_named() && child.kind() != "|" {
                        let p = self.parse_pat(stash, child, item_start);
                        pats.push(stash[p]);
                    }
                }
                PatCstKind::Or(stash.alloc_slice(&pats))
            }
            "integer_literal" => {
                let text = self.text[node.byte_range()].to_owned();
                PatCstKind::Literal(Literal::Int(Name::new(self.db, text)))
            }
            "string_literal" => {
                let text = self.text[node.byte_range()].to_owned();
                PatCstKind::Literal(Literal::String(Name::new(self.db, text)))
            }
            "boolean_literal" => {
                let val = &self.text[node.byte_range()] == "true";
                PatCstKind::Literal(Literal::Bool(val))
            }
            "rest_pattern" | ".." => PatCstKind::Rest,
            _ => PatCstKind::Missing,
        };

        stash.alloc(PatCst { kind, span })
    }

    fn parse_struct_pat(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> PatCstKind<'db> {
        let type_node = node.child_by_field_name("type");
        let path = type_node
            .map(|n| self.parse_path(stash, n, item_start))
            .unwrap_or_else(|| self.parse_path(stash, node, item_start));

        let mut fields = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "field_pattern" {
                let f_span = RelativeSpan {
                    start: child.start_byte() as u32 - item_start,
                    end: child.end_byte() as u32 - item_start,
                };
                let name = child
                    .child_by_field_name("name")
                    .map(|n| Name::new(self.db, self.text[n.byte_range()].to_owned()))
                    .unwrap_or_else(|| Name::new(self.db, "_".to_owned()));
                let pat = child
                    .child_by_field_name("pattern")
                    .map(|n| self.parse_pat(stash, n, item_start))
                    .unwrap_or_else(|| {
                        stash.alloc(PatCst {
                            kind: PatCstKind::Bind(name, Mutability::Shared),
                            span: f_span,
                        })
                    });
                fields.push(FieldPatCst {
                    name,
                    pat,
                    span: f_span,
                });
            }
        }

        PatCstKind::Struct(path, stash.alloc_slice(&fields))
    }

    fn parse_tuple_struct_pat(
        &self,
        stash: &mut Stash,
        node: tree_sitter::Node<'a>,
        item_start: u32,
    ) -> PatCstKind<'db> {
        let type_node = node.child_by_field_name("type");
        let path = type_node
            .map(|n| self.parse_path(stash, n, item_start))
            .unwrap_or_else(|| self.parse_path(stash, node, item_start));

        let mut pats = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.is_named()
                && child.kind() != ","
                && child.kind() != "("
                && child.kind() != ")"
                && !std::ptr::eq(
                    &child as *const _,
                    type_node
                        .as_ref()
                        .map_or(std::ptr::null(), |n| n as *const _),
                )
            {
                // Skip the type node itself
                if type_node.is_some_and(|t| t.id() == child.id()) {
                    continue;
                }
                let p = self.parse_pat(stash, child, item_start);
                pats.push(stash[p]);
            }
        }

        PatCstKind::TupleStruct(path, stash.alloc_slice(&pats))
    }
}
