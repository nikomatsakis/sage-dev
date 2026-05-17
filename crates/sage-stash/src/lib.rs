use std::any::TypeId;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::ops::Index;

pub use rustc_hash::FxHasher;
pub use sage_stash_macros::AllocStashData;

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
/// Prefer `#[derive(AllocStashData)]` over implementing this directly.
pub unsafe trait StashData<'db>: Copy {
    fn static_type_id() -> TypeId;
}

/// Stash-storable type with hash-consing support. All allocations are
/// content-addressed: equal values produce equal handles.
pub trait AllocStashData<'db>: StashData<'db> + StashHash + PartialEq {}

// ---------------------------------------------------------------------------
// StashHasher trait and implementations
// ---------------------------------------------------------------------------

pub trait StashHasher: Hasher {
    fn stash_hash_ptr<T: StashHash + Copy>(&mut self, ptr: Ptr<T>, stash: &Stash);
    fn stash_hash_slice<T: StashHash + Copy>(&mut self, slice: Slice<T>, stash: &Stash);
}

pub trait StashHash {
    fn stash_hash(&self, stash: &Stash, hasher: &mut impl StashHasher);
}

/// Adapts any `Hasher` into a `StashHasher`. `stash_hash_ptr` and
/// `stash_hash_slice` read the pre-computed inline hash from the entry
/// and write it into the wrapped hasher — no recursion needed since
/// children are always allocated before parents.
pub struct InternHasher<H: Hasher> {
    inner: H,
}

impl Default for InternHasher<FxHasher> {
    fn default() -> Self {
        Self {
            inner: FxHasher::default(),
        }
    }
}

impl InternHasher<FxHasher> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<H: Hasher> InternHasher<H> {
    pub fn with_hasher(hasher: H) -> Self {
        Self { inner: hasher }
    }
}

impl<H: Hasher> Hasher for InternHasher<H> {
    fn finish(&self) -> u64 {
        self.inner.finish()
    }

    fn write(&mut self, bytes: &[u8]) {
        self.inner.write(bytes);
    }

    fn write_u8(&mut self, i: u8) {
        self.inner.write_u8(i);
    }
    fn write_u16(&mut self, i: u16) {
        self.inner.write_u16(i);
    }
    fn write_u32(&mut self, i: u32) {
        self.inner.write_u32(i);
    }
    fn write_u64(&mut self, i: u64) {
        self.inner.write_u64(i);
    }
    fn write_u128(&mut self, i: u128) {
        self.inner.write_u128(i);
    }
    fn write_usize(&mut self, i: usize) {
        self.inner.write_usize(i);
    }
    fn write_i8(&mut self, i: i8) {
        self.inner.write_i8(i);
    }
    fn write_i16(&mut self, i: i16) {
        self.inner.write_i16(i);
    }
    fn write_i32(&mut self, i: i32) {
        self.inner.write_i32(i);
    }
    fn write_i64(&mut self, i: i64) {
        self.inner.write_i64(i);
    }
    fn write_i128(&mut self, i: i128) {
        self.inner.write_i128(i);
    }
    fn write_isize(&mut self, i: isize) {
        self.inner.write_isize(i);
    }
}

impl<H: Hasher> StashHasher for InternHasher<H> {
    fn stash_hash_ptr<T: StashHash + Copy>(&mut self, ptr: Ptr<T>, stash: &Stash) {
        let entry = &stash.entries[ptr.index.get() as usize];
        self.inner.write_u64(entry.inline_hash);
    }

    fn stash_hash_slice<T: StashHash + Copy>(&mut self, slice: Slice<T>, stash: &Stash) {
        let entry = &stash.entries[slice.index.get() as usize];
        self.inner.write_u64(entry.inline_hash);
    }
}

// ---------------------------------------------------------------------------
// EntryIndex / BufOffset newtypes
// ---------------------------------------------------------------------------

/// Index into the entries table. Stores index + 1 as NonZeroU32
/// so that Option<Ptr<T>> has niche optimization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct EntryIndex(NonZeroU32);

impl EntryIndex {
    fn new(index: u32) -> Self {
        Self(NonZeroU32::new(index.checked_add(1).expect("entry index overflow")).unwrap())
    }

    fn get(self) -> u32 {
        self.0.get() - 1
    }
}

/// Byte offset into the stash buffer. Used by intern hashmap and collision chains.
/// Always 4-byte aligned, so the low bit is available as a tag.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct BufOffset(u32);

// ---------------------------------------------------------------------------
// Handles
// ---------------------------------------------------------------------------

