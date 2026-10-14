//! Rust-declared architectural correspondence, read from unexpanded source.
//!
//! This is a source inventory across cfg alternatives, not an assertion that a
//! declaration is present in a particular binary. Runtime users must verify the
//! source snapshot and resolve each observed query location without guessing.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;

use crate::{Declaration, Diagnostic, Document, Span};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum RustItemKind {
    Type,
    Trait,
    Module,
    Function,
    Method,
    TraitMethod,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RustSite {
    pub file: String,
    pub line: usize,
    pub end_line: usize,
    pub column: usize,
    pub end_column: usize,
    pub rust_path: String,
    pub crate_name: String,
    pub target: String,
    pub kind: RustItemKind,
    /// Textual conditions, including inherited conditions. They are not evaluated.
    pub cfg: Vec<String>,
    pub macro_input: bool,
    /// Whether this declaration has a function body; this is not instrumentation coverage.
    pub executable: bool,
    pub span: Span,
    pub annotation_span: Span,
}

#[derive(Clone, Debug)]
pub struct RustSource {
    pub crate_name: String,
    pub target: String,
    pub file: String,
    pub source: String,
}

#[derive(Default, Debug, serde::Serialize)]
pub struct CorrespondenceIndex {
    pub entries: BTreeMap<String, Vec<RustSite>>,
    /// Explicit Rust comment references to specification rules. These are
    /// independent of model correspondences and test annotations.
    pub implementations: BTreeMap<String, Vec<crate::evidence::ImplementationSite>>,
    /// Test declarations linked to named specification claims. This is a
    /// source inventory, including cfg alternatives, not a record of test runs.
    pub tests: BTreeMap<String, Vec<RustSite>>,
    pub files: BTreeMap<String, String>,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip)]
    workspace: Option<PathBuf>,
    #[serde(skip)]
    visited: BTreeSet<(String, String, String)>,
    #[serde(skip)]
    callables: Vec<SourceRange>,
    #[serde(skip)]
    opaque_macros: Vec<SourceRange>,
    #[serde(skip)]
    recognized: BTreeSet<(String, usize)>,
}

#[derive(Debug)]
pub struct CorrespondenceMatch<'a> {
    pub sage_item: &'a str,
    pub site: &'a RustSite,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LookupError {
    Missing {
        file: String,
        line: usize,
    },
    Ambiguous {
        file: String,
        line: usize,
        candidates: Vec<String>,
    },
    SourceMismatch {
        file: String,
    },
}
impl std::fmt::Display for LookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { file, line } => write!(f, "unmapped Rust query at {file}:{line}"),
            Self::Ambiguous {
                file,
                line,
                candidates,
            } => write!(
                f,
                "ambiguous Rust query at {file}:{line}: {}",
                candidates.join(", ")
            ),
            Self::SourceMismatch { file } => write!(
                f,
                "Rust source does not match the compiled snapshot: {file}"
            ),
        }
    }
}
impl std::error::Error for LookupError {}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceRange {
    file: String,
    line: usize,
    end_line: usize,
    span: Span,
}
impl SourceRange {
    fn contains(&self, file: &str, line: usize) -> bool {
        self.file == file && self.line <= line && line <= self.end_line
    }
}

#[derive(Clone)]
struct Context {
    crate_name: String,
    target: String,
    module: String,
    file: String,
    directory: PathBuf,
    physical_parent: PathBuf,
    cfg: Vec<String>,
    macro_input: bool,
    test_harness: bool,
    inside_function: bool,
    source_root: bool,
}

impl CorrespondenceIndex {
    /// Cargo's actual workspace root; standalone in-memory fixtures have none.
    pub fn workspace_root(&self) -> Option<&Path> {
        self.workspace.as_deref()
    }

