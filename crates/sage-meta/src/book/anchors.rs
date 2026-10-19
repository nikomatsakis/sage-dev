//! Generated navigation over the same specification and evidence used by checks.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use sage_meta::correspondence::CorrespondenceIndex;
use sage_meta::review::{self, ReviewChecklist, ReviewInput, TestStatus};
use sage_meta::spec::{SpecAnchor, SpecIndex, anchor_url, relative_url as relative_href};
use serde_json::{Value, json};

use super::escape;

pub(super) const CHAPTER: &str = "design/anchor/README.md";
const ROOT_URL: &str = "design/anchor/index.html";

// {anchor}`meta.book.index_namespace_pages`
pub(super) fn generate(
    workspace: &Path,
    spec: &SpecIndex,
    landing: &mut Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let rust = CorrespondenceIndex::load(workspace)?;
    let inputs = review::inventory(spec, &rust)?;
    let checklist_path = workspace.join(review::CHECKLIST_FILE);
    let checklist = if checklist_path.exists() {
        review::load_saved(&checklist_path)?
    } else {
        ReviewChecklist::default()
    };
    let review_issues = review::validate(workspace, &inputs, &checklist);
    for anchor in spec.anchors.values() {
        spec.role_replacements(&anchor.file, &anchor.content)
            .map_err(|errors| {
                errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
            })?;
    }
    let view = AnchorPages {
        workspace,
        spec,
        rust: &rust,
        inputs: &inputs,
        checklist: &checklist,
        review_issues: &review_issues,
    };
    let mut parents = landing["parent_names"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    parents.push(json!("Anchors"));
    let number = landing["number"].as_array().cloned().unwrap_or_default();
    landing["content"] = format!(
        "# Anchors\n\nEvery specification rule, with its declared implementation and tests. These associations and reviews do not record test execution.\n\n{}",
        view.table("", ROOT_URL, None)
    ).into();
    landing["sub_items"] = view.children("", &parents, &number).into();
    Ok(())
}

struct AnchorPages<'a> {
    workspace: &'a Path,
    spec: &'a SpecIndex,
    rust: &'a CorrespondenceIndex,
    inputs: &'a BTreeMap<String, ReviewInput>,
    checklist: &'a ReviewChecklist,
    review_issues: &'a [review::ReviewIssue],
}

