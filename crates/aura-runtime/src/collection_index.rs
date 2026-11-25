//! Hash-indexed storage for `dict` and `set`.
//!
//! Entries live in a `Vec` in insertion order, so iteration, rendering, and
//! snapshots keep the order the language promises. Once a collection holds
//! more than [`INDEX_THRESHOLD`] entries, a hash table maps each key's hash to
//! its position, so lookups, inserts, and membership tests take expected
//! constant time instead of a scan. Smaller collections scan, which is faster
//! than hashing for a handful of keys and keeps small JSON objects light.
//!
//! [`hash_value`] agrees with `Value`'s `PartialEq`: equal values always hash
//! the same. Keys whose equality has no cheap structural hash, such as sets,
//! dictionaries, and runtime resources, hash by kind alone and fall back to
//! comparing candidates in one bucket.

use std::collections::TryReserveError;
use std::fmt;
use std::hash::{BuildHasher, Hash, Hasher, RandomState};
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use triomphe::Arc as EntriesArc;

use hashbrown::HashTable;

use crate::integer::{IntegerRepresentation, IntegerValue};
use crate::runtime_value::{
    nominal_runtime_base_name, Value, VariantPayloads, DIRECT_RUNTIME_TYPE_FIELD,
};

/// Collections with at most this many entries scan instead of hashing.
pub const INDEX_THRESHOLD: usize = 8;

/// The number of value nodes [`hash_value`] visits. Deeper or larger keys
/// still hash consistently because equal values visit the same prefix.
const HASH_NODE_BUDGET: usize = 64;

/// This process's hash keys, chosen once.
#[inline(always)]
fn hash_keys() -> (u64, u64) {
    static KEYS: OnceLock<(u64, u64)> = OnceLock::new();
    *KEYS.get_or_init(|| {
        // `RandomState` holds random keys it does not expose, so the keys
        // are two of its hashes: as secret and as random as its own.
        let state = RandomState::new();
        (state.hash_one(0u64), state.hash_one(1u64))
    })
}

/// A new hasher for a key, with this process's keys.
#[inline(always)]
fn hash_state() -> KeyedHasher {
    let (k0, k1) = hash_keys();
    KeyedHasher::with_keys(k0, k1)
}

/// The keyed hasher of `dict` and `set` keys: SipHash-1-3, the hash
/// `RandomState` computes, with keys chosen once per process. The standard
/// library's copy is not public, and it hashes a one-word key through the
/// general byte path; this one compresses a word that starts on a word
/// boundary directly, which halves the cost of an integer key.
pub(crate) type KeyedHasher = SipHasher<1, 3>;

/// SipHash with `C` compression and `D` finalization rounds over a byte
/// stream, as the reference describes it. Bytes split across writes hash
/// as one stream.
#[derive(Clone, Debug)]
pub(crate) struct SipHasher<const C: usize, const D: usize> {
    v0: u64,
    v1: u64,
    v2: u64,
    v3: u64,
    /// The bytes after the last whole word, little-endian.
    tail: u64,
    /// How many bytes `tail` holds, below 8.
    ntail: usize,
    /// How many bytes were written.
    length: usize,
}

impl<const C: usize, const D: usize> SipHasher<C, D> {
    pub(crate) fn with_keys(k0: u64, k1: u64) -> Self {
        Self {
            v0: k0 ^ 0x736f_6d65_7073_6575,
            v1: k1 ^ 0x646f_7261_6e64_6f6d,
            v2: k0 ^ 0x6c79_6765_6e65_7261,
            v3: k1 ^ 0x7465_6462_7974_6573,
            tail: 0,
            ntail: 0,
            length: 0,
        }
    }

    #[inline(always)]
    fn round(&mut self) {
        self.v0 = self.v0.wrapping_add(self.v1);
        self.v1 = self.v1.rotate_left(13) ^ self.v0;
        self.v0 = self.v0.rotate_left(32);
        self.v2 = self.v2.wrapping_add(self.v3);
        self.v3 = self.v3.rotate_left(16) ^ self.v2;
        self.v0 = self.v0.wrapping_add(self.v3);
        self.v3 = self.v3.rotate_left(21) ^ self.v0;
        self.v2 = self.v2.wrapping_add(self.v1);
        self.v1 = self.v1.rotate_left(17) ^ self.v2;
        self.v2 = self.v2.rotate_left(32);
    }

    #[inline(always)]
    fn compress(&mut self, word: u64) {
        self.v3 ^= word;
        for _ in 0..C {
            self.round();
        }
        self.v0 ^= word;
    }

    /// The last word of a stream of `length` bytes that ends with the
    /// bytes in `tail`, hashed in.
    #[inline(always)]
    fn finish_with(mut self, tail: u64, length: usize) -> u64 {
        self.compress(((length as u64 & 0xff) << 56) | tail);
        self.v2 ^= 0xff;
        for _ in 0..D {
            self.round();
        }
        self.v0 ^ self.v1 ^ self.v2 ^ self.v3
    }

    /// The hash of `bytes` written to a new hasher in one `write`, without
    /// the state a stream keeps between writes.
    #[inline(always)]
    pub(crate) fn hash_bytes(k0: u64, k1: u64, bytes: &[u8]) -> u64 {
        let mut state = Self::with_keys(k0, k1);
        let mut words = bytes.chunks_exact(8);
        for chunk in &mut words {
            state.compress(u64::from_le_bytes(
                chunk.try_into().expect("an 8-byte chunk"),
            ));
        }
        state.finish_with(short_tail(words.remainder()), bytes.len())
    }

    /// The hash of `word` written to a new hasher with `write_u64`.
    #[inline(always)]
    pub(crate) fn hash_word(k0: u64, k1: u64, word: u64) -> u64 {
        let mut state = Self::with_keys(k0, k1);
        state.compress(word);
        state.finish_with(0, 8)
    }
}

/// Up to 7 bytes, little-endian, as [`load_tail`] reads them, in at most
/// three loads instead of one per byte.
#[inline(always)]
fn short_tail(rest: &[u8]) -> u64 {
    let len = rest.len();
    debug_assert!(len < 8);
    if len >= 4 {
        let low = u32::from_le_bytes(rest[..4].try_into().expect("four bytes"));
        let high = u32::from_le_bytes(rest[len - 4..].try_into().expect("four bytes"));
        u64::from(low) | (u64::from(high) << (8 * (len - 4)))
    } else if len > 0 {
        // Bytes 0, len / 2 and len - 1 are every byte of a 1 to 3 byte
        // tail, and the ones that coincide hold the same bits.
        u64::from(rest[0])
            | (u64::from(rest[len / 2]) << (8 * (len / 2)))
            | (u64::from(rest[len - 1]) << (8 * (len - 1)))
    } else {
        0
    }
}

/// Up to 7 bytes of `bytes` from `start`, little-endian.
#[inline(always)]
fn load_tail(bytes: &[u8], start: usize, len: usize) -> u64 {
    debug_assert!(len < 8);
    let mut word = 0u64;
    for (index, byte) in bytes[start..start + len].iter().enumerate() {
        word |= u64::from(*byte) << (8 * index);
    }
    word
}

impl<const C: usize, const D: usize> Hasher for SipHasher<C, D> {
    fn write(&mut self, bytes: &[u8]) {
        let length = bytes.len();
        self.length = self.length.wrapping_add(length);
        let mut start = 0;
        if self.ntail != 0 {
            let needed = 8 - self.ntail;
            let fill = needed.min(length);
            self.tail |= load_tail(bytes, 0, fill) << (8 * self.ntail);
            if length < needed {
                self.ntail += length;
                return;
            }
            let word = self.tail;
            self.compress(word);
            self.tail = 0;
            self.ntail = 0;
            start = needed;
        }
        let mut words = bytes[start..].chunks_exact(8);
        for chunk in &mut words {
            let word = u64::from_le_bytes(chunk.try_into().expect("an 8-byte chunk"));
            self.compress(word);
        }
        let rest = words.remainder();
        self.tail = load_tail(rest, 0, rest.len());
        self.ntail = rest.len();
    }

    #[inline]
    fn write_u64(&mut self, word: u64) {
        if self.ntail == 0 {
            self.length = self.length.wrapping_add(8);
            self.compress(word);
        } else {
            self.write(&word.to_le_bytes());
        }
    }