/// Thin handle to one value in a `Stash`.
#[derive(Debug)]
pub struct Ptr<T> {
    index: EntryIndex,
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

impl<'db, T: StashData<'db> + StashHash> StashHash for Ptr<T> {
    fn stash_hash(&self, stash: &Stash, hasher: &mut impl StashHasher) {
        hasher.stash_hash_ptr(*self, stash);
    }
}

/// Thin handle to a contiguous slice in a `Stash`.
#[derive(Debug)]
pub struct Slice<T> {
    index: EntryIndex,
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

impl<'db, T: StashData<'db> + StashHash> StashHash for Slice<T> {
    fn stash_hash(&self, stash: &Stash, hasher: &mut impl StashHasher) {
        hasher.stash_hash_slice(*self, stash);
    }
}

impl<T: StashHash> StashHash for Option<T> {
    fn stash_hash(&self, stash: &Stash, hasher: &mut impl StashHasher) {
        match self {
            Some(v) => {
                1u8.hash(hasher);
                v.stash_hash(stash, hasher);
            }
            None => 0u8.hash(hasher),
        }
    }
}

// ---------------------------------------------------------------------------
// StashHash blanket for types that are plain Hash
// (no arena indirection needed — scalars, salsa IDs, etc.)
// ---------------------------------------------------------------------------

/// Marker trait: this type's `Eq`/`Hash`/`Ord` don't need stash context.
/// Implement this for scalars, salsa IDs, and other self-contained types.
pub trait StashDirect: Copy {}

impl<T: StashDirect + Hash> StashHash for T {
    fn stash_hash(&self, _stash: &Stash, hasher: &mut impl StashHasher) {
        self.hash(hasher);
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
// Fingerprint
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fingerprint([u8; 16]);

impl std::fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fingerprint({:032x})", u128::from_ne_bytes(self.0))
    }
}

// ---------------------------------------------------------------------------
// FingerprintHasher
// ---------------------------------------------------------------------------

pub struct FingerprintHasher {
    state: xxhash_rust::xxh3::Xxh3,
    cache: Vec<Option<Fingerprint>>,
}

impl FingerprintHasher {
    pub fn new() -> Self {
        Self {
            state: xxhash_rust::xxh3::Xxh3::new(),
            cache: Vec::new(),
        }
    }

    pub fn finalize(&self) -> Fingerprint {
        Fingerprint(self.state.digest128().to_ne_bytes())
    }

    fn ensure_cache(&mut self, index: u32) {
        let needed = index as usize + 1;
        if self.cache.len() < needed {
            self.cache.resize(needed, None);
        }
    }

    fn entry_fingerprint<T: StashHash>(
        &mut self,
        index: EntryIndex,
        value: &T,
        stash: &Stash,
    ) -> Fingerprint {
        let idx = index.get();
        self.ensure_cache(idx);
        if let Some(ref fp) = self.cache[idx as usize] {
            return fp.clone();
        }
        let mut sub = FingerprintHasher::new();
        sub.cache = std::mem::take(&mut self.cache);
        value.stash_hash(stash, &mut sub);
        let fp = sub.finalize();
        self.cache = sub.cache;
        self.ensure_cache(idx);
        self.cache[idx as usize] = Some(fp.clone());
        fp
    }
}

impl Default for FingerprintHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher for FingerprintHasher {
    fn finish(&self) -> u64 {
        self.state.finish()
    }

    fn write(&mut self, bytes: &[u8]) {
        self.state.write(bytes);
    }

    fn write_u8(&mut self, i: u8) {
        self.state.write_u8(i);
    }
    fn write_u16(&mut self, i: u16) {
        self.state.write_u16(i);
    }
    fn write_u32(&mut self, i: u32) {
        self.state.write_u32(i);
    }
    fn write_u64(&mut self, i: u64) {
        self.state.write_u64(i);
    }
    fn write_u128(&mut self, i: u128) {
        self.state.write_u128(i);
    }
    fn write_usize(&mut self, i: usize) {
        self.state.write_usize(i);
    }
    fn write_i8(&mut self, i: i8) {
        self.state.write_i8(i);
    }
    fn write_i16(&mut self, i: i16) {
        self.state.write_i16(i);
    }
    fn write_i32(&mut self, i: i32) {
        self.state.write_i32(i);
    }
    fn write_i64(&mut self, i: i64) {
        self.state.write_i64(i);
    }
    fn write_i128(&mut self, i: i128) {
        self.state.write_i128(i);
    }
    fn write_isize(&mut self, i: isize) {
        self.state.write_isize(i);
    }
}

impl StashHasher for FingerprintHasher {
    fn stash_hash_ptr<T: StashHash + Copy>(&mut self, ptr: Ptr<T>, stash: &Stash) {
        let value = &stash.entries[ptr.index.get() as usize];
        let data = unsafe { stash.read_one::<T>(value.offset) };
        let fp = self.entry_fingerprint(ptr.index, data, stash);
        self.state.write(&fp.0);
    }

