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
    let mut pending = vec![root];
    let mut budget = HASH_NODE_BUDGET;
    while let Some(mut value) = pending.pop() {
        if budget == 0 {
            break;
        }
        budget -= 1;
        // Union equality compares active payloads, and a bare member equals
        // a union holding it, so a union hashes as its payload.
        while let Value::Union(union) = value {
            value = &union.payload;
        }
        if hash_scalar(value, &mut hasher) {
            continue;
        }
        std::mem::discriminant(value).hash(&mut hasher);
        match value {
            Value::String(text) => text.hash(&mut hasher),
            Value::Duration(nanoseconds) => nanoseconds.hash(&mut hasher),
            Value::Range(range) => {
                range.start.hash(&mut hasher);
                range.end.hash(&mut hasher);
            }
            Value::ModuleNamespace(namespace) => namespace.path.hash(&mut hasher),
            Value::Function(function) => function.name.hash(&mut hasher),
            Value::Tuple(tuple) => {
                tuple.elements.len().hash(&mut hasher);
                pending.extend(tuple.elements.iter().rev());
            }
            Value::Vec(vector) => {
                vector.elements.len().hash(&mut hasher);
                match vector.elements.as_values() {
                    Some(values) => pending.extend(values.iter().rev()),
                    None => {
                        // Scalar elements would be the next nodes visited
                        // and push nothing, so hashing them here in order
                        // matches hashing the same list of `Value`s.
                        for element in &vector.elements {
                            if budget == 0 {
                                break;
                            }
                            budget -= 1;
                            hash_scalar(&element, &mut hasher);
                        }
                    }
                }
            }
            Value::Instance(instance) => {
                nominal_runtime_base_name(&instance.class_name).hash(&mut hasher);
                for (name, field) in instance.fields.iter().rev() {
                    if name.as_str() != DIRECT_RUNTIME_TYPE_FIELD {
                        pending.push(field);
                    }
                }
                for name in instance.fields.keys() {
                    if name.as_str() != DIRECT_RUNTIME_TYPE_FIELD {
                        name.hash(&mut hasher);
                    }
                }
            }
            Value::EnumVariant(variant) => {
                nominal_runtime_base_name(&variant.enum_name).hash(&mut hasher);
                variant.variant_name.hash(&mut hasher);
                variant.payloads.len().hash(&mut hasher);
                pending.extend(variant.payloads.iter().rev());
            }
            // Set and dict equality ignore order, so only the size is a
            // structural hash. Every other kind hashes by kind alone.
            Value::Set(set) => set.elements.len().hash(&mut hasher),
            Value::Map(map) => map.entries.len().hash(&mut hasher),
            _ => {}
        }
    }
    hasher.finish()
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
    matches!(key, Value::String(key) if key == text)
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
/// The type derefs to a slice for reading. Mutation goes through methods
/// that keep the index current, so a key can never change in place.
///
/// A small collection is a `Vec`. A large one keeps its entries and index
/// together. Either sits behind an `Arc`, so a clone shares the storage and
/// the first write through a shared handle copies it. Reads never copy.
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
    entries: Vec<T>,
    /// `(hash, position)` for each distinct key.
    index: HashTable<(u64, usize)>,
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

impl<T> Deref for IndexedEntries<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        self.entries()
    }
}

impl<T: PartialEq> PartialEq<Vec<T>> for IndexedEntries<T> {
    /// Compares the entries in order.
    fn eq(&self, other: &Vec<T>) -> bool {
        self.entries() == other.as_slice()
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
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries().iter()
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

impl<T> IndexedEntries<T> {
    fn entries(&self) -> &Vec<T> {
        match &self.storage {
            Storage::Small(entries) => entries,
            Storage::Large(large) => &large.entries,
        }
    }

    pub fn as_slice(&self) -> &[T] {
        self.entries()
    }

    pub fn capacity(&self) -> usize {
        self.entries().capacity()
    }

    /// Whether another handle shares this storage.
    pub fn is_shared(&self) -> bool {
        match &self.storage {
            Storage::Small(entries) => Arc::strong_count(entries) > 1,
            Storage::Large(large) => Arc::strong_count(large) > 1,
        }
    }
}

impl<T: Clone> IndexedEntries<T> {
    /// The entries for writing. Shared storage is copied first.
    fn entries_mut(&mut self) -> &mut Vec<T> {
        match &mut self.storage {
            Storage::Small(entries) => Arc::make_mut(entries),
            Storage::Large(large) => &mut Arc::make_mut(large).entries,
        }
    }

    pub fn into_vec(self) -> Vec<T> {
        match self.storage {
            Storage::Small(entries) => Arc::unwrap_or_clone(entries),
            Storage::Large(large) => Arc::unwrap_or_clone(large).entries,
        }
    }

    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.entries_mut().try_reserve(additional)
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
            Storage::Large(large) if entries.len() == large.entries.len() => Self {
                storage: Storage::Large(Arc::new(Large {
                    entries,
                    index: large.index.clone(),
                })),
            },
            _ => Self::from(entries),
        }
    }