    /// Inspect library, binary, and integration-test targets, including cfg
    /// alternatives. Cargo target settings determine whether annotated tests
    /// belong to a test harness.
    pub fn load(workspace: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        #[derive(serde::Deserialize)]
        struct Metadata {
            packages: Vec<Package>,
            workspace_root: PathBuf,
        }
        #[derive(serde::Deserialize)]
        struct Package {
            name: String,
            manifest_path: PathBuf,
            targets: Vec<Target>,
        }
        #[derive(serde::Deserialize)]
        struct Target {
            name: String,
            kind: Vec<String>,
            src_path: PathBuf,
            test: bool,
        }
        let output = std::process::Command::new("cargo")
            .args([
                "metadata",
                "--no-deps",
                "--offline",
                "--format-version",
                "1",
                "--manifest-path",
            ])
            .arg(workspace.join("Cargo.toml"))
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "cannot locate Rust crates: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        let metadata: Metadata = serde_json::from_slice(&output.stdout)?;
        let mut index = Self {
            workspace: Some(metadata.workspace_root.canonicalize()?),
            ..Self::default()
        };
        for package in metadata.packages {
            let crate_name = package.name.replace('-', "_");
            let manifest: CargoTestSettings =
                toml::from_str(&std::fs::read_to_string(&package.manifest_path)?)?;
            for target in package.targets {
                if !target.kind.iter().any(|kind| {
                    matches!(
                        kind.as_str(),
                        "lib"
                            | "rlib"
                            | "dylib"
                            | "cdylib"
                            | "staticlib"
                            | "proc-macro"
                            | "bin"
                            | "test"
                    )
                }) {
                    continue;
                }
                let file = index.relative(&target.src_path)?;
                let test_harness = target.test
                    && manifest.uses_harness(
                        &target.kind,
                        &target.name,
                        &target.src_path,
                        &package.manifest_path,
                    );
                index.load_file(Context {
                    module: crate_name.clone(),
                    crate_name: crate_name.clone(),
                    target: format!("{}:{}", target.kind.join("+"), target.name),
                    file,
                    directory: PathBuf::new(),
                    physical_parent: PathBuf::new(),
                    cfg: Vec::new(),
                    macro_input: false,
                    test_harness,
                    inside_function: false,
                    source_root: true,
                })?;
            }
        }
        Ok(index)
    }

    /// Index standalone fixture roots. Inline modules are traversed; out-of-line
    /// modules require `load`, which resolves them against Cargo's source roots.
    pub fn from_sources(sources: &[RustSource]) -> Result<Self, Box<dyn std::error::Error>> {
        let mut index = Self::default();
        for source in sources {
            let file = normalize(&source.file);
            if let Some(previous) = index.files.insert(file.clone(), source.source.clone())
                && previous != source.source
            {
                return Err(format!("conflicting source snapshots for {file}").into());
            }
            let syntax = syn::parse_file(&source.source)?;
            let context = Context {
                crate_name: source.crate_name.clone(),
                target: source.target.clone(),
                module: source.crate_name.clone(),
                file,
                directory: PathBuf::new(),
                physical_parent: PathBuf::new(),
                cfg: Vec::new(),
                macro_input: false,
                test_harness: true,
                inside_function: false,
                source_root: true,
            };
            index.items(&syntax.items, &context)?;
            index.check_attributes(&syntax, &context);
            index.implementation_entries(&syntax, &context);
        }
        Ok(index)
    }

    /// Compare with text embedded by the running binary, for example using
    /// `include_str!`. Reading two current disk copies cannot prove build identity.
    pub fn verify_source(&self, file: &str, compiled_source: &str) -> Result<(), LookupError> {
        let file = self.source_key(file);
        if self
            .files
            .get(&file)
            .is_some_and(|source| source == compiled_source)
        {
            Ok(())
        } else {
            Err(LookupError::SourceMismatch { file })
        }
    }

