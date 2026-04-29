use std::any::TypeId;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::marker::PhantomData;
use std::ops::Index;

pub use sage_stash_macros::{AllocStashData, InternStashData};

// ---------------------------------------------------------------------------
// Traits — arena-storable types
// ---------------------------------------------------------------------------

/// Supertrait for all stash-storable types. Provides the `static_type_id()`
/// used for runtime type checking on retrieval.
///
/// # Safety
/// - Only lifetimes in `Self` are `'db` or `'static`.
/// - `static_type_id()` returns `TypeId` of the `'static` version of Self.
/// - `Self: Copy`.
///
/// Prefer `#[derive(AllocStashData)]` or `#[derive(InternStashData)]` over
/// implementing this directly.
pub unsafe trait StashData<'db>: Copy {
    fn static_type_id() -> TypeId;
}

/// Allocated (not deduplicated). Two `alloc` calls with the same value
/// produce distinct `Ptr`s.
pub trait AllocStashData<'db>: StashData<'db> {}

/// Interned (deduplicated). Two `intern` calls with the same value return
/// the same `Ptr`, so `Ptr` identity implies equality.
pub trait InternStashData<'db>: StashData<'db> + Hash + Eq {}

// ---------------------------------------------------------------------------
// Traits — stash-contextual comparison
// ---------------------------------------------------------------------------

/// Like `PartialEq` but takes a `&Stash` for context, so arena-allocated
/// types can compare by value.
pub trait StashEq<'db> {
    fn stash_eq(&self, other: &Self, stash: &Stash) -> bool;
}

/// Like `Hash` but takes a `&Stash` for context.
pub trait StashHash<'db> {
    fn stash_hash<H: Hasher>(&self, stash: &Stash, state: &mut H);
}

/// Like `Ord` but takes a `&Stash` for context.
pub trait StashOrd<'db> {
    fn stash_cmp(&self, other: &Self, stash: &Stash) -> Ordering;
}

// ---------------------------------------------------------------------------
// Handles
// ---------------------------------------------------------------------------

/// Thin handle to one value in a `Stash`.
#[derive(Debug)]
pub struct Ptr<T> {
    index: u32,
    _marker: PhantomData<T>,
}

impl<T> Copy for Ptr<T> {}
impl<T> Clone for Ptr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Ptr<T> {
    /// Sentinel value for uninitialized pointers. Indexing with this will panic.
    pub const DANGLING: Self = Ptr {
        index: u32::MAX,
        _marker: PhantomData,
    };
}

impl<T> PartialEq for Ptr<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}
impl<T> Eq for Ptr<T> {}
impl<T> Hash for Ptr<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}

/// `Ptr<T>` compares by value: quick-check on index, then deep compare.
impl<'db, T: StashData<'db> + StashEq<'db>> StashEq<'db> for Ptr<T> {
    fn stash_eq(&self, other: &Self, stash: &Stash) -> bool {
        self.index == other.index || stash[*self].stash_eq(&stash[*other], stash)
    }
}

impl<'db, T: StashData<'db> + StashHash<'db>> StashHash<'db> for Ptr<T> {
    fn stash_hash<H: Hasher>(&self, stash: &Stash, state: &mut H) {
        stash[*self].stash_hash(stash, state);
    }
}

impl<'db, T: StashData<'db> + StashOrd<'db>> StashOrd<'db> for Ptr<T> {
    fn stash_cmp(&self, other: &Self, stash: &Stash) -> Ordering {
        if self.index == other.index {
            Ordering::Equal
        } else {
            stash[*self].stash_cmp(&stash[*other], stash)
        }
    }
}

/// Thin handle to a contiguous slice in a `Stash`.
#[derive(Debug)]
pub struct Slice<T> {
    index: u32,
    _marker: PhantomData<T>,
}

impl<T> Copy for Slice<T> {}
impl<T> Clone for Slice<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> PartialEq for Slice<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}
impl<T> Eq for Slice<T> {}
impl<T> Hash for Slice<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}

