use std::collections::{BTreeMap, BTreeSet};

use crate::*;

struct Entry<'a> {
    document: &'a Document,
    namespace: String,
    class: &'a Class,
}
struct Model<'a> {
    classes: BTreeMap<String, Entry<'a>>,
    types: BTreeMap<String, usize>,
    values: Vec<(&'a Document, String, &'a TypeDecl)>,
    dependencies: BTreeMap<String, BTreeSet<String>>,
    errors: Vec<Diagnostic>,
}

/// Validate the closed set of documents, including conservative virtual dispatch.
/// This checks the declared model, not the compiler's Rust implementation.
#[sage::item(crate::language::Language::validate)]
pub fn validate(documents: &[Document]) -> Vec<Diagnostic> {
    build_model(documents).check()
}

/// Resolve runtime contracts with exactly the same ownership, inheritance, and
/// virtual target rules used by static model validation.
pub(crate) fn resolve_contracts(
    documents: &[Document],
) -> Result<BTreeMap<String, crate::contracts::OperationContract>, Vec<Diagnostic>> {
    use crate::contracts::{CallPermission, ContractRule, OperationContract, UncheckedEffect};

    let mut model = build_model(documents);
    let errors = model.check();
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut operations = BTreeMap::new();
    for (class_key, entry) in &model.classes {
        for method in &entry.class.methods {
            let id = format!("{class_key}::{}", method.name);
            let mut operation = OperationContract {
                id: id.clone(),
                kind: method.kind.clone(),
                is_abstract: method.envelope.is_none(),
                invokes: Vec::new(),
                prohibits: Vec::new(),
                dispatches: BTreeSet::new(),
                unchecked_effects: BTreeSet::new(),
            };
            // Prohibitions belong to the interface contract as well as its
            // implementation. An override retains rules from every base method.
            for owner in model.ancestors(class_key) {
                let base = &model.classes[&owner];
                let Some(declaration) = base.class.methods.iter().find(|m| m.name == method.name)
                else {
                    continue;
                };
                let rule_operation = format!("{owner}::{}", method.name);
                for effect in declaration.envelope.iter().flatten() {
                    match effect {
                        Effect::Invokes(path) | Effect::ForbidTransitive(path) => {
                            let is_permission = matches!(effect, Effect::Invokes(_));
                            if is_permission && owner != *class_key {
                                continue;
                            }
                            let targets = model.targets(&owner, base, path).map_err(|message| {
                                vec![Diagnostic {
                                    file: base.document.file.clone(),
                                    span: path.span.clone(),
                                    message,
                                }]
                            })?;
                            let rule = CallPermission {
                                targets,
                                same_receiver: path.root == "self",
                                rule: ContractRule {
                                    file: base.document.file.clone(),
                                    span: path.span.clone(),
                                    operation: rule_operation.clone(),
                                },
                            };
                            if is_permission {
                                operation.invokes.push(rule);
                            } else {
                                operation.prohibits.push(rule);
                            }
                        }
                        Effect::Reads(_) => {
                            if owner == *class_key {
                                operation.unchecked_effects.insert(UncheckedEffect::Reads);
                            }
                        }
                        Effect::Creates { .. } => {
                            if owner == *class_key {
                                operation.unchecked_effects.insert(UncheckedEffect::Creates);
                            }
                        }
                    }
                }
            }
            if operation.is_abstract {
                for descendant in model.classes.keys() {
                    if model.ancestors(descendant).contains(class_key)
                        && let Some(target) = model.method(descendant, &method.name)
                        && target != id
                    {
                        operation.dispatches.insert(target);
                    }
                }
            }
            operations.insert(id, operation);
        }
    }
    Ok(operations)
}

