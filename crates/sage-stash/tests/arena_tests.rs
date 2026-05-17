#[cfg(test)]
mod tests {
    use sage_stash::*;
    use std::hash::Hasher;

    // -- Test types --------------------------------------------------------

    #[derive(Copy, Clone, Debug, PartialEq, AllocStashData)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[derive(Copy, Clone, Debug, PartialEq, AllocStashData)]
    struct Color {
        r: u8,
        g: u8,
        b: u8,
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
    struct Name {
        id: u32,
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
    struct Tag {
        val: u16,
    }

    // -- Basic alloc -------------------------------------------------------

    #[test]
    fn alloc_and_read_back() {
        let mut stash = Stash::new();
        let p = stash.alloc(Point { x: 1, y: 2 });
        assert_eq!(stash[p], Point { x: 1, y: 2 });
    }

    #[test]
    fn alloc_multiple_types() {
        let mut stash = Stash::new();
        let p = stash.alloc(Point { x: 10, y: 20 });
        let c = stash.alloc(Color {
            r: 255,
            g: 0,
            b: 128,
        });
        assert_eq!(stash[p], Point { x: 10, y: 20 });
        assert_eq!(
            stash[c],
            Color {
                r: 255,
                g: 0,
                b: 128
            }
        );
    }

    #[test]
    fn alloc_same_value_deduplicates() {
        let mut stash = Stash::new();
        let p1 = stash.alloc(Point { x: 1, y: 2 });
        let p2 = stash.alloc(Point { x: 1, y: 2 });
        assert_eq!(p1, p2);
    }

    // -- Basic slice -------------------------------------------------------

    #[test]
    fn alloc_slice_and_read_back() {
        let mut stash = Stash::new();
        let s = stash.alloc_slice(&[Point { x: 1, y: 2 }, Point { x: 3, y: 4 }]);
        assert_eq!(stash[s].len(), 2);
        assert_eq!(stash[s][0], Point { x: 1, y: 2 });
        assert_eq!(stash[s][1], Point { x: 3, y: 4 });
    }

    #[test]
    fn alloc_empty_slice() {
        let mut stash = Stash::new();
        let s = stash.alloc_slice::<Point>(&[]);
        assert_eq!(stash[s].len(), 0);
    }

    // -- Dedup with different types -------------------------------------------

    #[test]
    fn dedup_different_types_same_bits() {
        let mut stash = Stash::new();
        let n = stash.alloc(Name { id: 42 });
        let t = stash.alloc(Tag { val: 42 });
        assert_eq!(stash[n], Name { id: 42 });
        assert_eq!(stash[t], Tag { val: 42 });
    }

    // -- Ptr/Slice identity semantics --------------------------------------

    #[test]
    fn ptr_eq_is_value_eq() {
        let mut stash = Stash::new();
        let p1 = stash.alloc(Point { x: 0, y: 0 });
        let p2 = stash.alloc(Point { x: 0, y: 0 });
        assert_eq!(p1, p2);
    }

    #[test]
    fn ptr_ne_is_value_ne() {
        let mut stash = Stash::new();
        let p1 = stash.alloc(Point { x: 0, y: 0 });
        let p2 = stash.alloc(Point { x: 1, y: 1 });
        assert_ne!(p1, p2);
    }

    #[test]
    fn ptr_copy_clone() {
        let mut stash = Stash::new();
        let p = stash.alloc(Point { x: 1, y: 2 });
        let p2 = p;
        let p3 = p.clone();
        assert_eq!(p, p2);
        assert_eq!(p, p3);
        assert_eq!(stash[p], stash[p2]);
    }

    // -- Error conditions --------------------------------------------------

    #[test]
    #[should_panic(expected = "stash type mismatch")]
    fn type_mismatch_ptr_panics() {
        let mut stash = Stash::new();
        let p = stash.alloc(Point { x: 1, y: 2 });
        let bad: Ptr<Color> = unsafe { std::mem::transmute(p) };
        let _ = stash[bad];
    }