/// `Slice<T>` compares element-by-element by value.
impl<'db, T: StashData<'db> + StashEq<'db>> StashEq<'db> for Slice<T> {
    fn stash_eq(&self, other: &Self, stash: &Stash) -> bool {
        if self.index == other.index {
            return true;
        }
        let a = &stash[*self];
        let b = &stash[*other];
        a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.stash_eq(y, stash))
    }
}

impl<'db, T: StashData<'db> + StashHash<'db>> StashHash<'db> for Slice<T> {
    fn stash_hash<H: Hasher>(&self, stash: &Stash, state: &mut H) {
        let items = &stash[*self];
        items.len().hash(state);
        for item in items {
            item.stash_hash(stash, state);
        }
    }
}

impl<'db, T: StashData<'db> + StashOrd<'db>> StashOrd<'db> for Slice<T> {
    fn stash_cmp(&self, other: &Self, stash: &Stash) -> Ordering {
        if self.index == other.index {
            return Ordering::Equal;
        }
        let a = &stash[*self];
        let b = &stash[*other];
        a.len().cmp(&b.len()).then_with(|| {
            for (x, y) in a.iter().zip(b.iter()) {
                match x.stash_cmp(y, stash) {
                    Ordering::Equal => continue,
                    ord => return ord,
                }
            }
            Ordering::Equal
        })
    }
}

// ---------------------------------------------------------------------------
// StashEq/StashHash/StashOrd for Option<T>
// ---------------------------------------------------------------------------

impl<'db, T: StashEq<'db>> StashEq<'db> for Option<T> {
    fn stash_eq(&self, other: &Self, stash: &Stash) -> bool {
        match (self, other) {
            (Some(a), Some(b)) => a.stash_eq(b, stash),
            (None, None) => true,
            _ => false,
        }
    }
}

impl<'db, T: StashHash<'db>> StashHash<'db> for Option<T> {
    fn stash_hash<H: Hasher>(&self, stash: &Stash, state: &mut H) {
        match self {
            Some(v) => {
                1u8.hash(state);
                v.stash_hash(stash, state);
            }
            None => 0u8.hash(state),
        }
    }
}

impl<'db, T: StashOrd<'db>> StashOrd<'db> for Option<T> {
    fn stash_cmp(&self, other: &Self, stash: &Stash) -> Ordering {
        match (self, other) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(a), Some(b)) => a.stash_cmp(b, stash),
        }
    }
}

// ---------------------------------------------------------------------------
// StashEq/StashHash/StashOrd blanket for types that are plain Eq/Hash/Ord
// (no arena indirection needed — scalars, salsa IDs, etc.)
// ---------------------------------------------------------------------------

/// Marker trait: this type's `Eq`/`Hash`/`Ord` don't need stash context.
/// Implement this for scalars, salsa IDs, and other self-contained types.
pub trait StashDirect: Copy {}

impl<'db, T: StashDirect + PartialEq> StashEq<'db> for T {
    fn stash_eq(&self, other: &Self, _stash: &Stash) -> bool {
        self == other
    }
}

impl<'db, T: StashDirect + Hash> StashHash<'db> for T {
    fn stash_hash<H: Hasher>(&self, _stash: &Stash, state: &mut H) {
        self.hash(state);
    }
}

impl<'db, T: StashDirect + Ord> StashOrd<'db> for T {
    fn stash_cmp(&self, other: &Self, _stash: &Stash) -> Ordering {
        self.cmp(other)
    }
}

// Blanket impls for common scalars
impl StashDirect for bool {}
impl StashDirect for u8 {}
impl StashDirect for u16 {}
impl StashDirect for u32 {}
impl StashDirect for u64 {}
impl StashDirect for i8 {}
impl StashDirect for i16 {}
impl StashDirect for i32 {}
impl StashDirect for i64 {}

// ---------------------------------------------------------------------------
// Stashed<T> — pairs a Stash with a root value
// ---------------------------------------------------------------------------

/// A self-contained value backed by a `Stash`. Implements `PartialEq`/`Eq`/`Hash`
/// by comparing the stash content (byte-level) and the root value.
///
/// If two stashes have identical byte content, the root values are compared
/// directly (indices are equivalent). If stash content differs, the values differ.
pub struct Stashed<T> {
    stash: Stash,
    root: T,
}