impl AnchorPages<'_> {
    fn names(&self, prefix: &str) -> Vec<&str> {
        self.spec
            .anchors
            .keys()
            .filter(|name| within(name, prefix))
            .map(String::as_str)
            .collect()
    }

    fn children(&self, prefix: &str, parents: &[Value], number: &[Value]) -> Vec<Value> {
        let names: BTreeSet<_> = self
            .names(prefix)
            .into_iter()
            .filter(|name| *name != prefix)
            .map(|name| {
                let suffix = if prefix.is_empty() {
                    name
                } else {
                    &name[prefix.len() + 1..]
                };
                let segment = suffix.split('.').next().expect("nonempty anchor suffix");
                if prefix.is_empty() {
                    segment.to_owned()
                } else {
                    format!("{prefix}.{segment}")
                }
            })
            .collect();
        names
            .into_iter()
            .enumerate()
            .map(|(position, name)| {
                let title = name.rsplit('.').next().expect("anchor name");
                let mut child_number = number.to_vec();
                child_number.push(json!(position + 1));
                let mut child_parents = parents.to_vec();
                child_parents.push(json!(title));
                let url = anchor_url(&name);
                let path = Path::new(&url)
                    .with_extension("md")
                    .to_string_lossy()
                    .into_owned();
                json!({"Chapter": {
                    "name": title,
                    "content": self.page(&name, &url),
                    "path": path,
                    "source_path": null,
                    "parent_names": parents,
                    "number": child_number,
                    "sub_items": self.children(&name, &child_parents, &child_number),
                }})
            })
            .collect()
    }

    // {anchor}`meta.anchors.anchor_pages`
    // {anchor}`meta.anchors.context_navigation`
    // {anchor}`meta.anchors.evidence_status`
    // {anchor}`meta.anchors.placeholder_status`
    fn page(&self, name: &str, url: &str) -> String {
        let mut text = format!(
            "# {}\n\n<nav class=\"anchor-breadcrumbs\" aria-label=\"Anchor namespaces\"><a href=\"{}\">All anchors</a>",
            escape(name),
            relative_href(url, ROOT_URL)
        );
        let mut prefix = String::new();
        for component in name
            .split('.')
            .take(name.split('.').count().saturating_sub(1))
        {
            if !prefix.is_empty() {
                prefix.push('.');
            }
            prefix.push_str(component);
            text.push_str(&format!(
                " / <a href=\"{}\">{}</a>",
                escape(&relative_href(url, &anchor_url(&prefix))),
                escape(component)
            ));
        }
        text.push_str("</nav>\n\n");
        if let Some(anchor) = self.spec.anchors.get(name) {
            text.push_str(&format!("<div class=\"anchor-definition\">{}</div>\n\n<p><a class=\"anchor-context\" href=\"{}\">Read in specification context</a> · <code>{}</code></p>\n\n", self.claim(anchor, url), escape(&relative_href(url, &anchor.context_url)), escape(&anchor.file)));
            text.push_str("## Implementation\n\n");
            let sites = self
                .rust
                .implementations
                .get(name)
                .map(Vec::as_slice)
                .unwrap_or_default();
            if sites.is_empty() {
                text.push_str("<p class=\"anchor-missing\">Missing implementation association — validation error.</p>\n\n");
            }
            for site in sites {
                text.push_str(&format!("<section class=\"anchor-evidence\"><p>{} <code>{}:{}–{}</code></p><p><code>{}</code> · {}</p>", status_badge(site.status), escape(&site.file), site.line, site.end_line, escape(&site.module), escape(&site.target)));
                if let Some(reason) = &site.explanation {
                    text.push_str(&format!("<p>{}</p>", escape(reason)));
                }
                if !site.cfg.is_empty() {
                    text.push_str(&format!(
                        "<p>Conditional source: <code>{}</code></p>",
                        escape(&site.cfg.join(", "))
                    ));
                }
                text.push_str(&code(&site.excerpt));
                text.push_str(&source_link(url, &site.file, site.line, site.end_line));
                text.push_str("</section>\n\n");
            }
            text.push_str("## Tests and review\n\n<p>Indexed tests and the implementation-completeness and test-adequacy review; no test run is implied.</p>\n\n");
            if let Some(input) = self.inputs.get(name) {
                if input.tests.is_empty() {
                    text.push_str("<p class=\"anchor-missing\">Missing test association — validation error.</p>\n\n");
                }
                for test in &input.tests {
                    let source = input
                        .fragments
                        .iter()
                        .find(|fragment| {
                            fragment.path == test.file
                                && fragment.selector
                                    == review::Selector::Test {
                                        crate_name: test.crate_name.clone(),
                                        target: test.target.clone(),
                                        rust_path: test.rust_path.clone(),
                                    }
                        })
                        .map(|fragment| fragment.contents.join("\n"))
                        .unwrap_or_default();
                    text.push_str(&format!("<section class=\"anchor-evidence\"><p>{} <code>{}</code></p><p><code>{}</code> · {}</p>{}", status_badge(test.status), escape(&test.rust_path), escape(&test.file), escape(&test.target), code(&source)));
                    if !test.cfg.is_empty() {
                        text.push_str(&format!(
                            "<p>Conditional source: <code>{}</code></p>",
                            escape(&test.cfg.join(", "))
                        ));
                    }
                    if let Some(site) =
                        self.rust
                            .tests
                            .get(name)
                            .into_iter()
                            .flatten()
                            .find(|site| {
                                site.rust_path == test.rust_path
                                    && site.file == test.file
                                    && site.target == test.target
                            })
                    {
                        text.push_str(&source_link(url, &site.file, site.line, site.end_line));
                    }
                    text.push_str("</section>\n\n");
                }
                let record = self
                    .inputs
                    .get(name)
                    .and_then(|input| {
                        review::acceptable(self.workspace, name, input, self.checklist)
                    })
                    .or_else(|| {
                        self.checklist
                            .reviews
                            .iter()
                            .find(|record| record.anchor == name)
                    });
                match record {
                    Some(record) => {
                        let current = !self.review_issues.iter().any(|issue| {
                            issue.anchor.as_deref().is_none_or(|anchor| anchor == name)
                        });
                        if let Some(output) = &record.output {
                            let status = if output.requested_edits.is_empty() {
                                "Accepted"
                            } else {
                                "Changes requested"
                            };
                            text.push_str(&format!("<p class=\"anchor-review\">Review: <strong>{status}</strong>{} · {}.</p><p>{}</p>\n\n", if current { "" } else { " (requires review)" }, escape(&output.reviewer).replace('\n', "<br>"), escape(&output.report).replace('\n', "<br>")));
                            for edit in &output.requested_edits {
                                text.push_str(&format!(
                                    "<p>Requested edit: {}</p>\n",
                                    escape(edit).replace('\n', "<br>")
                                ));
                            }
                        } else {
                            text.push_str("<p class=\"anchor-review\">Review: <strong>Unfinished</strong>.</p>\n\n");
                        }
                    }
                    None => text.push_str("<p class=\"anchor-review\">Review: pending.</p>\n\n"),
                }
            }
            // Keep the surrounding namespace available without obscuring the claim.
            let descendants = self.names(name);
            let scope = if descendants.len() > 1 {
                name
            } else {
                name.split('.').next().unwrap_or(name)
            };
            text.push_str(&format!("<details class=\"anchor-neighbors\"><summary>Browse {scope} anchors</summary>{}</details>\n", self.table(scope, url, Some(name))));
        } else {
            text.push_str(&self.table(name, url, None));
        }
        text
    }

    // {anchor}`meta.reviews.claim_text`
    fn claim(&self, anchor: &SpecAnchor, url: &str) -> String {
        // mdBook parses this HTML inside a larger raw HTML wrapper. Encode
        // line breaks so blank lines in examples cannot end that wrapper.
        sage_meta::render_spec_markdown(self.spec, &anchor.file, &anchor.content, url)
            .expect("claim roles were validated before page generation")
            .replace('\n', "&#10;")
    }

    // {anchor}`meta.book.index_rows`
    // {anchor}`meta.book.index_groups`
    fn table(&self, prefix: &str, url: &str, selected: Option<&str>) -> String {
        let names = self.names(prefix);
        let mut rows = String::new();
        let mut groups = BTreeSet::new();
        for name in &names {
            let components: Vec<_> = name.split('.').collect();
            for end in 1..components.len() {
                let group = components[..end].join(".");
                if within(&group, prefix) && groups.insert(group.clone()) {
                    let members = names
                        .iter()
                        .filter(|name| within(name, &group))
                        .copied()
                        .collect::<Vec<_>>();
                    rows.push_str(&format!("<tr class=\"anchor-group\" data-group=\"{group}\"><th scope=\"row\" style=\"--anchor-depth:{}\"><button type=\"button\" data-toggle-group=\"{group}\" aria-expanded=\"true\" aria-label=\"Collapse {group}\">▾</button> <a href=\"{}\">{group}</a> <small>({})</small></th><td>{}</td><td>{}</td></tr>", end - 1, escape(&relative_href(url, &anchor_url(&group))), members.len(), self.counts(&members, false), self.counts(&members, true)));
                }
            }
            let anchor = &self.spec.anchors[*name];
            let id = format!("anchor-text-{}", name.replace('.', "-"));
            let label = name.rsplit('.').next().unwrap_or(name);
            rows.push_str(&format!("<tr class=\"anchor-row{}\" data-anchor=\"{name}\"><th scope=\"row\" style=\"--anchor-depth:{}\"><button type=\"button\" data-toggle-text=\"{id}\" aria-controls=\"{id}\" aria-expanded=\"false\" aria-label=\"Show text for {name}\">▸</button> <a href=\"{}\" title=\"{name}\"{}><code>{label}</code></a></th><td>{}</td><td>{}</td></tr><tr class=\"anchor-text\" data-anchor-text=\"{name}\" id=\"{id}\" hidden><td colspan=\"3\">{}<p><a href=\"{}\">Read in specification context</a></p></td></tr>", if selected == Some(name) { " anchor-selected" } else { "" }, components.len() - 1, escape(&relative_href(url, &anchor_url(name))), if selected == Some(name) { " aria-current=\"page\"" } else { "" }, self.statuses(name, false), self.statuses(name, true), self.claim(anchor, url), escape(&relative_href(url, &anchor.context_url))));
        }
        format!(
            "<div class=\"anchor-index\"><p class=\"anchor-tools\"><button type=\"button\" data-expand-groups>Expand groups</button> <button type=\"button\" data-collapse-groups>Collapse groups</button><span>{} anchors</span></p><div class=\"anchor-scroll\"><table><caption>Specification anchors and declared coverage</caption><thead><tr><th scope=\"col\">Anchor</th><th scope=\"col\">Implementation</th><th scope=\"col\">Tests</th></tr></thead><tbody>{rows}</tbody></table></div></div><script>{}</script>",
            names.len(),
            include_str!("../../web/anchors.js")
        )
    }

    // {anchor}`meta.book.index_status`
    fn states(&self, name: &str, tests: bool) -> BTreeSet<TestStatus> {
        if tests {
            self.inputs
                .get(name)
                .into_iter()
                .flat_map(|input| &input.tests)
                .map(|site| site.status)
                .collect()
        } else {
            self.rust
                .implementations
                .get(name)
                .into_iter()
                .flatten()
                .map(|site| site.status)
                .collect()
        }
    }

    fn statuses(&self, name: &str, tests: bool) -> String {
        let states = self.states(name, tests);
        if states.is_empty() {
            return "<span class=\"anchor-status missing\">Missing</span>".into();
        }
        states.into_iter().map(status_badge).collect()
    }

    fn counts(&self, names: &[&str], tests: bool) -> String {
        let mut counts = BTreeMap::new();
        for name in names {
            for status in self.states(name, tests) {
                *counts.entry(status).or_insert(0) += 1;
            }
        }
        counts
            .into_iter()
            .map(|(status, count)| {
                format!(
                    "<span class=\"anchor-count\">{count} {}</span>",
                    status_label(status)
                )
            })
            .collect()
    }
}

