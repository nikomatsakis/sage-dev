#![feature(register_tool)]
#![register_tool(sage)]

use sage_stash::{AllocStashData, Ptr, Slice, Stash, StashData, StashHash, StashHasher, Stashed};
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, AllocStashData)]
struct Branch {
    value: Ptr<u32>,
    children: Slice<Ptr<u32>>,
}

// The scalar hashes exactly like an empty slice, exercising the shared table.
#[derive(Clone, Copy, Debug, PartialEq)]
struct EmptySliceCollision(u32);

// Safety: the value is Copy and contains no borrowed data.
unsafe impl StashData<'_> for EmptySliceCollision {
    type StaticSelf = Self;
}
impl AllocStashData<'_> for EmptySliceCollision {}
impl StashHash for EmptySliceCollision {
    fn stash_hash(&self, _: &Stash, hasher: &mut impl StashHasher) {
        0_usize.hash(hasher);
    }
}

#[test]
#[sage::test(crates.sage_stash.storage.typed_interning)]
fn scalar_collision_with_empty_slice_never_reads_a_slice_as_a_scalar() {
    for slice_first in [true, false] {
        let mut stash = Stash::new();
        if slice_first {
            stash.alloc_slice::<EmptySliceCollision>(&[]);
        }
        let scalar = stash.alloc(EmptySliceCollision(42));
        let empty = stash.alloc_slice::<EmptySliceCollision>(&[]);
        assert_eq!(stash[scalar], EmptySliceCollision(42));
        assert!(stash[empty].is_empty());
        assert_eq!(scalar, stash.alloc(EmptySliceCollision(42)));
        assert_eq!(empty, stash.alloc_slice::<EmptySliceCollision>(&[]));
    }
}

#[test]
#[sage::test(crates.sage_stash.storage.typed_interning)]
fn typed_scalars_slices_and_derived_nested_values_deduplicate() {
    let mut stash = Stash::new();
    let value = stash.alloc(42_u32);
    assert_eq!(value, stash.alloc(42_u32));
    let signed = stash.alloc(42_i32);
    assert_eq!(stash[signed], 42_i32);
    assert_eq!(stash[value], 42_u32);
    let children = stash.alloc_slice(&[value, value]);
    assert_eq!(children, stash.alloc_slice_from_iter([value, value]));
    let branch = Branch { value, children };
    let root = stash.alloc(branch);
    assert_eq!(root, stash.alloc(branch));
    let roots = stash.alloc_slice(&[branch, branch]);
    assert_eq!(roots, stash.alloc_slice(&[branch, branch]));
    let empty = stash.alloc_slice::<u32>(&[]);
    assert_eq!(empty, stash.alloc_slice_from_iter([]));
    assert!(stash[empty].is_empty());
}

#[test]
#[sage::test(crates.sage_stash.storage.handle_ownership)]
fn copied_nested_handles_survive_source_drop_and_destination_growth() {
    let mut source = Stash::new();
    let value = source.alloc(42_u32);
    let other = source.alloc(99_u32);
    let children = source.alloc_slice(&[value, other, value]);
    let root = source.alloc(Branch { value, children });
    let tree = Stashed::new(source, root);
    let mut destination = Stash::new();
    // Different entry layout detects a shallow copy even in release builds.
    destination.alloc(7_u64);
    destination.alloc_slice(&[1_u8, 2, 3]);
    let copied = tree.copy_into(&mut destination);
    drop(tree);
    for n in 0..4096_u64 {
        destination.alloc(n);
    }
    let Branch { value, children } = destination[copied];
    assert_eq!(destination[value], 42);
    let children = &destination[children];
    assert_eq!(children.len(), 3);
    assert_eq!(children[0], children[2]);
    assert_eq!(children[0], value);
    assert_eq!(
        children.iter().map(|p| destination[*p]).collect::<Vec<_>>(),
        [42, 99, 42]
    );
}

#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "stash identity mismatch"))]
#[cfg_attr(not(debug_assertions), should_panic(expected = "stash type mismatch"))]
#[sage::test(crates.sage_stash.storage.handle_ownership)]
fn cross_stash_slice_with_different_element_type_is_rejected() {
    let mut source = Stash::new();
    let handle = source.alloc_slice(&[42_u32]);
    let mut destination = Stash::new();
    destination.alloc_slice(&[42_u64]);
    let _ = &destination[handle];
}

#[test]
#[sage::test(crates.sage_stash.storage.immutable_entries)]
fn appending_and_deduplicating_preserves_existing_entries() {
    let mut stash = Stash::new();
    let value = stash.alloc(42_u32);
    let slice = stash.alloc_slice(&[1_u32, 2, 3]);
    for n in 0..4096_u64 {
        stash.alloc(n);
    }
    assert_eq!(stash[value], 42);
    assert_eq!(stash[slice], [1, 2, 3]);
    assert_eq!(value, stash.alloc(42_u32));
    assert_eq!(slice, stash.alloc_slice(&[1_u32, 2, 3]));
}