impl<T> Stashed<T> {
    pub fn new(stash: Stash, root: T) -> Self {
        Self { stash, root }
    }

    /// Access the root value.
    pub fn root(&self) -> &T {
        &self.root
    }

    /// Access the stash (for indexing into arena-allocated data).
    pub fn stash(&self) -> &Stash {
        &self.stash
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Stashed<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stashed").field("root", &self.root).finish()
    }
}

impl<T: PartialEq> PartialEq for Stashed<T> {
    fn eq(&self, other: &Self) -> bool {
        self.stash == other.stash && self.root == other.root
    }
}

impl<T: Eq> Eq for Stashed<T> {}

impl<T: Hash> Hash for Stashed<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.stash.hash(state);
        self.root.hash(state);
    }
}

// ---------------------------------------------------------------------------
// Stash
// ---------------------------------------------------------------------------

/// Entry metadata: type id, byte offset into `buf`, element count.
struct Entry {
    type_id: TypeId,
    offset: u32,
    count: u32,
}

/// Key for the intern deduplication map.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct InternKey {
    type_id: TypeId,
    content_hash: u64,
    collision: u32,
}

/// Type-erased heterogeneous storage for `Copy`-only data with thin handles.
pub struct Stash {
    buf: Vec<u8>,
    entries: Vec<Entry>,
    intern_map: HashMap<InternKey, u32>,
}

impl Stash {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            entries: Vec::new(),
            intern_map: HashMap::new(),
        }
    }

    // -- alloc (no dedup) --------------------------------------------------

    /// Allocate a single value, returning a `Ptr` handle.
    pub fn alloc<'db, T: AllocStashData<'db>>(&mut self, value: T) -> Ptr<T> {
        let index = self.push_raw(&[value], <T as StashData>::static_type_id());
        Ptr {
            index,
            _marker: PhantomData,
        }
    }

    /// Allocate a contiguous slice, returning a `Slice` handle.
    pub fn alloc_slice<'db, T: AllocStashData<'db>>(&mut self, values: &[T]) -> Slice<T> {
        let index = self.push_raw(values, <T as StashData>::static_type_id());
        Slice {
            index,
            _marker: PhantomData,
        }
    }

    // -- intern (dedup) ----------------------------------------------------

    /// Intern a single value. Returns the same `Ptr` for equal values.
    pub fn intern<'db, T: InternStashData<'db>>(&mut self, value: T) -> Ptr<T> {
        let type_id = <T as StashData>::static_type_id();
        let content_hash = hash_value(&value);

        for collision in 0u32.. {
            let key = InternKey {
                type_id,
                content_hash,
                collision,
            };
            match self.intern_map.get(&key) {
                Some(&entry_idx) => {
                    let entry = &self.entries[entry_idx as usize];
                    debug_assert_eq!(entry.count, 1);
                    let existing = unsafe { self.read_one::<T>(entry.offset) };
                    if *existing == value {
                        return Ptr {
                            index: entry_idx,
                            _marker: PhantomData,
                        };
                    }
                }
                None => {
                    let entry_idx = self.push_raw(&[value], type_id);
                    self.intern_map.insert(key, entry_idx);
                    return Ptr {
                        index: entry_idx,
                        _marker: PhantomData,
                    };
                }
            }
        }
        unreachable!()
    }

    /// Intern a contiguous slice. Returns the same `Slice` for equal slices.
    pub fn intern_slice<'db, T: InternStashData<'db>>(&mut self, values: &[T]) -> Slice<T> {
        let type_id = <T as StashData>::static_type_id();
        let content_hash = hash_slice(values);

        for collision in 0u32.. {
            let key = InternKey {
                type_id,
                content_hash,
                collision,
            };
            match self.intern_map.get(&key) {
                Some(&entry_idx) => {
                    let entry = &self.entries[entry_idx as usize];
                    let existing = unsafe { self.read_slice::<T>(entry.offset, entry.count) };
                    if existing == values {
                        return Slice {
                            index: entry_idx,
                            _marker: PhantomData,
                        };
                    }
                }
                None => {
                    let entry_idx = self.push_raw(values, type_id);
                    self.intern_map.insert(key, entry_idx);
                    return Slice {
                        index: entry_idx,
                        _marker: PhantomData,
                    };
                }
            }
        }
        unreachable!()
    }

    // -- internal helpers --------------------------------------------------

    fn push_raw<T: Copy>(&mut self, values: &[T], type_id: TypeId) -> u32 {
        let align = std::mem::align_of::<T>();
        let cur = self.buf.len();
        let padding = cur.wrapping_neg() & (align - 1);
        self.buf.resize(cur + padding, 0);

        let offset = self.buf.len() as u32;
        let byte_len = std::mem::size_of::<T>() * values.len();
        self.buf.reserve(byte_len);
        unsafe {
            let dst = self.buf.as_mut_ptr().add(offset as usize);
            std::ptr::copy_nonoverlapping(values.as_ptr() as *const u8, dst, byte_len);
            self.buf.set_len(self.buf.len() + byte_len);
        }

        let entry_idx = self.entries.len() as u32;
        self.entries.push(Entry {
            type_id,
            offset,
            count: values.len() as u32,
        });
        entry_idx
    }

    unsafe fn read_one<T: Copy>(&self, offset: u32) -> &T {
        unsafe { &*(self.buf.as_ptr().add(offset as usize) as *const T) }
    }

    unsafe fn read_slice<T: Copy>(&self, offset: u32, count: u32) -> &[T] {
        if count == 0 {
            return &[];
        }
        unsafe {
            std::slice::from_raw_parts(
                self.buf.as_ptr().add(offset as usize) as *const T,
                count as usize,
            )
        }
    }

    fn validate_entry<T>(&self, index: u32, expected_type_id: TypeId) -> &Entry {
        let entry = &self.entries[index as usize];
        assert_eq!(
            entry.type_id,
            expected_type_id,
            "stash type mismatch: handle for `{}` used on entry with a different type",
            std::any::type_name::<T>(),
        );
        entry
    }
}

