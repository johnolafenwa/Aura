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
use std::ops::Deref;
use std::sync::OnceLock;

use hashbrown::HashTable;

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
    let mut hasher = hash_state().build_hasher();
    let mut pending = vec![value];
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
        std::mem::discriminant(value).hash(&mut hasher);
        match value {
            Value::Int(integer) => integer.hash(&mut hasher),
            Value::Float(float) => {
                // `0.0 == -0.0`, so both hash as zero. NaN never equals
                // anything, so its bits are as good as any hash.
                if *float == 0.0 {
                    0u64.hash(&mut hasher);
                } else {
                    float.to_bits().hash(&mut hasher);
                }
            }
            Value::Bool(flag) => flag.hash(&mut hasher),
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
                pending.extend(vector.elements.iter().rev());
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
/// A small collection is a bare `Vec`. A large one moves its entries and
/// index behind one box, so the type stays the size of a `Vec` and adding
/// it to `MapValue` and `SetValue` does not grow `Value`.
#[derive(Debug)]
pub struct IndexedEntries<T> {
    storage: Storage<T>,
}

#[derive(Debug)]
enum Storage<T> {
    Small(Vec<T>),
    Large(Box<Large<T>>),
}

#[derive(Debug)]
struct Large<T> {
    entries: Vec<T>,
    /// `(hash, position)` for each distinct key.
    index: HashTable<(u64, usize)>,
}

/// The entries of a `dict`: key and value pairs.
pub type MapEntries = IndexedEntries<(Value, Value)>;

/// The members of a `set`.
pub type SetElements = IndexedEntries<Value>;

impl<T> Default for IndexedEntries<T> {
    fn default() -> Self {
        Self {
            storage: Storage::Small(Vec::new()),
        }
    }
}

impl<T: Clone> Clone for IndexedEntries<T> {
    fn clone(&self) -> Self {
        let storage = match &self.storage {
            Storage::Small(entries) => Storage::Small(entries.clone()),
            Storage::Large(large) => Storage::Large(Box::new(Large {
                entries: large.entries.clone(),
                index: large.index.clone(),
            })),
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

impl<T> IntoIterator for IndexedEntries<T> {
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

impl<T: Keyed> From<Vec<T>> for IndexedEntries<T> {
    /// Wraps entries the caller already holds. When two entries have equal
    /// keys, lookups find the first, as a scan of the same entries would.
    fn from(entries: Vec<T>) -> Self {
        let mut collection = Self {
            storage: Storage::Small(entries),
        };
        collection.rebuild_index();
        collection
    }
}

impl<T: Keyed> FromIterator<T> for IndexedEntries<T> {
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

    fn entries_mut(&mut self) -> &mut Vec<T> {
        match &mut self.storage {
            Storage::Small(entries) => entries,
            Storage::Large(large) => &mut large.entries,
        }
    }

    pub fn into_vec(self) -> Vec<T> {
        match self.storage {
            Storage::Small(entries) => entries,
            Storage::Large(large) => large.entries,
        }
    }

    pub fn as_slice(&self) -> &[T] {
        self.entries()
    }

    pub fn capacity(&self) -> usize {
        self.entries().capacity()
    }

    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.entries_mut().try_reserve(additional)
    }
}

impl<T: Keyed> IndexedEntries<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Wraps a clone of `source`'s entries, in the same order, reusing its
    /// index instead of hashing every key again.
    pub fn cloned_from(entries: Vec<T>, source: &Self) -> Self {
        match &source.storage {
            Storage::Large(large) if entries.len() == large.entries.len() => Self {
                storage: Storage::Large(Box::new(Large {
                    entries,
                    index: large.index.clone(),
                })),
            },
            _ => Self::from(entries),
        }
    }

    /// The position of the entry whose key equals `key`.
    pub fn position(&self, key: &Value) -> Option<usize> {
        match &self.storage {
            Storage::Small(entries) => entries.iter().position(|entry| entry.key() == key),
            Storage::Large(large) => {
                let hash = hash_value(key);
                large
                    .index
                    .find(hash, |&(entry_hash, position)| {
                        entry_hash == hash && large.entries[position].key() == key
                    })
                    .map(|&(_, position)| position)
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
        match &mut self.storage {
            Storage::Small(entries) => {
                entries.push(entry);
                if entries.len() > INDEX_THRESHOLD {
                    self.rebuild_index();
                }
            }
            Storage::Large(large) => {
                let position = large.entries.len();
                let hash = hash_value(entry.key());
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
            Storage::Small(entries) => return entries.remove(position),
            Storage::Large(large) => {
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
            Storage::Small(entries) => entries.clear(),
            Storage::Large(large) => {
                let mut entries = std::mem::take(&mut large.entries);
                entries.clear();
                self.storage = Storage::Small(entries);
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
            Storage::Small(entries) => std::mem::take(entries),
            Storage::Large(large) => std::mem::take(&mut large.entries),
        };
        if entries.len() <= INDEX_THRESHOLD {
            self.storage = Storage::Small(entries);
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
        self.storage = Storage::Large(Box::new(Large { entries, index }));
    }
}

impl MapEntries {
    pub fn value_of(&self, key: &Value) -> Option<&Value> {
        self.find(key).map(|(_, value)| value)
    }

    /// The value at `position`, for writing.
    pub fn value_at_mut(&mut self, position: usize) -> &mut Value {
        &mut self.entries_mut()[position].1
    }

    /// Stores `value` under `key`. An equal key keeps its slot and its
    /// original key, and the replaced value is returned.
    pub fn insert(&mut self, key: Value, value: Value) -> Option<Value> {
        match self.position(&key) {
            Some(position) => Some(std::mem::replace(
                &mut self.entries_mut()[position].1,
                value,
            )),
            None => {
                self.push_new((key, value));
                None
            }
        }
    }
}

impl SetElements {
    /// Adds `element` unless an equal one is present. Returns whether it was added.
    pub fn insert(&mut self, element: Value) -> bool {
        if self.contains_key(&element) {
            return false;
        }
        self.push_new(element);
        true
    }
}

#[cfg(test)]
#[path = "collection_index_tests.rs"]
mod tests;