    #[inline]
    fn write_i64(&mut self, word: i64) {
        self.write_u64(word as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        let mut state = self.clone();
        let last = ((self.length as u64 & 0xff) << 56) | self.tail;
        state.compress(last);
        state.v2 ^= 0xff;
        for _ in 0..D {
            state.round();
        }
        state.v0 ^ state.v1 ^ state.v2 ^ state.v3
    }
}

/// A hash of `value` that agrees with `Value`'s equality.
pub fn hash_value(value: &Value) -> u64 {
    // A scalar or `str` key, the common case, hashes with one write. The
    // walk below hashes every other key, so the two never meet: equal
    // values are of one kind and take the same path.
    let mut root = value;
    while let Value::Union(union) = root {
        root = &union.payload;
    }
    match root {
        Value::String(text) => return hash_str(text.as_str()),
        Value::Int(integer) => {
            // An integer that fits in `i64`, as nearly all do, hashes as
            // `hash_i64` does. A wider one takes the general hasher.
            let narrow = match integer.representation() {
                IntegerRepresentation::Signed(wide) => i64::try_from(wide).ok(),
                IntegerRepresentation::Unsigned(wide) => i64::try_from(wide).ok(),
            };
            if let Some(word) = narrow {
                return hash_i64(word);
            }
            let mut hasher = hash_state();
            write_integer(integer, &mut hasher);
            return hasher.finish();
        }
        Value::Float(float) => {
            let (k0, k1) = hash_keys();
            return KeyedHasher::hash_word(k0, k1, float_word(*float));
        }
        Value::Bool(flag) => {
            let (k0, k1) = hash_keys();
            return KeyedHasher::hash_bytes(k0, k1, &[u8::from(*flag)]);
        }
        _ => {}
    }
    let mut hasher = hash_state();
    let mut budget = HASH_NODE_BUDGET;
    hash_nodes(root, &mut hasher, &mut budget);
    hasher.finish()
}

/// Hashes `root` and everything it holds, depth first, visiting at most
/// `budget` nodes.
fn hash_nodes(root: &Value, hasher: &mut impl Hasher, budget: &mut usize) {
    let mut pending = vec![root];
    while let Some(mut value) = pending.pop() {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        // Union equality compares active payloads, and a bare member equals
        // a union holding it, so a union hashes as its payload.
        while let Value::Union(union) = value {
            value = &union.payload;
        }
        if hash_scalar(value, hasher) {
            continue;
        }
        std::mem::discriminant(value).hash(hasher);
        match value {
            Value::String(text) => text.hash(hasher),
            Value::Duration(nanoseconds) => nanoseconds.hash(hasher),
            Value::Range(range) => {
                range.start.hash(hasher);
                range.end.hash(hasher);
                range.step.hash(hasher);
            }
            Value::ModuleNamespace(namespace) => namespace.path.hash(hasher),
            Value::Function(function) => function.name.hash(hasher),
            Value::Tuple(tuple) => {
                tuple.elements.len().hash(hasher);
                pending.extend(tuple.elements.iter().rev());
            }
            Value::Vec(vector) => {
                vector.elements.len().hash(hasher);
                match vector.elements.value_refs() {
                    Some(values) => pending.extend(values.rev()),
                    None => {
                        // The elements would be the next nodes visited, so
                        // hashing each here in order, scalars in one write
                        // and records kept in place as their values,
                        // matches hashing the same list of `Value`s.
                        for element in &vector.elements {
                            if *budget == 0 {
                                break;
                            }
                            if hash_scalar(&element, hasher) {
                                *budget -= 1;
                            } else {
                                hash_nodes(&element, hasher, budget);
                            }
                        }
                    }
                }
            }
            Value::Instance(instance) => {
                nominal_runtime_base_name(&instance.class_name).hash(hasher);
                if instance.fields.typed().is_some() {
                    // The fields would be the next nodes visited, so hashing
                    // each here, in order, after the names, matches hashing
                    // the same instance with its fields in a map. A field
                    // read out of the layout has nothing to borrow from.
                    for (name, _) in instance.fields.iter() {
                        name.hash(hasher);
                    }
                    for (_, field) in instance.fields.iter() {
                        if *budget == 0 {
                            break;
                        }
                        hash_nodes(&field, hasher, budget);
                    }
                    continue;
                }
                let fields = instance.fields.map();
                for (name, field) in fields.iter().rev() {
                    if name.as_str() != DIRECT_RUNTIME_TYPE_FIELD {
                        pending.push(field);
                    }
                }
                for name in fields.keys() {
                    if name.as_str() != DIRECT_RUNTIME_TYPE_FIELD {
                        name.hash(hasher);
                    }
                }
            }
            Value::EnumVariant(variant) => {
                nominal_runtime_base_name(&variant.enum_name).hash(hasher);
                variant.variant_name.hash(hasher);
                variant.payloads.len().hash(hasher);
                if let VariantPayloads::Values(payloads) = &variant.payloads {
                    pending.extend(payloads.iter().rev());
                    continue;
                }
                // The payloads would be the next nodes visited, so hashing
                // each here, in order, matches hashing the same variant with
                // its payloads as values. A payload read out of the layout
                // has nothing to borrow from.
                for payload in variant.payloads.iter() {
                    if *budget == 0 {
                        break;
                    }
                    hash_nodes(&payload, hasher, budget);
                }
            }
            // Set and dict equality ignore order, so only the size is a
            // structural hash. Every other kind hashes by kind alone.
            Value::Set(set) => set.elements.len().hash(hasher),
            Value::Map(map) => map.entries.len().hash(hasher),
            _ => {}
        }
    }
}

/// [`hash_value`] of a `str` value holding `text`, without building one.
#[inline]
pub fn hash_str(text: &str) -> u64 {
    let (k0, k1) = hash_keys();
    KeyedHasher::hash_bytes(k0, k1, text.as_bytes())
}

/// [`hash_value`] of an integer value equal to `integer`, of any width,
/// without building one.
#[inline]
pub fn hash_i64(integer: i64) -> u64 {
    let (k0, k1) = hash_keys();
    KeyedHasher::hash_word(k0, k1, integer as u64)
}

/// Writes an integer key as one word. Integers of different widths compare
/// equal when their values do, so the word depends on the value alone: a
/// value that fits in `i64` writes that `i64`, and any other value writes
/// its 128 bits.
#[inline(always)]
fn write_integer(integer: &IntegerValue, hasher: &mut impl Hasher) {
    match integer.representation() {
        IntegerRepresentation::Signed(wide) => match i64::try_from(wide) {
            Ok(word) => hasher.write_i64(word),
            Err(_) => hasher.write_u128(wide as u128),
        },
        IntegerRepresentation::Unsigned(wide) => match i64::try_from(wide) {
            Ok(word) => hasher.write_i64(word),
            Err(_) => hasher.write_u128(wide),
        },
    }
}

/// The word a `float` key hashes as. `0.0 == -0.0`, so both are zero. NaN
/// never equals anything, so its bits are as good as any word.
fn float_word(float: f64) -> u64 {
    if float == 0.0 {
        0
    } else {
        float.to_bits()
    }
}

/// Whether `key` is an integer equal to `integer`. A union equals its
/// active member.
fn key_is_i64(mut key: &Value, integer: i64) -> bool {
    while let Value::Union(union) = key {
        key = &union.payload;
    }
    let Value::Int(key) = key else {
        return false;
    };
    match key.representation() {
        IntegerRepresentation::Signed(wide) => wide == i128::from(integer),
        IntegerRepresentation::Unsigned(wide) => integer >= 0 && wide == integer as u128,
    }
}

/// Whether `key` equals a `str` value holding `text`. A union equals its
/// active member.
fn key_is_str(mut key: &Value, text: &str) -> bool {
    while let Value::Union(union) = key {
        key = &union.payload;
    }
    matches!(key, Value::String(key) if short_bytes_eq(key.as_bytes(), text.as_bytes()))
}

/// `candidate == key`, with the common `str` and integer keys compared
/// inline instead of through `Value`'s whole equality.
#[inline(always)]
fn key_equals(candidate: &Value, key: &Value) -> bool {
    match (candidate, key) {
        (Value::String(candidate), Value::String(key)) => {
            short_bytes_eq(candidate.as_bytes(), key.as_bytes())
        }
        (Value::Int(candidate), Value::Int(key)) => candidate == key,
        _ => candidate == key,
    }
}

/// `a == b`, comparing up to sixteen bytes in two overlapping words
/// instead of calling `memcmp`. Keys, enum names, and variant names are
/// mostly short.
pub(crate) fn short_bytes_eq(a: &[u8], b: &[u8]) -> bool {
    fn word<const N: usize>(bytes: &[u8], at: usize) -> [u8; N] {
        bytes[at..at + N].try_into().expect("in range")
    }
    let len = a.len();
    if len != b.len() {
        return false;
    }
    match len {
        0 => true,
        1..=3 => a[0] == b[0] && a[len / 2] == b[len / 2] && a[len - 1] == b[len - 1],
        4..=7 => {
            word::<4>(a, 0) == word::<4>(b, 0) && word::<4>(a, len - 4) == word::<4>(b, len - 4)
        }
        8..=16 => {
            word::<8>(a, 0) == word::<8>(b, 0) && word::<8>(a, len - 8) == word::<8>(b, len - 8)
        }
        _ => a == b,
    }
}

/// Hashes an `int`, `float`, or `bool` value and returns true, or returns
/// false for any other value.
#[inline(always)]
fn hash_scalar(value: &Value, hasher: &mut impl Hasher) -> bool {
    match value {
        Value::Int(integer) => {
            std::mem::discriminant(value).hash(hasher);
            integer.hash(hasher);
        }
        Value::Float(float) => {
            std::mem::discriminant(value).hash(hasher);
            float_word(*float).hash(hasher);
        }
        Value::Bool(flag) => {
            std::mem::discriminant(value).hash(hasher);
            flag.hash(hasher);
        }
        _ => return false,
    }
    true
}

/// What a stored entry exposes as its key.
pub trait Keyed {
    fn key(&self) -> &Value;
}

impl Keyed for Value {
    fn key(&self) -> &Value {
        self
    }
}

impl Keyed for (Value, Value) {
    fn key(&self) -> &Value {
        &self.0
    }
}

/// Insertion-ordered entries with a hash index over their keys.
///
/// Reads go through [`Self::iter`], [`Self::len`], and the lookup methods.
/// Mutation goes through methods that keep the index current, so a key can
/// never change in place.
///
/// A small collection is a `Vec`. A large one keeps its entries and index
/// together. Either sits behind an `Arc`, so a clone shares the storage and
/// the first write through a shared handle copies it. Reads never copy.
///
/// [`Self::remove`] takes expected constant time and keeps the order of the
/// rest, as CPython's `dict` does: a large collection leaves a hole in the
/// removed entry's slot instead of shifting every later entry, and packs
/// its slots again once holes make up more than half of them.
#[derive(Debug)]
pub struct IndexedEntries<T> {
    storage: Storage<T>,
}

#[derive(Debug)]
enum Storage<T> {
    // Eager entries have only strong owners. A write to unique storage
    // needs one acquire load, even when every lookup and store takes its
    // own lock. Snapshots and transfers still share immutable storage.
    Small(EntriesArc<Vec<T>>),
    Large(EntriesArc<Large<T>>),
    /// The entries of an object in a parsed JSON document's tape, made
    /// values the first time a read needs them as values. A write first
    /// takes them as eager storage of its own.
    Lazy(Arc<LazyEntries<T>>),
    /// Two immutable ordered sets and the exact selection of new right-hand slots.
    Union(Arc<UnionEntries<T>>),
}

/// Entries held as values: what a tape's entries become. `Storage` has the
/// same two forms beside `Lazy`, so a read of a dict that never came from a
/// tape tests its form once.
#[derive(Debug)]
enum Eager<T> {
    Small(EntriesArc<Vec<T>>),
    Large(EntriesArc<Large<T>>),
}

impl<T> Eager<T> {
    /// The same storage, shared.
    fn share(&self) -> Self {
        match self {
            Self::Small(entries) => Self::Small(EntriesArc::clone(entries)),
            Self::Large(large) => Self::Large(EntriesArc::clone(large)),
        }
    }