    #[test]
    #[should_panic(expected = "stash type mismatch")]
    fn type_mismatch_slice_panics() {
        let mut stash = Stash::new();
        let s = stash.alloc_slice(&[Point { x: 1, y: 2 }]);
        let bad: Slice<Color> = unsafe { std::mem::transmute(s) };
        let _ = &stash[bad];
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn out_of_bounds_ptr_panics() {
        let mut stash = Stash::new();
        let p = stash.alloc(Point { x: 0, y: 0 });
        let mut bad = p;
        let raw: u32 = unsafe { std::mem::transmute(bad) };
        bad = unsafe { std::mem::transmute(raw + 1) };
        let _ = stash[bad];
    }

    #[test]
    #[should_panic(expected = "stash type mismatch")]
    fn cross_stash_ptr_wrong_type() {
        let mut stash1 = Stash::new();
        let mut stash2 = Stash::new();
        let p1 = stash1.alloc(Point { x: 1, y: 2 });
        let _c2 = stash2.alloc(Color { r: 0, g: 0, b: 0 });
        let _ = stash2[p1];
    }

    #[test]
    fn cross_stash_ptr_same_type_reads_wrong_data() {
        let mut stash1 = Stash::new();
        let mut stash2 = Stash::new();
        let p1 = stash1.alloc(Point { x: 1, y: 2 });
        let _p2 = stash2.alloc(Point { x: 99, y: 100 });
        assert_eq!(stash2[p1], Point { x: 99, y: 100 });
    }

    #[test]
    #[should_panic(expected = "stash type mismatch")]
    fn alloc_type_mismatch_panics() {
        let mut stash = Stash::new();
        let n = stash.alloc(Name { id: 1 });
        let bad: Ptr<Tag> = unsafe { std::mem::transmute(n) };
        let _ = stash[bad];
    }

    #[test]
    fn mixed_alloc_types() {
        let mut stash = Stash::new();
        let p = stash.alloc(Point { x: 5, y: 6 });
        let n = stash.alloc(Name { id: 10 });
        let s = stash.alloc_slice(&[Point { x: 7, y: 8 }]);
        let ns = stash.alloc_slice(&[Name { id: 20 }, Name { id: 30 }]);

        assert_eq!(stash[p], Point { x: 5, y: 6 });
        assert_eq!(stash[n], Name { id: 10 });
        assert_eq!(stash[s][0], Point { x: 7, y: 8 });
        assert_eq!(stash[ns], [Name { id: 20 }, Name { id: 30 }]);
    }

    #[test]
    fn alignment_preserved_for_mixed_sizes() {
        #[derive(Copy, Clone, Debug, PartialEq, AllocStashData)]
        struct Small(u8);

        #[derive(Copy, Clone, Debug, PartialEq, AllocStashData)]
        struct Big(u64);

        let mut stash = Stash::new();
        let s = stash.alloc(Small(1));
        let b = stash.alloc(Big(0xDEAD_BEEF_CAFE_BABE));
        assert_eq!(stash[s], Small(1));
        assert_eq!(stash[b], Big(0xDEAD_BEEF_CAFE_BABE));
    }

    // -- Stashed tests -------------------------------------------

    #[test]
    fn stashed_eq_same_content() {
        let mut s1 = Stash::new();
        let r1 = s1.alloc(Point { x: 10, y: 20 });
        let stashed1 = Stashed::new(s1, r1);

        let mut s2 = Stash::new();
        let r2 = s2.alloc(Point { x: 10, y: 20 });
        let stashed2 = Stashed::new(s2, r2);

        assert_eq!(stashed1, stashed2);
    }

    #[test]
    fn stashed_ne_different_content() {
        let mut s1 = Stash::new();
        let r1 = s1.alloc(Point { x: 10, y: 20 });
        let stashed1 = Stashed::new(s1, r1);

        let mut s2 = Stash::new();
        let r2 = s2.alloc(Point { x: 99, y: 99 });
        let stashed2 = Stashed::new(s2, r2);

        assert_ne!(stashed1, stashed2);
    }

    #[test]
    fn stashed_slice_eq() {
        let mut s1 = Stash::new();
        let sl1 = s1.alloc_slice(&[Point { x: 1, y: 2 }, Point { x: 3, y: 4 }]);
        let stashed1 = Stashed::new(s1, sl1);

        let mut s2 = Stash::new();
        let sl2 = s2.alloc_slice(&[Point { x: 1, y: 2 }, Point { x: 3, y: 4 }]);
        let stashed2 = Stashed::new(s2, sl2);

        assert_eq!(stashed1, stashed2);
    }

    // -- Phase 1: StashHasher tests ----------------------------------------

    #[test]
    fn stash_hasher_scalar() {
        use std::hash::Hash;

        let stash = Stash::new();

        let mut intern_hasher = InternHasher::new();
        42u32.stash_hash(&stash, &mut intern_hasher);
        let intern_result = intern_hasher.finish();

        let mut fx = FxHasher::default();
        42u32.hash(&mut fx);
        let fx_result = fx.finish();

        assert_eq!(intern_result, fx_result);
    }

    #[test]
    fn stash_hasher_stash_direct() {
        use std::hash::Hash;

        let stash = Stash::new();

        let mut intern_hasher = InternHasher::new();
        true.stash_hash(&stash, &mut intern_hasher);
        let intern_result = intern_hasher.finish();

        let mut fx = FxHasher::default();
        true.hash(&mut fx);
        let fx_result = fx.finish();

        assert_eq!(intern_result, fx_result);
    }

    // -- Phase 2: Derive macro StashHash tests -----------------------------

    #[test]
    fn derived_stash_hash_leaf_struct() {
        use std::hash::Hash;

        #[derive(Copy, Clone, Debug, PartialEq, AllocStashData)]
        struct Leaf {
            x: i32,
            y: i32,
        }

        let stash = Stash::new();

        let val = Leaf { x: 10, y: 20 };
        let mut intern_hasher = InternHasher::new();
        val.stash_hash(&stash, &mut intern_hasher);
        let stash_result = intern_hasher.finish();

        let mut fx = FxHasher::default();
        10i32.hash(&mut fx);
        20i32.hash(&mut fx);
        let fx_result = fx.finish();

        assert_eq!(stash_result, fx_result);
    }

    #[test]
    fn derived_stash_hash_with_ptr_field() {
        #[derive(Copy, Clone, Debug, PartialEq, AllocStashData)]
        struct Wrapper {
            inner: Ptr<Point>,
        }

        let mut stash = Stash::new();
        let p = stash.alloc(Point { x: 1, y: 2 });
        let _w = Wrapper { inner: p };
    }

    // -- Phase 3: NonZeroU32, niche optimization -----

    #[test]
    fn option_ptr_size() {
        assert_eq!(
            std::mem::size_of::<Option<Ptr<Point>>>(),
            std::mem::size_of::<Ptr<Point>>()
        );
    }

    #[test]
    fn option_slice_size() {
        assert_eq!(
            std::mem::size_of::<Option<Slice<Point>>>(),
            std::mem::size_of::<Slice<Point>>()
        );
    }

    // -- Phase 4: Hash-consed allocation tests -----

    #[test]
    fn hash_cons_dedup() {
        let mut stash = Stash::new();
        let p1 = stash.alloc(Point { x: 1, y: 2 });
        let p2 = stash.alloc(Point { x: 1, y: 2 });
        assert_eq!(p1, p2);
    }

    #[test]
    fn hash_cons_distinct() {
        let mut stash = Stash::new();
        let p1 = stash.alloc(Point { x: 1, y: 2 });
        let p2 = stash.alloc(Point { x: 3, y: 4 });
        assert_ne!(p1, p2);
    }

    #[test]
    fn hash_cons_slice_dedup() {
        let mut stash = Stash::new();
        let s1 = stash.alloc_slice(&[Point { x: 1, y: 2 }, Point { x: 3, y: 4 }]);
        let s2 = stash.alloc_slice(&[Point { x: 1, y: 2 }, Point { x: 3, y: 4 }]);
        assert_eq!(s1, s2);
    }

    #[test]
    fn hash_cons_slice_distinct() {
        let mut stash = Stash::new();
        let s1 = stash.alloc_slice(&[Point { x: 1, y: 2 }]);
        let s2 = stash.alloc_slice(&[Point { x: 3, y: 4 }]);
        assert_ne!(s1, s2);
    }

    #[test]
    fn hash_cons_compound_type() {
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
        struct Pair {
            a: Ptr<Point>,
            b: Ptr<Point>,
        }

        let mut stash = Stash::new();
        let a = stash.alloc(Point { x: 1, y: 2 });
        let b = stash.alloc(Point { x: 3, y: 4 });
        let pair1 = stash.alloc(Pair { a, b });
        let pair2 = stash.alloc(Pair { a, b });
        assert_eq!(pair1, pair2);
    }

    // -- Collision tests -----

    #[test]
    fn hash_cons_collision_stores_both() {
        #[derive(Copy, Clone, Debug, PartialEq)]
        struct Collider(u32);

        unsafe impl<'db> StashData<'db> for Collider {
            fn static_type_id() -> std::any::TypeId {
                std::any::TypeId::of::<Collider>()
            }
        }

        impl AllocStashData<'_> for Collider {}

        impl StashHash for Collider {
            fn stash_hash(&self, _stash: &Stash, hasher: &mut impl StashHasher) {
                hasher.write_u64(0);
            }
        }

        let mut stash = Stash::new();
        let a = stash.alloc(Collider(1));
        let b = stash.alloc(Collider(2));
        let c = stash.alloc(Collider(1));

        assert_ne!(a, b);
        assert_eq!(a, c);
        assert_eq!(stash[a], Collider(1));
        assert_eq!(stash[b], Collider(2));
    }

    // -- Phase 6: Fingerprint / Stashed tests -----

    #[test]
    fn stashed_eq_same_content_fingerprint() {
        let mut s1 = Stash::new();
        let r1 = s1.alloc(Point { x: 10, y: 20 });
        let stashed1 = Stashed::new(s1, r1);

        let mut s2 = Stash::new();
        let r2 = s2.alloc(Point { x: 10, y: 20 });
        let stashed2 = Stashed::new(s2, r2);

        assert_eq!(stashed1, stashed2);
    }

    #[test]
    fn stashed_ne_different_content_fingerprint() {
        let mut s1 = Stash::new();
        let r1 = s1.alloc(Point { x: 10, y: 20 });
        let stashed1 = Stashed::new(s1, r1);

        let mut s2 = Stash::new();
        let r2 = s2.alloc(Point { x: 99, y: 99 });
        let stashed2 = Stashed::new(s2, r2);

        assert_ne!(stashed1, stashed2);
    }

    #[test]
    fn stashed_hash_consistent_with_eq() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hash;

        let mut s1 = Stash::new();
        let r1 = s1.alloc(Point { x: 5, y: 6 });
        let stashed1 = Stashed::new(s1, r1);

        let mut s2 = Stash::new();
        let r2 = s2.alloc(Point { x: 5, y: 6 });
        let stashed2 = Stashed::new(s2, r2);

        assert_eq!(stashed1, stashed2);

        let hash = |v: &Stashed<Ptr<Point>>| {
            let mut h = DefaultHasher::new();
            v.hash(&mut h);
            h.finish()
        };
        assert_eq!(hash(&stashed1), hash(&stashed2));
    }

