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
use std::hash::{BuildHasher, Hash, Hasher, RandomState};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, OnceLock};

use hashbrown::HashTable;

use crate::integer::{IntegerRepresentation, IntegerValue};
use crate::runtime_value::{nominal_runtime_base_name, Value, DIRECT_RUNTIME_TYPE_FIELD};

/// Collections with at most this many entries scan instead of hashing.
pub const INDEX_THRESHOLD: usize = 8;

/// The number of value nodes [`hash_value`] visits. Deeper or larger keys
/// still hash consistently because equal values visit the same prefix.
const HASH_NODE_BUDGET: usize = 64;

fn hash_state() -> &'static RandomState {
    static STATE: OnceLock<RandomState> = OnceLock::new();
    STATE.get_or_init(RandomState::new)
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
    let mut hasher = hash_state().build_hasher();
    match root {
        Value::Int(integer) => {
            write_integer(integer, &mut hasher);
            return hasher.finish();
        }
        Value::Float(float) => {
            hasher.write_u64(float_word(*float));
            return hasher.finish();
        }
        Value::Bool(flag) => {
            hasher.write_u8(u8::from(*flag));
            return hasher.finish();
        }
        Value::String(text) => {
            hasher.write(text.as_bytes());
            return hasher.finish();
        }
        _ => {}
    }
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
                pending.extend(variant.payloads.iter().rev());
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
pub fn hash_str(text: &str) -> u64 {
    let mut hasher = hash_state().build_hasher();
    hasher.write(text.as_bytes());
    hasher.finish()
}

/// [`hash_value`] of an integer value equal to `integer`, of any width,
/// without building one.
pub fn hash_i64(integer: i64) -> u64 {
    let mut hasher = hash_state().build_hasher();
    hasher.write_i64(integer);
    hasher.finish()
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
    Small(Arc<Vec<T>>),
    Large(Arc<Large<T>>),
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
}