    fn by_ref(&self) -> EagerRef<'_, T> {
        match self {
            Self::Small(entries) => EagerRef::Small(entries),
            Self::Large(large) => EagerRef::Large(large),
        }
    }

    fn into_storage(self) -> Storage<T> {
        match self {
            Self::Small(entries) => Storage::Small(entries),
            Self::Large(large) => Storage::Large(large),
        }
    }
}

/// The storage a read sees (see [`IndexedEntries::eager`]).
enum EagerRef<'a, T> {
    Small(&'a EntriesArc<Vec<T>>),
    Large(&'a EntriesArc<Large<T>>),
}

/// The storage a write sees (see [`IndexedEntries::eager_mut`]).
enum EagerMut<'a, T> {
    Small(&'a mut EntriesArc<Vec<T>>),
    Large(&'a mut EntriesArc<Large<T>>),
}

/// See [`Storage::Lazy`].
#[derive(Debug)]
struct LazyEntries<T> {
    source: crate::json_tape::JsonNode,
    build: fn(&crate::json_tape::JsonNode) -> Eager<T>,
    realized: OnceLock<Eager<T>>,
}

impl<T> LazyEntries<T> {
    fn realized(&self) -> &Eager<T> {
        self.realized.get_or_init(|| (self.build)(&self.source))
    }

    /// Kept out of line, so a read of entries that are already values
    /// pays for one test of their form and no call.
    #[cold]
    #[inline(never)]
    fn realized_ref(&self) -> EagerRef<'_, T> {
        self.realized().by_ref()
    }
}

#[derive(Clone, Debug)]
struct Large<T> {
    /// The entries in insertion order. A removed entry leaves `None` behind.
    /// The last slot is never a hole.
    slots: Vec<Option<T>>,
    /// `(hash, slot)` for each distinct key.
    index: HashTable<(u64, usize)>,
    /// How many slots are holes.
    holes: usize,
    /// The slot the last lookup found, a hint only: a loop that reads the
    /// entries in order, such as `for key in d.keys(): d.lookup(key)`,
    /// finds the next key in the slot after it without hashing.
    cursor: LookupCursor,
}

/// [`Large::cursor`]: a word any thread may overwrite, since a wrong hint
/// costs one compare and a stale one is never trusted without one.
#[derive(Debug, Default)]
struct LookupCursor(std::sync::atomic::AtomicUsize);

impl Clone for LookupCursor {
    fn clone(&self) -> Self {
        Self(std::sync::atomic::AtomicUsize::new(self.get()))
    }
}

impl LookupCursor {
    #[inline(always)]
    fn get(&self) -> usize {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }

    #[inline(always)]
    fn set(&self, slot: usize) {
        self.0.store(slot, std::sync::atomic::Ordering::Relaxed);
    }
}

impl<T: Keyed> Large<T> {
    /// The slot of the entry whose key `matches`, where `hash` is the hash
    /// of every such key. It first tries the entry after the one the last
    /// lookup found, skipping a hole or two, and remembers the slot it
    /// finds.
    #[inline(always)]
    fn lookup(
        &self,
        hash: impl FnOnce() -> u64,
        matches: impl Fn(&Value) -> bool,
    ) -> Option<usize> {
        let start = self.cursor.get().wrapping_add(1);
        for slot in start..self.slots.len().min(start.saturating_add(3)) {
            if let Some(entry) = &self.slots[slot] {
                if matches(entry.key()) {
                    self.cursor.set(slot);
                    return Some(slot);
                }
                break;
            }
        }
        let found = self.find_hashed(hash(), |_, entry| matches(entry.key()));
        if let Some(slot) = found {
            self.cursor.set(slot);
        }
        found
    }
}

impl<T> Large<T> {
    #[inline(always)]
    fn find_hashed(&self, hash: u64, matches: impl Fn(usize, &T) -> bool) -> Option<usize> {
        self.index
            .find(hash, |&(entry_hash, slot)| {
                entry_hash == hash
                    && self.slots[slot]
                        .as_ref()
                        .is_some_and(|entry| matches(slot, entry))
            })
            .map(|&(_, slot)| slot)
    }

    /// Takes the entry out of `slot`, which `(hash, slot)` indexes, and
    /// leaves a hole.
    fn take_slot(&mut self, slot: usize, hash: u64) -> T {
        let removed = self.slots[slot]
            .take()
            .expect("the index names only slots that hold an entry");
        if let Ok(found) = self
            .index
            .find_entry(hash, |&(_, candidate)| candidate == slot)
        {
            found.remove();
        }
        self.holes += 1;
        while let Some(None) = self.slots.last() {
            self.slots.pop();
            self.holes -= 1;
        }
        if self.holes * 2 > self.slots.len() {
            self.pack();
        }
        removed
    }

    /// Closes every hole, keeping the order of the entries.
    fn pack(&mut self) {
        let mut index = std::mem::take(&mut self.index);
        self.pack_index(&mut index);
        self.index = index;
        self.slots.retain(Option::is_some);
        self.holes = 0;
    }

    /// Rewrites `index`, an index of these slots, for the same entries
    /// with their holes closed: each slot moves down by the holes before it.
    fn pack_index(&self, index: &mut HashTable<(u64, usize)>) {
        if self.holes == 0 {
            return;
        }
        let mut moved_to = Vec::with_capacity(self.slots.len());
        let mut next = 0;
        for slot in &self.slots {
            moved_to.push(next);
            next += usize::from(slot.is_some());
        }
        for (_, slot) in index.iter_mut() {
            *slot = moved_to[*slot];
        }
    }

    /// The slot of the entry `index` entries from the start.
    fn slot_of(&self, index: usize) -> Option<usize> {
        if self.holes == 0 {
            return (index < self.slots.len()).then_some(index);
        }
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.is_some())
            .nth(index)
            .map(|(slot, _)| slot)
    }
}

