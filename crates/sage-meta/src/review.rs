//! Persisted reviews of specification text against its complete annotated test set.
//!
//! These snapshots cache a review of what tests express. They do not cache test
//! execution or establish that unrecorded helpers and implementations conform.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::correspondence::CorrespondenceIndex;
use crate::spec::SpecIndex;

pub const CHECKLIST_FILE: &str = "review-checklist.json";
pub const SCHEMA_VERSION: u32 = 1;
pub const POLICY_VERSION: u32 = 1;
pub const REVIEW_SKILL: &str = ".agents/skills/sage-review-checklist/SKILL.md";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewChecklist {
    pub schema_version: u32,
    pub policy_version: u32,
    pub reviews: Vec<ReviewRecord>,
}

impl Default for ReviewChecklist {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            policy_version: POLICY_VERSION,
            reviews: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewRecord {
    pub anchor: String,
    pub input: ReviewInput,
    pub decision: Decision,
    pub reason: String,
    pub reviewer: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewInput {
    /// Original Markdown, split on newlines for readable Git diffs.
    pub claim: Vec<String>,
    pub tests: Vec<ReviewTest>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewTest {
    pub crate_name: String,
    pub target: String,
    pub rust_path: String,
    pub file: String,
    pub cfg: Vec<String>,
    pub source: Vec<String>,
    pub status: TestStatus,
}

pub use crate::evidence::EvidenceStatus as TestStatus;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Pending,
    Approved,
    Todo,
    NotApplicable,
    NeedsWork,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReviewIssue {
    pub anchor: Option<String>,
    pub message: String,
}

// {anchor}`meta.reviews.source_representation`
fn source_lines(text: &str) -> Vec<String> {
    text.split('\n').map(str::to_owned).collect()
}

/// Extract exact, deterministic review inputs without recording any decision.
// {anchor}`meta.reviews.review_scope`
pub fn inventory(
    spec: &SpecIndex,
    rust: &CorrespondenceIndex,
) -> Result<BTreeMap<String, ReviewInput>, Box<dyn Error>> {
    let mut inputs = BTreeMap::new();
    for (name, anchor) in &spec.anchors {
        let mut tests = Vec::new();
        for site in rust.tests.get(name).into_iter().flatten() {
            // {anchor}`meta.reviews.test_source`
            let source = rust
                .files
                .get(&site.file)
                .and_then(|source| source.get(site.span.start..site.span.end))
                .ok_or_else(|| {
                    format!(
                        "missing test source for {} in {}",
                        site.rust_path, site.file
                    )
                })?;
            let owner = site
                .rust_path
                .rsplit_once("::")
                .map_or("", |(owner, _)| owner);
            let status = crate::evidence::placeholder_status(owner)?;
            tests.push(ReviewTest {
                crate_name: site.crate_name.clone(),
                target: site.target.clone(),
                rust_path: site.rust_path.clone(),
                file: site.file.clone(),
                cfg: site.cfg.clone(),
                source: source_lines(source),
                status,
            });
        }
        // {anchor}`meta.reviews.complete_test_set`
        // Repeated annotations on one declaration do not create another test.
        tests.sort();
        tests.dedup();
        inputs.insert(
            name.clone(),
            ReviewInput {
                // {anchor}`meta.reviews.claim_text`
                claim: source_lines(&anchor.content),
                tests,
            },
        );
    }
    Ok(inputs)
}

fn check_format(checklist: &ReviewChecklist) -> Result<(), Box<dyn Error>> {
    if checklist.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported review schema version {}; expected {SCHEMA_VERSION}",
            checklist.schema_version
        )
        .into());
    }
    let mut names = BTreeSet::new();
    for review in &checklist.reviews {
        if !names.insert(&review.anchor) {
            return Err(format!("duplicate review for {}", review.anchor).into());
        }
    }
    Ok(())
}

pub fn load(path: &Path) -> Result<ReviewChecklist, Box<dyn Error>> {
    let checklist: ReviewChecklist = serde_json::from_slice(&std::fs::read(path)?)?;
    check_format(&checklist)?;
    Ok(checklist)
}

/// Determine whether the recorded review accounts for the visible evidence kind.
fn decision_error(review: &ReviewRecord) -> Option<&'static str> {
    // {anchor}`meta.reviews.decisions`
    if review.input.tests.is_empty() {
        return Some("a review must include at least one annotated test");
    }
    if review.reason.trim().is_empty() || review.reviewer.trim().is_empty() {
        return Some("a completed review needs a reason and reviewer");
    }
    match review.decision {
        Decision::Pending => Some("review is pending"),
        Decision::NeedsWork => Some("review found test coverage that needs work"),
        Decision::Approved => review
            .input
            .tests
            .iter()
            .any(|test| test.status != TestStatus::Ordinary)
            .then_some("approval cannot present TODO or N/A tests as behavioral evidence"),
        Decision::Todo => (!review
            .input
            .tests
            .iter()
            .any(|test| test.status == TestStatus::Todo))
        .then_some("a TODO decision requires an explicit todo test"),
        Decision::NotApplicable => review
            .input
            .tests
            .iter()
            .any(|test| test.status != TestStatus::NotApplicable)
            .then_some("an N/A decision requires all associated tests to be not_applicable"),
    }
}

