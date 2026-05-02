//! `rustc_proc_macro::bridge::server::Server` implementation for sage.
//!
//! Uses `proc_macro2` for token stream manipulation with a dummy `SageSpan`
//! (unit struct) because `proc_macro2::Span` doesn't implement `Eq + Hash`.
//!
//! Note: we implement the `Server` trait from `rustc_proc_macro` (the compiler's
//! internal copy of `proc_macro`), not the standard library's `proc_macro`.
//! The `Client` stored in `DeriveProcMacro` uses `rustc_proc_macro` types.

use std::ops::{Bound, Range};

use rustc_proc_macro::Delimiter;
use rustc_proc_macro::bridge::{
    self, DelimSpan, Diagnostic, ExpnGlobals, Group, Ident, Literal, Punct, TokenTree,
    server::Server,
};

/// Dummy span — we don't track span info through proc-macro expansion.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct SageSpan;

pub struct SageServer;

impl SageServer {
    pub fn new() -> Self {
        SageServer
    }
}

// -- Bridge ↔ proc_macro2 conversion --

type BridgeTokenTree = TokenTree<proc_macro2::TokenStream, SageSpan, String>;

fn bridge_to_pm2(tree: BridgeTokenTree) -> proc_macro2::TokenTree {
    match tree {
        TokenTree::Group(g) => {
            let delim = match g.delimiter {
                Delimiter::Parenthesis => proc_macro2::Delimiter::Parenthesis,
                Delimiter::Brace => proc_macro2::Delimiter::Brace,
                Delimiter::Bracket => proc_macro2::Delimiter::Bracket,
                Delimiter::None => proc_macro2::Delimiter::None,
            };
            let stream = g.stream.unwrap_or_default();
            let mut group = proc_macro2::Group::new(delim, stream);
            group.set_span(proc_macro2::Span::call_site());
            proc_macro2::TokenTree::Group(group)
        }
        TokenTree::Ident(id) => {
            let ident = if id.is_raw {
                proc_macro2::Ident::new_raw(&id.sym, proc_macro2::Span::call_site())
            } else {
                proc_macro2::Ident::new(&id.sym, proc_macro2::Span::call_site())
            };
            proc_macro2::TokenTree::Ident(ident)
        }
        TokenTree::Punct(p) => {
            let spacing = if p.joint {
                proc_macro2::Spacing::Joint
            } else {
                proc_macro2::Spacing::Alone
            };
            let mut punct = proc_macro2::Punct::new(p.ch as char, spacing);
            punct.set_span(proc_macro2::Span::call_site());
            proc_macro2::TokenTree::Punct(punct)
        }
        TokenTree::Literal(lit) => {
            let text = literal_to_string(&lit);
            let ts: proc_macro2::TokenStream = text.parse().unwrap_or_default();
            ts.into_iter()
                .next()
                .unwrap_or(proc_macro2::TokenTree::Literal(
                    proc_macro2::Literal::string(""),
                ))
        }
    }
}

fn pm2_to_bridge(tree: proc_macro2::TokenTree) -> BridgeTokenTree {
    match tree {
        proc_macro2::TokenTree::Group(g) => {
            let delimiter = match g.delimiter() {
                proc_macro2::Delimiter::Parenthesis => Delimiter::Parenthesis,
                proc_macro2::Delimiter::Brace => Delimiter::Brace,
                proc_macro2::Delimiter::Bracket => Delimiter::Bracket,
                proc_macro2::Delimiter::None => Delimiter::None,
            };
            let stream = if g.stream().is_empty() {
                None
            } else {
                Some(g.stream())
            };
            TokenTree::Group(Group {
                delimiter,
                stream,
                span: DelimSpan::from_single(SageSpan),
            })
        }
        proc_macro2::TokenTree::Ident(id) => {
            let s = id.to_string();
            let (sym, is_raw) = if let Some(stripped) = s.strip_prefix("r#") {
                (stripped.to_owned(), true)
            } else {
                (s, false)
            };
            TokenTree::Ident(Ident {
                sym,
                is_raw,
                span: SageSpan,
            })
        }
        proc_macro2::TokenTree::Punct(p) => TokenTree::Punct(Punct {
            ch: p.as_char() as u8,
            joint: p.spacing() == proc_macro2::Spacing::Joint,
            span: SageSpan,
        }),
        proc_macro2::TokenTree::Literal(lit) => {
            let s = lit.to_string();
            parse_literal_string(&s)
        }
    }
}

