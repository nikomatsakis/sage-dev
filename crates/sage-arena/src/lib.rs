use std::any::TypeId;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::marker::PhantomData;
use std::ops::Index;

pub use sage_arena_macros::{AllocArenaData, InternArenaData};

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Supertrait for all arena-storable types. Provides the `static_type_id()`
/// used for runtime type checking on retrieval.
///
/// # Safety
/// - Only lifetimes in `Self` are `'db` or `'static`.
/// - `static_type_id()` returns `TypeId` of the `'static` version of Self.
/// - `Self: Copy`.
///
/// Prefer `#[derive(AllocArenaData)]` or `#[derive(InternArenaData)]` over
/// implementing this directly.
pub unsafe trait ArenaData<'db>: Copy {
    fn static_type_id() -> TypeId;
}

/// Allocated (not deduplicated). Two `alloc` calls with the same value
/// produce distinct `Ptr`s.
pub trait AllocArenaData<'db>: ArenaData<'db> {}

/// Interned (deduplicated). Two `intern` calls with the same value return
/// the same `Ptr`, so `Ptr` identity implies equality.
pub trait InternArenaData<'db>: ArenaData<'db> + Hash + Eq {}

// ---------------------------------------------------------------------------
// Handles
// ---------------------------------------------------------------------------

/// Thin handle to one value in an `Arena`.
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

/// Thin handle to a contiguous slice in an `Arena`.
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

// ---------------------------------------------------------------------------
// Arena
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
    /// `TypeId` of the `'static` version of the interned type.
    type_id: TypeId,
    /// Hash of the value's content (via `Hash` trait).
    content_hash: u64,
    /// Collision index — disambiguates distinct values with the same hash.
    collision: u32,
}

/// Type-erased heterogeneous arena storing `Copy`-only data.
pub struct Arena<'db> {
    buf: Vec<u8>,
    entries: Vec<Entry>,
    intern_map: HashMap<InternKey, u32>,
    _marker: PhantomData<&'db ()>,
}

impl<'db> Arena<'db> {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            entries: Vec::new(),
            intern_map: HashMap::new(),
            _marker: PhantomData,
        }
    }

    // -- alloc (no dedup) --------------------------------------------------

    /// Allocate a single value, returning a `Ptr` handle.
    pub fn alloc<T: AllocArenaData<'db>>(&mut self, value: T) -> Ptr<T> {
        let index = self.push_raw(&[value], <T as ArenaData>::static_type_id());
        Ptr {
            index,
            _marker: PhantomData,
        }
    }

    /// Allocate a contiguous slice, returning a `Slice` handle.
    pub fn alloc_slice<T: AllocArenaData<'db>>(&mut self, values: &[T]) -> Slice<T> {
        let index = self.push_raw(values, <T as ArenaData>::static_type_id());
        Slice {
            index,
            _marker: PhantomData,
        }
    }

    // -- intern (dedup) ----------------------------------------------------

    /// Intern a single value. Returns the same `Ptr` for equal values.
    pub fn intern<T: InternArenaData<'db>>(&mut self, value: T) -> Ptr<T> {
        let type_id = <T as ArenaData>::static_type_id();
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
    pub fn intern_slice<T: InternArenaData<'db>>(&mut self, values: &[T]) -> Slice<T> {
        let type_id = <T as ArenaData>::static_type_id();
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
            "arena type mismatch: handle for `{}` used on entry with a different type",
            std::any::type_name::<T>(),
        );
        entry
    }
}

impl<'db> Default for Arena<'db> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Index impls — single impl via sealed ArenaData
// ---------------------------------------------------------------------------

impl<'db, T: ArenaData<'db>> Index<Ptr<T>> for Arena<'db> {
    type Output = T;
    fn index(&self, ptr: Ptr<T>) -> &T {
        let entry = self.validate_entry::<T>(ptr.index, <T as ArenaData>::static_type_id());
        debug_assert_eq!(entry.count, 1);
        unsafe { self.read_one(entry.offset) }
    }
}

impl<'db, T: ArenaData<'db>> Index<Slice<T>> for Arena<'db> {
    type Output = [T];
    fn index(&self, slice: Slice<T>) -> &[T] {
        let entry = self.validate_entry::<T>(slice.index, <T as ArenaData>::static_type_id());
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