/// Check freshness and decisions without executing tests or modifying snapshots.
pub fn validate(
    current: &BTreeMap<String, ReviewInput>,
    checklist: &ReviewChecklist,
) -> Vec<ReviewIssue> {
    let mut issues = Vec::new();
    let mut report = |anchor: Option<&str>, message: String| {
        issues.push(ReviewIssue {
            anchor: anchor.map(str::to_owned),
            message: format!("{message}; run the review skill at {REVIEW_SKILL}"),
        });
    };
    if let Err(error) = check_format(checklist) {
        report(None, error.to_string());
        return issues;
    }
    if checklist.policy_version != POLICY_VERSION {
        report(
            None,
            format!(
                "review policy version {} is stale; expected {POLICY_VERSION}",
                checklist.policy_version
            ),
        );
        return issues;
    }
    let reviews: BTreeMap<_, _> = checklist
        .reviews
        .iter()
        .map(|review| (review.anchor.as_str(), review))
        .collect();
    for (anchor, input) in current {
        match reviews.get(anchor.as_str()) {
            None => report(Some(anchor), format!("missing review for {anchor}")),
            // {anchor}`meta.reviews.staleness`
            Some(review) if &review.input != input => report(
                Some(anchor),
                format!("stale review for {anchor}: claim or complete test set changed"),
            ),
            Some(review) => {
                if let Some(message) = decision_error(review) {
                    report(Some(anchor), format!("{anchor}: {message}"));
                }
            }
        }
    }
    for review in &checklist.reviews {
        if !current.contains_key(&review.anchor) {
            report(
                Some(&review.anchor),
                format!(
                    "review for retired anchor {}; record a review packet to prune retired entries",
                    review.anchor
                ),
            );
        }
    }
    issues
}

fn load_current(
    workspace: &Path,
) -> Result<(PathBuf, BTreeMap<String, ReviewInput>), Box<dyn Error>> {
    let rust = CorrespondenceIndex::load(workspace)?;
    let workspace = rust.workspace_root().unwrap_or(workspace);
    let spec = SpecIndex::load_workspace(workspace)?;
    let mut errors = rust.diagnostics.clone();
    errors.extend(spec.validate(&rust.tests));
    errors.extend(spec.validate_implementations(&rust));
    errors.extend(spec.validate_implementation_references(&rust));
    if !errors.is_empty() {
        return Err(format!(
            "cannot prepare reviews from invalid source inventory:\n{}",
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        )
        .into());
    }
    Ok((workspace.to_owned(), inventory(&spec, &rust)?))
}

fn load_existing(workspace: &Path) -> Result<ReviewChecklist, Box<dyn Error>> {
    let path = workspace.join(CHECKLIST_FILE);
    match std::fs::read(&path) {
        Ok(source) => {
            let checklist: ReviewChecklist = serde_json::from_slice(&source)?;
            check_format(&checklist)?;
            Ok(checklist)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(ReviewChecklist::default())
        }
        Err(error) => Err(error.into()),
    }
}

/// Emit only missing, changed, or unsuccessful reviews. Never updates the cache.
pub fn prepare(workspace: &Path) -> Result<ReviewChecklist, Box<dyn Error>> {
    // {anchor}`meta.reviews.preparation`
    let (workspace, current) = load_current(workspace)?;
    let checklist = load_existing(&workspace)?;
    let mut packet = ReviewChecklist::default();
    for (anchor, input) in current {
        let reusable = checklist.policy_version == POLICY_VERSION
            && checklist.reviews.iter().any(|review| {
                review.anchor == anchor && review.input == input && decision_error(review).is_none()
            });
        if !reusable {
            packet.reviews.push(ReviewRecord {
                anchor,
                input,
                decision: Decision::Pending,
                reason: String::new(),
                reviewer: String::new(),
            });
        }
    }
    Ok(packet)
}

/// Merge completed decisions only if their supplied inputs still match source.
pub fn record(workspace: &Path, reviewed: &ReviewChecklist) -> Result<(), Box<dyn Error>> {
    // {anchor}`meta.reviews.recording`
    check_format(reviewed)?;
    if reviewed.policy_version != POLICY_VERSION {
        return Err("cannot record reviews under an outdated policy".into());
    }
    let (workspace, current) = load_current(workspace)?;
    for review in &reviewed.reviews {
        if current.get(&review.anchor) != Some(&review.input) {
            return Err(format!(
                "cannot record {}: reviewed claim or tests no longer match current source",
                review.anchor
            )
            .into());
        }
        if review.decision == Decision::NeedsWork {
            if review.reason.trim().is_empty() || review.reviewer.trim().is_empty() {
                return Err("a needs_work review needs a reason and reviewer".into());
            }
        } else if let Some(message) = decision_error(review) {
            return Err(format!("cannot record {}: {message}", review.anchor).into());
        }
    }
    let existing = load_existing(&workspace)?;
    // A policy change cannot upgrade earlier decisions that were not resubmitted.
    let mut merged: BTreeMap<_, _> = existing
        .reviews
        .into_iter()
        .filter(|review| {
            existing.policy_version == POLICY_VERSION && current.contains_key(&review.anchor)
        })
        .map(|review| (review.anchor.clone(), review))
        .collect();
    for review in &reviewed.reviews {
        merged.insert(review.anchor.clone(), review.clone());
    }
    let result = ReviewChecklist {
        reviews: merged.into_values().collect(),
        ..ReviewChecklist::default()
    };
    let source = format!("{}\n", serde_json::to_string_pretty(&result)?);
    let temporary = workspace.join(format!(".review-checklist-{}.tmp", std::process::id()));
    std::fs::write(&temporary, source)?;
    std::fs::rename(&temporary, workspace.join(CHECKLIST_FILE))?;
    Ok(())
}