/// An ordered union has a materialized membership selection. Reads borrow
/// the original entries. Later writes to either source detach by the ordinary
/// COW rules. A write to the result gives it private slots and an index.
#[derive(Debug)]
struct UnionEntries<T> {
    left: EntriesArc<Large<T>>,
    right: EntriesArc<Large<T>>,
    added: Vec<u64>,
    len: usize,
    right_end: usize,
    materialize: fn(&Self) -> EntriesArc<Large<T>>,
    realized: OnceLock<EntriesArc<Large<T>>>,
}

impl<T> UnionEntries<T> {
    fn realized(&self) -> &EntriesArc<Large<T>> {
        self.realized.get_or_init(|| (self.materialize)(self))
    }

    fn has_right_slot(&self, slot: usize) -> bool {
        slot < self.right_end && self.added[slot / 64] & (1 << (slot % 64)) != 0
    }

    fn next_right_slot(&self, from: usize) -> Option<usize> {
        if from >= self.right_end {
            return None;
        }
        let index = from / 64;
        let first = self.added[index] & (u64::MAX << (from % 64));
        if first != 0 {
            return Some(index * 64 + first.trailing_zeros() as usize);
        }
        self.added[index + 1..]
            .iter()
            .enumerate()
            .find(|(_, bits)| **bits != 0)
            .map(|(i, bits)| (index + 1 + i) * 64 + bits.trailing_zeros() as usize)
    }

    fn slot(&self, slot: usize) -> Option<&T> {
        if slot < self.left.slots.len() {
            return self.left.slots[slot].as_ref();
        }
        let right = slot - self.left.slots.len();
        if self.has_right_slot(right) {
            self.right.slots[right].as_ref()
        } else {
            None
        }
    }

    fn next_slot(&self, from: usize) -> Option<usize> {
        let offset = self.left.slots.len();
        if from < offset {
            if self.left.holes == 0 {
                return Some(from);
            }
            if let Some(at) = self.left.slots[from..].iter().position(Option::is_some) {
                return Some(from + at);
            }
        }
        self.next_right_slot(from.saturating_sub(offset))
            .map(|s| offset + s)
    }

    fn ordinal(&self, index: usize) -> Option<&T> {
        let count = self.left.slots.len() - self.left.holes;
        if index < count {
            return self.left.slots[self.left.slot_of(index)?].as_ref();
        }
        let mut remaining = index.checked_sub(count)?;
        for (word, mut bits) in self.added.iter().copied().enumerate() {
            let n = bits.count_ones() as usize;
            if remaining < n {
                for _ in 0..remaining {
                    bits &= bits - 1;
                }
                return self.right.slots[word * 64 + bits.trailing_zeros() as usize].as_ref();
            }
            remaining -= n;
        }
        None
    }
}

impl<T: Clone> UnionEntries<T> {
    fn copy_slots(&self) -> Large<T> {
        // Keep the virtual slot numbers: remove may already have probed the
        // union and will use that same slot immediately after detaching.
        let mut result = (*self.left).clone();
        let offset = result.slots.len();
        result.slots.reserve(self.right_end);
        for slot in 0..self.right_end {
            let value = if self.has_right_slot(slot) {
                self.right.slots[slot].clone()
            } else {
                None
            };
            result.slots.push(value);
        }
        result.holes = result.slots.len() - self.len;
        result
            .index
            .reserve(self.len - result.index.len(), |&(hash, _)| hash);
        for &(hash, slot) in &self.right.index {
            if self.has_right_slot(slot) {
                result
                    .index
                    .insert_unique(hash, (hash, offset + slot), |&(hash, _)| hash);
            }
        }
        result
    }
}

/// The entries of a `dict`: key and value pairs.
pub type DictEntries = IndexedEntries<(Value, Value)>;

/// The members of a `set`.
pub type SetElements = IndexedEntries<Value>;

impl<T> Default for IndexedEntries<T> {
    fn default() -> Self {
        Self {
            storage: Storage::Small(EntriesArc::new(Vec::new())),
        }
    }
}

impl<T> Clone for IndexedEntries<T> {
    /// Shares the storage. A later write copies it first.
    fn clone(&self) -> Self {
        let storage = match &self.storage {
            Storage::Small(entries) => Storage::Small(EntriesArc::clone(entries)),
            Storage::Large(large) => Storage::Large(EntriesArc::clone(large)),
            Storage::Lazy(lazy) => Storage::Lazy(Arc::clone(lazy)),
            Storage::Union(union) => Storage::Union(Arc::clone(union)),
        };
        Self { storage }
    }
}

impl<T: PartialEq> PartialEq<Vec<T>> for IndexedEntries<T> {
    /// Compares the entries in order.
    fn eq(&self, other: &Vec<T>) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

impl<T: Clone> IntoIterator for IndexedEntries<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}

impl<'a, T> IntoIterator for &'a IndexedEntries<T> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T: Keyed + Clone> From<Vec<T>> for IndexedEntries<T> {
    /// Wraps entries the caller already holds. When two entries have equal
    /// keys, lookups find the first, as a scan of the same entries would.
    fn from(entries: Vec<T>) -> Self {
        let mut collection = Self {
            storage: Storage::Small(EntriesArc::new(entries)),
        };
        collection.rebuild_index();
        collection
    }
}

impl<T: Keyed + Clone> FromIterator<T> for IndexedEntries<T> {
    /// Collects entries the caller already holds, as [`From<Vec<T>>`] does.
    fn from_iter<I: IntoIterator<Item = T>>(entries: I) -> Self {
        Self::from(entries.into_iter().collect::<Vec<_>>())
    }
}

/// The entries of an [`IndexedEntries`] in order.
#[derive(Debug)]
pub struct Iter<'a, T> {
    slots: Slots<'a, T>,
    remaining: usize,
}

#[derive(Debug)]
enum Slots<'a, T> {
    Dense(std::slice::Iter<'a, T>),
    Holes(std::slice::Iter<'a, Option<T>>),
    Union(&'a UnionEntries<T>, std::ops::Range<usize>),
}

impl<T> Clone for Iter<'_, T> {
    fn clone(&self) -> Self {
        let slots = match &self.slots {
            Slots::Dense(entries) => Slots::Dense(entries.clone()),
            Slots::Holes(slots) => Slots::Holes(slots.clone()),
            Slots::Union(union, range) => Slots::Union(union, range.clone()),
        };
        Self {
            slots,
            remaining: self.remaining,
        }
    }
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<&'a T> {
        let entry = match &mut self.slots {
            Slots::Dense(entries) => entries.next(),
            Slots::Holes(slots) => slots.find_map(Option::as_ref),
            Slots::Union(union, range) => range.find_map(|i| union.slot(i)),
        };
        self.remaining -= usize::from(entry.is_some());
        entry
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<T> DoubleEndedIterator for Iter<'_, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let entry = match &mut self.slots {
            Slots::Dense(entries) => entries.next_back(),
            Slots::Holes(slots) => slots.rev().find_map(Option::as_ref),
            Slots::Union(union, range) => range.rev().find_map(|i| union.slot(i)),
        };
        self.remaining -= usize::from(entry.is_some());
        entry
    }
}

impl<T> ExactSizeIterator for Iter<'_, T> {}

