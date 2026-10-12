//! Named, paragraph-sized specification rules and their Rust test evidence.
//!
//! The custom Markdown role `{anchor}` followed immediately by a single-backtick
//! name starts a specification paragraph. Markdown parsing keeps examples in code
//! and HTML from accidentally becoming requirements.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::correspondence::RustSite;
use crate::{Diagnostic, Span};

#[derive(Clone, Debug)]
pub struct SpecSource {
    pub file: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SpecAnchor {
    pub name: String,
    pub file: String,
    /// The byte range of the role marker, including the quoted name.
    pub span: Span,
    /// Markdown for the rule following its marker, without surrounding whitespace.
    pub paragraph: String,
}

#[derive(Default, Debug, serde::Serialize)]
pub struct SpecIndex {
    pub anchors: BTreeMap<String, SpecAnchor>,
    pub diagnostics: Vec<Diagnostic>,
    pub documents: usize,
    #[serde(skip)]
    pub(crate) sources: BTreeMap<String, String>,
}

impl SpecIndex {
    /// Read Markdown documents beneath `md_root`, without following symlinks.
    /// File names in diagnostics are relative to that documentation root.
    pub fn load(md_root: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let mut paths = Vec::new();
        markdown_files(md_root, &mut paths)?;
        paths.sort();
        let sources = paths
            .into_iter()
            .map(|path| {
                Ok(SpecSource {
                    file: path
                        .strip_prefix(md_root)?
                        .to_string_lossy()
                        .replace('\\', "/"),
                    source: std::fs::read_to_string(path)?,
                })
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
        Ok(Self::from_sources(&sources))
    }

    /// Parse a complete documentation inventory. Anchor names are globally unique.
    pub fn from_sources(sources: &[SpecSource]) -> Self {
        let mut index = Self {
            documents: sources.len(),
            ..Self::default()
        };
        for source in sources {
            index
                .sources
                .insert(source.file.clone(), source.source.clone());
            let (anchors, diagnostics) = parse_anchors(source);
            index.diagnostics.extend(diagnostics);
            for anchor in anchors {
                if let Some(previous) = index.anchors.get(&anchor.name) {
                    index.diagnostics.push(Diagnostic {
                        file: anchor.file.clone(),
                        span: anchor.span.clone(),
                        message: format!(
                            "duplicate specification anchor `{}`; first declared in {} at byte {}",
                            anchor.name, previous.file, previous.span.start
                        ),
                    });
                } else {
                    index.anchors.insert(anchor.name.clone(), anchor);
                }
            }
        }
        index
    }

    /// Check both directions of the paragraph-to-test correspondence.
    /// This checks declared evidence, not whether the test passes or proves its rule.
    pub fn validate(&self, tests: &BTreeMap<String, Vec<RustSite>>) -> Vec<Diagnostic> {
        let mut diagnostics = self.validate_anchors(tests);
        diagnostics.extend(self.validate_test_references(tests));
        diagnostics
    }

    /// Keep Markdown and Rust diagnostics separate until their source excerpts
    /// are rendered: the two inventories use different relative-path roots.
    pub(crate) fn validate_anchors(
        &self,
        tests: &BTreeMap<String, Vec<RustSite>>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = self.diagnostics.clone();
        for anchor in self.anchors.values() {
            if tests.get(&anchor.name).is_none_or(Vec::is_empty) {
                diagnostics.push(Diagnostic {
                    file: anchor.file.clone(),
                    span: anchor.span.clone(),
                    message: format!(
                        "specification anchor `{}` has no associated Rust test",
                        anchor.name
                    ),
                });
            }
        }
        diagnostics
    }

    pub(crate) fn validate_test_references(
        &self,
        tests: &BTreeMap<String, Vec<RustSite>>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for (name, sites) in tests {
            if !self.anchors.contains_key(name) {
                for site in sites {
                    diagnostics.push(Diagnostic {
                        file: site.file.clone(),
                        span: site.annotation_span.clone(),
                        message: format!(
                            "Rust test references unknown specification anchor `{name}`"
                        ),
                    });
                }
            }
        }
        diagnostics
    }
}

/// Replace specification role markers with named, shield-style HTML permalinks.
/// Everything else, including Markdown code examples, is preserved byte-for-byte.
/// Invalid or duplicate markers are errors; test coverage belongs to `validate`.
pub fn render_anchors(source: &str) -> Result<String, Vec<Diagnostic>> {
    let index = SpecIndex::from_sources(&[SpecSource {
        file: "<markdown>".into(),
        source: source.into(),
    }]);
    if !index.diagnostics.is_empty() {
        return Err(index.diagnostics);
    }
    let mut anchors = index.anchors.values().collect::<Vec<_>>();
    anchors.sort_by_key(|anchor| anchor.span.start);
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    for anchor in anchors {
        output.push_str(&source[cursor..anchor.span.start]);
        // Names are ASCII identifiers separated by dots, so no HTML escaping is needed.
        output.push_str(&format!(
            "<a class=\"spec-anchor\" id=\"{0}\" href=\"#{0}\" aria-label=\"Specification {0}\" title=\"Permalink to {0}\"><span class=\"spec-anchor-label\" aria-hidden=\"true\">spec</span><span class=\"spec-anchor-name\">{0}</span></a>",
            anchor.name
        ));
        cursor = anchor.span.end;
    }
    output.push_str(&source[cursor..]);
    Ok(output)
}

/// Names have ASCII identifier components separated by dots.
pub fn valid_anchor_name(name: &str) -> bool {
    name.split('.').all(|component| {
        let mut chars = component.chars();
        chars
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
            && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    })
}

fn markdown_files(directory: &Path, paths: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            markdown_files(&entry.path(), paths)?;
        } else if kind.is_file() && entry.path().extension().is_some_and(|ext| ext == "md") {
            paths.push(entry.path());
        }
    }
    Ok(())
}

fn parse_anchors(source: &SpecSource) -> (Vec<SpecAnchor>, Vec<Diagnostic>) {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let events = Parser::new_ext(&source.source, options)
        .into_offset_iter()
        .collect::<Vec<_>>();
    let mut anchors = Vec::new();
    let mut diagnostics = Vec::new();
    let mut paragraph: Option<Range<usize>> = None;
    let mut in_code_block = false;
    let mut html_tags = Vec::new();
    for (event_index, (event, range)) in events.iter().enumerate() {
        match event {
            Event::Start(Tag::Paragraph) => paragraph = Some(range.clone()),
            Event::End(TagEnd::Paragraph) => paragraph = None,
            Event::Start(Tag::CodeBlock(_)) => in_code_block = true,
            Event::End(TagEnd::CodeBlock) => in_code_block = false,
            Event::InlineHtml(html) => update_html_tags(html, &mut html_tags),
            Event::Text(_) if !in_code_block && html_tags.is_empty() => {
                let text = &source.source[range.clone()];
                for (relative, _) in text.match_indices("{anchor}") {
                    let start = range.start + relative;
                    if is_escaped(&source.source, start) {
                        continue;
                    }
                    let error = |message: &str| Diagnostic {
                        file: source.file.clone(),
                        span: Span {
                            start,
                            end: start + "{anchor}".len(),
                        },
                        message: message.into(),
                    };
                    let Some(paragraph) = &paragraph else {
                        diagnostics.push(error("a specification anchor must start a paragraph"));
                        continue;
                    };
                    if !source.source[paragraph.start..start].trim().is_empty() {
                        diagnostics.push(error("a specification anchor must start its paragraph"));
                        continue;
                    }
                    let Some((Event::Code(name), code_range)) = events.get(event_index + 1) else {
                        diagnostics
                            .push(error("expected {anchor}`name` with a single-backtick name"));
                        continue;
                    };
                    if code_range.start != start + "{anchor}".len()
                        || &source.source[code_range.clone()] != format!("`{name}`")
                    {
                        diagnostics
                            .push(error("expected {anchor}`name` with a single-backtick name"));
                        continue;
                    }
                    if !valid_anchor_name(name) {
                        diagnostics.push(error(
                            "anchor names must be dot-separated ASCII identifiers",
                        ));
                        continue;
                    }
                    let rule = &source.source[code_range.end..paragraph.end];
                    if !rule.starts_with(char::is_whitespace) {
                        diagnostics
                            .push(error("separate the anchor from its rule with whitespace"));
                        continue;
                    }
                    let has_prose = events[event_index + 2..]
                        .iter()
                        .take_while(|(_, range)| range.start < paragraph.end)
                        .any(|(event, _)| match event {
                            Event::Text(text) | Event::Code(text) => !text.trim().is_empty(),
                            _ => false,
                        });
                    if !has_prose {
                        diagnostics.push(error("a specification anchor must have a nonempty rule in the same paragraph"));
                        continue;
                    }
                    anchors.push(SpecAnchor {
                        name: name.to_string(),
                        file: source.file.clone(),
                        span: Span {
                            start,
                            end: code_range.end,
                        },
                        paragraph: rule.trim().into(),
                    });
                }
            }
            _ => {}
        }
    }
    (anchors, diagnostics)
}

fn is_escaped(source: &str, offset: usize) -> bool {
    source.as_bytes()[..offset]
        .iter()
        .rev()
        .take_while(|&&byte| byte == b'\\')
        .count()
        % 2
        == 1
}

// pulldown-cmark handles raw HTML blocks itself. Track inline element boundaries
// so role-looking text inside `<span>...</span>` is also treated as HTML content.
fn update_html_tags(html: &str, tags: &mut Vec<String>) {
    let Some(tag) = html.strip_prefix('<') else {
        return;
    };
    let closing = tag.starts_with('/');
    let tag = tag.strip_prefix('/').unwrap_or(tag);
    let name = tag
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.is_empty() {
        return;
    }
    if closing {
        if let Some(index) = tags.iter().rposition(|open| open == &name) {
            tags.truncate(index);
        }
    } else if !html.trim_end().ends_with("/>")
        && !matches!(
            name.as_str(),
            "area"
                | "base"
                | "br"
                | "col"
                | "embed"
                | "hr"
                | "img"
                | "input"
                | "link"
                | "meta"
                | "param"
                | "source"
                | "track"
                | "wbr"
        )
    {
        tags.push(name);
    }
}