    /// Resolve an observed declaration location. Never fall back to debug names,
    /// path suffixes, or arbitrary tokens inside macro invocations.
    pub fn lookup(&self, file: &str, line: usize) -> Result<CorrespondenceMatch<'_>, LookupError> {
        let file = self.source_key(file);
        if self
            .opaque_macros
            .iter()
            .any(|region| region.contains(&file, line))
        {
            return Err(LookupError::Missing { file, line });
        }
        let mut regions: Vec<_> = self
            .callables
            .iter()
            .filter(|region| region.contains(&file, line))
            .collect();
        regions.retain(|outer| {
            !self.callables.iter().any(|inner| {
                inner.contains(&file, line)
                    && inner.line >= outer.line
                    && inner.end_line <= outer.end_line
                    && (inner.line > outer.line || inner.end_line < outer.end_line)
            })
        });
        regions.sort_by_key(|region| region.span.start);
        regions.dedup_by_key(|region| (region.span.start, region.span.end));
        if regions.is_empty() {
            return Err(LookupError::Missing { file, line });
        }
        if regions.len() > 1 {
            return Err(LookupError::Ambiguous {
                file,
                line,
                candidates: regions
                    .iter()
                    .map(|region| format!("Rust declaration at {}:{}", region.file, region.line))
                    .collect(),
            });
        }
        let mut matches: Vec<_> = self
            .entries
            .iter()
            .flat_map(|(name, sites)| sites.iter().map(move |site| (name, site)))
            .filter(|(_, site)| {
                site.executable && site.file == file && site.span == regions[0].span
            })
            .collect();
        matches
            .sort_by_key(|(name, site)| (name.as_str(), site.rust_path.as_str(), site.span.start));
        matches.dedup_by(|(left_name, left), (right_name, right)| {
            left_name == right_name
                && left.file == right.file
                && left.span == right.span
                && left.rust_path == right.rust_path
        });
        match matches.as_slice() {
            [] => Err(LookupError::Missing { file, line }),
            [(sage_item, site)] => Ok(CorrespondenceMatch { sage_item, site }),
            _ => Err(LookupError::Ambiguous {
                file,
                line,
                candidates: matches
                    .iter()
                    .map(|(name, site)| format!("{name} ({})", site.rust_path))
                    .collect(),
            }),
        }
    }

    /// Bidirectional source coverage. Module anchors count as source locations,
    /// while `RustSite::executable` reports the separate body-mapping coverage.
    // {anchor}`meta.contracts.correspondence`
    // {anchor}`meta.book.crate_ownership`
    pub fn validate(&self, documents: &[Document]) -> Vec<Diagnostic> {
        #[derive(Clone, Copy)]
        enum Unit {
            Namespace,
            Type,
            Operation,
        }
        let mut units: BTreeMap<String, (&Document, Span, Unit)> = BTreeMap::new();
        for document in documents {
            let Some(namespace) = crate::workspace::document_namespace(documents, document) else {
                continue;
            };
            units.insert(
                namespace.clone(),
                (document, Span { start: 0, end: 0 }, Unit::Namespace),
            );
            for declaration in &document.declarations {
                match declaration {
                    Declaration::Type(ty) => {
                        units.insert(
                            format!("{namespace}::{}", ty.name),
                            (document, ty.span.clone(), Unit::Type),
                        );
                    }
                    Declaration::Class(class) => {
                        let key = format!("{namespace}::{}", class.name);
                        units.insert(key.clone(), (document, class.span.clone(), Unit::Type));
                        for method in &class.methods {
                            units.insert(
                                format!("{key}::{}", method.name),
                                (document, method.span.clone(), Unit::Operation),
                            );
                        }
                    }
                }
            }
        }
        let mut diagnostics = self.diagnostics.clone();
        for (name, (document, span, _)) in &units {
            if self.entries.get(name).is_none_or(Vec::is_empty) {
                diagnostics.push(Diagnostic {
                    file: document.file.clone(),
                    span: span.clone(),
                    message: format!("Sage item {name} has no Rust #[sage::item] correspondence"),
                });
            }
        }
        for (name, sites) in &self.entries {
            for site in sites {
                let mut report = |message| {
                    diagnostics.push(Diagnostic {
                        file: site.file.clone(),
                        span: site.annotation_span.clone(),
                        message,
                    })
                };
                if name.split("::").next() != Some(site.crate_name.as_str()) {
                    report(format!(
                        "Sage target {name} belongs to a different crate than Rust owner {}",
                        site.crate_name
                    ));
                }
                let Some((_, _, unit)) = units.get(name) else {
                    report(format!(
                        "Rust annotation refers to unknown Sage item {name}"
                    ));
                    continue;
                };
                let compatible = match unit {
                    Unit::Namespace => site.kind == RustItemKind::Module,
                    Unit::Type => matches!(
                        site.kind,
                        RustItemKind::Type | RustItemKind::Trait | RustItemKind::Module
                    ),
                    Unit::Operation => matches!(
                        site.kind,
                        RustItemKind::Function
                            | RustItemKind::Method
                            | RustItemKind::TraitMethod
                            | RustItemKind::Module
                    ),
                };
                if !compatible {
                    report(format!(
                        "incompatible Rust {:?} correspondence for Sage item {name}",
                        site.kind
                    ));
                }
            }
        }
        diagnostics
    }

    /// Normalize a callback path to the exact key used by this source inventory.
    pub fn source_key(&self, file: &str) -> String {
        if let Some(workspace) = &self.workspace
            && let Ok(relative) = Path::new(file).strip_prefix(workspace)
        {
            return normalize(&relative.to_string_lossy());
        }
        normalize(file)
    }
    fn relative(&self, file: &Path) -> Result<String, Box<dyn std::error::Error>> {
        let file = file.canonicalize()?;
        let workspace = self
            .workspace
            .as_ref()
            .expect("disk indexes have a workspace");
        Ok(normalize(
            &file
                .strip_prefix(workspace)
                .map_err(|_| {
                    format!(
                        "Rust source {} is outside workspace {}",
                        file.display(),
                        workspace.display()
                    )
                })?
                .to_string_lossy(),
        ))
    }
    fn load_file(&mut self, mut context: Context) -> Result<(), Box<dyn std::error::Error>> {
        if !self.visited.insert((
            context.file.clone(),
            context.module.clone(),
            context.target.clone(),
        )) {
            return Ok(());
        }
        let path = self.workspace.as_ref().unwrap().join(&context.file);
        let source = std::fs::read_to_string(&path)?;
        let syntax =
            syn::parse_file(&source).map_err(|error| format!("{}: {error}", path.display()))?;
        self.files.insert(context.file.clone(), source);
        let parent = path.parent().unwrap();
        context.physical_parent = parent.to_path_buf();
        context.directory =
            if context.source_root || path.file_name().is_some_and(|name| name == "mod.rs") {
                parent.to_path_buf()
            } else {
                parent.join(path.file_stem().unwrap())
            };
        self.items(&syntax.items, &context)?;
        self.check_attributes(&syntax, &context);
        self.implementation_entries(&syntax, &context);
        Ok(())
    }
    fn implementation_entries(&mut self, syntax: &syn::File, context: &Context) {
        crate::evidence::index_source(
            &self.files[&context.file],
            syntax,
            crate::evidence::SourceContext {
                file: &context.file,
                crate_name: &context.crate_name,
                target: &context.target,
                module: &context.module,
                cfg: &context.cfg,
            },
            &mut self.implementations,
            &mut self.diagnostics,
        );
    }
    fn items(
        &mut self,
        items: &[syn::Item],
        context: &Context,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for item in items {
            match item {
                syn::Item::Mod(module) => {
                    self.record(
                        &module.attrs,
                        &module.ident,
                        item.span(),
                        RustItemKind::Module,
                        false,
                        context,
                    );
                    let mut child = context.clone();
                    child.module = format!("{}::{}", context.module, ident(&module.ident));
                    child.cfg.extend(self.conditions(&module.attrs, context));
                    if let Some((_, contents)) = &module.content {
                        child.directory = context.directory.join(ident(&module.ident));
                        child.physical_parent = child.directory.clone();
                        self.items(contents, &child)?;
                    } else if self.workspace.is_some() {
                        let explicit = module.attrs.iter().find_map(|attr| match &attr.meta {
                            syn::Meta::NameValue(value) if value.path.is_ident("path") => {
                                match &value.value {
                                    syn::Expr::Lit(literal) => match &literal.lit {
                                        syn::Lit::Str(path) => Some(path.value()),
                                        _ => None,
                                    },
                                    _ => None,
                                }
                            }
                            _ => None,
                        });
                        let path = if let Some(path) = explicit {
                            context.physical_parent.join(path)
                        } else {
                            let flat = context
                                .directory
                                .join(format!("{}.rs", ident(&module.ident)));
                            if flat.is_file() {
                                flat
                            } else {
                                context.directory.join(ident(&module.ident)).join("mod.rs")
                            }
                        };
                        child.file = self.relative(&path)?;
                        child.source_root = false;
                        self.load_file(child)?;
                    }
                }
                syn::Item::Fn(function) => {
                    self.record(
                        &function.attrs,
                        &function.sig.ident,
                        item.span(),
                        RustItemKind::Function,
                        true,
                        context,
                    );
                    let mut child = context.clone();
                    child.module = format!("{}::{}", context.module, ident(&function.sig.ident));
                    child.cfg.extend(self.conditions(&function.attrs, context));
                    self.nested_items(&function.block, &child)?;
                }
                syn::Item::Struct(ty) => self.record(
                    &ty.attrs,
                    &ty.ident,
                    item.span(),
                    RustItemKind::Type,
                    false,
                    context,
                ),
                syn::Item::Enum(ty) => self.record(
                    &ty.attrs,
                    &ty.ident,
                    item.span(),
                    RustItemKind::Type,
                    false,
                    context,
                ),
                syn::Item::Union(ty) => self.record(
                    &ty.attrs,
                    &ty.ident,
                    item.span(),
                    RustItemKind::Type,
                    false,
                    context,
                ),
                syn::Item::Type(ty) => self.record(
                    &ty.attrs,
                    &ty.ident,
                    item.span(),
                    RustItemKind::Type,
                    false,
                    context,
                ),
                syn::Item::Trait(trait_) => {
                    self.record(
                        &trait_.attrs,
                        &trait_.ident,
                        item.span(),
                        RustItemKind::Trait,
                        false,
                        context,
                    );
                    let mut child = context.clone();
                    child.module = format!("{}::{}", context.module, ident(&trait_.ident));
                    child.cfg.extend(self.conditions(&trait_.attrs, context));
                    for item in &trait_.items {
                        if let syn::TraitItem::Fn(method) = item {
                            self.record(
                                &method.attrs,
                                &method.sig.ident,
                                method.span(),
                                RustItemKind::TraitMethod,
                                method.default.is_some(),
                                &child,
                            );
                            if let Some(body) = &method.default {
                                self.nested_items(body, &child)?;
                            }
                        }
                    }
                }
                syn::Item::Impl(impl_) => {
                    let mut child = context.clone();
                    child.module = format!(
                        "{}::{}",
                        context.module,
                        self.text(context, impl_.self_ty.span())
                    );
                    child.cfg.extend(self.conditions(&impl_.attrs, context));
                    for item in &impl_.items {
                        if let syn::ImplItem::Fn(method) = item {
                            self.record(
                                &method.attrs,
                                &method.sig.ident,
                                method.span(),
                                RustItemKind::Method,
                                true,
                                &child,
                            );
                            self.nested_items(&method.block, &child)?;
                        }
                    }
                }
                syn::Item::Macro(mac) if mac.mac.path.is_ident("define_kind_symbols") => {
                    let declarations = syn::parse2::<SymbolDefinitions>(mac.mac.tokens.clone())?;
                    let mut child = context.clone();
                    child.macro_input = true;
                    child.cfg.extend(self.conditions(&mac.attrs, context));
                    for declaration in declarations.0 {
                        self.record(
                            &declaration.attrs,
                            &declaration.name,
                            declaration.span,
                            RustItemKind::Type,
                            false,
                            &child,
                        );
                        for attribute in &declaration.attrs {
                            self.check_attribute(attribute, &child);
                        }
                    }
                }
                syn::Item::Macro(mac) if mac.ident.is_none() => {
                    self.opaque_macro(mac.span(), context);
                }
                // Arbitrary macro arguments are not Rust declarations. They need
                // a deliberate parser adapter before they can provide coverage.
                _ => (),
            }
        }
        Ok(())
    }
    fn record(
        &mut self,
        attrs: &[syn::Attribute],
        name: &syn::Ident,
        span: proc_macro2::Span,
        kind: RustItemKind,
        executable: bool,
        context: &Context,
    ) {
        if executable {
            self.callables.push(SourceRange {
                file: context.file.clone(),
                line: span.start().line,
                end_line: span.end().line,
                span: self.byte_span(context, span),
            });
        }
        let mut cfg = context.cfg.clone();
        cfg.extend(self.conditions(attrs, context));
        let site = RustSite {
            file: context.file.clone(),
            line: span.start().line,
            end_line: span.end().line,
            column: span.start().column,
            end_column: span.end().column,
            rust_path: format!("{}::{}", context.module, ident(name)),
            crate_name: context.crate_name.clone(),
            target: context.target.clone(),
            kind,
            cfg,
            macro_input: context.macro_input,
            executable,
            span: self.byte_span(context, span),
            annotation_span: Span { start: 0, end: 0 },
        };
        self.record_tests(attrs, &site, context);
        let attributes: Vec<_> = attrs
            .iter()
            .filter(|attr| {
                attr.path().segments.len() == 2
                    && attr.path().segments[0].ident == "sage"
                    && attr.path().segments[1].ident == "item"
            })
            .collect();
        if executable && attributes.len() > 1 {
            self.diagnostics.push(Diagnostic {
                file: context.file.clone(),
                span: self.byte_span(context, span),
                message:
                    "an executable Rust declaration must have exactly one Sage operation identity"
                        .into(),
            });
        }
        for attribute in attributes {
            let annotation_span = self.byte_span(context, attribute.span());
            self.recognized
                .insert((context.file.clone(), annotation_span.start));
            let path = match attribute.parse_args::<syn::Path>() {
                Ok(path)
                    if path.leading_colon.is_none()
                        && path
                            .segments
                            .iter()
                            .all(|segment| segment.arguments.is_empty()) =>
                {
                    path
                }
                _ => {
                    self.diagnostics.push(Diagnostic {
                        file: context.file.clone(),
                        span: annotation_span,
                        message: "#[sage::item] expects one crate-qualified Sage path".into(),
                    });
                    continue;
                }
            };
            let mut parts: Vec<_> = path
                .segments
                .iter()
                .map(|segment| ident(&segment.ident))
                .collect();
            if parts[0] == "crate" {
                parts[0] = context.crate_name.clone();
            } else if parts[0] == "self"
                || parts[0] == "super"
                || (parts.len() == 1 && parts[0] != context.crate_name)
            {
                self.diagnostics.push(Diagnostic {
                    file: context.file.clone(),
                    span: annotation_span,
                    message: "Sage paths start with crate or an explicit crate name".into(),
                });
                continue;
            }
            self.entries
                .entry(parts.join("::"))
                .or_default()
                .push(RustSite {
                    annotation_span,
                    ..site.clone()
                });
        }
    }
    // {anchor}`meta.anchors.test_references`
    fn record_tests(&mut self, attrs: &[syn::Attribute], site: &RustSite, context: &Context) {
        for attribute in attrs.iter().filter(|attribute| {
            attribute.path().segments.len() == 2
                && attribute.path().segments[0].ident == "sage"
                && attribute.path().segments[1].ident == "test"
        }) {
            let annotation_span = self.byte_span(context, attribute.span());
            self.recognized
                .insert((context.file.clone(), annotation_span.start));
            let result = (|| -> Result<String, String> {
                let anchor = attribute
                    .parse_args::<AnchorName>()
                    .map_err(|_| {
                        "#[sage::test] expects one dot-separated anchor name (for example, meta.language.calls.declared)"
                            .to_owned()
                    })?;
                if site.kind != RustItemKind::Function
                    || !attrs
                        .iter()
                        .any(|attribute| attribute.path().is_ident("test"))
                    || context.inside_function
                {
                    return Err(
                        "#[sage::test] must annotate a module-level #[test] function".into(),
                    );
                }
                if !context.test_harness {
                    return Err(
                        "#[sage::test] cannot provide coverage in a target with test = false or harness = false"
                            .into(),
                    );
                }
                if attrs
                    .iter()
                    .any(|attribute| potentially_ignored(&attribute.meta))
                {
                    return Err(
                        "#[sage::test] cannot provide coverage from an ignored or conditionally ignored test"
                            .into(),
                    );
                }
                Ok(anchor.0)
            })();
            match result {
                Ok(anchor) => self.tests.entry(anchor).or_default().push(RustSite {
                    annotation_span,
                    ..site.clone()
                }),
                Err(message) => self.diagnostics.push(Diagnostic {
                    file: context.file.clone(),
                    span: annotation_span,
                    message,
                }),
            }
        }
    }
    fn opaque_macro(&mut self, span: proc_macro2::Span, context: &Context) {
        self.opaque_macros.push(SourceRange {
            file: context.file.clone(),
            line: span.start().line,
            end_line: span.end().line,
            span: self.byte_span(context, span),
        });
    }
    fn nested_items(
        &mut self,
        block: &syn::Block,
        context: &Context,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use syn::visit::Visit;
        struct Nested<'a> {
            index: &'a mut CorrespondenceIndex,
            context: &'a Context,
            error: Option<Box<dyn std::error::Error>>,
        }
        impl<'ast> Visit<'ast> for Nested<'_> {
            fn visit_item(&mut self, item: &'ast syn::Item) {
                if self.error.is_none() {
                    self.error = self
                        .index
                        .items(std::slice::from_ref(item), self.context)
                        .err();
                }
            }
            fn visit_macro(&mut self, mac: &'ast syn::Macro) {
                self.index.opaque_macro(mac.span(), self.context);
            }
        }
        let mut context = context.clone();
        context.inside_function = true;
        let mut nested = Nested {
            index: self,
            context: &context,
            error: None,
        };
        nested.visit_block(block);
        match nested.error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    fn check_attribute(&mut self, attribute: &syn::Attribute, context: &Context) {
        fn conditional_mapping(meta: &syn::Meta) -> syn::Result<bool> {
            let syn::Meta::List(list) = meta else {
                return Ok(false);
            };
            if !list.path.is_ident("cfg_attr") {
                return Ok(false);
            }
            let arguments = list.parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )?;
            for attribute in arguments.iter().skip(1) {
                if attribute.path().is_ident("path")
                    || attribute
                        .path()
                        .segments
                        .first()
                        .is_some_and(|segment| segment.ident == "sage")
                    || conditional_mapping(attribute)?
                {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        match conditional_mapping(&attribute.meta) {
            Ok(true) => self.diagnostics.push(Diagnostic {
                file: context.file.clone(),
                span: self.byte_span(context, attribute.span()),
                message: "conditional Sage annotations and module paths in cfg_attr are unsupported; use unconditional #[sage::item] or #[sage::test] annotations on cfg-gated declarations".into(),
            }),
            Err(error) => self.diagnostics.push(Diagnostic {
                file: context.file.clone(),
                span: self.byte_span(context, attribute.span()),
                message: format!("cannot inspect cfg_attr metadata: {error}"),
            }),
            Ok(false) => {}
        }
        if attribute
            .path()
            .segments
            .first()
            .is_some_and(|segment| segment.ident == "sage")
        {
            let span = self.byte_span(context, attribute.span());
            if !self
                .recognized
                .contains(&(context.file.clone(), span.start))
            {
                self.diagnostics.push(Diagnostic { file: context.file.clone(), span,
                    message: "unsupported Sage attribute or placement; use #[sage::item(...)] on a type, trait, module, function, or method, or #[sage::test(...)] on a #[test] function".into() });
            }
        }
    }
    fn check_attributes(&mut self, syntax: &syn::File, context: &Context) {
        use syn::visit::Visit;
        struct Attributes<'a> {
            index: &'a mut CorrespondenceIndex,
            context: &'a Context,
        }
        impl<'ast> Visit<'ast> for Attributes<'_> {
            fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
                self.index.check_attribute(attribute, self.context);
            }
        }
        Attributes {
            index: self,
            context,
        }
        .visit_file(syntax);
    }
    fn byte_span(&self, context: &Context, span: proc_macro2::Span) -> Span {
        let source = &self.files[&context.file];
        let offset = |position: proc_macro2::LineColumn| {
            let line = source
                .split_inclusive('\n')
                .take(position.line.saturating_sub(1))
                .map(str::len)
                .sum::<usize>();
            line + source[line..]
                .chars()
                .take(position.column)
                .map(char::len_utf8)
                .sum::<usize>()
        };
        Span {
            start: offset(span.start()),
            end: offset(span.end()),
        }
    }
    fn text(&self, context: &Context, span: proc_macro2::Span) -> String {
        let span = self.byte_span(context, span);
        self.files[&context.file][span.start..span.end].to_owned()
    }
    fn conditions(&self, attributes: &[syn::Attribute], context: &Context) -> Vec<String> {
        attributes
            .iter()
            .filter(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"))
            .map(|attr| self.text(context, attr.span()))
            .collect()
    }
}