impl<T> IndexedEntries<T> {
    /// The storage, a tape's entries made values first.
    #[inline(always)]
    fn eager(&self) -> EagerRef<'_, T> {
        match &self.storage {
            Storage::Small(entries) => EagerRef::Small(entries),
            Storage::Large(large) => EagerRef::Large(large),
            Storage::Lazy(lazy) => lazy.realized_ref(),
            Storage::Union(union) => EagerRef::Large(union.realized()),
        }
    }

    /// The storage for a write: a tape's entries become storage of this
    /// collection's own, shared with any other handle until it writes.
    #[inline(always)]
    fn eager_mut(&mut self) -> EagerMut<'_, T> {
        if matches!(self.storage, Storage::Lazy(_) | Storage::Union(_)) {
            self.take_realized();
        }
        match &mut self.storage {
            Storage::Small(entries) => EagerMut::Small(entries),
            Storage::Large(large) => EagerMut::Large(large),
            Storage::Lazy(_) | Storage::Union(_) => unreachable!("the entries were made eager"),
        }
    }

    /// Replaces a tape's entries with storage of this collection's own.
    #[cold]
    #[inline(never)]
    fn take_realized(&mut self) {
        match &self.storage {
            Storage::Lazy(lazy) => self.storage = lazy.realized().share().into_storage(),
            Storage::Union(union) => {
                self.storage = Storage::Large(EntriesArc::clone(union.realized()))
            }
            _ => {}
        }
    }

    fn into_eager(self) -> Eager<T> {
        match self.storage {
            Storage::Small(entries) => Eager::Small(entries),
            Storage::Large(large) => Eager::Large(large),
            Storage::Lazy(lazy) => lazy.realized().share(),
            Storage::Union(union) => Eager::Large(EntriesArc::clone(union.realized())),
        }
    }

    pub fn len(&self) -> usize {
        if let Storage::Union(union) = &self.storage {
            return union.len;
        }
        if let Storage::Lazy(lazy) = &self.storage {
            if lazy.realized.get().is_none() {
                return lazy.source.entry_count();
            }
        }
        match self.eager() {
            EagerRef::Small(entries) => entries.len(),
            EagerRef::Large(large) => large.slots.len() - large.holes,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The entries in order.
    pub fn iter(&self) -> Iter<'_, T> {
        if let Storage::Union(union) = &self.storage {
            return Iter {
                slots: Slots::Union(union, 0..union.left.slots.len() + union.right_end),
                remaining: union.len,
            };
        }
        let slots = match self.eager() {
            EagerRef::Small(entries) => Slots::Dense(entries.iter()),
            EagerRef::Large(large) => Slots::Holes(large.slots.iter()),
        };
        Iter {
            slots,
            remaining: self.len(),
        }
    }

    /// The entry `index` entries from the start. Dense collections use direct
    /// indexing. Ordered unions scan the selection bitmap for right-hand entries,
    /// and collections with removed entries may scan their slots.
    pub fn get(&self, index: usize) -> Option<&T> {
        if let Storage::Union(union) = &self.storage {
            return union.ordinal(index);
        }
        match self.eager() {
            EagerRef::Small(entries) => entries.get(index),
            EagerRef::Large(large) => large
                .slot_of(index)
                .and_then(|slot| large.slots[slot].as_ref()),
        }
    }

    pub fn first(&self) -> Option<&T> {
        self.iter().next()
    }

    pub fn last(&self) -> Option<&T> {
        self.iter().next_back()
    }

    /// The entry in `slot`, as the lookup methods return it.
    ///
    /// # Panics
    ///
    /// Panics when no entry is in `slot`.
    pub fn at_slot(&self, slot: usize) -> &T {
        if let Storage::Union(union) = &self.storage {
            return union.slot(slot).expect("a probed union slot is live");
        }
        let entry = match self.eager() {
            EagerRef::Small(entries) => entries.get(slot),
            EagerRef::Large(large) => large.slots.get(slot).and_then(Option::as_ref),
        };
        entry.expect("a slot from a lookup holds an entry")
    }

    /// The entry in `slot`, or `None` for a hole or a slot past the end.
    pub fn slot(&self, slot: usize) -> Option<&T> {
        if let Storage::Union(union) = &self.storage {
            return union.slot(slot);
        }
        match self.eager() {
            EagerRef::Small(entries) => entries.get(slot),
            EagerRef::Large(large) => large.slots.get(slot).and_then(Option::as_ref),
        }
    }

    /// The first slot at or after `from` that holds an entry. Walking from
    /// slot 0, each time from the slot after the last one found, visits the
    /// entries in order while the collection is unchanged.
    pub fn next_slot(&self, from: usize) -> Option<usize> {
        if let Storage::Union(union) = &self.storage {
            return union.next_slot(from);
        }
        match self.eager() {
            EagerRef::Small(entries) => (from < entries.len()).then_some(from),
            EagerRef::Large(large) => large
                .slots
                .get(from..)?
                .iter()
                .position(Option::is_some)
                .map(|offset| from + offset),
        }
    }

    /// The slot of the first entry that `matches`, scanning in order.
    pub fn slot_where(&self, mut matches: impl FnMut(&T) -> bool) -> Option<usize> {
        if let Storage::Union(union) = &self.storage {
            return (0..union.left.slots.len() + union.right_end)
                .find(|&i| union.slot(i).is_some_and(&mut matches));
        }
        match self.eager() {
            EagerRef::Small(entries) => entries.iter().position(matches),
            EagerRef::Large(large) => large
                .slots
                .iter()
                .position(|slot| slot.as_ref().is_some_and(&mut matches)),
        }
    }

    /// How many entries fit before the storage grows.
    pub fn capacity(&self) -> usize {
        if let Storage::Union(union) = &self.storage {
            return union.len;
        }
        match self.eager() {
            EagerRef::Small(entries) => entries.capacity(),
            EagerRef::Large(large) => large.slots.capacity() - large.holes,
        }
    }

    /// Whether another handle shares this storage. Entries read from a
    /// tape count as shared, so a write takes a copy first.
    pub fn is_shared(&self) -> bool {
        if matches!(self.storage, Storage::Lazy(_) | Storage::Union(_)) {
            return true;
        }
        match self.eager() {
            EagerRef::Small(entries) => !entries.is_unique(),
            EagerRef::Large(large) => !large.is_unique(),
        }
    }

    /// The address of the storage, which clones share until a write.
    pub fn as_ptr(&self) -> *const T {
        if let Storage::Union(union) = &self.storage {
            return Arc::as_ptr(union).cast();
        }
        match self.eager() {
            EagerRef::Small(entries) => entries.as_slice().as_ptr(),
            EagerRef::Large(large) => large.slots.as_ptr().cast(),
        }
    }
}

impl<T: Clone> IndexedEntries<T> {
    pub fn into_vec(self) -> Vec<T> {
        if matches!(self.storage, Storage::Union(_)) {
            return self.iter().cloned().collect();
        }
        match self.into_eager() {
            Eager::Small(entries) => EntriesArc::unwrap_or_clone(entries),
            Eager::Large(large) => EntriesArc::unwrap_or_clone(large)
                .slots
                .into_iter()
                .flatten()
                .collect(),
        }
    }

    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        match self.eager_mut() {
            EagerMut::Small(entries) => EntriesArc::make_mut(entries).try_reserve(additional),
            EagerMut::Large(large) => EntriesArc::make_mut(large).slots.try_reserve(additional),
        }
    }
}