    fn stash_hash_slice<T: StashHash + Copy>(&mut self, slice: Slice<T>, stash: &Stash) {
        let entry = &stash.entries[slice.index.get() as usize];
        let data = unsafe { stash.read_slice::<T>(entry.offset, entry.count) };
        let idx = slice.index.get();
        self.ensure_cache(idx);
        if let Some(ref fp) = self.cache[idx as usize] {
            self.state.write(&fp.0);
            return;
        }
        let mut sub = FingerprintHasher::new();
        sub.cache = std::mem::take(&mut self.cache);
        data.len().hash(&mut sub);
        for item in data {
            item.stash_hash(stash, &mut sub);
        }
        let fp = sub.finalize();
        self.cache = sub.cache;
        self.ensure_cache(idx);
        self.cache[idx as usize] = Some(fp.clone());
        self.state.write(&fp.0);
    }
}

// ---------------------------------------------------------------------------
// Stashed<T> — pairs a Stash with a root value
// ---------------------------------------------------------------------------

pub struct Stashed<T> {
    stash: Stash,
    root: T,
    fingerprint: Fingerprint,
}

impl<T: StashHash> Stashed<T> {
    pub fn new(stash: Stash, root: T) -> Self {
        let mut hasher = FingerprintHasher::new();
        root.stash_hash(&stash, &mut hasher);
        let fingerprint = hasher.finalize();
        Self {
            stash,
            root,
            fingerprint,
        }
    }
}

impl<T> Stashed<T> {
    pub fn root(&self) -> &T {
        &self.root
    }

    pub fn stash(&self) -> &Stash {
        &self.stash
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Stashed<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stashed").field("root", &self.root).finish()
    }
}

impl<T> PartialEq for Stashed<T> {
    fn eq(&self, other: &Self) -> bool {
        self.fingerprint == other.fingerprint
    }
}

impl<T> Eq for Stashed<T> {}

impl<T> Hash for Stashed<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.fingerprint.hash(state);
    }
}

impl<T> PartialOrd for Stashed<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Stashed<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.fingerprint.cmp(&other.fingerprint)
    }
}

// ---------------------------------------------------------------------------
// Stash
// ---------------------------------------------------------------------------

/// Entry metadata: type id, byte offset into `buf`, element count, FxHash.
struct Entry {
    type_id: TypeId,
    offset: u32,
    count: u32,
    inline_hash: u64,
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
    intern_map: HashMap<InternKey, EntryIndex>,
}

impl Stash {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            entries: Vec::new(),
            intern_map: HashMap::new(),
        }
    }

    /// Hash-cons a single value. Equal content always produces equal `Ptr`s.
    pub fn alloc<'db, T: AllocStashData<'db>>(&mut self, value: T) -> Ptr<T> {
        let type_id = <T as StashData>::static_type_id();
        let mut hasher = InternHasher::new();
        value.stash_hash(self, &mut hasher);
        let content_hash = hasher.finish();

        for collision in 0u32.. {
            let key = InternKey {
                type_id,
                content_hash,
                collision,
            };
            match self.intern_map.get(&key) {
                Some(&entry_idx) => {
                    let entry = &self.entries[entry_idx.get() as usize];
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
                    let entry_idx = self.push_raw(&[value], type_id, content_hash);
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

    /// Hash-cons a contiguous slice. Equal content always produces equal `Slice`s.
    pub fn alloc_slice<'db, T: AllocStashData<'db>>(&mut self, values: &[T]) -> Slice<T> {
        let type_id = <T as StashData>::static_type_id();
        let mut hasher = InternHasher::new();
        values.len().hash(&mut hasher);
        for v in values {
            v.stash_hash(self, &mut hasher);
        }
        let content_hash = hasher.finish();

        for collision in 0u32.. {
            let key = InternKey {
                type_id,
                content_hash,
                collision,
            };
            match self.intern_map.get(&key) {
                Some(&entry_idx) => {
                    let entry = &self.entries[entry_idx.get() as usize];
                    let existing = unsafe { self.read_slice::<T>(entry.offset, entry.count) };
                    if existing == values {
                        return Slice {
                            index: entry_idx,
                            _marker: PhantomData,
                        };
                    }
                }
                None => {
                    let entry_idx = self.push_raw(values, type_id, content_hash);
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

    fn push_raw<T: Copy>(&mut self, values: &[T], type_id: TypeId, inline_hash: u64) -> EntryIndex {
        let align = std::mem::align_of::<T>().max(4);
        let cur = self.buf.len();
        let padding = cur.wrapping_neg() & (align - 1);
        self.buf.resize(cur + padding, 0);

        let offset = self.buf.len() as u32;
        let byte_len = std::mem::size_of_val(values);
        self.buf.reserve(byte_len);
        unsafe {
            let dst = self.buf.as_mut_ptr().add(offset as usize);
            std::ptr::copy_nonoverlapping(values.as_ptr() as *const u8, dst, byte_len);
            self.buf.set_len(self.buf.len() + byte_len);
        }

        let entry_idx = EntryIndex::new(self.entries.len() as u32);
        self.entries.push(Entry {
            type_id,
            offset,
            count: values.len() as u32,
            inline_hash,
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

    fn validate_entry<T>(&self, index: EntryIndex, expected_type_id: TypeId) -> &Entry {
        let entry = &self.entries[index.get() as usize];
        assert_eq!(
            entry.type_id,
            expected_type_id,
            "stash type mismatch: handle for `{}` used on entry with a different type",
            std::any::type_name::<T>(),
        );
        entry
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