    #[test]
    fn stashed_ord_consistent_with_eq() {
        use std::cmp::Ordering;

        let mut s1 = Stash::new();
        let r1 = s1.alloc(Point { x: 5, y: 6 });
        let stashed1 = Stashed::new(s1, r1);

        let mut s2 = Stash::new();
        let r2 = s2.alloc(Point { x: 5, y: 6 });
        let stashed2 = Stashed::new(s2, r2);

        assert_eq!(stashed1.cmp(&stashed2), Ordering::Equal);
    }

    #[test]
    fn stashed_eq_compound_dag() {
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
        struct Node {
            val: i32,
            left: Option<Ptr<Node>>,
            right: Option<Ptr<Node>>,
        }

        let mut s1 = Stash::new();
        let leaf = s1.alloc(Node {
            val: 1,
            left: None,
            right: None,
        });
        let root1 = s1.alloc(Node {
            val: 2,
            left: Some(leaf),
            right: Some(leaf),
        });
        let stashed1 = Stashed::new(s1, root1);

        let mut s2 = Stash::new();
        let leaf2 = s2.alloc(Node {
            val: 1,
            left: None,
            right: None,
        });
        let root2 = s2.alloc(Node {
            val: 2,
            left: Some(leaf2),
            right: Some(leaf2),
        });
        let stashed2 = Stashed::new(s2, root2);

        assert_eq!(stashed1, stashed2);
    }

    #[test]
    fn fingerprint_deterministic() {
        let mut s1 = Stash::new();
        let r1 = s1.alloc(Point { x: 42, y: 99 });
        let stashed1 = Stashed::new(s1, r1);

        let mut s2 = Stash::new();
        let r2 = s2.alloc(Point { x: 42, y: 99 });
        let stashed2 = Stashed::new(s2, r2);

        assert_eq!(stashed1, stashed2);
    }
}