fn within(name: &str, prefix: &str) -> bool {
    prefix.is_empty()
        || name == prefix
        || name
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('.'))
}

fn status_label(status: TestStatus) -> &'static str {
    match status {
        TestStatus::Ordinary => "Implemented",
        TestStatus::Todo => "TODO",
        TestStatus::NotApplicable => "N/A",
    }
}

fn status_badge(status: TestStatus) -> String {
    format!(
        "<span class=\"anchor-status\" data-status=\"{}\">{}</span>",
        status_label(status),
        status_label(status)
    )
}

fn code(source: &str) -> String {
    format!(
        "<pre><code class=\"language-rust\">{}</code></pre>",
        escape(source).replace('\n', "&#10;")
    )
}

fn source_link(page: &str, file: &str, start: usize, end: usize) -> String {
    let route = super::encode_url_component(&format!("source:{file}:{start}:{end}"));
    let href = relative_href(page, &format!("sage-explorer.html#{route}"));
    format!(
        "<p><a class=\"anchor-rust-source\" href=\"{}\" target=\"_blank\" rel=\"noopener\">See more source ↗</a></p>",
        escape(&href)
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn generated_excerpts_keep_role_shaped_rust_source_literal() {
        let source = "// {anchor}`demo.rule`\nfn check() { assert!(true); }";
        let html = super::code(source);
        assert!(html.contains("// {anchor}&#96;demo.rule&#96;&#10;fn check()"));
        assert!(!html.contains("{anchor}`"));
        let model = sage_meta::parse(
            "demo/crate.sage",
            "crate demo { \"\"\"{anchor}`purpose` A purpose.\"\"\" }",
        )
        .unwrap();
        let outline = super::super::document_outline(&model);
        assert!(outline.contains("{anchor}&#96;purpose&#96;"));
        assert!(!outline.contains("{anchor}`"));
    }
}
