use sage_stash::{Slice, Stash};

use crate::Db;
use crate::cst::generics::GenericParamCst;
use crate::cst::where_clause::WhereClauseCst;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Delimiter {
    Paren,
    Bracket,
    Brace,
    Angle,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Punct {
    Comma,
    Semi,
    Colon,
    ColonColon,
    Arrow,
    FatArrow,
    Dot,
    Amp,
    Plus,
    Bang,
    Hash,
    HashBang,
    Eq,
    Underscore,
    SemiUnderscore,
}

pub trait TokenSink {
    fn ident(&mut self, name: &str);
    fn punct(&mut self, p: Punct);
    fn literal(&mut self, text: &str);
    fn group(&mut self, delim: Delimiter, f: &mut dyn FnMut(&mut dyn TokenSink));
    /// Emit raw pre-formatted text (e.g., a function body from source).
    fn raw(&mut self, text: &str);
}

pub struct TokenCtx<'a, 'db> {
    pub db: &'db dyn Db,
    pub stash: &'a Stash,
    /// The full source text this CST was parsed from. Used to emit
    /// expression/body spans as raw text.
    pub source_text: &'a str,
}

pub trait ToTokens<'db> {
    fn to_tokens(&self, ctx: &TokenCtx<'_, 'db>, sink: &mut dyn TokenSink);
}

// ---------------------------------------------------------------------------
// StringSink: concatenates tokens into a string with spacing
// ---------------------------------------------------------------------------

pub struct StringSink {
    buf: String,
    needs_space: bool,
}

impl StringSink {
    pub fn new() -> Self {
        Self {
            buf: String::new(),
            needs_space: false,
        }
    }

    pub fn into_string(self) -> String {
        self.buf
    }

    fn space_before(&mut self) {
        if self.needs_space {
            self.buf.push(' ');
        }
    }
}

impl TokenSink for StringSink {
    fn ident(&mut self, name: &str) {
        self.space_before();
        self.buf.push_str(name);
        self.needs_space = true;
    }

    fn punct(&mut self, p: Punct) {
        match p {
            Punct::Comma => {
                self.buf.push(',');
                self.needs_space = true;
            }
            Punct::Semi => {
                self.buf.push(';');
                self.needs_space = true;
            }
            Punct::Colon => {
                self.buf.push(':');
                self.needs_space = true;
            }
            Punct::ColonColon => {
                self.buf.push_str("::");
                self.needs_space = false;
            }
            Punct::Arrow => {
                self.space_before();
                self.buf.push_str("->");
                self.needs_space = true;
            }
            Punct::FatArrow => {
                self.space_before();
                self.buf.push_str("=>");
                self.needs_space = true;
            }
            Punct::Dot => {
                self.buf.push('.');
                self.needs_space = false;
            }
            Punct::Amp => {
                self.space_before();
                self.buf.push('&');
                self.needs_space = false;
            }
            Punct::Plus => {
                self.space_before();
                self.buf.push('+');
                self.needs_space = true;
            }
            Punct::Bang => {
                self.buf.push('!');
                self.needs_space = false;
            }
            Punct::Hash => {
                self.space_before();
                self.buf.push('#');
                self.needs_space = false;
            }
            Punct::HashBang => {
                self.space_before();
                self.buf.push_str("#!");
                self.needs_space = false;
            }
            Punct::Eq => {
                self.space_before();
                self.buf.push('=');
                self.needs_space = true;
            }
            Punct::Underscore => {
                self.space_before();
                self.buf.push('_');
                self.needs_space = true;
            }
            Punct::SemiUnderscore => {
                self.buf.push_str("; _");
                self.needs_space = false;
            }
        }
    }

    fn literal(&mut self, text: &str) {
        self.space_before();
        self.buf.push_str(text);
        self.needs_space = true;
    }

    fn group(&mut self, delim: Delimiter, f: &mut dyn FnMut(&mut dyn TokenSink)) {
        self.space_before();
        self.buf.push_str(delim.open());
        self.needs_space = false;
        f(self);
        self.buf.push_str(delim.close());
        self.needs_space = true;
    }

    fn raw(&mut self, text: &str) {
        self.space_before();
        self.buf.push_str(text);
        self.needs_space = true;
    }
}

impl Delimiter {
    fn open(self) -> &'static str {
        match self {
            Delimiter::Paren => "(",
            Delimiter::Bracket => "[",
            Delimiter::Brace => "{",
            Delimiter::Angle => "<",
        }
    }

    fn close(self) -> &'static str {
        match self {
            Delimiter::Paren => ")",
            Delimiter::Bracket => "]",
            Delimiter::Brace => "}",
            Delimiter::Angle => ">",
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers for emitting common patterns
// ---------------------------------------------------------------------------

pub fn emit_generics<'db>(
    ctx: &TokenCtx<'_, 'db>,
    sink: &mut dyn TokenSink,
    generics: Slice<GenericParamCst<'db>>,
) {
    let params = &ctx.stash[generics];
    if !params.is_empty() {
        sink.group(Delimiter::Angle, &mut |s| {
            for (i, param) in params.iter().enumerate() {
                if i > 0 {
                    s.punct(Punct::Comma);
                }
                param.to_tokens(ctx, s);
            }
        });
    }
}

pub fn emit_where_clauses<'db>(
    ctx: &TokenCtx<'_, 'db>,
    sink: &mut dyn TokenSink,
    clauses: Slice<WhereClauseCst<'db>>,
) {
    let wc = &ctx.stash[clauses];
    if !wc.is_empty() {
        sink.ident("where");
        for (i, clause) in wc.iter().enumerate() {
            if i > 0 {
                sink.punct(Punct::Comma);
            }
            clause.to_tokens(ctx, sink);
        }
    }
}

pub fn emit_comma_sep<'db, T: ToTokens<'db>>(
    ctx: &TokenCtx<'_, 'db>,
    sink: &mut dyn TokenSink,
    items: &[T],
) {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            sink.punct(Punct::Comma);
        }
        item.to_tokens(ctx, sink);
    }
}

pub fn emit_attrs_filtered<'db>(
    ctx: &TokenCtx<'_, 'db>,
    sink: &mut dyn TokenSink,
    attrs: Slice<crate::cst::attrs::AttrCst<'db>>,
    skip: &dyn Fn(usize) -> bool,
) {
    for (i, attr) in ctx.stash[attrs].iter().enumerate() {
        if !skip(i) {
            attr.to_tokens(ctx, sink);
        }
    }
}

/// Emit a raw span from the source text.
pub fn emit_span_raw(ctx: &TokenCtx<'_, '_>, sink: &mut dyn TokenSink, start: u32, end: u32) {
    let s = start as usize;
    let e = end as usize;
    if s < ctx.source_text.len() && e <= ctx.source_text.len() {
        sink.raw(&ctx.source_text[s..e]);
    }
}
