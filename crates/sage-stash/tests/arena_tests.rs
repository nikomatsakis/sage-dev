#[cfg(test)]
mod tests {
    use sage_stash::*;

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

    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, InternStashData)]
    struct Name {
        id: u32,
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, InternStashData)]
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
    fn alloc_same_value_produces_distinct_ptrs() {
        let mut stash = Stash::new();
        let p1 = stash.alloc(Point { x: 1, y: 2 });
        let p2 = stash.alloc(Point { x: 1, y: 2 });
        assert_ne!(p1, p2);
        assert_eq!(stash[p1], stash[p2]);
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

    // -- Intern (dedup) ----------------------------------------------------

    #[test]
    fn intern_deduplicates() {
        let mut stash = Stash::new();
        let a = stash.intern(Name { id: 42 });
        let b = stash.intern(Name { id: 42 });
        assert_eq!(a, b);
    }

    #[test]
    fn intern_distinct_values_get_distinct_ptrs() {
        let mut stash = Stash::new();
        let a = stash.intern(Name { id: 1 });
        let b = stash.intern(Name { id: 2 });
        assert_ne!(a, b);
        assert_eq!(stash[a], Name { id: 1 });
        assert_eq!(stash[b], Name { id: 2 });
    }

    #[test]
    fn intern_slice_deduplicates() {
        let mut stash = Stash::new();
        let vals = [Name { id: 1 }, Name { id: 2 }];
        let a = stash.intern_slice(&vals);
        let b = stash.intern_slice(&vals);
        assert_eq!(a, b);
    }

    #[test]
    fn intern_slice_distinct_content() {
        let mut stash = Stash::new();
        let a = stash.intern_slice(&[Name { id: 1 }]);
        let b = stash.intern_slice(&[Name { id: 2 }]);
        assert_ne!(a, b);
    }

    #[test]
    fn intern_different_types_same_bits() {
        let mut stash = Stash::new();
        let n = stash.intern(Name { id: 42 });
        let t = stash.intern(Tag { val: 42 });
        assert_eq!(stash[n], Name { id: 42 });
        assert_eq!(stash[t], Tag { val: 42 });
    }

    // -- Ptr/Slice identity semantics --------------------------------------

    #[test]
    fn ptr_equality_is_identity() {
        let mut stash = Stash::new();
        let p1 = stash.alloc(Point { x: 0, y: 0 });
        let p2 = stash.alloc(Point { x: 0, y: 0 });
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
    fn intern_type_mismatch_panics() {
        let mut stash = Stash::new();
        let n = stash.intern(Name { id: 1 });
        let bad: Ptr<Tag> = unsafe { std::mem::transmute(n) };
        let _ = stash[bad];
    }

    #[test]
    fn mixed_alloc_and_intern() {
        let mut stash = Stash::new();
        let p = stash.alloc(Point { x: 5, y: 6 });
        let n = stash.intern(Name { id: 10 });
        let s = stash.alloc_slice(&[Point { x: 7, y: 8 }]);
        let ns = stash.intern_slice(&[Name { id: 20 }, Name { id: 30 }]);

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

    // -- StashEq / Stashed tests -------------------------------------------

    #[test]
    fn stash_eq_same_stash_same_index() {
        let mut stash = Stash::new();
        let p = stash.alloc(Point { x: 1, y: 2 });
        // Same index → quick-check returns true.
        assert!(p.stash_eq(&p, &stash));
    }

    // Point is not StashDirect (it's not Eq+Hash in the std sense for our purposes),
    // so let's use a type that is.
    impl StashDirect for Point {}

    #[test]
    fn stash_eq_different_index_same_value() {
        let mut stash = Stash::new();
        let p1 = stash.alloc(Point { x: 1, y: 2 });
        let p2 = stash.alloc(Point { x: 1, y: 2 });
        assert_ne!(p1, p2); // different indices
        assert!(p1.stash_eq(&p2, &stash)); // same value
    }

    #[test]
    fn stash_eq_different_value() {
        let mut stash = Stash::new();
        let p1 = stash.alloc(Point { x: 1, y: 2 });
        let p2 = stash.alloc(Point { x: 3, y: 4 });
        assert!(!p1.stash_eq(&p2, &stash));
    }

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
}
