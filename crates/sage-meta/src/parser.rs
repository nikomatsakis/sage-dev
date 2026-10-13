use crate::*;

#[derive(Clone, Debug, PartialEq, Eq)]
enum TokenKind {
    Word(String),
    Prose(String),
    Mark(String),
}
#[derive(Clone, Debug)]
struct Token {
    kind: TokenKind,
    span: Span,
}

/// Parse one crate manifest or module. Errors carry source byte ranges.
#[sage::item(crate::language::Language::parse)]
pub fn parse(file: &str, source: &str) -> Result<Document, Diagnostic> {
    let mut tokens = Vec::new();
    let mut offset = 0;
    while offset < source.len() {
        let tail = &source[offset..];
        let start = offset;
        let ch = tail.chars().next().unwrap();
        if ch.is_whitespace() {
            offset += ch.len_utf8();
            continue;
        }
        if tail.starts_with("//") {
            offset += tail.find('\n').unwrap_or(tail.len());
            continue;
        }
        let kind = if let Some(rest) = tail.strip_prefix("\"\"\"") {
            let Some(end) = rest.find("\"\"\"") else {
                return Err(Diagnostic {
                    file: file.into(),
                    span: Span {
                        start,
                        end: source.len(),
                    },
                    message: "unterminated triple-quoted prose".into(),
                });
            };
            offset += end + 6;
            TokenKind::Prose(rest[..end].into())
        } else if ch.is_ascii_alphabetic() || ch == '_' {
            let len = tail
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(tail.len());
            offset += len;
            TokenKind::Word(tail[..len].into())
        } else if tail.starts_with("::") || tail.starts_with("->") {
            offset += 2;
            TokenKind::Mark(tail[..2].into())
        } else if "{}():;.$<>,#[]".contains(ch) {
            offset += 1;
            TokenKind::Mark(ch.to_string())
        } else {
            return Err(Diagnostic {
                file: file.into(),
                span: Span {
                    start,
                    end: start + ch.len_utf8(),
                },
                message: format!("unexpected character {ch:?}"),
            });
        };
        tokens.push(Token {
            kind,
            span: Span { start, end: offset },
        });
    }
    Parser {
        file,
        tokens,
        cursor: 0,
        len: source.len(),
    }
    .document()
}