impl PartialEq for Stash {
    fn eq(&self, other: &Self) -> bool {
        self.buf == other.buf
    }
}

impl Eq for Stash {}

impl Hash for Stash {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.buf.hash(state);
    }
}

impl Default for Stash {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Index impls
// ---------------------------------------------------------------------------

impl<'db, T: StashData<'db>> Index<Ptr<T>> for Stash {
    type Output = T;
    fn index(&self, ptr: Ptr<T>) -> &T {
        let entry = self.validate_entry::<T>(ptr.index, <T as StashData>::static_type_id());
        debug_assert_eq!(entry.count, 1);
        unsafe { self.read_one(entry.offset) }
    }
}

impl<'db, T: StashData<'db>> Index<Slice<T>> for Stash {
    type Output = [T];
    fn index(&self, slice: Slice<T>) -> &[T] {
        let entry = self.validate_entry::<T>(slice.index, <T as StashData>::static_type_id());
        unsafe { self.read_slice(entry.offset, entry.count) }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn hash_value<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn hash_slice<T: Hash>(values: &[T]) -> u64 {
    let mut hasher = DefaultHasher::new();
    values.hash(&mut hasher);
    hasher.finish()
}

// ---------------------------------------------------------------------------
// salsa::Update impls
// ---------------------------------------------------------------------------

#[cfg(feature = "salsa")]
unsafe impl<T> salsa::Update for Ptr<T> {
    unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
        let old = unsafe { &*old_pointer };
        if old.index == new_value.index {
            false
        } else {
            unsafe { *old_pointer = new_value };
            true
        }
    }
}

#[cfg(feature = "salsa")]
unsafe impl<T> salsa::Update for Slice<T> {
    unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
        let old = unsafe { &*old_pointer };
        if old.index == new_value.index {
            false
        } else {
            unsafe { *old_pointer = new_value };
            true
        }
    }
}

#[cfg(feature = "salsa")]
unsafe impl<T: PartialEq> salsa::Update for Stashed<T> {
    unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
        let old = unsafe { &*old_pointer };
        if *old == new_value {
            false
        } else {
            unsafe { *old_pointer = new_value };
            true
        }
    }
}
