#[cfg(test)]
mod tests {
    use sage_arena::*;

    // -- Test types --------------------------------------------------------

    #[derive(Copy, Clone, Debug, PartialEq, AllocArenaData)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[derive(Copy, Clone, Debug, PartialEq, AllocArenaData)]
    struct Color {
        r: u8,
        g: u8,
        b: u8,
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, InternArenaData)]
    struct Name {
        id: u32,
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, InternArenaData)]
    struct Tag {
        val: u16,
    }

    // -- Basic alloc -------------------------------------------------------

    #[test]
    fn alloc_and_read_back() {
        let mut arena = Arena::new();
        let p = arena.alloc(Point { x: 1, y: 2 });
        assert_eq!(arena[p], Point { x: 1, y: 2 });
    }

    #[test]
    fn alloc_multiple_types() {
        let mut arena = Arena::new();
        let p = arena.alloc(Point { x: 10, y: 20 });
        let c = arena.alloc(Color {
            r: 255,
            g: 0,
            b: 128,
        });
        assert_eq!(arena[p], Point { x: 10, y: 20 });
        assert_eq!(
            arena[c],
            Color {
                r: 255,
                g: 0,
                b: 128
            }
        );
    }

    #[test]
    fn alloc_same_value_produces_distinct_ptrs() {
        let mut arena = Arena::new();
        let p1 = arena.alloc(Point { x: 1, y: 2 });
        let p2 = arena.alloc(Point { x: 1, y: 2 });
        assert_ne!(p1, p2);
        assert_eq!(arena[p1], arena[p2]);
    }

    // -- Basic slice -------------------------------------------------------

    #[test]
    fn alloc_slice_and_read_back() {
        let mut arena = Arena::new();
        let s = arena.alloc_slice(&[Point { x: 1, y: 2 }, Point { x: 3, y: 4 }]);
        assert_eq!(arena[s].len(), 2);
        assert_eq!(arena[s][0], Point { x: 1, y: 2 });
        assert_eq!(arena[s][1], Point { x: 3, y: 4 });
    }

    #[test]
    fn alloc_empty_slice() {
        let mut arena = Arena::new();
        let s = arena.alloc_slice::<Point>(&[]);
        assert_eq!(arena[s].len(), 0);
    }

    // -- Intern (dedup) ----------------------------------------------------

    #[test]
    fn intern_deduplicates() {
        let mut arena = Arena::new();
        let a = arena.intern(Name { id: 42 });
        let b = arena.intern(Name { id: 42 });
        assert_eq!(a, b); // same Ptr
    }

    #[test]
    fn intern_distinct_values_get_distinct_ptrs() {
        let mut arena = Arena::new();
        let a = arena.intern(Name { id: 1 });
        let b = arena.intern(Name { id: 2 });
        assert_ne!(a, b);
        assert_eq!(arena[a], Name { id: 1 });
        assert_eq!(arena[b], Name { id: 2 });
    }

    #[test]
    fn intern_slice_deduplicates() {
        let mut arena = Arena::new();
        let vals = [Name { id: 1 }, Name { id: 2 }];
        let a = arena.intern_slice(&vals);
        let b = arena.intern_slice(&vals);
        assert_eq!(a, b);
    }

    #[test]
    fn intern_slice_distinct_content() {
        let mut arena = Arena::new();
        let a = arena.intern_slice(&[Name { id: 1 }]);
        let b = arena.intern_slice(&[Name { id: 2 }]);
        assert_ne!(a, b);
    }

    #[test]
    fn intern_different_types_same_bits() {
        // Name { id: 42 } and Tag { val: 42 } might have overlapping bit
        // patterns but must be stored separately due to different TypeIds.
        let mut arena = Arena::new();
        let n = arena.intern(Name { id: 42 });
        let t = arena.intern(Tag { val: 42 });
        // They're different types so Ptr<Name> != Ptr<Tag> at the type level.
        // Just verify both read back correctly.
        assert_eq!(arena[n], Name { id: 42 });
        assert_eq!(arena[t], Tag { val: 42 });
    }

    // -- Ptr/Slice identity semantics --------------------------------------

    #[test]
    fn ptr_equality_is_identity() {
        let mut arena = Arena::new();
        let p1 = arena.alloc(Point { x: 0, y: 0 });
        let p2 = arena.alloc(Point { x: 0, y: 0 });
        // Same value, different allocations → different Ptrs.
        assert_ne!(p1, p2);
    }

    #[test]
    fn ptr_copy_clone() {
        let mut arena = Arena::new();
        let p = arena.alloc(Point { x: 1, y: 2 });
        let p2 = p; // Copy
        let p3 = p.clone();
        assert_eq!(p, p2);
        assert_eq!(p, p3);
        assert_eq!(arena[p], arena[p2]);
    }

    // -- Error conditions --------------------------------------------------

    #[test]
    #[should_panic(expected = "arena type mismatch")]
    fn type_mismatch_ptr_panics() {
        let mut arena = Arena::new();
        let p = arena.alloc(Point { x: 1, y: 2 });
        // Transmute the Ptr to a different type — simulates using a handle
        // from one type on another.
        let bad: Ptr<Color> = unsafe { std::mem::transmute(p) };
        let _ = arena[bad]; // should panic
    }

    #[test]
    #[should_panic(expected = "arena type mismatch")]
    fn type_mismatch_slice_panics() {
        let mut arena = Arena::new();
        let s = arena.alloc_slice(&[Point { x: 1, y: 2 }]);
        let bad: Slice<Color> = unsafe { std::mem::transmute(s) };
        let _ = &arena[bad]; // should panic
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn out_of_bounds_ptr_panics() {
        let mut arena = Arena::new();
        // Allocate something so index 0 exists, then forge index 1.
        let p = arena.alloc(Point { x: 0, y: 0 });
        // Forge a Ptr with index 1 (doesn't exist).
        let mut bad = p;
        // Increment the index via transmute.
        let raw: u32 = unsafe { std::mem::transmute(bad) };
        bad = unsafe { std::mem::transmute(raw + 1) };
        let _ = arena[bad];
    }

    #[test]
    #[should_panic(expected = "arena type mismatch")]
    fn cross_arena_ptr_wrong_type() {
        let mut arena1 = Arena::new();
        let mut arena2 = Arena::new();
        let p1 = arena1.alloc(Point { x: 1, y: 2 });
        let _c2 = arena2.alloc(Color { r: 0, g: 0, b: 0 });

        // Transmute Ptr<Point> to Ptr<Color> — same index, wrong type in arena2.
        let _forged: Ptr<Color> = unsafe { std::mem::transmute(p1) };
        // arena2 has Color at index 0, forged asks for Color — this actually
        // matches! Instead, use p1 directly on arena2 (Point vs Color).
        let _ = arena2[p1]; // arena2[0] is Color, p1 is Ptr<Point> → mismatch
    }

    #[test]
    fn cross_arena_ptr_same_type_reads_wrong_data() {
        // Both arenas have Point at index 0, but different values.
        // This is the "silent misuse" case — no panic, but wrong data.
        let mut arena1 = Arena::new();
        let mut arena2 = Arena::new();
        let p1 = arena1.alloc(Point { x: 1, y: 2 });
        let _p2 = arena2.alloc(Point { x: 99, y: 100 });

        // Using p1 (from arena1) on arena2 — same type, same index, wrong data.
        assert_eq!(arena2[p1], Point { x: 99, y: 100 });
        // This demonstrates why Ptrs should not be used across arenas.
    }

    // -- Interned type mismatch --------------------------------------------

    #[test]
    #[should_panic(expected = "arena type mismatch")]
    fn intern_type_mismatch_panics() {
        let mut arena = Arena::new();
        let n = arena.intern(Name { id: 1 });
        let bad: Ptr<Tag> = unsafe { std::mem::transmute(n) };
        let _ = arena[bad]; // should panic
    }

    // -- Mixed alloc and intern in same arena ------------------------------

    #[test]
    fn mixed_alloc_and_intern() {
        let mut arena = Arena::new();
        let p = arena.alloc(Point { x: 5, y: 6 });
        let n = arena.intern(Name { id: 10 });
        let s = arena.alloc_slice(&[Point { x: 7, y: 8 }]);
        let ns = arena.intern_slice(&[Name { id: 20 }, Name { id: 30 }]);

        assert_eq!(arena[p], Point { x: 5, y: 6 });
        assert_eq!(arena[n], Name { id: 10 });
        assert_eq!(arena[s][0], Point { x: 7, y: 8 });
        assert_eq!(arena[ns], [Name { id: 20 }, Name { id: 30 }]);
    }

    // -- Alignment ---------------------------------------------------------

    #[test]
    fn alignment_preserved_for_mixed_sizes() {
        #[derive(Copy, Clone, Debug, PartialEq, AllocArenaData)]
        struct Small(u8);

        #[derive(Copy, Clone, Debug, PartialEq, AllocArenaData)]
        struct Big(u64);

        let mut arena = Arena::new();
        // Alloc a u8-aligned type, then a u64-aligned type.
        let s = arena.alloc(Small(1));
        let b = arena.alloc(Big(0xDEAD_BEEF_CAFE_BABE));
        assert_eq!(arena[s], Small(1));
        assert_eq!(arena[b], Big(0xDEAD_BEEF_CAFE_BABE));
    }
}