fn ident(ident: &syn::Ident) -> String {
    ident.to_string().trim_start_matches("r#").to_owned()
}
fn normalize(file: &str) -> String {
    file.replace('\\', "/").trim_start_matches("./").to_owned()
}

/// Cargo metadata exposes `test`, but not `harness`. Read that setting from
/// the manifest so a source declaration in a custom runner cannot masquerade
/// as a test executed by Rust's built-in harness.
#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct CargoTestSettings {
    lib: Option<CargoTargetSettings>,
    bin: Vec<CargoTargetSettings>,
    test: Vec<CargoTargetSettings>,
}

#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct CargoTargetSettings {
    name: Option<String>,
    path: Option<PathBuf>,
    harness: Option<bool>,
}

impl CargoTestSettings {
    fn uses_harness(&self, kind: &[String], name: &str, source: &Path, manifest: &Path) -> bool {
        let settings = if kind.iter().any(|kind| kind == "test") {
            &self.test
        } else if kind.iter().any(|kind| kind == "bin") {
            &self.bin
        } else {
            return self
                .lib
                .as_ref()
                .and_then(|target| target.harness)
                .unwrap_or(true);
        };
        settings
            .iter()
            .find(|target| {
                target.name.as_deref().map_or_else(
                    || {
                        target.path.as_ref().is_some_and(|path| {
                            manifest
                                .parent()
                                .unwrap()
                                .join(path)
                                .canonicalize()
                                .ok()
                                .as_deref()
                                == Some(source)
                        })
                    },
                    |target_name| target_name == name,
                )
            })
            .and_then(|target| target.harness)
            .unwrap_or(true)
    }
}