    /// The position of the entry whose key equals `key`.
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

    /// The position of the entry whose key `matches`, and the key's hash
    /// when the collection hashed it. A small collection scans and hashes
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
                    .find(hash, |&(entry_hash, position)| {
                        entry_hash == hash && matches(large.entries[position].key())
                    })
                    .map(|&(_, position)| position);
                (position, Some(hash))
            }
        }
    }

    pub fn contains_key(&self, key: &Value) -> bool {
        self.position(key).is_some()
    }

    pub fn find(&self, key: &Value) -> Option<&T> {
        self.position(key).map(|position| &self.entries()[position])
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
                let position = large.entries.len();
                let hash = hash.unwrap_or_else(|| hash_value(entry.key()));
                large.entries.push(entry);
                large
                    .index
                    .insert_unique(hash, (hash, position), |&(hash, _)| hash);
            }
        }
    }

    /// Removes the entry at `position` and keeps the order of the rest.
    pub fn shift_remove_at(&mut self, position: usize) -> T {
        let removed = match &mut self.storage {
            Storage::Small(entries) => return Arc::make_mut(entries).remove(position),
            Storage::Large(large) => {
                let large = Arc::make_mut(large);
                let removed = large.entries.remove(position);
                let hash = hash_value(removed.key());
                if let Ok(found) = large
                    .index
                    .find_entry(hash, |&(_, candidate)| candidate == position)
                {
                    found.remove();
                }
                if position < large.entries.len() {
                    for (_, candidate) in large.index.iter_mut() {
                        if *candidate > position {
                            *candidate -= 1;
                        }
                    }
                }
                removed
            }
        };
        if self.len() <= INDEX_THRESHOLD {
            self.rebuild_index();
        }
        removed
    }

    /// Removes the entry whose key equals `key` and keeps the order of the rest.
    pub fn shift_remove(&mut self, key: &Value) -> Option<T> {
        self.position(key)
            .map(|position| self.shift_remove_at(position))
    }

    pub fn clear(&mut self) {
        match &mut self.storage {
            Storage::Small(entries) => match Arc::get_mut(entries) {
                Some(entries) => entries.clear(),
                None => *entries = Arc::new(Vec::new()),
            },
            Storage::Large(large) => {
                let entries = match Arc::get_mut(large) {
                    Some(large) => {
                        let mut entries = std::mem::take(&mut large.entries);
                        entries.clear();
                        entries
                    }
                    None => Vec::new(),
                };
                self.storage = Storage::Small(Arc::new(entries));
            }
        }
    }

    /// Removes and returns the last entry.
    pub fn pop(&mut self) -> Option<T> {
        let position = self.len().checked_sub(1)?;
        Some(self.shift_remove_at(position))
    }

    /// Picks the storage that fits the current length and rebuilds the
    /// index from the entries.
    fn rebuild_index(&mut self) {
        let entries = match &mut self.storage {
            Storage::Small(entries) => std::mem::take(Arc::make_mut(entries)),
            Storage::Large(large) => std::mem::take(&mut Arc::make_mut(large).entries),
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
        self.storage = Storage::Large(Arc::new(Large { entries, index }));
    }
}

impl DictEntries {
    pub fn value_of(&self, key: &Value) -> Option<&Value> {
        self.find(key).map(|(_, value)| value)
    }

    /// [`Self::value_of`] a `str` key holding `text`.
    pub fn value_of_str(&self, text: &str) -> Option<&Value> {
        self.position_str(text)
            .map(|position| &self.entries()[position].1)
    }

    /// [`Self::value_of`] an integer key equal to `integer`.
    pub fn value_of_i64(&self, integer: i64) -> Option<&Value> {
        self.position_i64(integer)
            .map(|position| &self.entries()[position].1)
    }

    /// The value at `position`, for writing.
    pub fn value_at_mut(&mut self, position: usize) -> &mut Value {
        &mut self.entries_mut()[position].1
    }

    /// Stores `value` under `key`. An equal key keeps its slot and its
    /// original key, and the replaced value is returned.
    pub fn insert(&mut self, key: Value, value: Value) -> Option<Value> {
        match self.probe(|| hash_value(&key), |candidate| *candidate == key) {
            (Some(position), _) => Some(std::mem::replace(
                &mut self.entries_mut()[position].1,
                value,
            )),
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