/// Reconstruct literal text from bridge `Literal` fields.
fn literal_to_string(lit: &Literal<SageSpan, String>) -> String {
    use bridge::LitKind;
    let mut s = match lit.kind {
        LitKind::Byte => format!("b'{}'", lit.symbol),
        LitKind::Char => format!("'{}'", lit.symbol),
        LitKind::Integer | LitKind::Float => lit.symbol.clone(),
        LitKind::Str => format!("\"{}\"", lit.symbol),
        LitKind::StrRaw(n) => {
            let hashes: String = "#".repeat(n as usize);
            format!("r{hashes}\"{}\"{hashes}", lit.symbol)
        }
        LitKind::ByteStr => format!("b\"{}\"", lit.symbol),
        LitKind::ByteStrRaw(n) => {
            let hashes: String = "#".repeat(n as usize);
            format!("br{hashes}\"{}\"{hashes}", lit.symbol)
        }
        LitKind::CStr => format!("c\"{}\"", lit.symbol),
        LitKind::CStrRaw(n) => {
            let hashes: String = "#".repeat(n as usize);
            format!("cr{hashes}\"{}\"{hashes}", lit.symbol)
        }
        LitKind::ErrWithGuar => lit.symbol.clone(),
    };
    if let Some(ref suffix) = lit.suffix {
        s.push_str(suffix);
    }
    s
}

/// Parse a literal string (from proc_macro2::Literal::to_string()) into a bridge Literal.
fn parse_literal_string(s: &str) -> BridgeTokenTree {
    use bridge::LitKind;

    let (kind, symbol, suffix_start) = if s.starts_with("b'") {
        let end = s[2..].find('\'').map(|i| i + 3).unwrap_or(s.len());
        (LitKind::Byte, &s[2..end - 1], end)
    } else if s.starts_with('\'') {
        let end = s[1..].find('\'').map(|i| i + 2).unwrap_or(s.len());
        (LitKind::Char, &s[1..end - 1], end)
    } else if s.starts_with("br") {
        let hashes = s[2..].chars().take_while(|&c| c == '#').count();
        let start = 3 + hashes;
        let end_pat = format!("\"{}", "#".repeat(hashes));
        let end = s[start..]
            .find(&end_pat)
            .map(|i| i + start)
            .unwrap_or(s.len());
        (
            LitKind::ByteStrRaw(hashes as u8),
            &s[start..end],
            end + end_pat.len(),
        )
    } else if s.starts_with("b\"") {
        let end = s[2..].rfind('"').map(|i| i + 2).unwrap_or(s.len());
        (LitKind::ByteStr, &s[2..end], end + 1)
    } else if s.starts_with("cr") {
        let hashes = s[2..].chars().take_while(|&c| c == '#').count();
        let start = 3 + hashes;
        let end_pat = format!("\"{}", "#".repeat(hashes));
        let end = s[start..]
            .find(&end_pat)
            .map(|i| i + start)
            .unwrap_or(s.len());
        (
            LitKind::CStrRaw(hashes as u8),
            &s[start..end],
            end + end_pat.len(),
        )
    } else if s.starts_with("c\"") {
        let end = s[2..].rfind('"').map(|i| i + 2).unwrap_or(s.len());
        (LitKind::CStr, &s[2..end], end + 1)
    } else if s.starts_with('r') {
        let hashes = s[1..].chars().take_while(|&c| c == '#').count();
        let start = 2 + hashes;
        let end_pat = format!("\"{}", "#".repeat(hashes));
        let end = s[start..]
            .find(&end_pat)
            .map(|i| i + start)
            .unwrap_or(s.len());
        (
            LitKind::StrRaw(hashes as u8),
            &s[start..end],
            end + end_pat.len(),
        )
    } else if s.starts_with('"') {
        let end = s[1..].rfind('"').map(|i| i + 1).unwrap_or(s.len());
        (LitKind::Str, &s[1..end], end + 1)
    } else if s.contains('.') || s.contains('e') || s.contains('E') {
        let suffix_start = find_numeric_suffix_start(s);
        (LitKind::Float, &s[..suffix_start], suffix_start)
    } else {
        let suffix_start = find_numeric_suffix_start(s);
        (LitKind::Integer, &s[..suffix_start], suffix_start)
    };

    let suffix = if suffix_start < s.len() {
        Some(s[suffix_start..].to_owned())
    } else {
        None
    };

    TokenTree::Literal(Literal {
        kind,
        symbol: symbol.to_owned(),
        suffix,
        span: SageSpan,
    })
}