struct AnchorName(String);

impl Parse for AnchorName {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        use syn::ext::IdentExt;
        let mut parts = Vec::new();
        loop {
            let component = input.call(syn::Ident::parse_any)?;
            let name = component.to_string();
            if !crate::spec::valid_anchor_name(&name) {
                return Err(syn::Error::new(
                    component.span(),
                    "expected an ASCII identifier",
                ));
            }
            parts.push(name);
            if input.is_empty() {
                return Ok(Self(parts.join(".")));
            }
            input.parse::<syn::Token![.]>()?;
        }
    }
}

fn potentially_ignored(meta: &syn::Meta) -> bool {
    if meta.path().is_ident("ignore") {
        return true;
    }
    let syn::Meta::List(list) = meta else {
        return false;
    };
    if !list.path.is_ident("cfg_attr") {
        return false;
    }
    let Ok(arguments) = list.parse_args_with(
        syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
    ) else {
        return true;
    };
    arguments.iter().skip(1).any(potentially_ignored)
}

/// Adapter for the exact `define_kind_symbols!` input grammar in sage-ir.
/// In particular `{ .. }` and the two separators are not ordinary Rust items.
struct SymbolDefinitions(Vec<SymbolDefinition>);
struct SymbolDefinition {
    attrs: Vec<syn::Attribute>,
    name: syn::Ident,
    span: proc_macro2::Span,
}
impl Parse for SymbolDefinitions {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut definitions = Vec::new();
        let attrs = input.call(syn::Attribute::parse_outer)?;
        let structure: syn::ItemStruct = input.parse()?;
        definitions.push(SymbolDefinition {
            attrs,
            name: structure.ident.clone(),
            span: structure.span(),
        });
        let attrs = input.call(syn::Attribute::parse_outer)?;
        let _: syn::Visibility = input.parse()?;
        let start: syn::Token![enum] = input.parse()?;
        let name: syn::Ident = input.parse()?;
        let _: syn::Generics = input.parse()?;
        let content;
        let brace = syn::braced!(content in input);
        let _: syn::Token![..] = content.parse()?;
        if !content.is_empty() {
            return Err(content.error("expected symbol data placeholder { .. }"));
        }
        definitions.push(SymbolDefinition {
            attrs,
            name,
            span: start.span.join(brace.span.close()).unwrap_or(start.span),
        });
        for _ in 0..2 {
            let separator;
            syn::braced!(separator in input);
            if !separator.is_empty() {
                return Err(separator.error("expected empty symbol-family separator"));
            }
            while !input.is_empty() && !input.peek(syn::token::Brace) {
                let declaration: syn::ItemEnum = input.parse()?;
                definitions.push(SymbolDefinition {
                    attrs: declaration.attrs.clone(),
                    name: declaration.ident.clone(),
                    span: declaration.span(),
                });
            }
        }
        if !input.is_empty() {
            return Err(input.error("unexpected tokens after symbol definitions"));
        }
        Ok(Self(definitions))
    }
}