fn build_model(documents: &[Document]) -> Model<'_> {
    let mut model = Model {
        classes: BTreeMap::new(),
        types: BTreeMap::new(),
        values: Vec::new(),
        dependencies: BTreeMap::new(),
        errors: Vec::new(),
    };
    let mut roots = BTreeMap::new();
    let mut expected_modules = BTreeSet::new();
    for document in documents {
        let (name, dependencies, modules) = match &document.header {
            Header::Crate {
                name,
                dependencies,
                modules,
                ..
            } => (name, dependencies, modules),
            Header::Module(_) => continue,
        };
        let parent = std::path::Path::new(&document.file)
            .parent()
            .unwrap_or(std::path::Path::new(""));
        if std::path::Path::new(&document.file)
            .file_name()
            .is_none_or(|f| f != "crate.sage")
        {
            model.error(
                document,
                &Span { start: 0, end: 0 },
                "crate declarations belong in crate.sage",
            );
        }
        if roots.insert(name.clone(), parent.to_path_buf()).is_some() {
            model.error(
                document,
                &Span { start: 0, end: 0 },
                format!("duplicate crate {name}"),
            );
        }
        let deps: BTreeSet<_> = dependencies.iter().cloned().collect();
        if deps.len() != dependencies.len() || deps.contains(name) {
            model.error(
                document,
                &Span { start: 0, end: 0 },
                "duplicate or self crate dependency",
            );
        }
        model.dependencies.insert(name.clone(), deps);
        for module in modules {
            if !expected_modules.insert(format!("{name}::{module}")) {
                model.error(
                    document,
                    &Span { start: 0, end: 0 },
                    format!("duplicate module {module}"),
                );
            }
        }
    }
    let mut seen_modules = BTreeSet::new();
    for document in documents {
        let namespace = match &document.header {
            Header::Crate {
                name, dependencies, ..
            } => {
                for dep in dependencies {
                    if !roots.contains_key(dep) {
                        model.error(
                            document,
                            &Span { start: 0, end: 0 },
                            format!("missing dependency crate ${dep}"),
                        );
                    }
                }
                name.clone()
            }
            Header::Module(path) => {
                let file = std::path::Path::new(&document.file);
                let owner = roots
                    .iter()
                    .filter(|(_, root)| file.starts_with(root))
                    .max_by_key(|(_, root)| root.components().count());
                let Some((name, root)) = owner else {
                    model.error(document, &path.span, "module has no containing crate.sage");
                    continue;
                };
                if path.root != "crate" || path.segments.len() != 1 {
                    model.error(
                        document,
                        &path.span,
                        "module must name one manifest module: crate::name",
                    );
                    continue;
                }
                let module = &path.segments[0];
                if file != root.join(format!("{module}.sage")) {
                    model.error(document, &path.span, "module path does not match its file");
                }
                let namespace = format!("{name}::{module}");
                if !expected_modules.contains(&namespace) {
                    model.error(document, &path.span, "module is not declared in crate.sage");
                }
                if !seen_modules.insert(namespace.clone()) {
                    model.error(document, &path.span, "duplicate module");
                }
                namespace
            }
        };
        for declaration in &document.declarations {
            let (name, span) = match declaration {
                Declaration::Type(decl) => (&decl.name, &decl.span),
                Declaration::Class(class) => (&class.name, &class.span),
            };
            let key = format!("{namespace}::{name}");
            let arity = match declaration {
                Declaration::Type(decl) => decl.parameters.len(),
                Declaration::Class(_) => 0,
            };
            if model.types.insert(key.clone(), arity).is_some() {
                model.error(document, span, format!("duplicate declaration {key}"));
            }
            match declaration {
                Declaration::Type(decl) => model.values.push((document, namespace.clone(), decl)),
                Declaration::Class(class) => {
                    model.classes.insert(
                        key,
                        Entry {
                            document,
                            namespace: namespace.clone(),
                            class,
                        },
                    );
                }
            }
        }
    }
    for missing in expected_modules.difference(&seen_modules) {
        model.errors.push(Diagnostic {
            file: "sage-src".into(),
            span: Span { start: 0, end: 0 },
            message: format!("missing module {missing}"),
        });
    }
    model
}