/// Find where a numeric suffix starts (e.g. "42u32" → 2, "3.14f64" → 4).
fn find_numeric_suffix_start(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut i = bytes.len();
    while i > 0 && (bytes[i - 1].is_ascii_alphabetic() || bytes[i - 1] == b'_') {
        i -= 1;
    }
    if i == 0 { s.len() } else { i }
}

// -- Server trait implementation --

impl Server for SageServer {
    type TokenStream = proc_macro2::TokenStream;
    type Span = SageSpan;
    type Symbol = String;

    fn globals(&mut self) -> ExpnGlobals<SageSpan> {
        ExpnGlobals {
            def_site: SageSpan,
            call_site: SageSpan,
            mixed_site: SageSpan,
        }
    }

    fn intern_symbol(ident: &str) -> String {
        ident.to_owned()
    }

    fn with_symbol_string(symbol: &String, f: impl FnOnce(&str)) {
        f(symbol);
    }

    fn injected_env_var(&mut self, _var: &str) -> Option<String> {
        None
    }

    fn track_env_var(&mut self, _var: &str, _value: Option<&str>) {}

    fn track_path(&mut self, _path: &str) {}

    fn literal_from_str(&mut self, s: &str) -> Result<Literal<SageSpan, String>, String> {
        let ts: proc_macro2::TokenStream = s
            .parse()
            .map_err(|e: proc_macro2::LexError| e.to_string())?;
        let mut iter = ts.into_iter();
        match iter.next() {
            Some(proc_macro2::TokenTree::Literal(lit)) => {
                let text = lit.to_string();
                match parse_literal_string(&text) {
                    TokenTree::Literal(l) => Ok(l),
                    _ => Err("not a literal".into()),
                }
            }
            _ => Err("not a literal".into()),
        }
    }

    fn emit_diagnostic(&mut self, _diagnostic: Diagnostic<SageSpan>) {}

    fn ts_drop(&mut self, _stream: proc_macro2::TokenStream) {}

    fn ts_clone(&mut self, stream: &proc_macro2::TokenStream) -> proc_macro2::TokenStream {
        stream.clone()
    }

    fn ts_is_empty(&mut self, stream: &proc_macro2::TokenStream) -> bool {
        stream.is_empty()
    }

    fn ts_expand_expr(
        &mut self,
        _stream: &proc_macro2::TokenStream,
    ) -> Result<proc_macro2::TokenStream, ()> {
        Err(())
    }

    fn ts_from_str(&mut self, src: &str) -> Result<proc_macro2::TokenStream, String> {
        src.parse()
            .map_err(|e: proc_macro2::LexError| e.to_string())
    }

    fn ts_to_string(&mut self, stream: &proc_macro2::TokenStream) -> String {
        stream.to_string()
    }

    fn ts_from_token_tree(&mut self, tree: BridgeTokenTree) -> proc_macro2::TokenStream {
        let tt = bridge_to_pm2(tree);
        proc_macro2::TokenStream::from(tt)
    }

    fn ts_concat_trees(
        &mut self,
        base: Option<proc_macro2::TokenStream>,
        trees: Vec<BridgeTokenTree>,
    ) -> proc_macro2::TokenStream {
        let mut ts = base.unwrap_or_default();
        for tree in trees {
            ts.extend(std::iter::once(bridge_to_pm2(tree)));
        }
        ts
    }

    fn ts_concat_streams(
        &mut self,
        base: Option<proc_macro2::TokenStream>,
        streams: Vec<proc_macro2::TokenStream>,
    ) -> proc_macro2::TokenStream {
        let mut ts = base.unwrap_or_default();
        for stream in streams {
            ts.extend(stream);
        }
        ts
    }