impl<T: Keyed + Clone> IndexedEntries<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Wraps a clone of `source`'s entries, in the same order, reusing its
    /// index instead of hashing every key again.
    pub fn cloned_from(entries: Vec<T>, source: &Self) -> Self {
        match source.eager() {
            EagerRef::Large(large) if entries.len() == source.len() => {
                let mut index = large.index.clone();
                large.pack_index(&mut index);
                Self {
                    storage: Storage::Large(EntriesArc::new(Large {
                        slots: entries.into_iter().map(Some).collect(),
                        index,
                        holes: 0,
                        cursor: LookupCursor::default(),
                    })),
                }
            }
            _ => Self::from(entries),
        }
    }

    /// The slot of the entry whose key equals `key`, for [`Self::at_slot`].
    pub fn position(&self, key: &Value) -> Option<usize> {
        self.lookup(|| hash_value(key), |candidate| key_equals(candidate, key))
    }

    /// [`Self::position`] of a `str` key holding `text`, without building it.
    pub fn position_str(&self, text: &str) -> Option<usize> {
        self.lookup(|| hash_str(text), |candidate| key_is_str(candidate, text))
    }

    /// [`Self::position`] of an integer key equal to `integer`, without
    /// building it.
    pub fn position_i64(&self, integer: i64) -> Option<usize> {
        self.lookup(
            || hash_i64(integer),
            |candidate| key_is_i64(candidate, integer),
        )
    }

    /// [`Self::probe`] for a read, which first tries the entry after the
    /// one the last read found, skipping a hole or two, and remembers the
    /// slot it finds.
    #[inline(always)]
    fn lookup(
        &self,
        hash: impl FnOnce() -> u64,
        matches: impl Fn(&Value) -> bool,
    ) -> Option<usize> {
        // The two eager forms, nearly every dict and set, are tested first
        // and once.
        match &self.storage {
            Storage::Small(entries) => entries.iter().position(|entry| matches(entry.key())),
            Storage::Large(large) => large.lookup(hash, matches),
            Storage::Lazy(lazy) => match lazy.realized_ref() {
                EagerRef::Small(entries) => entries.iter().position(|entry| matches(entry.key())),
                EagerRef::Large(large) => large.lookup(hash, matches),
            },
            Storage::Union(_) => self.probe(hash, matches).0,
        }
    }

    /// The slot of the entry whose key `matches`, and the key's hash when
    /// the collection hashed it. A small collection scans and hashes
    /// nothing. `hash` is the hash of every key that `matches`.
    #[inline(always)]
    fn probe(
        &self,
        hash: impl FnOnce() -> u64,
        matches: impl Fn(&Value) -> bool,
    ) -> (Option<usize>, Option<u64>) {
        if let Storage::Union(union) = &self.storage {
            let hash = hash();
            let found = union
                .left
                .find_hashed(hash, |_, entry| matches(entry.key()))
                .or_else(|| {
                    union
                        .right
                        .find_hashed(hash, |slot, entry| {
                            union.has_right_slot(slot) && matches(entry.key())
                        })
                        .map(|right| union.left.slots.len() + right)
                });
            return (found, Some(hash));
        }
        match self.eager() {
            EagerRef::Small(entries) => {
                (entries.iter().position(|entry| matches(entry.key())), None)
            }
            EagerRef::Large(large) => {
                let hash = hash();
                let position = large.find_hashed(hash, |_, entry| matches(entry.key()));
                (position, Some(hash))
            }
        }
    }

    pub fn contains_key(&self, key: &Value) -> bool {
        self.position(key).is_some()
    }

    pub fn find(&self, key: &Value) -> Option<&T> {
        self.position(key).map(|slot| self.at_slot(slot))
    }

    /// The entry in `slot`, for writing.
    fn at_slot_mut(&mut self, slot: usize) -> &mut T {
        let entry = match self.eager_mut() {
            EagerMut::Small(entries) => EntriesArc::make_mut(entries).get_mut(slot),
            EagerMut::Large(large) => EntriesArc::make_mut(large)
                .slots
                .get_mut(slot)
                .and_then(Option::as_mut),
        };
        entry.expect("a slot from a lookup holds an entry")
    }

    /// Appends an entry whose key the caller knows is absent.
    pub fn push_new(&mut self, entry: T) {
        self.push_new_hashed(entry, None);
    }

    /// [`Self::push_new`] with the key's hash when a probe already has it.
    fn push_new_hashed(&mut self, entry: T, hash: Option<u64>) {
        match self.eager_mut() {
            EagerMut::Small(entries) => {
                let entries = EntriesArc::make_mut(entries);
                entries.push(entry);
                if entries.len() > INDEX_THRESHOLD {
                    self.rebuild_index();
                }
            }
            EagerMut::Large(large) => {
                let large = EntriesArc::make_mut(large);
                let slot = large.slots.len();
                let hash = hash.unwrap_or_else(|| hash_value(entry.key()));
                large.slots.push(Some(entry));
                large
                    .index
                    .insert_unique(hash, (hash, slot), |&(hash, _)| hash);
            }
        }
    }

    /// Removes the entry whose key equals `key` in expected constant time,
    /// and keeps the order of the rest. A large collection may keep a hole
    /// where the entry was, which makes [`Self::get`] scan.
    pub fn remove(&mut self, key: &Value) -> Option<T> {
        let (slot, hash) = self.probe(|| hash_value(key), |candidate| key_equals(candidate, key));
        Some(self.remove_slot(slot?, hash, false))
    }

    /// [`Self::remove`] of the entry under an integer key equal to
    /// `integer`, without building the key.
    pub fn remove_i64(&mut self, integer: i64) -> Option<T> {
        let (slot, hash) = self.probe(
            || hash_i64(integer),
            |candidate| key_is_i64(candidate, integer),
        );
        Some(self.remove_slot(slot?, hash, false))
    }

    /// [`Self::remove`] of a `str` key holding `text`, without building it.
    pub fn remove_str(&mut self, text: &str) -> Option<T> {
        let (slot, hash) = self.probe(|| hash_str(text), |candidate| key_is_str(candidate, text));
        Some(self.remove_slot(slot?, hash, false))
    }

    /// Removes the entry whose key equals `key` and keeps the order of the
    /// rest without leaving a hole, so [`Self::get`] stays constant time.
    /// It takes time proportional to the collection's size, unless the
    /// entry is the last one.
    pub fn shift_remove(&mut self, key: &Value) -> Option<T> {
        let (slot, hash) = self.probe(|| hash_value(key), |candidate| key_equals(candidate, key));
        Some(self.remove_slot(slot?, hash, true))
    }

    /// Removes the entry `index` entries from the start, as
    /// [`Self::shift_remove`] does.
    pub fn shift_remove_index(&mut self, index: usize) -> Option<T> {
        let slot = match self.eager() {
            EagerRef::Small(entries) => (index < entries.len()).then_some(index)?,
            EagerRef::Large(large) => large.slot_of(index)?,
        };
        Some(self.remove_slot(slot, None, true))
    }

    /// Removes the entry in `slot`, and closes every hole when `pack` is
    /// set. `hash` is its key's hash when a probe already has it.
    fn remove_slot(&mut self, slot: usize, hash: Option<u64>, pack: bool) -> T {
        let removed = match self.eager_mut() {
            EagerMut::Small(entries) => return EntriesArc::make_mut(entries).remove(slot),
            EagerMut::Large(large) => {
                let large = EntriesArc::make_mut(large);
                let hash = hash.unwrap_or_else(|| {
                    hash_value(
                        large.slots[slot]
                            .as_ref()
                            .expect("a slot from a lookup holds an entry")
                            .key(),
                    )
                });
                let removed = large.take_slot(slot, hash);
                if pack && large.holes > 0 {
                    large.pack();
                }
                removed
            }
        };
        if self.len() <= INDEX_THRESHOLD {
            self.rebuild_index();
        }
        removed
    }

    pub fn clear(&mut self) {
        if matches!(self.storage, Storage::Union(_)) {
            *self = Self::default();
            return;
        }
        match self.eager_mut() {
            EagerMut::Small(entries) => match EntriesArc::get_mut(entries) {
                Some(entries) => entries.clear(),
                None => *entries = EntriesArc::new(Vec::new()),
            },
            EagerMut::Large(large) => {
                // Clearing an unshared collection keeps its room.
                let entries = match EntriesArc::get_mut(large) {
                    Some(large) => Vec::with_capacity(large.slots.capacity()),
                    None => Vec::new(),
                };
                self.storage = Storage::Small(EntriesArc::new(entries));
            }
        }
    }

    /// Removes and returns the last entry.
    pub fn pop(&mut self) -> Option<T> {
        let index = self.len().checked_sub(1)?;
        self.shift_remove_index(index)
    }

    /// Picks the storage that fits the current length and rebuilds the
    /// index from the entries.
    fn rebuild_index(&mut self) {
        let entries = match self.eager_mut() {
            EagerMut::Small(entries) => std::mem::take(EntriesArc::make_mut(entries)),
            EagerMut::Large(large) => std::mem::take(&mut EntriesArc::make_mut(large).slots)
                .into_iter()
                .flatten()
                .collect(),
        };
        if entries.len() <= INDEX_THRESHOLD {
            self.storage = Storage::Small(EntriesArc::new(entries));
            return;
        }
        let mut index = HashTable::with_capacity(entries.len());
        for (position, entry) in entries.iter().enumerate() {
            let hash = hash_value(entry.key());
            let duplicate = index
                .find(hash, |&(entry_hash, candidate): &(u64, usize)| {
                    entry_hash == hash && entries[candidate].key() == entry.key()
                })
                .is_some();
            if !duplicate {
                index.insert_unique(hash, (hash, position), |&(hash, _)| hash);
            }
        }
        self.storage = Storage::Large(EntriesArc::new(Large {
            slots: entries.into_iter().map(Some).collect(),
            index,
            holes: 0,
            cursor: LookupCursor::default(),
        }));
    }
}