impl Model<'_> {
    fn error(&mut self, document: &Document, span: &Span, message: impl Into<String>) {
        self.errors.push(Diagnostic {
            file: document.file.clone(),
            span: span.clone(),
            message: message.into(),
        });
    }
    fn resolve(&self, entry: &Entry<'_>, path: &Path) -> Result<String, String> {
        self.resolve_path(&entry.namespace, path)
    }
    fn resolve_path(&self, namespace: &str, path: &Path) -> Result<String, String> {
        let owner = namespace.split("::").next().unwrap();
        let root = if path.root == "crate" {
            owner
        } else if let Some(external) = path.root.strip_prefix('$') {
            if external != owner
                && !self
                    .dependencies
                    .get(owner)
                    .is_some_and(|deps| deps.contains(external))
            {
                return Err(format!("undeclared dependency ${external}"));
            }
            external
        } else {
            return Err("self is only valid for field reads and method calls".into());
        };
        Ok(std::iter::once(root)
            .chain(path.segments.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("::"))
    }
    fn base(&self, key: &str) -> Option<String> {
        let entry = self.classes.get(key)?;
        self.resolve(entry, entry.class.base.as_ref()?).ok()
    }
    fn ancestors(&self, key: &str) -> Vec<String> {
        let mut ancestors = Vec::new();
        let mut next = Some(key.to_owned());
        while let Some(key) = next {
            if ancestors.contains(&key) {
                break;
            }
            next = self.base(&key);
            ancestors.push(key);
        }
        ancestors
    }
    fn method(&self, class: &str, name: &str) -> Option<String> {
        self.ancestors(class)
            .into_iter()
            .find(|key| {
                self.classes
                    .get(key)
                    .is_some_and(|e| e.class.methods.iter().any(|m| m.name == name))
            })
            .map(|key| format!("{key}::{name}"))
    }
    fn targets(
        &self,
        class_key: &str,
        entry: &Entry<'_>,
        path: &Path,
    ) -> Result<BTreeSet<String>, String> {
        let (class, name) = if path.root == "self" && path.segments.len() == 1 {
            (class_key.to_owned(), path.segments[0].clone())
        } else {
            let resolved = self.resolve(entry, path)?;
            let Some((class, name)) = resolved.rsplit_once("::") else {
                return Err("expected a class method path".into());
            };
            (class.to_owned(), name.to_owned())
        };
        if !self.classes.contains_key(&class) {
            return Err(format!("unknown class {class}"));
        }
        let Some(declared) = self.method(&class, &name) else {
            return Err(format!("unknown method {class}::{name}"));
        };
        let mut targets = BTreeSet::from([declared]);
        for descendant in self.classes.keys() {
            if self.ancestors(descendant).contains(&class)
                && let Some(target) = self.method(descendant, &name)
            {
                targets.insert(target);
            }
        }
        Ok(targets)
    }
    fn check(&mut self) -> Vec<Diagnostic> {
        let mut errors = Vec::new();
        self.check_values(&mut errors);
        let mut graph = BTreeMap::<String, BTreeSet<String>>::new();
        let mut prohibitions = Vec::new();
        for (key, entry) in &self.classes {
            let report = |errors: &mut Vec<Diagnostic>, span: &Span, message: String| {
                errors.push(Diagnostic {
                    file: entry.document.file.clone(),
                    span: span.clone(),
                    message,
                });
            };
            if let Some(base) = &entry.class.base {
                match self.resolve(entry, base) {
                    Ok(target) if self.classes.contains_key(&target) => {
                        if self.ancestors(&target).contains(key) {
                            report(&mut errors, &base.span, "inheritance cycle".into());
                        }
                        if (entry.class.kind == ClassKind::Context)
                            != (self.classes[&target].class.kind == ClassKind::Context)
                        {
                            report(
                                &mut errors,
                                &base.span,
                                "contexts and symbol classes cannot inherit from each other".into(),
                            );
                        }
                    }
                    Ok(target) => report(
                        &mut errors,
                        &base.span,
                        format!("unknown base class {target}"),
                    ),
                    Err(message) => report(&mut errors, &base.span, message),
                }
            }
            let mut names = BTreeSet::new();
            for field in &entry.class.fields {
                if !names.insert(&field.name) {
                    report(
                        &mut errors,
                        &field.span,
                        format!("duplicate member {}", field.name),
                    );
                }
                if field.kind == FieldKind::State && entry.class.kind != ClassKind::Context {
                    report(
                        &mut errors,
                        &field.span,
                        "state fields require a context".into(),
                    );
                }
                if self.ancestors(key).iter().skip(1).any(|base| {
                    self.classes
                        .get(base)
                        .is_some_and(|e| e.class.fields.iter().any(|f| f.name == field.name))
                }) {
                    report(
                        &mut errors,
                        &field.span,
                        "inherited fields cannot be redeclared".into(),
                    );
                }
                if let Err(message) = self.resolve_type(&entry.namespace, &[], &field.ty) {
                    report(&mut errors, &field.ty.span, message);
                }
            }
            for method in &entry.class.methods {
                let method_key = format!("{key}::{}", method.name);
                graph.entry(method_key.clone()).or_default();
                if !names.insert(&method.name) {
                    report(
                        &mut errors,
                        &method.span,
                        format!("duplicate member {}", method.name),
                    );
                }
                if let Err(message) = self.resolve_type(&entry.namespace, &[], &method.result) {
                    report(&mut errors, &method.result.span, message);
                }
                let inherited = self
                    .base(key)
                    .and_then(|base| self.method(&base, &method.name));
                if method.is_override != inherited.is_some() {
                    report(
                        &mut errors,
                        &method.span,
                        "override must exactly match an inherited method".into(),
                    );
                }
                if let Some(inherited) = inherited {
                    let (base, name) = inherited.rsplit_once("::").unwrap();
                    let base_entry = &self.classes[base];
                    let base_method = base_entry
                        .class
                        .methods
                        .iter()
                        .find(|m| m.name == name)
                        .unwrap();
                    if base_method.kind != method.kind
                        || self.resolve_type(&base_entry.namespace, &[], &base_method.result)
                            != self.resolve_type(&entry.namespace, &[], &method.result)
                    {
                        report(
                            &mut errors,
                            &method.span,
                            "override must preserve method kind and result type".into(),
                        );
                    }
                }
                let Some(effects) = &method.envelope else {
                    if entry.class.kind != ClassKind::Abstract {
                        report(
                            &mut errors,
                            &method.span,
                            "abstract methods require an abstract class".into(),
                        );
                    }
                    continue;
                };
                let mut contexts = BTreeSet::new();
                for effect in effects {
                    match effect {
                        Effect::Invokes(path) | Effect::ForbidTransitive(path) => match self
                            .targets(key, entry, path)
                        {
                            Ok(targets) => {
                                if matches!(effect, Effect::Invokes(_)) {
                                    graph.entry(method_key.clone()).or_default().extend(targets);
                                } else {
                                    prohibitions.push((
                                        method_key.clone(),
                                        targets,
                                        entry.document,
                                        path.span.clone(),
                                    ));
                                }
                            }
                            Err(message) => report(&mut errors, &path.span, message),
                        },
                        Effect::Reads(path) => {
                            let valid = path.root == "self"
                                && path.segments.len() == 1
                                && self.ancestors(key).iter().any(|ancestor| {
                                    self.classes.get(ancestor).is_some_and(|e| {
                                        e.class.fields.iter().any(|f| f.name == path.segments[0])
                                    })
                                });
                            if !valid {
                                report(
                                    &mut errors,
                                    &path.span,
                                    "reads must reference an existing self field".into(),
                                );
                            }
                        }
                        Effect::Creates { name, context } => {
                            if !contexts.insert(name) {
                                report(
                                    &mut errors,
                                    &context.span,
                                    format!("duplicate context binding {name}"),
                                );
                            }
                            match self.resolve(entry, context) {
                                Ok(target)
                                    if self
                                        .classes
                                        .get(&target)
                                        .is_some_and(|e| e.class.kind == ClassKind::Context) => {}
                                Ok(target) => report(
                                    &mut errors,
                                    &context.span,
                                    format!("not a context: {target}"),
                                ),
                                Err(message) => report(&mut errors, &context.span, message),
                            }
                        }
                    }
                }
            }
            if entry.class.kind != ClassKind::Abstract {
                for ancestor in self.ancestors(key) {
                    if let Some(base) = self.classes.get(&ancestor) {
                        for method in &base.class.methods {
                            if let Some(selected) = self.method(key, &method.name) {
                                let (owner, name) = selected.rsplit_once("::").unwrap();
                                if self.classes[owner]
                                    .class
                                    .methods
                                    .iter()
                                    .any(|m| m.name == name && m.envelope.is_none())
                                {
                                    report(
                                        &mut errors,
                                        &entry.class.span,
                                        format!("concrete class does not implement {selected}"),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        for (start, forbidden, document, span) in prohibitions {
            let mut queue = std::collections::VecDeque::new();
            let mut seen = BTreeSet::new();
            // A base method's prohibition also constrains each overriding implementation.
            let (owner, name) = start.rsplit_once("::").unwrap();
            let implementations: BTreeSet<_> = self
                .classes
                .keys()
                .filter(|class| self.ancestors(class).iter().any(|base| base == owner))
                .filter_map(|class| self.method(class, name))
                .collect();
            for implementation in implementations {
                for target in graph.get(&implementation).into_iter().flatten() {
                    queue.push_back(vec![implementation.clone(), target.clone()]);
                }
            }
            while let Some(path) = queue.pop_front() {
                let last = path.last().unwrap();
                if forbidden.contains(last) {
                    errors.push(Diagnostic {
                        file: document.file.clone(),
                        span: span.clone(),
                        message: format!("forbidden transitive call: {}", path.join(" -> ")),
                    });
                    break;
                }
                if !seen.insert(last.clone()) {
                    continue;
                }
                for next in graph.get(last).into_iter().flatten() {
                    let mut path = path.clone();
                    path.push(next.clone());
                    queue.push_back(path);
                }
            }
        }
        self.errors.extend(errors);
        std::mem::take(&mut self.errors)
    }
}

impl Model<'_> {
    /// A structural canonical spelling, independent of source ranges and path spelling.
    fn resolve_type(
        &self,
        namespace: &str,
        parameters: &[String],
        ty: &TypeExpr,
    ) -> Result<String, String> {
        match &ty.kind {
            TypeExprKind::Tuple(items) => {
                let items = items
                    .iter()
                    .map(|item| self.resolve_type(namespace, parameters, item))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(format!("({})", items.join(",")))
            }
            TypeExprKind::Named { path, arguments } => {
                let (key, arity) = if path.root == "crate" || path.root.starts_with('$') {
                    let key = self.resolve_path(namespace, path)?;
                    let arity = *self
                        .types
                        .get(&key)
                        .ok_or_else(|| format!("unknown type {key}"))?;
                    (key, arity)
                } else if path.root == "self" || !path.segments.is_empty() {
                    return Err("type names must be local names or crate-qualified paths".into());
                } else if parameters.contains(&path.root) {
                    (format!("parameter::{}", path.root), 0)
                } else {
                    let local = format!("{namespace}::{}", path.root);
                    if let Some(arity) = self.types.get(&local) {
                        (local, *arity)
                    } else {
                        let arity = match path.root.as_str() {
                            "Vec" | "Option" => 1,
                            "bool" | "u32" | "u64" | "String" => 0,
                            _ => return Err(format!("unknown type {}", path.root)),
                        };
                        (format!("builtin::{}", path.root), arity)
                    }
                };
                if arguments.len() != arity {
                    return Err(format!(
                        "type {key} expects {arity} type arguments, found {}",
                        arguments.len()
                    ));
                }
                let arguments = arguments
                    .iter()
                    .map(|arg| self.resolve_type(namespace, parameters, arg))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(format!("{key}<{}>", arguments.join(",")))
            }
        }
    }

    fn check_values(&self, errors: &mut Vec<Diagnostic>) {
        for (document, namespace, declaration) in &self.values {
            let report = |errors: &mut Vec<Diagnostic>, span: &Span, message: String| {
                errors.push(Diagnostic {
                    file: document.file.clone(),
                    span: span.clone(),
                    message,
                });
            };
            let mut parameters = BTreeSet::new();
            for parameter in &declaration.parameters {
                if !parameters.insert(parameter) {
                    report(
                        errors,
                        &declaration.span,
                        format!("duplicate type parameter {parameter}"),
                    );
                }
                if matches!(parameter.as_str(), "crate" | "self") {
                    report(
                        errors,
                        &declaration.span,
                        format!("reserved type parameter {parameter}"),
                    );
                }
            }
            let check_fields = |errors: &mut Vec<Diagnostic>, fields: &[ValueField]| {
                let mut names = BTreeSet::new();
                for field in fields {
                    if !names.insert(&field.name) {
                        report(
                            errors,
                            &field.span,
                            format!("duplicate field {}", field.name),
                        );
                    }
                    if let Err(message) =
                        self.resolve_type(namespace, &declaration.parameters, &field.ty)
                    {
                        report(errors, &field.ty.span, message);
                    }
                }
            };
            match &declaration.shape {
                TypeShape::Opaque => (),
                TypeShape::Struct(fields) => check_fields(errors, fields),
                TypeShape::Enum(variants) => {
                    let mut names = BTreeSet::new();
                    for variant in variants {
                        if !names.insert(&variant.name) {
                            report(
                                errors,
                                &variant.span,
                                format!("duplicate variant {}", variant.name),
                            );
                        }
                        match &variant.fields {
                            VariantFields::Unit => (),
                            VariantFields::Named(fields) => check_fields(errors, fields),
                            VariantFields::Tuple(fields) => {
                                for field in fields {
                                    if let Err(message) =
                                        self.resolve_type(namespace, &declaration.parameters, field)
                                    {
                                        report(errors, &field.span, message);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