    fn ts_into_trees(&mut self, stream: proc_macro2::TokenStream) -> Vec<BridgeTokenTree> {
        stream.into_iter().map(pm2_to_bridge).collect()
    }

    fn span_debug(&mut self, _span: SageSpan) -> String {
        "#0 bytes(0..0)".into()
    }

    fn span_parent(&mut self, _span: SageSpan) -> Option<SageSpan> {
        None
    }

    fn span_source(&mut self, _span: SageSpan) -> SageSpan {
        SageSpan
    }

    fn span_byte_range(&mut self, _span: SageSpan) -> Range<usize> {
        0..0
    }

    fn span_start(&mut self, _span: SageSpan) -> SageSpan {
        SageSpan
    }

    fn span_end(&mut self, _span: SageSpan) -> SageSpan {
        SageSpan
    }

    fn span_line(&mut self, _span: SageSpan) -> usize {
        0
    }

    fn span_column(&mut self, _span: SageSpan) -> usize {
        0
    }

    fn span_file(&mut self, _span: SageSpan) -> String {
        String::new()
    }

    fn span_local_file(&mut self, _span: SageSpan) -> Option<String> {
        None
    }

    fn span_join(&mut self, _span: SageSpan, _other: SageSpan) -> Option<SageSpan> {
        Some(SageSpan)
    }

    fn span_subspan(
        &mut self,
        _span: SageSpan,
        _start: Bound<usize>,
        _end: Bound<usize>,
    ) -> Option<SageSpan> {
        Some(SageSpan)
    }

    fn span_resolved_at(&mut self, _span: SageSpan, _at: SageSpan) -> SageSpan {
        SageSpan
    }

    fn span_source_text(&mut self, _span: SageSpan) -> Option<String> {
        None
    }

    fn span_save_span(&mut self, _span: SageSpan) -> usize {
        0
    }

    fn span_recover_proc_macro_span(&mut self, _id: usize) -> SageSpan {
        SageSpan
    }

    fn symbol_normalize_and_validate_ident(&mut self, string: &str) -> Result<String, ()> {
        let mut chars = string.chars();
        match chars.next() {
            Some(c) if c == '_' || c.is_alphabetic() => {}
            _ => return Err(()),
        }
        if chars.all(|c| c == '_' || c.is_alphanumeric()) {
            Ok(string.to_owned())
        } else {
            Err(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ts_round_trip() {
        let mut srv = SageServer::new();
        let ts = srv.ts_from_str("struct Foo { x: i32 }").unwrap();
        let s = srv.ts_to_string(&ts);
        assert!(!s.is_empty());
        let ts2 = srv.ts_from_str(&s).unwrap();
        assert_eq!(srv.ts_to_string(&ts), srv.ts_to_string(&ts2));
    }

    #[test]
    fn tree_round_trip() {
        let mut srv = SageServer::new();
        let ts = srv.ts_from_str("fn foo(x: u32) -> bool { true }").unwrap();
        let trees = srv.ts_into_trees(ts.clone());
        assert!(!trees.is_empty());
        let ts2 = srv.ts_concat_trees(None, trees);
        assert_eq!(srv.ts_to_string(&ts), srv.ts_to_string(&ts2));
    }

    #[test]
    fn empty_stream() {
        let mut srv = SageServer::new();
        let ts: proc_macro2::TokenStream = Default::default();
        assert!(srv.ts_is_empty(&ts));
        assert!(srv.ts_into_trees(ts).is_empty());
    }

    #[test]
    fn symbols() {
        let s = SageServer::intern_symbol("foo");
        assert_eq!(s, "foo");
        SageServer::with_symbol_string(&s, |text| assert_eq!(text, "foo"));
    }

    #[test]
    fn concat_streams() {
        let mut srv = SageServer::new();
        let a = srv.ts_from_str("struct A;").unwrap();
        let b = srv.ts_from_str("struct B;").unwrap();
        let combined = srv.ts_concat_streams(Some(a), vec![b]);
        let text = srv.ts_to_string(&combined);
        assert!(text.contains("A"));
        assert!(text.contains("B"));
    }
}