impl DictEntries {
    /// The entries of the object `node` in a tape, read from the tape until
    /// a read needs them as values.
    pub fn from_json_object(node: crate::json_tape::JsonNode) -> Self {
        fn build(node: &crate::json_tape::JsonNode) -> Eager<(Value, Value)> {
            // The parse took no object that repeats a key.
            DictEntries::from(node.object_entries()).into_eager()
        }
        Self {
            storage: Storage::Lazy(Arc::new(LazyEntries {
                source: node,
                build,
                realized: OnceLock::new(),
            })),
        }
    }

    /// The tape object these entries are still read from, before any read
    /// needed them as values.
    pub fn unrealized_json(&self) -> Option<&crate::json_tape::JsonNode> {
        match &self.storage {
            Storage::Lazy(lazy) if lazy.realized.get().is_none() => Some(&lazy.source),
            _ => None,
        }
    }

    pub fn value_of(&self, key: &Value) -> Option<&Value> {
        self.find(key).map(|(_, value)| value)
    }

    /// [`Self::value_of`] a `str` key holding `text`.
    pub fn value_of_str(&self, text: &str) -> Option<&Value> {
        self.position_str(text).map(|slot| &self.at_slot(slot).1)
    }

    /// [`Self::value_of`] an integer key equal to `integer`.
    pub fn value_of_i64(&self, integer: i64) -> Option<&Value> {
        self.position_i64(integer).map(|slot| &self.at_slot(slot).1)
    }

    /// The value in `slot`, for writing.
    pub fn value_at_mut(&mut self, slot: usize) -> &mut Value {
        &mut self.at_slot_mut(slot).1
    }

    /// [`Self::insert`] under an `int64` key, which is hashed once and
    /// becomes a value only when the key is new.
    pub fn insert_i64(&mut self, key: i64, value: Value) -> Option<Value> {
        match self.probe(|| hash_i64(key), |candidate| key_is_i64(candidate, key)) {
            (Some(slot), _) => Some(std::mem::replace(self.value_at_mut(slot), value)),
            (None, hash) => {
                self.push_new_hashed((Value::Int(IntegerValue::from_i64(key)), value), hash);
                None
            }
        }
    }

    /// [`Self::insert`] under a `str` key holding `text`, which is hashed
    /// once and copied into a value only when the key is new.
    pub fn insert_str(&mut self, text: &str, value: Value) -> Option<Value> {
        match self.probe(|| hash_str(text), |candidate| key_is_str(candidate, text)) {
            (Some(slot), _) => Some(std::mem::replace(self.value_at_mut(slot), value)),
            (None, hash) => {
                self.push_new_hashed((Value::String(text.into()), value), hash);
                None
            }
        }
    }

    /// Stores `value` under `key`. An equal key keeps its slot and its
    /// original key, and the replaced value is returned.
    pub fn insert(&mut self, key: Value, value: Value) -> Option<Value> {
        // A small dict scans and writes its one vector, made unshared once.
        if let EagerMut::Small(entries) = self.eager_mut() {
            let entries = EntriesArc::make_mut(entries);
            if let Some(entry) = entries
                .iter_mut()
                .find(|(candidate, _)| key_equals(candidate, &key))
            {
                return Some(std::mem::replace(&mut entry.1, value));
            }
            if entries.len() < INDEX_THRESHOLD {
                entries.push((key, value));
                return None;
            }
        }
        match self.probe(|| hash_value(&key), |candidate| key_equals(candidate, &key)) {
            (Some(slot), _) => Some(std::mem::replace(self.value_at_mut(slot), value)),
            (None, hash) => {
                self.push_new_hashed((key, value), hash);
                None
            }
        }
    }
}

impl SetElements {
    /// Keeps the left-hand insertion order and appends previously absent right-hand
    /// entries. An indexed source already holds every key's hash.
    fn union(&self, other: &Self) -> Self {
        let (EagerRef::Large(left), EagerRef::Large(right)) = (self.eager(), other.eager()) else {
            let mut result = self.clone();
            for element in other.iter() {
                let (found, hash) =
                    result.probe(|| hash_value(element), |key| key_equals(key, element));
                if found.is_none() {
                    result.push_new_hashed(element.clone(), hash);
                }
            }
            return result;
        };
        if EntriesArc::ptr_eq(left, right) {
            return self.clone();
        }

        let mut added = Vec::new();
        let mut additional = 0;
        let mut right_end = 0;
        // These keys have value semantics even when two sets share their
        // backing. Resource and class payloads retain the eager copy/drop
        // behavior of the original union, including for discarded duplicates.
        let shareable = |element: &Value| {
            matches!(
                element,
                Value::Int(_)
                    | Value::Float(_)
                    | Value::Bool(_)
                    | Value::String(_)
                    | Value::Duration(_)
                    | Value::Unit
            )
        };
        let mut share_right = true;
        for &(hash, slot) in &right.index {
            let element = right.slots[slot].as_ref().expect("indexed set entry");
            share_right &= shareable(element);
            let found = left.find_hashed(hash, |_, candidate| key_equals(candidate, element));
            if found.is_none() {
                if added.is_empty() {
                    added.resize(right.slots.len().div_ceil(64), 0u64);
                }
                added[slot / 64] |= 1 << (slot % 64);
                additional += 1;
                right_end = right_end.max(slot + 1);
            }
        }
        if additional == 0 {
            return self.clone();
        }
        let snapshot = UnionEntries {
            left: EntriesArc::clone(left),
            right: EntriesArc::clone(right),
            added,
            len: self.len() + additional,
            right_end,
            materialize: |snapshot| EntriesArc::new(snapshot.copy_slots()),
            realized: OnceLock::new(),
        };
        let storage = if share_right && left.slots.iter().flatten().all(shareable) {
            Storage::Union(Arc::new(snapshot))
        } else {
            let mut stored = snapshot.copy_slots();
            if stored.holes != 0 {
                stored.pack();
            }
            Storage::Large(EntriesArc::new(stored))
        };
        Self { storage }
    }

    /// Adds `element` unless an equal one is present. Returns whether it was added.
    pub fn insert(&mut self, element: Value) -> bool {
        match self.probe(
            || hash_value(&element),
            |candidate| key_equals(candidate, &element),
        ) {
            (Some(_), _) => false,
            (None, hash) => {
                self.push_new_hashed(element, hash);
                true
            }
        }
    }

    /// [`Self::insert`] of a `str` holding `text`, which is hashed once
    /// and copied into a value only when it is new.
    pub fn insert_str(&mut self, text: &str) -> bool {
        match self.probe(|| hash_str(text), |candidate| key_is_str(candidate, text)) {
            (Some(_), _) => false,
            (None, hash) => {
                self.push_new_hashed(Value::String(text.into()), hash);
                true
            }
        }
    }

    /// [`Self::insert`] of `int64` `integer`, which is hashed and compared
    /// before a value is built for it.
    pub fn insert_i64(&mut self, integer: i64) -> bool {
        match self.probe(
            || hash_i64(integer),
            |candidate| key_is_i64(candidate, integer),
        ) {
            (Some(_), _) => false,
            (None, hash) => {
                self.push_new_hashed(Value::Int(IntegerValue::from_i64(integer)), hash);
                true
            }
        }
    }
}

/// A `Vec` that clones share, for `list` elements. A clone shares the
/// storage. Mutable access through `DerefMut` copies shared storage first,
/// so every existing `Vec` write keeps value semantics.
///
/// The handle holds the `Vec` itself, so a list that is never copied makes
/// one allocation, its elements, and a read reaches them without a second
/// pointer. The first clone makes the count the handles then share. While
/// the count is above one, every handle holds the same pointer, capacity
/// and length and none changes them, so each reads the storage through its
/// own copy of the three words. The handle that brings the count to zero
/// drops the `Vec`.
pub struct SharedVec<T> {
    values: ManuallyDrop<Vec<T>>,
    /// The number of handles that share `values`, or null while this is
    /// the only one and no clone was ever made of it.
    holders: AtomicPtr<AtomicUsize>,
    /// The handles share the elements across threads as an `Arc` would.
    shared: PhantomData<Arc<Vec<T>>>,
}