#[test]
#[sage::test(crates.sage_stash.storage.immutable_entries)]
fn safe_api_rejects_mutating_interned_scalars_and_slices() {
    // Keep the rejected operations in the reviewed snapshot as well as the fixtures.
    assert_eq!(
        include_str!("ui/mutate_scalar.rs"),
        r#"use sage_stash::Stash;

fn main() {
    let mut stash = Stash::new();
    let value = stash.alloc(42_u32);
    stash[value] = 99;
}
"#
    );
    assert_eq!(
        include_str!("ui/mutate_slice.rs"),
        r#"use sage_stash::Stash;

fn main() {
    let mut stash = Stash::new();
    let values = stash.alloc_slice(&[1_u32, 2]);
    stash[values][0] = 99;
}
"#
    );
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/mutate_scalar.rs");
    tests.compile_fail("tests/ui/mutate_slice.rs");
}

// A repr(C) value has padding, but its derived structural hash visits fields.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, AllocStashData)]
struct Padded {
    tag: u8,
    value: u64,
}

fn tree(reverse: bool, garbage: bool, last: u64) -> Stashed<Slice<Ptr<Padded>>> {
    let mut stash = Stash::new();
    if garbage {
        stash.alloc_slice(&[99_u64; 17]);
    }
    let a = Padded { tag: 1, value: 10 };
    let b = Padded {
        tag: 2,
        value: last,
    };
    let (a, b) = if reverse {
        let b = stash.alloc(b);
        (stash.alloc(a), b)
    } else {
        let a = stash.alloc(a);
        (a, stash.alloc(b))
    };
    let root = stash.alloc_slice(&[a, b, a]);
    if garbage {
        stash.alloc(1000_u32);
    }
    Stashed::new(stash, root)
}

fn hash(value: &impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[test]
#[sage::test(crates.sage_stash.storage.rooted_equality)]
fn reachable_content_controls_equality_hash_and_order() {
    let baseline = tree(false, false, 20);
    for reverse in [false, true] {
        for garbage in [false, true] {
            let equal = tree(reverse, garbage, 20);
            assert_eq!(baseline, equal);
            assert_eq!(hash(&baseline), hash(&equal));
            assert_eq!(baseline.cmp(&equal), std::cmp::Ordering::Equal);
            let changed = tree(reverse, garbage, 21);
            assert_ne!(baseline, changed);
            assert_ne!(baseline.cmp(&changed), std::cmp::Ordering::Equal);
            assert_eq!(baseline.cmp(&changed), changed.cmp(&baseline).reverse());
            assert_eq!(
                baseline.cmp(&changed),
                baseline.cmp(&tree(false, false, 21))
            );
        }
    }
}

#[test]
#[cfg(feature = "salsa")]
#[sage::test(crates.sage_stash.storage.rooted_equality)]
fn salsa_update_keeps_equal_root_and_replaces_changed_root() {
    let mut old = tree(false, false, 20);
    let original_root = old.root();
    let equal = tree(true, true, 20);
    // Safety: old_pointer is an exclusive live pointer to the same concrete type.
    assert!(!unsafe { salsa::Update::maybe_update(&mut old, equal) });
    assert_eq!(old.root(), original_root);
    let changed = tree(true, true, 21);
    let expected = changed.clone();
    // Safety: old_pointer is an exclusive live pointer to the same concrete type.
    assert!(unsafe { salsa::Update::maybe_update(&mut old, changed) });
    assert_eq!(old, expected);
    let (stash, root) = old.open();
    assert_eq!(stash[stash[root][1]].value, 21);
}

#[test]
#[sage::test(crates.sage_stash.storage.rooted_equality)]
fn padded_values_fingerprint_only_semantic_fields() {
    use sage_stash::FingerprintHasher;
    let value = Padded { tag: 7, value: 42 };
    assert!(std::mem::size_of::<Padded>() > std::mem::size_of::<u8>() + std::mem::size_of::<u64>());
    let stash = Stash::new();
    let mut actual = FingerprintHasher::new();
    value.stash_hash(&stash, &mut actual);
    let mut fields = FingerprintHasher::new();
    7_u8.stash_hash(&stash, &mut fields);
    42_u64.stash_hash(&stash, &mut fields);
    assert_eq!(actual.finalize(), fields.finalize());
}

impl sage_stash::StashCopy for EmptySliceCollision {
    fn stash_copy(&self, _: &Stash, _: &mut Stash) -> Self {
        *self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, AllocStashData)]
struct DerivedCollision {
    value: EmptySliceCollision,
}

#[test]
#[sage::test(crates.sage_stash.storage.typed_interning)]
fn derived_values_and_slices_resolve_forced_hash_collisions_by_content() {
    let mut stash = Stash::new();
    let one = DerivedCollision {
        value: EmptySliceCollision(1),
    };
    let two = DerivedCollision {
        value: EmptySliceCollision(2),
    };
    let a = stash.alloc(one);
    let b = stash.alloc(two);
    assert_ne!(a, b);
    assert_eq!(a, stash.alloc(one));
    assert_eq!(stash[a], one);
    assert_eq!(stash[b], two);
    let ab = stash.alloc_slice(&[one, two]);
    let ba = stash.alloc_slice(&[two, one]);
    assert_ne!(ab, ba);
    assert_eq!(ab, stash.alloc_slice(&[one, two]));
    assert_eq!(stash[ab], [one, two]);
    assert_eq!(stash[ba], [two, one]);
}