impl<T> Large<T> {
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

/// The entries of a `dict`: key and value pairs.
pub type DictEntries = IndexedEntries<(Value, Value)>;

/// The members of a `set`.
pub type SetElements = IndexedEntries<Value>;

impl<T> Default for IndexedEntries<T> {
    fn default() -> Self {
        Self {
            storage: Storage::Small(Arc::new(Vec::new())),
        }
    }
}

impl<T> Clone for IndexedEntries<T> {
    /// Shares the storage. A later write copies it first.
    fn clone(&self) -> Self {
        let storage = match &self.storage {
            Storage::Small(entries) => Storage::Small(Arc::clone(entries)),
            Storage::Large(large) => Storage::Large(Arc::clone(large)),
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
            storage: Storage::Small(Arc::new(entries)),
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
}

impl<T> Clone for Iter<'_, T> {
    fn clone(&self) -> Self {
        let slots = match &self.slots {
            Slots::Dense(entries) => Slots::Dense(entries.clone()),
            Slots::Holes(slots) => Slots::Holes(slots.clone()),
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
        };
        self.remaining -= usize::from(entry.is_some());
        entry
    }
}

impl<T> ExactSizeIterator for Iter<'_, T> {}

impl<T> IndexedEntries<T> {
    pub fn len(&self) -> usize {
        match &self.storage {
            Storage::Small(entries) => entries.len(),
            Storage::Large(large) => large.slots.len() - large.holes,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The entries in order.
    pub fn iter(&self) -> Iter<'_, T> {
        let slots = match &self.storage {
            Storage::Small(entries) => Slots::Dense(entries.iter()),
            Storage::Large(large) => Slots::Holes(large.slots.iter()),
        };
        Iter {
            slots,
            remaining: self.len(),
        }
    }

    /// The entry `index` entries from the start. It takes constant time
    /// unless removals left holes in a large collection.
    pub fn get(&self, index: usize) -> Option<&T> {
        match &self.storage {
            Storage::Small(entries) => entries.get(index),
            Storage::Large(large) => large
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
        let entry = match &self.storage {
            Storage::Small(entries) => entries.get(slot),
            Storage::Large(large) => large.slots.get(slot).and_then(Option::as_ref),
        };
        entry.expect("a slot from a lookup holds an entry")
    }

    /// The slot of the first entry that `matches`, scanning in order.
    pub fn slot_where(&self, mut matches: impl FnMut(&T) -> bool) -> Option<usize> {
        match &self.storage {
            Storage::Small(entries) => entries.iter().position(matches),
            Storage::Large(large) => large
                .slots
                .iter()
                .position(|slot| slot.as_ref().is_some_and(&mut matches)),
        }
    }

    /// How many entries fit before the storage grows.
    pub fn capacity(&self) -> usize {
        match &self.storage {
            Storage::Small(entries) => entries.capacity(),
            Storage::Large(large) => large.slots.capacity() - large.holes,
        }
    }

    /// Whether another handle shares this storage.
    pub fn is_shared(&self) -> bool {
        match &self.storage {
            Storage::Small(entries) => Arc::strong_count(entries) > 1,
            Storage::Large(large) => Arc::strong_count(large) > 1,
        }
    }

    /// The address of the storage, which clones share until a write.
    pub fn as_ptr(&self) -> *const T {
        match &self.storage {
            Storage::Small(entries) => entries.as_ptr(),
            Storage::Large(large) => large.slots.as_ptr().cast(),
        }
    }
}

impl<T: Clone> IndexedEntries<T> {
    pub fn into_vec(self) -> Vec<T> {
        match self.storage {
            Storage::Small(entries) => Arc::unwrap_or_clone(entries),
            Storage::Large(large) => Arc::unwrap_or_clone(large)
                .slots
                .into_iter()
                .flatten()
                .collect(),
        }
    }

    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        match &mut self.storage {
            Storage::Small(entries) => Arc::make_mut(entries).try_reserve(additional),
            Storage::Large(large) => Arc::make_mut(large).slots.try_reserve(additional),
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
        match &source.storage {
            Storage::Large(large) if entries.len() == source.len() => {
                let mut index = large.index.clone();
                large.pack_index(&mut index);
                Self {
                    storage: Storage::Large(Arc::new(Large {
                        slots: entries.into_iter().map(Some).collect(),
                        index,
                        holes: 0,
                    })),
                }
            }
            _ => Self::from(entries),
        }
    }

    /// The slot of the entry whose key equals `key`, for [`Self::at_slot`].
    pub fn position(&self, key: &Value) -> Option<usize> {
        self.probe(|| hash_value(key), |candidate| candidate == key)
            .0
    }

    /// [`Self::position`] of a `str` key holding `text`, without building it.
    pub fn position_str(&self, text: &str) -> Option<usize> {
        self.probe(|| hash_str(text), |candidate| key_is_str(candidate, text))
            .0
    }

    /// [`Self::position`] of an integer key equal to `integer`, without
    /// building it.
    pub fn position_i64(&self, integer: i64) -> Option<usize> {
        self.probe(
            || hash_i64(integer),
            |candidate| key_is_i64(candidate, integer),
        )
        .0
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
        match &self.storage {
            Storage::Small(entries) => {
                (entries.iter().position(|entry| matches(entry.key())), None)
            }
            Storage::Large(large) => {
                let hash = hash();
                let position = large
                    .index
                    .find(hash, |&(entry_hash, slot)| {
                        entry_hash == hash
                            && large.slots[slot]
                                .as_ref()
                                .is_some_and(|entry| matches(entry.key()))
                    })
                    .map(|&(_, slot)| slot);
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
        let entry = match &mut self.storage {
            Storage::Small(entries) => Arc::make_mut(entries).get_mut(slot),
            Storage::Large(large) => Arc::make_mut(large)
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
        match &mut self.storage {
            Storage::Small(entries) => {
                let entries = Arc::make_mut(entries);
                entries.push(entry);
                if entries.len() > INDEX_THRESHOLD {
                    self.rebuild_index();
                }
            }
            Storage::Large(large) => {
                let large = Arc::make_mut(large);
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
        let (slot, hash) = self.probe(|| hash_value(key), |candidate| candidate == key);
        Some(self.remove_slot(slot?, hash, false))
    }

    /// Removes the entry whose key equals `key` and keeps the order of the
    /// rest without leaving a hole, so [`Self::get`] stays constant time.
    /// It takes time proportional to the collection's size, unless the
    /// entry is the last one.
    pub fn shift_remove(&mut self, key: &Value) -> Option<T> {
        let (slot, hash) = self.probe(|| hash_value(key), |candidate| candidate == key);
        Some(self.remove_slot(slot?, hash, true))
    }

    /// Removes the entry `index` entries from the start, as
    /// [`Self::shift_remove`] does.
    pub fn shift_remove_index(&mut self, index: usize) -> Option<T> {
        let slot = match &self.storage {
            Storage::Small(entries) => (index < entries.len()).then_some(index)?,
            Storage::Large(large) => large.slot_of(index)?,
        };
        Some(self.remove_slot(slot, None, true))
    }

    /// Removes the entry in `slot`, and closes every hole when `pack` is
    /// set. `hash` is its key's hash when a probe already has it.
    fn remove_slot(&mut self, slot: usize, hash: Option<u64>, pack: bool) -> T {
        let removed = match &mut self.storage {
            Storage::Small(entries) => return Arc::make_mut(entries).remove(slot),
            Storage::Large(large) => {
                let large = Arc::make_mut(large);
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
        match &mut self.storage {
            Storage::Small(entries) => match Arc::get_mut(entries) {
                Some(entries) => entries.clear(),
                None => *entries = Arc::new(Vec::new()),
            },
            Storage::Large(large) => {
                // Clearing an unshared collection keeps its room.
                let entries = match Arc::get_mut(large) {
                    Some(large) => Vec::with_capacity(large.slots.capacity()),
                    None => Vec::new(),
                };
                self.storage = Storage::Small(Arc::new(entries));
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
        let entries = match &mut self.storage {
            Storage::Small(entries) => std::mem::take(Arc::make_mut(entries)),
            Storage::Large(large) => std::mem::take(&mut Arc::make_mut(large).slots)
                .into_iter()
                .flatten()
                .collect(),
        };
        if entries.len() <= INDEX_THRESHOLD {
            self.storage = Storage::Small(Arc::new(entries));
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
        self.storage = Storage::Large(Arc::new(Large {
            slots: entries.into_iter().map(Some).collect(),
            index,
            holes: 0,
        }));
    }
}

impl DictEntries {
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

    /// Stores `value` under `key`. An equal key keeps its slot and its
    /// original key, and the replaced value is returned.
    pub fn insert(&mut self, key: Value, value: Value) -> Option<Value> {
        match self.probe(|| hash_value(&key), |candidate| *candidate == key) {
            (Some(slot), _) => Some(std::mem::replace(self.value_at_mut(slot), value)),
            (None, hash) => {
                self.push_new_hashed((key, value), hash);
                None
            }
        }
    }
}

impl SetElements {
    /// Adds `element` unless an equal one is present. Returns whether it was added.
    pub fn insert(&mut self, element: Value) -> bool {
        match self.probe(|| hash_value(&element), |candidate| *candidate == element) {
            (Some(_), _) => false,
            (None, hash) => {
                self.push_new_hashed(element, hash);
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

/// A `Vec` behind an `Arc`, for `list` elements. A clone shares the
/// storage. Mutable access through `DerefMut` copies shared storage first,
/// so every existing `Vec` write keeps value semantics.
#[derive(Debug)]
pub struct SharedVec<T>(Arc<Vec<T>>);

impl<T> SharedVec<T> {
    pub fn new() -> Self {
        Self(Arc::new(Vec::new()))
    }

    /// Whether another handle shares this storage.
    pub fn is_shared(&self) -> bool {
        Arc::strong_count(&self.0) > 1
    }

    /// The storage for writing when no other handle shares it, without a
    /// copy.
    pub fn unshared_mut(&mut self) -> Option<&mut Vec<T>> {
        Arc::get_mut(&mut self.0)
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
        Self(Arc::clone(&self.0))
    }
}

impl<T> Deref for SharedVec<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Vec<T> {
        &self.0
    }
}

impl<T: Clone> DerefMut for SharedVec<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        Arc::make_mut(&mut self.0)
    }
}

impl<T: Clone> SharedVec<T> {
    pub fn into_vec(self) -> Vec<T> {
        Arc::unwrap_or_clone(self.0)
    }
}

impl<T> From<Vec<T>> for SharedVec<T> {
    fn from(elements: Vec<T>) -> Self {
        Self(Arc::new(elements))
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
        self.0.iter()
    }
}

impl<'a, T: Clone> IntoIterator for &'a mut SharedVec<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        Arc::make_mut(&mut self.0).iter_mut()
    }
}

impl<T: PartialEq> PartialEq for SharedVec<T> {
    fn eq(&self, other: &Self) -> bool {
        // No pointer shortcut: a list holding NaN is not equal to itself.
        self.0 == other.0
    }
}

impl<T: PartialEq> PartialEq<Vec<T>> for SharedVec<T> {
    fn eq(&self, other: &Vec<T>) -> bool {
        *self.0 == *other
    }
}

#[cfg(test)]
#[path = "collection_index_tests.rs"]
mod tests;