impl<T> SharedVec<T> {
    pub const fn new() -> Self {
        Self::of(Vec::new())
    }

    const fn of(values: Vec<T>) -> Self {
        Self {
            values: ManuallyDrop::new(values),
            holders: AtomicPtr::new(std::ptr::null_mut()),
            shared: PhantomData,
        }
    }

    /// Whether another handle shares this storage.
    #[inline]
    pub fn is_shared(&self) -> bool {
        // SAFETY: a count lives until its last handle drops.
        unsafe { self.holders.load(Ordering::Acquire).as_ref() }
            .is_some_and(|holders| holders.load(Ordering::Acquire) > 1)
    }

    /// The storage for writing when no other handle shares it, without a
    /// copy.
    #[inline]
    pub fn unshared_mut(&mut self) -> Option<&mut Vec<T>> {
        // A count of one cannot grow: only a clone of this handle could
        // raise it, and the caller holds the handle mutably.
        if self.is_shared() {
            None
        } else {
            Some(&mut self.values)
        }
    }

    /// Gives up this handle's share of the storage, and returns the `Vec`
    /// when the handle was the last one. The handle must not be used
    /// again, except to be given a new `Vec`.
    ///
    /// # Safety
    ///
    /// The caller forgets the handle or overwrites its fields without
    /// dropping them.
    unsafe fn release(&mut self) -> Option<Vec<T>> {
        let holders = *self.holders.get_mut();
        // SAFETY: the count lives until its last handle drops.
        let last = match unsafe { holders.as_ref() } {
            None => true,
            Some(count) => count.fetch_sub(1, Ordering::AcqRel) == 1,
        };
        if !last {
            return None;
        }
        if !holders.is_null() {
            // SAFETY: the last handle frees the count a clone boxed.
            drop(unsafe { Box::from_raw(holders) });
        }
        // SAFETY: no other handle is left to read the storage, and the
        // caller never reads this one's copy again.
        Some(unsafe { ManuallyDrop::take(&mut self.values) })
    }
}

impl<T> Drop for SharedVec<T> {
    #[inline]
    fn drop(&mut self) {
        if self.holders.get_mut().is_null() {
            // SAFETY: a handle with no count is the only one, and is not
            // used after its drop.
            unsafe { ManuallyDrop::drop(&mut self.values) };
            return;
        }
        // SAFETY: the handle is not used after its drop.
        drop(unsafe { self.release() });
    }
}

impl<T> Default for SharedVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Clone for SharedVec<T> {
    /// Shares the storage. A later write copies it first.
    fn clone(&self) -> Self {
        let mut holders = self.holders.load(Ordering::Acquire);
        if holders.is_null() {
            // The first clone makes the count, for this handle and the
            // clone. Another thread may clone the same handle at once
            // under a shared lock, and then one of the two counts wins.
            let made = Box::into_raw(Box::new(AtomicUsize::new(2)));
            match self.holders.compare_exchange(
                std::ptr::null_mut(),
                made,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => holders = made,
                Err(installed) => {
                    // SAFETY: the count was never published.
                    drop(unsafe { Box::from_raw(made) });
                    holders = installed;
                    // SAFETY: this handle keeps the count alive.
                    unsafe { &*holders }.fetch_add(1, Ordering::AcqRel);
                }
            }
        } else {
            // SAFETY: this handle keeps the count alive.
            unsafe { &*holders }.fetch_add(1, Ordering::AcqRel);
        }
        Self {
            // SAFETY: the clone reads the same storage through its own
            // copy of the pointer, capacity and length, which no handle
            // changes while the count is above one, and only the last
            // handle drops the `Vec`.
            values: unsafe { std::ptr::read(&self.values) },
            holders: AtomicPtr::new(holders),
            shared: PhantomData,
        }
    }
}

impl<T> Deref for SharedVec<T> {
    type Target = Vec<T>;

    #[inline]
    fn deref(&self) -> &Vec<T> {
        &self.values
    }
}

impl<T: Clone> DerefMut for SharedVec<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Vec<T> {
        if self.is_shared() {
            self.unshare();
        }
        &mut self.values
    }
}

impl<T: Clone> SharedVec<T> {
    /// Gives this handle a copy of the storage that other handles share.
    #[cold]
    fn unshare(&mut self) {
        let copy = Vec::clone(&self.values);
        // SAFETY: the fields are overwritten below without a drop. The
        // other handles may have dropped since the check, and then the
        // old `Vec` is this handle's to drop.
        drop(unsafe { self.release() });
        self.values = ManuallyDrop::new(copy);
        *self.holders.get_mut() = std::ptr::null_mut();
    }

    pub fn into_vec(self) -> Vec<T> {
        let mut this = ManuallyDrop::new(self);
        let copy = this.is_shared().then(|| Vec::clone(&this.values));
        // SAFETY: the handle is forgotten.
        match (unsafe { this.release() }, copy) {
            (Some(values), _) => values,
            (None, Some(copy)) => copy,
            // The other handles copied nothing: they still hold the
            // storage, which this handle read while it held its share.
            (None, None) => unreachable!("an unshared handle is the last one"),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for SharedVec<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("SharedVec")
            .field(&*self.values)
            .finish()
    }
}

impl<T> From<Vec<T>> for SharedVec<T> {
    fn from(elements: Vec<T>) -> Self {
        Self::of(elements)
    }
}

impl<T> FromIterator<T> for SharedVec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(elements: I) -> Self {
        Self::from(elements.into_iter().collect::<Vec<_>>())
    }
}

impl<T: Clone> IntoIterator for SharedVec<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}

impl<'a, T> IntoIterator for &'a SharedVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}

impl<'a, T: Clone> IntoIterator for &'a mut SharedVec<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.deref_mut().iter_mut()
    }
}

impl<T: PartialEq> PartialEq for SharedVec<T> {
    fn eq(&self, other: &Self) -> bool {
        // No pointer shortcut: a list holding NaN is not equal to itself.
        *self.values == *other.values
    }
}

impl<T: PartialEq> PartialEq<Vec<T>> for SharedVec<T> {
    fn eq(&self, other: &Vec<T>) -> bool {
        *self.values == *other
    }
}

/// The result of a set-algebra method, by name: a new set for `union`,
/// `intersection`, `difference`, and `symmetric_difference`, and whether the
/// relation holds for `is_subset`, `is_superset`, and `is_disjoint`. A new
/// set keeps the receiver's order, then `other`'s for the elements only
/// `other` has. `None` for any other name.
pub fn set_algebra(method: &str, left: &SetElements, right: &SetElements) -> Option<SetAlgebra> {
    let collect = |elements: &mut dyn Iterator<Item = &Value>| {
        let mut result = SetElements::new();
        for element in elements {
            result.insert(element.clone());
        }
        SetAlgebra::Elements(result)
    };
    let within = |inner: &SetElements, outer: &SetElements| {
        inner.len() <= outer.len() && inner.iter().all(|element| outer.contains_key(element))
    };
    Some(match method {
        "union" => SetAlgebra::Elements(left.union(right)),
        "intersection" => collect(&mut left.iter().filter(|element| right.contains_key(element))),
        "difference" => collect(&mut left.iter().filter(|element| !right.contains_key(element))),
        "symmetric_difference" => collect(
            &mut left
                .iter()
                .filter(|element| !right.contains_key(element))
                .chain(right.iter().filter(|element| !left.contains_key(element))),
        ),
        "is_subset" => SetAlgebra::Holds(within(left, right)),
        "is_superset" => SetAlgebra::Holds(within(right, left)),
        "is_disjoint" => {
            // Probe the larger set with the elements of the smaller.
            let (small, large) = if left.len() <= right.len() {
                (left, right)
            } else {
                (right, left)
            };
            SetAlgebra::Holds(!small.iter().any(|element| large.contains_key(element)))
        }
        _ => return None,
    })
}

/// What `set_algebra` computed.
pub enum SetAlgebra {
    Elements(SetElements),
    Holds(bool),
}

/// The set-algebra methods, in the order of their collection-operation
/// opcodes, which start at `SET_ALGEBRA_FIRST_OPCODE`.
pub const SET_ALGEBRA_METHODS: [&str; 7] = [
    "union",
    "intersection",
    "difference",
    "symmetric_difference",
    "is_subset",
    "is_superset",
    "is_disjoint",
];

pub const SET_ALGEBRA_FIRST_OPCODE: i64 = 8;

#[cfg(test)]
#[path = "collection_index_tests.rs"]
mod tests;
