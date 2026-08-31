extern crate self as sage_reflect;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use sage_stash::{Ptr, Slice, Stash, StashData, StashHash, Stashed};
use serde::{Deserialize, Serialize};

pub use sage_reflect_derive::Reflect;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ValueNode {
    Record {
        type_name: String,
        fields: Vec<ValueField>,
    },
    Variant {
        enum_name: String,
        variant_name: String,
        fields: Vec<ValueField>,
    },
    Sequence {
        type_name: String,
        items: Vec<ValueNode>,
    },
    Scalar {
        type_name: String,
        value: ScalarValue,
    },
    Reference {
        target: SymbolReference,
    },
    Shared {
        identity: String,
        value: Box<ValueNode>,
    },
    SharedReference {
        identity: String,
    },
    Truncated {
        summary: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        continuation: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ValueField {
    pub name: String,
    pub value: ValueNode,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScalarValue {
    Null,
    Bool(bool),
    Number(i64),
    String(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolReference {
    pub path: String,
    pub label: String,
    pub presentation: SymbolPresentation,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolPresentation {
    pub eyebrow: Option<String>,
    pub badges: Vec<Badge>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Badge {
    pub label: String,
    pub tone: BadgeTone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BadgeTone {
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ReferenceKey {
    pub family: &'static str,
    pub id: u64,
}

pub trait ReflectionResolver {
    fn symbol_reference(&self, key: &ReferenceKey) -> Option<SymbolReference>;

    fn reflected_value(&self, key: &ReferenceKey) -> Option<ValueNode>;
}

pub struct ReflectionContext<'resolver> {
    max_depth: usize,
    max_nodes: usize,
    depth: usize,
    continuation_prefix: String,
    continuations: Rc<RefCell<ContinuationStore>>,
    remaining_build_nodes: Rc<Cell<usize>>,
    shared: Rc<RefCell<HashSet<String>>>,
    stash_namespaces: Rc<RefCell<StashNamespaceStore>>,
    resolver: Option<&'resolver dyn ReflectionResolver>,
    allow_continuations: bool,
}

struct ContinuationStore {
    next: usize,
    remaining_nodes: usize,
    values: HashMap<String, Vec<ValueNode>>,
}

struct StashNamespaceStore {
    next: u64,
    by_address: HashMap<usize, u64>,
}

impl<'resolver> ReflectionContext<'resolver> {
    pub fn new(max_depth: usize, max_nodes: usize) -> Self {
        Self::with_continuation_prefix(max_depth, max_nodes, "cont")
    }

    pub fn with_continuation_prefix(
        max_depth: usize,
        max_nodes: usize,
        prefix: impl Into<String>,
    ) -> Self {
        Self {
            max_depth,
            max_nodes,
            depth: 0,
            continuation_prefix: prefix.into(),
            continuations: Rc::new(RefCell::new(ContinuationStore {
                next: 0,
                remaining_nodes: max_nodes,
                values: HashMap::new(),
            })),
            remaining_build_nodes: Rc::new(Cell::new(max_nodes)),
            shared: Rc::new(RefCell::new(HashSet::new())),
            stash_namespaces: Rc::new(RefCell::new(StashNamespaceStore {
                next: 0,
                by_address: HashMap::new(),
            })),
            resolver: None,
            allow_continuations: true,
        }
    }

    pub fn with_resolver(mut self, resolver: &'resolver dyn ReflectionResolver) -> Self {
        self.resolver = Some(resolver);
        self
    }

    pub fn reflect_node(
        &mut self,
        summary: &str,
        reflect: impl FnOnce(&mut ReflectionContext<'resolver>) -> ValueNode,
    ) -> ValueNode {
        if !self.claim_build_node() {
            return Self::terminal_truncation("value omitted by the node limit");
        }
        if self.depth >= self.max_depth {
            if !self.allow_continuations {
                return Self::terminal_truncation(summary);
            }
            let Some(reservation) = self.reserve_continuation() else {
                return Self::terminal_truncation(summary);
            };
            let mut page = Self {
                max_depth: self.max_depth,
                max_nodes: self.max_nodes,
                depth: 0,
                continuation_prefix: self.continuation_prefix.clone(),
                continuations: self.continuations.clone(),
                remaining_build_nodes: self.remaining_build_nodes.clone(),
                shared: self.shared.clone(),
                stash_namespaces: self.stash_namespaces.clone(),
                resolver: self.resolver,
                allow_continuations: false,
            };
            let value = reflect(&mut page);
            return self.store_continuation(summary, reservation, value);
        }

        self.depth += 1;
        let reflected = reflect(self);
        self.depth -= 1;
        reflected
    }

    fn claim_build_node(&self) -> bool {
        let remaining = self.remaining_build_nodes.get();
        if remaining == 0 {
            return false;
        }
        self.remaining_build_nodes.set(remaining - 1);
        true
    }

    fn can_reflect_more(&self) -> bool {
        self.remaining_build_nodes.get() > 0
    }

    /// Apply the global output budget after a value has been structurally
    /// reflected. The visible root and all retained continuation pages are
    /// independently bounded by `max_nodes`, so a product retains at most
    /// twice that many value nodes.
    pub fn finish(&mut self, value: ValueNode) -> ValueNode {
        let mut budget = self.max_nodes;
        if budget == 0 {
            return self.freeze_continuation("value omitted by the node limit", value);
        }
        self.bound_node(value, &mut budget, true)
    }

    fn bound_node(
        &mut self,
        mut node: ValueNode,
        budget: &mut usize,
        allow_continuation: bool,
    ) -> ValueNode {
        if *budget == 0 {
            return Self::terminal_truncation("additional value nodes omitted");
        }
        *budget -= 1;
        let has_children = match &node {
            ValueNode::Record { fields, .. } | ValueNode::Variant { fields, .. } => {
                !fields.is_empty()
            }
            ValueNode::Sequence { items, .. } => !items.is_empty(),
            ValueNode::Shared { .. } => true,
            ValueNode::Scalar { .. }
            | ValueNode::Reference { .. }
            | ValueNode::SharedReference { .. }
            | ValueNode::Truncated { .. } => false,
        };
        if has_children && *budget == 0 {
            return self.cutoff("nested value omitted", node, allow_continuation);
        }

        match &mut node {
            ValueNode::Record { type_name, fields } => {
                let omitted_type = type_name.clone();
                self.bound_fields(fields, budget, allow_continuation, move |fields| {
                    ValueNode::record(omitted_type, fields)
                });
            }
            ValueNode::Variant {
                enum_name,
                variant_name,
                fields,
            } => {
                let omitted_enum = enum_name.clone();
                let omitted_variant = variant_name.clone();
                self.bound_fields(fields, budget, allow_continuation, move |fields| {
                    ValueNode::variant(omitted_enum, omitted_variant, fields)
                });
            }
            ValueNode::Sequence { type_name, items } => {
                let omitted_type = type_name.clone();
                self.bound_items(items, budget, allow_continuation, move |items| {
                    ValueNode::Sequence {
                        type_name: omitted_type,
                        items,
                    }
                });
            }
            ValueNode::Shared { value, .. } => {
                let child = std::mem::replace(
                    value,
                    Box::new(Self::terminal_truncation("shared value omitted")),
                );
                **value = self.bound_node(*child, budget, allow_continuation);
            }
            ValueNode::Scalar { .. }
            | ValueNode::Reference { .. }
            | ValueNode::SharedReference { .. }
            | ValueNode::Truncated { .. } => {}
        }
        node
    }

    fn bound_fields(
        &mut self,
        fields: &mut Vec<ValueField>,
        budget: &mut usize,
        allow_continuation: bool,
        omitted_node: impl FnOnce(Vec<ValueField>) -> ValueNode,
    ) {
        let mut source = std::mem::take(fields).into_iter().peekable();
        while let Some(mut field) = source.next() {
            let has_more = source.peek().is_some();
            if has_more && *budget <= 1 {
                let mut omitted = vec![field];
                omitted.extend(source);
                if *budget == 1 {
                    *budget -= 1;
                    fields.push(ValueField::new(
                        "…",
                        self.cutoff(
                            "additional fields omitted",
                            omitted_node(omitted),
                            allow_continuation,
                        ),
                    ));
                }
                return;
            }
            if *budget == 0 {
                return;
            }
            let reserve = usize::from(has_more);
            let mut child_budget = *budget - reserve;
            field.value = self.bound_node(field.value, &mut child_budget, allow_continuation);
            *budget = child_budget + reserve;
            fields.push(field);
        }
    }

    fn bound_items(
        &mut self,
        items: &mut Vec<ValueNode>,
        budget: &mut usize,
        allow_continuation: bool,
        omitted_node: impl FnOnce(Vec<ValueNode>) -> ValueNode,
    ) {
        let mut source = std::mem::take(items).into_iter().peekable();
        while let Some(item) = source.next() {
            let has_more = source.peek().is_some();
            if has_more && *budget <= 1 {
                let mut omitted = vec![item];
                omitted.extend(source);
                if *budget == 1 {
                    *budget -= 1;
                    items.push(self.cutoff(
                        "additional sequence elements omitted",
                        omitted_node(omitted),
                        allow_continuation,
                    ));
                }
                return;
            }
            if *budget == 0 {
                return;
            }
            let reserve = usize::from(has_more);
            let mut child_budget = *budget - reserve;
            items.push(self.bound_node(item, &mut child_budget, allow_continuation));
            *budget = child_budget + reserve;
        }
    }

    fn cutoff(&mut self, summary: &str, value: ValueNode, allow_continuation: bool) -> ValueNode {
        if allow_continuation {
            self.freeze_continuation(summary, value)
        } else {
            Self::terminal_truncation(summary)
        }
    }

    fn freeze_continuation(&mut self, summary: &str, value: ValueNode) -> ValueNode {
        let Some(reservation) = self.reserve_continuation() else {
            return Self::terminal_truncation(summary);
        };
        self.store_continuation(summary, reservation, value)
    }

    fn reserve_continuation(&self) -> Option<(String, usize)> {
        {
            let mut continuations = self.continuations.borrow_mut();
            (continuations.remaining_nodes > 0).then(|| {
                let handle = format!("{}_{}", self.continuation_prefix, continuations.next);
                continuations.next += 1;
                let budget = std::mem::take(&mut continuations.remaining_nodes);
                (handle, budget)
            })
        }
    }

    fn store_continuation(
        &mut self,
        summary: &str,
        (handle, mut budget): (String, usize),
        value: ValueNode,
    ) -> ValueNode {
        let value = self.bound_node(value, &mut budget, false);
        self.continuations
            .borrow_mut()
            .values
            .insert(handle.clone(), vec![value]);
        ValueNode::Truncated {
            summary: summary.to_owned(),
            continuation: Some(handle),
        }
    }

    fn terminal_truncation(summary: &str) -> ValueNode {
        ValueNode::Truncated {
            summary: summary.to_owned(),
            continuation: None,
        }
    }

    pub fn continuations(&self) -> HashMap<String, Vec<ValueNode>> {
        self.continuations.borrow().values.clone()
    }

    pub fn symbol_reference(&self, key: &ReferenceKey) -> Option<SymbolReference> {
        self.resolver?.symbol_reference(key)
    }

    pub fn reflected_value(&self, key: &ReferenceKey) -> Option<ValueNode> {
        self.resolver?.reflected_value(key)
    }

    fn mark_shared(&mut self, identity: &str) -> bool {
        self.shared.borrow_mut().insert(identity.to_owned())
    }

    fn stash_namespace(&mut self, stash: &Stash) -> u64 {
        let address = std::ptr::from_ref(stash).addr();
        let mut namespaces = self.stash_namespaces.borrow_mut();
        if let Some(namespace) = namespaces.by_address.get(&address) {
            return *namespace;
        }
        let namespace = namespaces.next;
        namespaces.next += 1;
        namespaces.by_address.insert(address, namespace);
        namespace
    }
}

impl Default for ReflectionContext<'_> {
    fn default() -> Self {
        Self::new(64, 10_000)
    }
}

pub trait Reflect<'db> {
    fn reflect(&self, context: &mut ReflectionContext<'_>, stash: Option<&Stash>) -> ValueNode;

    fn reflected(&self) -> ValueNode {
        let mut context = ReflectionContext::default();
        let value = self.reflect(&mut context, None);
        context.finish(value)
    }
}

impl ValueNode {
    pub fn record(type_name: impl Into<String>, fields: Vec<ValueField>) -> Self {
        Self::Record {
            type_name: type_name.into(),
            fields,
        }
    }

    pub fn variant(
        enum_name: impl Into<String>,
        variant_name: impl Into<String>,
        fields: Vec<ValueField>,
    ) -> Self {
        Self::Variant {
            enum_name: enum_name.into(),
            variant_name: variant_name.into(),
            fields,
        }
    }

    pub fn scalar(type_name: impl Into<String>, value: impl Into<ScalarValue>) -> Self {
        Self::Scalar {
            type_name: type_name.into(),
            value: value.into(),
        }
    }
}

impl ValueField {
    pub fn new(name: impl Into<String>, value: ValueNode) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }
}

macro_rules! scalar_reflect {
    ($($ty:ty => $name:literal),* $(,)?) => {
        $(
            impl<'db> Reflect<'db> for $ty {
                fn reflect(
                    &self,
                    context: &mut ReflectionContext<'_>,
                    _stash: Option<&Stash>,
                ) -> ValueNode {
                    context.reflect_node($name, |_context| ValueNode::scalar($name, *self))
                }
            }
        )*
    };
}

scalar_reflect!(
    bool => "bool",
    i8 => "i8",
    i16 => "i16",
    i32 => "i32",
    i64 => "i64",
    u8 => "u8",
    u16 => "u16",
    u32 => "u32",
);

impl<'db> Reflect<'db> for u64 {
    fn reflect(&self, context: &mut ReflectionContext<'_>, _stash: Option<&Stash>) -> ValueNode {
        context.reflect_node("u64", |_context| ValueNode::scalar("u64", *self))
    }
}

impl<'db> Reflect<'db> for () {
    fn reflect(&self, context: &mut ReflectionContext<'_>, _stash: Option<&Stash>) -> ValueNode {
        context.reflect_node("()", |_context| ValueNode::record("()", vec![]))
    }
}

impl<'db> Reflect<'db> for usize {
    fn reflect(&self, context: &mut ReflectionContext<'_>, _stash: Option<&Stash>) -> ValueNode {
        context.reflect_node("usize", |_context| {
            ValueNode::scalar("usize", self.to_string())
        })
    }
}

impl<'db> Reflect<'db> for str {
    fn reflect(&self, context: &mut ReflectionContext<'_>, _stash: Option<&Stash>) -> ValueNode {
        context.reflect_node("str", |_context| ValueNode::scalar("str", self))
    }
}

impl<'db> Reflect<'db> for String {
    fn reflect(&self, context: &mut ReflectionContext<'_>, _stash: Option<&Stash>) -> ValueNode {
        context.reflect_node("String", |_context| {
            ValueNode::scalar("String", self.as_str())
        })
    }
}

impl<'db, T: Reflect<'db>> Reflect<'db> for Option<T> {
    fn reflect(&self, context: &mut ReflectionContext<'_>, stash: Option<&Stash>) -> ValueNode {
        context.reflect_node("Option", |context| match self {
            Some(value) => ValueNode::variant(
                "Option",
                "Some",
                vec![ValueField::new("0", value.reflect(context, stash))],
            ),
            None => ValueNode::variant("Option", "None", vec![]),
        })
    }
}

impl<'db, T: Reflect<'db>> Reflect<'db> for [T] {
    fn reflect(&self, context: &mut ReflectionContext<'_>, stash: Option<&Stash>) -> ValueNode {
        reflect_sequence("slice", self, context, stash)
    }
}

impl<'db, T: Reflect<'db>> Reflect<'db> for Vec<T> {
    fn reflect(&self, context: &mut ReflectionContext<'_>, stash: Option<&Stash>) -> ValueNode {
        reflect_sequence("Vec", self, context, stash)
    }
}

fn reflect_sequence<'db, T: Reflect<'db>>(
    type_name: &'static str,
    values: &[T],
    context: &mut ReflectionContext<'_>,
    stash: Option<&Stash>,
) -> ValueNode {
    context.reflect_node(type_name, |context| {
        let mut items = Vec::with_capacity(values.len().min(context.max_nodes));
        for item in values {
            if !context.can_reflect_more() {
                items.push(ReflectionContext::terminal_truncation(
                    "additional sequence elements omitted",
                ));
                break;
            }
            items.push(item.reflect(context, stash));
        }
        ValueNode::Sequence {
            type_name: type_name.to_owned(),
            items,
        }
    })
}

impl<'db, T: Reflect<'db> + ?Sized> Reflect<'db> for &T {
    fn reflect(&self, context: &mut ReflectionContext<'_>, stash: Option<&Stash>) -> ValueNode {
        (*self).reflect(context, stash)
    }
}

impl<'db, T> Reflect<'db> for Ptr<T>
where
    T: StashData<'db> + Reflect<'db> + Copy + std::fmt::Debug,
{
    fn reflect(&self, context: &mut ReflectionContext<'_>, stash: Option<&Stash>) -> ValueNode {
        context.reflect_node("stash pointer", |context| {
            let Some(stash) = stash else {
                return ValueNode::scalar("Unavailable", "stash pointer outside its owning stash");
            };
            let identity = format!(
                "stash-{}-ptr:{}:{}",
                context.stash_namespace(stash),
                std::any::type_name::<T>(),
                self.reflection_index()
            );
            if !context.mark_shared(&identity) {
                return ValueNode::SharedReference { identity };
            }
            ValueNode::Shared {
                identity,
                value: Box::new(stash[*self].reflect(context, Some(stash))),
            }
        })
    }
}

impl<'db, T> Reflect<'db> for Slice<T>
where
    T: StashData<'db> + Reflect<'db> + Copy + std::fmt::Debug,
{
    fn reflect(&self, context: &mut ReflectionContext<'_>, stash: Option<&Stash>) -> ValueNode {
        context.reflect_node("stash slice", |context| {
            let Some(stash) = stash else {
                return ValueNode::scalar("Unavailable", "stash slice outside its owning stash");
            };
            let identity = format!(
                "stash-{}-slice:{}:{}",
                context.stash_namespace(stash),
                std::any::type_name::<T>(),
                self.reflection_index()
            );
            if !context.mark_shared(&identity) {
                return ValueNode::SharedReference { identity };
            }
            ValueNode::Shared {
                identity,
                value: Box::new(stash[*self].reflect(context, Some(stash))),
            }
        })
    }
}

impl<'db, T> Reflect<'db> for Stashed<T>
where
    T: StashHash + Reflect<'db> + Copy,
{
    fn reflect(&self, context: &mut ReflectionContext<'_>, _stash: Option<&Stash>) -> ValueNode {
        context.reflect_node("Stashed", |context| {
            let (stash, root) = self.open();
            ValueNode::record(
                "Stashed",
                vec![ValueField::new("root", root.reflect(context, Some(stash)))],
            )
        })
    }
}

impl From<bool> for ScalarValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

macro_rules! numeric_scalar {
    ($($ty:ty),* $(,)?) => {
        $(
            impl From<$ty> for ScalarValue {
                fn from(value: $ty) -> Self {
                    Self::Number(value as i64)
                }
            }
        )*
    };
}

numeric_scalar!(i8, i16, i32, i64, u8, u16, u32);

impl From<u64> for ScalarValue {
    fn from(value: u64) -> Self {
        i64::try_from(value).map_or_else(|_| Self::String(value.to_string()), Self::Number)
    }
}

impl From<String> for ScalarValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for ScalarValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sage_stash::AllocStashData;
    use std::cell::Cell;

    #[derive(Reflect)]
    struct Example {
        label: String,
        values: Vec<u32>,
    }

    #[derive(Reflect)]
    enum Choice {
        Unit,
        Tuple(u32, bool),
        Record { value: String },
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, Reflect)]
    struct SharedPair {
        left: Ptr<u32>,
        right: Ptr<u32>,
    }

    #[derive(Reflect)]
    struct NestedStash {
        value: Stashed<Ptr<u32>>,
    }

    #[derive(Reflect)]
    struct StashesAcrossContinuation {
        nested: NestedStash,
        direct: Stashed<Ptr<u32>>,
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, Reflect)]
    struct NestedUnit {
        value: (),
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, Reflect)]
    struct NestedPointer {
        value: Ptr<u32>,
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData, Reflect)]
    struct SharedAfterContinuationExhaustion {
        exhaust: NestedUnit,
        discarded: NestedPointer,
        retained: Ptr<u32>,
    }

    #[derive(Clone, Copy)]
    struct Counted<'a>(&'a Cell<usize>);

    impl<'db> Reflect<'db> for Counted<'_> {
        fn reflect(
            &self,
            context: &mut ReflectionContext<'_>,
            _stash: Option<&Stash>,
        ) -> ValueNode {
            self.0.set(self.0.get() + 1);
            context.reflect_node("Counted", |_context| ValueNode::scalar("Counted", 0_u32))
        }
    }

    #[test]
    fn derive_preserves_fields() {
        let value = Example {
            label: "value".to_owned(),
            values: vec![1, 2],
        };
        let ValueNode::Record { fields, .. } = value.reflected() else {
            panic!("expected record")
        };
        assert_eq!(fields[0].name, "label");
        assert_eq!(fields[1].name, "values");
        assert!(matches!(
            &fields[0].value,
            ValueNode::Scalar { type_name, value: ScalarValue::String(value) }
                if type_name == "String" && value == "value"
        ));
        assert!(matches!(
            &fields[1].value,
            ValueNode::Sequence { type_name, items }
                if type_name == "Vec" && items.len() == 2
        ));
    }

    #[test]
    fn derive_preserves_every_enum_payload_shape() {
        assert!(
            matches!(Choice::Unit.reflected(), ValueNode::Variant { variant_name, fields, .. } if variant_name == "Unit" && fields.is_empty())
        );
        assert!(
            matches!(Choice::Tuple(1, true).reflected(), ValueNode::Variant { variant_name, fields, .. } if variant_name == "Tuple" && fields.len() == 2)
        );
        assert!(
            matches!(Choice::Record { value: "x".to_owned() }.reflected(), ValueNode::Variant { variant_name, fields, .. } if variant_name == "Record" && fields[0].name == "value")
        );
    }

    #[test]
    fn truncation_retains_a_frozen_continuation_page() {
        let value = Example {
            label: "value".to_owned(),
            values: vec![1, 2],
        };
        let mut context = ReflectionContext::with_continuation_prefix(0, 10, "product-7");
        let raw = value.reflect(&mut context, None);
        let node = context.finish(raw);
        let ValueNode::Truncated { continuation, .. } = node else {
            panic!("expected a truncation node")
        };
        assert_eq!(continuation.as_deref(), Some("product-7_0"));
        assert!(matches!(
            context.continuations()[continuation.as_ref().unwrap()].as_slice(),
            [ValueNode::Record { type_name, .. }] if type_name == "Example"
        ));
    }

    #[test]
    fn node_budget_bounds_root_and_all_continuations() {
        let value = vec![1_u32; 100];
        let mut context = ReflectionContext::with_continuation_prefix(64, 8, "bounded");
        let raw = value.reflect(&mut context, None);
        let root = context.finish(raw);
        fn count(node: &ValueNode) -> usize {
            1 + match node {
                ValueNode::Record { fields, .. } | ValueNode::Variant { fields, .. } => {
                    fields.iter().map(|field| count(&field.value)).sum()
                }
                ValueNode::Sequence { items, .. } => items.iter().map(count).sum(),
                ValueNode::Shared { value, .. } => count(value),
                ValueNode::Scalar { .. }
                | ValueNode::Reference { .. }
                | ValueNode::SharedReference { .. }
                | ValueNode::Truncated { .. } => 0,
            }
        }
        assert!(count(&root) <= 8);
        assert!(
            context
                .continuations()
                .values()
                .flatten()
                .map(count)
                .sum::<usize>()
                <= 8
        );
    }

    #[test]
    fn node_budget_stops_reflecting_a_wide_sequence() {
        let count = Cell::new(0);
        let value = vec![Counted(&count); 100_000];
        let mut context = ReflectionContext::new(64, 8);
        let raw = value.reflect(&mut context, None);
        let _ = context.finish(raw);
        assert_eq!(count.get(), 7);
    }

    #[test]
    fn large_unsigned_values_do_not_wrap() {
        assert!(matches!(
            u64::MAX.reflected(),
            ValueNode::Scalar { value: ScalarValue::String(value), .. } if value == u64::MAX.to_string()
        ));
    }

    #[test]
    fn repeated_arena_edges_use_one_shared_value_and_a_back_reference() {
        let mut stash = Stash::new();
        let value = stash.alloc(22_u32);
        let root = stash.alloc(SharedPair {
            left: value,
            right: value,
        });
        let reflected = Stashed::new(stash, root).reflected();
        let json = serde_json::to_string(&reflected).unwrap();
        assert_eq!(json.matches("\"kind\":\"shared\"").count(), 2);
        assert_eq!(json.matches("\"kind\":\"shared-reference\"").count(), 1);
    }

    #[test]
    fn equal_indices_in_distinct_stashes_have_distinct_shared_identities() {
        fn stashed(value: u32) -> Stashed<Ptr<u32>> {
            let mut stash = Stash::new();
            let root = stash.alloc(value);
            Stashed::new(stash, root)
        }

        let reflected = vec![stashed(1), stashed(2)].reflected();
        let json = serde_json::to_string(&reflected).unwrap();
        assert!(json.contains("stash-0-ptr:u32:0"), "{json}");
        assert!(json.contains("stash-1-ptr:u32:0"), "{json}");
        assert!(!json.contains("\"kind\":\"shared-reference\""));
    }

    #[test]
    fn stash_namespaces_remain_unique_across_continuation_boundaries() {
        fn stashed(value: u32) -> Stashed<Ptr<u32>> {
            let mut stash = Stash::new();
            let root = stash.alloc(value);
            Stashed::new(stash, root)
        }

        let value = StashesAcrossContinuation {
            nested: NestedStash { value: stashed(1) },
            direct: stashed(2),
        };
        let mut context = ReflectionContext::with_continuation_prefix(3, 100, "stashes");
        let raw = value.reflect(&mut context, None);
        let root = context.finish(raw);
        let mut json = serde_json::to_string(&root).unwrap();
        json.push_str(&serde_json::to_string(&context.continuations()).unwrap());
        assert!(json.contains("stash-0-ptr:u32:0"), "{json}");
        assert!(json.contains("stash-1-ptr:u32:0"), "{json}");
    }

    #[test]
    fn discarded_depth_page_cannot_orphan_a_shared_reference() {
        let mut stash = Stash::new();
        let shared = stash.alloc(22_u32);
        let root = SharedAfterContinuationExhaustion {
            exhaust: NestedUnit { value: () },
            discarded: NestedPointer { value: shared },
            retained: shared,
        };
        let mut context = ReflectionContext::with_continuation_prefix(2, 100, "exhausted");
        let raw = root.reflect(&mut context, Some(&stash));
        let reflected = context.finish(raw);
        let json = serde_json::to_string(&reflected).unwrap();
        assert!(json.contains("\"kind\":\"shared\""), "{json}");
        assert!(!json.contains("\"kind\":\"shared-reference\""), "{json}");
    }
}