struct Parser<'a> {
    file: &'a str,
    tokens: Vec<Token>,
    cursor: usize,
    len: usize,
}
impl Parser<'_> {
    fn span(&self) -> Span {
        self.tokens.get(self.cursor).map_or(
            Span {
                start: self.len,
                end: self.len,
            },
            |t| t.span.clone(),
        )
    }
    fn error(&self, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            file: self.file.into(),
            span: self.span(),
            message: message.into(),
        }
    }
    fn at(&self, text: &str) -> bool {
        self.tokens.get(self.cursor).is_some_and(|t| match &t.kind {
            TokenKind::Word(s) | TokenKind::Mark(s) => s == text,
            TokenKind::Prose(_) => false,
        })
    }
    fn eat(&mut self, text: &str) -> bool {
        if self.at(text) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, text: &str) -> Result<(), Diagnostic> {
        if self.eat(text) {
            Ok(())
        } else {
            Err(self.error(format!("expected `{text}`")))
        }
    }
    fn word(&mut self) -> Result<String, Diagnostic> {
        match self.tokens.get(self.cursor).map(|t| &t.kind) {
            Some(TokenKind::Word(word)) => {
                let result = word.clone();
                self.cursor += 1;
                Ok(result)
            }
            _ => Err(self.error("expected identifier")),
        }
    }
    fn prose(&mut self) -> Result<String, Diagnostic> {
        match self.tokens.get(self.cursor).map(|t| &t.kind) {
            Some(TokenKind::Prose(prose)) => {
                let result = prose.clone();
                self.cursor += 1;
                Ok(result)
            }
            _ => Err(self.error("expected triple-quoted description")),
        }
    }
    fn path(&mut self) -> Result<Path, Diagnostic> {
        self.parse_path(false)
    }
    fn parse_path(&mut self, local_type: bool) -> Result<Path, Diagnostic> {
        let start = self.span().start;
        let external = self.eat("$");
        let mut root = self.word()?;
        if external {
            root.insert(0, '$');
        }
        if !local_type && !external && root != "crate" && root != "self" {
            return Err(self.error("paths must start with `crate`, `self`, or `$cratename`"));
        }
        let mut segments = Vec::new();
        if root == "self" {
            self.expect(".")?;
            segments.push(self.word()?);
        }
        while self.eat("::") {
            segments.push(self.word()?);
        }
        Ok(Path {
            root,
            segments,
            span: Span {
                start,
                end: self.tokens[self.cursor - 1].span.end,
            },
        })
    }
    fn optional_prose(&mut self) -> Result<Option<String>, Diagnostic> {
        if matches!(
            self.tokens.get(self.cursor).map(|t| &t.kind),
            Some(TokenKind::Prose(_))
        ) {
            Ok(Some(self.prose()?))
        } else {
            Ok(None)
        }
    }
    // {anchor}`meta.types.type_expressions`
    fn type_expr(&mut self) -> Result<TypeExpr, Diagnostic> {
        let start = self.span().start;
        // {anchor}`meta.types.tuples`
        let kind = if self.eat("(") {
            let items = self.type_list(")")?;
            TypeExprKind::Tuple(items)
        } else {
            let path = self.parse_path(true)?;
            let arguments = if self.eat("<") {
                self.type_list(">")?
            } else {
                Vec::new()
            };
            TypeExprKind::Named { path, arguments }
        };
        Ok(TypeExpr {
            kind,
            span: Span {
                start,
                end: self.tokens[self.cursor - 1].span.end,
            },
        })
    }
    fn type_list(&mut self, end: &str) -> Result<Vec<TypeExpr>, Diagnostic> {
        let mut items = Vec::new();
        while !self.eat(end) {
            items.push(self.type_expr()?);
            if self.eat(end) {
                break;
            }
            // {anchor}`meta.types.member_commas`
            self.expect(",")?;
        }
        Ok(items)
    }
    // {anchor}`meta.types.struct_fields`
    fn value_fields(&mut self) -> Result<Vec<ValueField>, Diagnostic> {
        let mut fields = Vec::new();
        while !self.eat("}") {
            let span = self.span();
            // {anchor}`meta.types.field_prose`
            let description = self.optional_prose()?;
            let name = self.word()?;
            self.expect(":")?;
            let ty = self.type_expr()?;
            fields.push(ValueField {
                name,
                description,
                ty,
                span,
            });
            if self.eat("}") {
                break;
            }
            // {anchor}`meta.types.member_commas`
            self.expect(",")?;
        }
        Ok(fields)
    }
    fn type_declaration(&mut self) -> Result<TypeDecl, Diagnostic> {
        let span = self.span();
        let keyword = self.word()?;
        let name = self.word()?;
        // {anchor}`meta.types.generic_parameters`
        let mut parameters = Vec::new();
        if self.eat("<") {
            parameters.push(self.word()?);
            while self.eat(",") {
                parameters.push(self.word()?);
            }
            self.expect(">")?;
        }
        // Keep the compact opaque spelling for existing leaf descriptions.
        // {anchor}`meta.types.opaque_representation`
        if keyword == "type" && !self.at("{") {
            let description = Some(self.prose()?);
            self.expect(";")?;
            return Ok(TypeDecl {
                name,
                parameters,
                description,
                shape: TypeShape::Opaque,
                span,
            });
        }
        self.expect("{")?;
        // {anchor}`meta.types.declaration_prose`
        let description = if keyword == "type" {
            Some(self.prose()?)
        } else {
            self.optional_prose()?
        };
        let shape = match keyword.as_str() {
            // {anchor}`meta.types.opaque_representation`
            "type" => {
                self.expect("}")?;
                TypeShape::Opaque
            }
            "struct" => TypeShape::Struct(self.value_fields()?),
            // {anchor}`meta.types.enum_variants`
            "enum" => {
                let mut variants = Vec::new();
                while !self.eat("}") {
                    let span = self.span();
                    let name = self.word()?;
                    // {anchor}`meta.types.variant_prose`
                    let (description, fields) = if self.eat("(") {
                        (
                            self.optional_prose()?,
                            VariantFields::Tuple(self.type_list(")")?),
                        )
                    } else if self.eat("{") {
                        (
                            self.optional_prose()?,
                            VariantFields::Named(self.value_fields()?),
                        )
                    } else {
                        (self.optional_prose()?, VariantFields::Unit)
                    };
                    variants.push(Variant {
                        name,
                        description,
                        fields,
                        span,
                    });
                    if self.eat("}") {
                        break;
                    }
                    // {anchor}`meta.types.member_commas`
                    self.expect(",")?;
                }
                TypeShape::Enum(variants)
            }
            _ => return Err(self.error("expected type, struct, or enum")),
        };
        Ok(TypeDecl {
            name,
            parameters,
            description,
            shape,
            span,
        })
    }
    fn document(mut self) -> Result<Document, Diagnostic> {
        let header = if self.eat("crate") {
            let name = self.word()?;
            self.expect("{")?;
            let purpose = self.prose()?;
            let mut dependencies = Vec::new();
            let mut modules = Vec::new();
            while !self.eat("}") {
                if self.eat("depends") {
                    self.expect("$")?;
                    dependencies.push(self.word()?);
                } else {
                    self.expect("module")?;
                    modules.push(self.word()?);
                }
                self.expect(";")?;
            }
            Header::Crate {
                name,
                purpose,
                dependencies,
                modules,
            }
        } else {
            self.expect("module")?;
            let path = self.path()?;
            self.expect(";")?;
            Header::Module(path)
        };
        let mut declarations = Vec::new();
        while self.cursor < self.tokens.len() {
            let span = self.span();
            if self.at("type") || self.at("struct") || self.at("enum") {
                declarations.push(Declaration::Type(self.type_declaration()?));
                continue;
            }
            let kind = if self.eat("abstract") {
                self.expect("class")?;
                ClassKind::Abstract
            } else if self.eat("context") {
                ClassKind::Context
            } else {
                self.expect("class")?;
                ClassKind::Class
            };
            let name = self.word()?;
            // {anchor}`meta.classes.single_inheritance`
            let base = if self.eat("extends") {
                Some(self.path()?)
            } else {
                None
            };
            self.expect("{")?;
            let description = self.prose()?;
            let mut fields = Vec::new();
            let mut methods = Vec::new();
            while !self.eat("}") {
                let span = self.span();
                // {anchor}`meta.fields.kinds`
                let field_kind = if self.eat("identity") {
                    Some(FieldKind::Identity)
                } else if self.eat("input") {
                    Some(FieldKind::Input)
                } else if self.eat("state") {
                    Some(FieldKind::State)
                } else {
                    None
                };
                if let Some(kind) = field_kind {
                    let name = self.word()?;
                    self.expect(":")?;
                    let ty = self.type_expr()?;
                    self.expect(";")?;
                    fields.push(Field {
                        name,
                        kind,
                        ty,
                        span,
                    });
                } else {
                    methods.push(self.method()?);
                }
            }
            declarations.push(Declaration::Class(Class {
                name,
                kind,
                base,
                description,
                fields,
                methods,
                span,
            }));
        }
        Ok(Document {
            file: self.file.into(),
            header,
            declarations,
        })
    }
    fn method(&mut self) -> Result<Method, Diagnostic> {
        let span = self.span();
        let is_override = self.eat("override");
        // {anchor}`meta.methods.kinds`
        let kind = if self.eat("query") {
            MethodKind::Query
        } else {
            self.expect("fn")?;
            MethodKind::Function
        };
        let name = self.word()?;
        self.expect("(")?;
        self.expect(")")?;
        self.expect("->")?;
        let result = self.type_expr()?;
        self.expect("{")?;
        // {anchor}`meta.methods.prose`
        let description = self.prose()?;
        // {anchor}`meta.envelopes.leaf`
        let envelope = if self.eat("abstract") {
            self.expect(";")?;
            None
        } else {
            self.expect("envelope")?;
            self.expect("{")?;
            let mut effects = Vec::new();
            while !self.eat("}") {
                let effect = if self.eat("invokes") {
                    let path = self.path()?;
                    self.expect("(")?;
                    self.expect(")")?;
                    Effect::Invokes(path)
                } else if self.eat("forbid") {
                    self.expect("transitive")?;
                    Effect::ForbidTransitive(self.path()?)
                } else if self.eat("reads") {
                    Effect::Reads(self.path()?)
                } else {
                    self.expect("creates")?;
                    let name = self.word()?;
                    self.expect(":")?;
                    Effect::Creates {
                        name,
                        context: self.path()?,
                    }
                };
                self.expect(";")?;
                effects.push(effect);
            }
            Some(effects)
        };
        self.expect("}")?;
        Ok(Method {
            name,
            kind,
            is_override,
            result,
            description,
            envelope,
            span,
        })
    }
}
