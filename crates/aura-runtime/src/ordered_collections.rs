//! Storage for the standard collections. A value owns its store and shares a
//! snapshot until the first write. The source wrappers retain user-defined
//! comparison methods for types that have no built-in order.

use super::*;
use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BinaryHeap, VecDeque};
use std::ops::Bound::{Excluded, Included, Unbounded};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Store {
    data: Arc<Data>,
}

#[derive(Clone, Debug)]
enum Data {
    Deque(VecDeque<Value>),
    Min(BinaryHeap<Reverse<Key>>),
    Max(BinaryHeap<Key>),
    Map(BTreeMap<Key, Value>),
    I64Deque(VecDeque<i64>),
    I64Min(BinaryHeap<Reverse<i64>>),
    I64Max(BinaryHeap<i64>),
    I64Map(I64Map),
}

#[derive(Clone, Debug, PartialEq)]
struct I64Map {
    entries: BTreeMap<i64, i64>,
    unit: bool,
}

impl I64Map {
    fn value(&self, word: i64) -> Value {
        if self.unit {
            Value::Unit
        } else {
            i64_value(word)
        }
    }

    fn entry(&self, (&key, &word): (&i64, &i64)) -> Value {
        pair(i64_value(key), self.value(word))
    }
}

impl Data {
    // Public collection values can come from either lowering. Equality
    // compares their contents even when the two storage layouts differ.
    fn expanded(&self) -> Option<Self> {
        let key = |n| Key::Integer(IntegerValue::from_i64(n));
        match self {
            Self::I64Deque(items) => {
                Some(Self::Deque(items.iter().copied().map(i64_value).collect()))
            }
            Self::I64Min(items) => Some(Self::Min(BinaryHeap::from(
                items
                    .as_slice()
                    .iter()
                    .map(|v| Reverse(key(v.0)))
                    .collect::<Vec<_>>(),
            ))),
            Self::I64Max(items) => Some(Self::Max(BinaryHeap::from(
                items.as_slice().iter().map(|v| key(*v)).collect::<Vec<_>>(),
            ))),
            Self::I64Map(map) => Some(Self::Map(
                map.entries
                    .iter()
                    .map(|(&k, &v)| (key(k), map.value(v)))
                    .collect(),
            )),
            _ => None,
        }
    }
}

impl PartialEq for Data {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Deque(a), Self::Deque(b)) => a == b,
            (Self::Min(a), Self::Min(b)) => a.as_slice() == b.as_slice(),
            (Self::Max(a), Self::Max(b)) => a.as_slice() == b.as_slice(),
            (Self::Map(a), Self::Map(b)) => a == b,
            (Self::I64Deque(a), Self::I64Deque(b)) => a == b,
            (Self::I64Min(a), Self::I64Min(b)) => a.as_slice() == b.as_slice(),
            (Self::I64Max(a), Self::I64Max(b)) => a.as_slice() == b.as_slice(),
            (Self::I64Map(a), Self::I64Map(b)) => a == b,
            (a, b) => {
                if let Some(a) = a.expanded() {
                    a.eq(b)
                } else {
                    b.expanded().is_some_and(|b| a.eq(&b))
                }
            }
        }
    }
}

impl Default for Data {
    fn default() -> Self {
        Self::Map(BTreeMap::new())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Key {
    Integer(IntegerValue),
    Float(OrderedFloat),
    Text(Text),
}

#[derive(Clone, Copy, Debug)]
struct OrderedFloat(f64);

impl PartialEq for OrderedFloat {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for OrderedFloat {}
impl PartialOrd for OrderedFloat {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OrderedFloat {
    fn cmp(&self, other: &Self) -> Ordering {
        crate::list_sort::float_sort_order(self.0, other.0)
    }
}

impl Key {
    fn extract(value: Value) -> Result<Self> {
        match value {
            Value::Int(value) => Ok(Self::Integer(value)),
            Value::Float(value) => Ok(Self::Float(OrderedFloat(value))),
            Value::String(value) => Ok(Self::Text(value)),
            _ => Err(invalid()),
        }
    }

    fn into_value(self) -> Value {
        match self {
            Self::Integer(v) => Value::Int(v),
            Self::Float(v) => Value::Float(v.0),
            Self::Text(v) => Value::String(v),
        }
    }
}

fn invalid() -> Diagnostic {
    Diagnostic::coded("AU4001", "invalid internal collection operation")
}

fn arguments<const N: usize>(args: Vec<Value>) -> Result<[Value; N]> {
    let actual = args.len();
    args.try_into().map_err(|_| {
        Diagnostic::coded(
            "AU4001",
            format!("collection operation expects {N} arguments, found {actual}"),
        )
    })
}

fn store(value: Value) -> Result<Store> {
    match value {
        Value::CollectionStore(value) => Ok(value),
        _ => Err(invalid()),
    }
}

fn boolean(value: Value) -> Result<bool> {
    match value {
        Value::Bool(value) => Ok(value),
        _ => Err(invalid()),
    }
}

fn integer(value: Value) -> Result<i64> {
    match value {
        Value::Int(value) => value
            .as_i128()
            .and_then(|v| i64::try_from(v).ok())
            .ok_or_else(invalid),
        _ => Err(invalid()),
    }
}

fn pair(left: Value, right: Value) -> Value {
    Value::Tuple(TupleValue {
        element_types: vec![
            crate::native_runtime::inferred_collection_type(&left),
            crate::native_runtime::inferred_collection_type(&right),
        ],
        elements: vec![left, right],
    })
}

fn list(values: Vec<Value>) -> Value {
    let ty = values
        .first()
        .map(crate::native_runtime::inferred_collection_type)
        .unwrap_or_else(|| Type::named("Unknown"));
    Value::Vec(VecValue::new(ty, values.into()))
}

fn entry(key: Key, value: Value) -> Value {
    pair(key.into_value(), value)
}

fn lookup(value: Option<Value>) -> Value {
    value.map_or_else(lookup_missing, lookup_found)
}

pub fn new(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [kind] = arguments(args)?;
    let data = match integer(kind)? {
        0 => Data::Deque(VecDeque::new()),
        1 => Data::Min(BinaryHeap::new()),
        2 => Data::Max(BinaryHeap::new()),
        3 => Data::Map(BTreeMap::new()),
        _ => return Err(invalid()),
    };
    Ok(Value::CollectionStore(Store {
        data: Arc::new(data),
    }))
}

pub fn supported(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [value] = arguments(args)?;
    Ok(Value::Bool(matches!(
        value,
        Value::Int(_) | Value::Float(_) | Value::String(_)
    )))
}

pub fn len(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state] = arguments(args)?;
    let state = store(state)?;
    let count = match state.data.as_ref() {
        Data::Deque(items) => items.len(),
        Data::Min(items) => items.len(),
        Data::Max(items) => items.len(),
        Data::Map(items) => items.len(),
        Data::I64Deque(items) => items.len(),
        Data::I64Min(items) => items.len(),
        Data::I64Max(items) => items.len(),
        Data::I64Map(map) => map.entries.len(),
    };
    Ok(Value::Int(IntegerValue::from_i64(count as i64)))
}

pub fn deque_push(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state, value, front] = arguments(args)?;
    let mut state = store(state)?;
    let data = Arc::make_mut(&mut state.data);
    if let Data::I64Deque(items) = data {
        let value = integer(value)?;
        if boolean(front)? {
            items.push_front(value);
        } else {
            items.push_back(value);
        }
        return Ok(Value::CollectionStore(state));
    }
    let Data::Deque(items) = data else {
        return Err(invalid());
    };
    if boolean(front)? {
        items.push_front(value);
    } else {
        items.push_back(value);
    }
    Ok(Value::CollectionStore(state))
}

pub fn deque_pop(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state, front] = arguments(args)?;
    let mut state = store(state)?;
    let data = Arc::make_mut(&mut state.data);
    if let Data::I64Deque(items) = data {
        let value = if boolean(front)? {
            items.pop_front()
        } else {
            items.pop_back()
        };
        let value =
            value.ok_or_else(|| Diagnostic::coded("AU4003", "cannot pop from an empty deque"))?;
        return Ok(pair(Value::CollectionStore(state), i64_value(value)));
    }
    let Data::Deque(items) = data else {
        return Err(invalid());
    };
    let value = if boolean(front)? {
        items.pop_front()
    } else {
        items.pop_back()
    }
    .ok_or_else(|| Diagnostic::coded("AU4003", "cannot pop from an empty deque"))?;
    Ok(pair(Value::CollectionStore(state), value))
}

pub fn deque_get(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state, index] = arguments(args)?;
    let state = store(state)?;
    if let Data::I64Deque(items) = state.data.as_ref() {
        let raw = integer(index)?;
        let index = if raw < 0 {
            raw + items.len() as i64
        } else {
            raw
        };
        return Ok(lookup(
            usize::try_from(index)
                .ok()
                .and_then(|i| items.get(i))
                .copied()
                .map(i64_value),
        ));
    }
    let Data::Deque(items) = state.data.as_ref() else {
        return Err(invalid());
    };
    let raw = integer(index)?;
    let index = if raw < 0 {
        raw + items.len() as i64
    } else {
        raw
    };
    let value = usize::try_from(index)
        .ok()
        .and_then(|i| items.get(i))
        .cloned();
    Ok(lookup(value))
}

pub fn heap_from(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [items] = arguments(args)?;
    let Value::Vec(items) = items else {
        return Err(invalid());
    };
    let keys = items
        .elements
        .into_iter()
        .map(|item| Key::extract(item).map(Reverse))
        .collect::<Result<Vec<_>>>()?;
    let heap = BinaryHeap::from(keys);
    Ok(Value::CollectionStore(Store {
        data: Arc::new(Data::Min(heap)),
    }))
}

pub fn heap_push(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state, value] = arguments(args)?;
    let key = Key::extract(value)?;
    let mut state = store(state)?;
    match Arc::make_mut(&mut state.data) {
        Data::I64Min(items) => items.push(Reverse(integer(key.into_value())?)),
        Data::I64Max(items) => items.push(integer(key.into_value())?),
        Data::Min(items) => items.push(Reverse(key)),
        Data::Max(items) => items.push(key),
        _ => return Err(invalid()),
    }
    Ok(Value::CollectionStore(state))
}

pub fn heap_take(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state] = arguments(args)?;
    let mut state = store(state)?;
    let key = match Arc::make_mut(&mut state.data) {
        Data::I64Min(items) => items
            .pop()
            .map(|v| Key::Integer(IntegerValue::from_i64(v.0))),
        Data::I64Max(items) => items.pop().map(|v| Key::Integer(IntegerValue::from_i64(v))),
        Data::Min(items) => items.pop().map(|it| it.0),
        Data::Max(items) => items.pop(),
        _ => return Err(invalid()),
    };
    let key = key.ok_or_else(|| Diagnostic::coded("AU4003", "cannot pop from an empty heap"))?;
    Ok(pair(Value::CollectionStore(state), key.into_value()))
}

pub fn heap_peek(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state] = arguments(args)?;
    let state = store(state)?;
    let key = match state.data.as_ref() {
        Data::I64Min(items) => return Ok(lookup(items.peek().map(|v| i64_value(v.0)))),
        Data::I64Max(items) => return Ok(lookup(items.peek().copied().map(i64_value))),
        Data::Min(items) => items.peek().map(|it| &it.0),
        Data::Max(items) => items.peek(),
        _ => return Err(invalid()),
    };
    Ok(lookup(key.cloned().map(Key::into_value)))
}

pub fn map_put(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state, key, value] = arguments(args)?;
    let mut state = store(state)?;
    let data = Arc::make_mut(&mut state.data);
    if let Data::I64Map(map) = data {
        let value = if map.unit { 0 } else { integer(value)? };
        map.entries.insert(integer(key)?, value);
        return Ok(Value::CollectionStore(state));
    }
    let Data::Map(entries) = data else {
        return Err(invalid());
    };
    entries.insert(Key::extract(key)?, value);
    Ok(Value::CollectionStore(state))
}

pub fn map_get(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state, key] = arguments(args)?;
    let state = store(state)?;
    if let Data::I64Map(map) = state.data.as_ref() {
        return Ok(lookup(
            map.entries.get(&integer(key)?).map(|&v| map.value(v)),
        ));
    }
    let Data::Map(entries) = state.data.as_ref() else {
        return Err(invalid());
    };
    Ok(lookup(entries.get(&Key::extract(key)?).cloned()))
}

pub fn map_has(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state, key] = arguments(args)?;
    let state = store(state)?;
    if let Data::I64Map(map) = state.data.as_ref() {
        return Ok(Value::Bool(map.entries.contains_key(&integer(key)?)));
    }
    let Data::Map(entries) = state.data.as_ref() else {
        return Err(invalid());
    };
    Ok(Value::Bool(entries.contains_key(&Key::extract(key)?)))
}

pub fn map_take(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state, key] = arguments(args)?;
    let mut state = store(state)?;
    let data = Arc::make_mut(&mut state.data);
    if let Data::I64Map(map) = data {
        let removed = map.entries.remove(&integer(key)?).map(|v| map.value(v));
        return Ok(pair(Value::CollectionStore(state), lookup(removed)));
    }
    let Data::Map(entries) = data else {
        return Err(invalid());
    };
    let value = entries.remove(&Key::extract(key)?);
    Ok(pair(Value::CollectionStore(state), lookup(value)))
}

pub fn map_edge(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state, last] = arguments(args)?;
    let state = store(state)?;
    if let Data::I64Map(map) = state.data.as_ref() {
        let entry = if boolean(last)? {
            map.entries.last_key_value()
        } else {
            map.entries.first_key_value()
        };
        return Ok(lookup(entry.map(|pair| map.entry(pair))));
    }
    let Data::Map(entries) = state.data.as_ref() else {
        return Err(invalid());
    };
    let found = if boolean(last)? {
        entries.last_key_value()
    } else {
        entries.first_key_value()
    };
    Ok(lookup(found.map(|(k, v)| entry(k.clone(), v.clone()))))
}

pub fn map_bound(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state, key, mode] = arguments(args)?;
    let state = store(state)?;
    if let Data::I64Map(map) = state.data.as_ref() {
        let key = integer(key)?;
        let entry = match integer(mode)? {
            0 => map.entries.range(..=key).next_back(),
            1 => map.entries.range(key..).next(),
            2 => map.entries.get_key_value(&key),
            _ => return Err(invalid()),
        };
        return Ok(lookup(entry.map(|pair| map.entry(pair))));
    }
    let key = Key::extract(key)?;
    let Data::Map(entries) = state.data.as_ref() else {
        return Err(invalid());
    };
    let found = match integer(mode)? {
        0 => entries.range((Unbounded, Included(key))).next_back(),
        1 => entries.range((Included(key), Unbounded)).next(),
        2 => entries.get_key_value(&key),
        _ => return Err(invalid()),
    };
    Ok(lookup(found.map(|(k, v)| entry(k.clone(), v.clone()))))
}

pub fn map_range(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state, low, high] = arguments(args)?;
    let state = store(state)?;
    if let Data::I64Map(map) = state.data.as_ref() {
        let low = integer(low)?;
        let high = integer(high)?;
        let values = if low < high {
            map.entries
                .range(low..high)
                .map(|pair| map.entry(pair))
                .collect()
        } else {
            Vec::new()
        };
        return Ok(list(values));
    }
    let low = Key::extract(low)?;
    let high = Key::extract(high)?;
    let Data::Map(entries) = state.data.as_ref() else {
        return Err(invalid());
    };
    let values = if low < high {
        entries
            .range((Included(low), Excluded(high)))
            .map(|(k, v)| entry(k.clone(), v.clone()))
            .collect()
    } else {
        Vec::new()
    };
    Ok(list(values))
}

pub fn map_drain(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state] = arguments(args)?;
    let data = Arc::unwrap_or_clone(store(state)?.data);
    if let Data::I64Map(map) = data {
        let values = map
            .entries
            .into_iter()
            .map(|(key, value)| {
                pair(
                    i64_value(key),
                    if map.unit {
                        Value::Unit
                    } else {
                        i64_value(value)
                    },
                )
            })
            .collect();
        return Ok(list(values));
    }
    let Data::Map(entries) = data else {
        return Err(invalid());
    };
    Ok(list(
        entries.into_iter().map(|(k, v)| entry(k, v)).collect(),
    ))
}

/// Direct scalar calls use the same store as the standard module. This table
/// defines the compiler/runtime operation codes for concrete int64 receivers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i64)]
pub enum I64Op {
    PushFront,
    PushBack,
    PopFront,
    PopBack,
    HeapPush,
    HeapPop,
    MapSet,
    MapGet,
    MapRemove,
    MapContains,
    SetAdd,
    SetRemove,
    SetContains,
}

impl I64Op {
    pub fn from_code(code: i64) -> Option<Self> {
        use I64Op::*;
        [
            PushFront,
            PushBack,
            PopFront,
            PopBack,
            HeapPush,
            HeapPop,
            MapSet,
            MapGet,
            MapRemove,
            MapContains,
            SetAdd,
            SetRemove,
            SetContains,
        ]
        .get(usize::try_from(code).ok()?)
        .copied()
    }

    pub fn for_method(name: &str) -> Option<Self> {
        use I64Op::*;
        Some(match name.split('[').next()? {
            "collections::Deque.push_front" => PushFront,
            "collections::Deque.push_back" => PushBack,
            "collections::Deque.pop_front" => PopFront,
            "collections::Deque.pop_back" => PopBack,
            "collections::Heap.push" => HeapPush,
            "collections::Heap.pop" => HeapPop,
            "collections::SortedMap.set" => MapSet,
            "collections::SortedMap.get" => MapGet,
            "collections::SortedMap.remove" => MapRemove,
            "collections::SortedMap.contains" => MapContains,
            "collections::SortedSet.add" => SetAdd,
            "collections::SortedSet.remove" => SetRemove,
            "collections::SortedSet.contains" => SetContains,
            _ => return None,
        })
    }

    /// The source position passed in the argument word of a zero-argument
    /// pop. These are positions in the compiler-supplied collections module.
    pub fn pop_site(span: Span) -> i64 {
        ((span.line as u64) << 32 | span.column as u64) as i64
    }

    pub(crate) fn pop_frame(self, site: i64) -> Option<(&'static str, Span)> {
        if site == 0 {
            return None;
        }
        let method = match self {
            Self::PopFront => "collections::Deque.pop_front",
            Self::PopBack => "collections::Deque.pop_back",
            Self::HeapPop => "collections::Heap.pop",
            _ => return None,
        };
        let bits = site as u64;
        Some((
            method,
            Span::new((bits >> 32) as usize, bits as u32 as usize),
        ))
    }

    pub fn is_word_sequence(self) -> bool {
        matches!(
            self,
            Self::PushFront
                | Self::PushBack
                | Self::PopFront
                | Self::PopBack
                | Self::HeapPush
                | Self::HeapPop
        )
    }

    pub fn is_set(self) -> bool {
        matches!(self, Self::SetAdd | Self::SetContains | Self::SetRemove)
    }

    pub fn returns_lookup(self) -> bool {
        matches!(self, Self::MapGet | Self::MapRemove)
    }

    pub fn returns_bool(self) -> bool {
        matches!(
            self,
            Self::MapContains | Self::SetContains | Self::SetRemove
        )
    }

    pub fn returns_number(self) -> bool {
        matches!(self, Self::PopFront | Self::PopBack | Self::HeapPop)
    }

    pub(crate) fn is_read(self) -> bool {
        matches!(self, Self::MapGet | Self::MapContains | Self::SetContains)
    }

    pub(crate) fn new_store(self, descending: bool) -> Store {
        use I64Op::*;
        let data = match self {
            PushFront | PushBack | PopFront | PopBack => Data::I64Deque(VecDeque::new()),
            HeapPush | HeapPop if descending => Data::I64Max(BinaryHeap::new()),
            HeapPush | HeapPop => Data::I64Min(BinaryHeap::new()),
            _ => Data::I64Map(I64Map {
                entries: BTreeMap::new(),
                unit: self.is_set(),
            }),
        };
        Store {
            data: Arc::new(data),
        }
    }
}

fn i64_value(word: i64) -> Value {
    Value::Int(IntegerValue::from_i64(word))
}

impl Store {
    pub(crate) fn local_words(&mut self) -> Option<*mut std::ffi::c_void> {
        let data = Arc::make_mut(&mut self.data);
        matches!(data, Data::I64Deque(_) | Data::I64Min(_) | Data::I64Max(_))
            .then(|| std::ptr::from_mut(data).cast())
    }

    /// The caller guarantees that `address` points into the live, exclusive
    /// store returned by `local_words`. A container reallocation changes
    /// its element buffer, not the address of the container itself.
    #[inline]
    pub(crate) unsafe fn local_step(
        address: *mut std::ffi::c_void,
        code: i64,
        word: i64,
    ) -> Option<i64> {
        // SAFETY: the contract above grants unique access for this call.
        let data = unsafe { address.cast::<Data>().as_mut() }?;
        match (I64Op::from_code(code)?, data) {
            (I64Op::PushFront, Data::I64Deque(items)) => items.push_front(word),
            (I64Op::PushBack, Data::I64Deque(items)) => items.push_back(word),
            (I64Op::PopFront, Data::I64Deque(items)) => return items.pop_front(),
            (I64Op::PopBack, Data::I64Deque(items)) => return items.pop_back(),
            (I64Op::HeapPush, Data::I64Min(items)) => items.push(Reverse(word)),
            (I64Op::HeapPush, Data::I64Max(items)) => items.push(word),
            (I64Op::HeapPop, Data::I64Min(items)) => return items.pop().map(|v| v.0),
            (I64Op::HeapPop, Data::I64Max(items)) => return items.pop(),
            _ => return None,
        }
        Some(0)
    }

    pub(crate) fn read_i64(&self, op: I64Op, key: i64) -> Result<Option<i64>> {
        if let Data::I64Map(map) = self.data.as_ref() {
            return match op {
                I64Op::MapGet => Ok(map.entries.get(&key).copied()),
                I64Op::MapContains | I64Op::SetContains => {
                    Ok(Some(i64::from(map.entries.contains_key(&key))))
                }
                _ => Err(invalid()),
            };
        }
        let Data::Map(entries) = self.data.as_ref() else {
            return Err(invalid());
        };
        let key = Key::Integer(IntegerValue::from_i64(key));
        match op {
            I64Op::MapGet => entries.get(&key).map(|v| integer(v.clone())).transpose(),
            I64Op::MapContains | I64Op::SetContains => {
                Ok(Some(i64::from(entries.contains_key(&key))))
            }
            _ => Err(invalid()),
        }
    }

    pub(crate) fn write_i64(&mut self, op: I64Op, first: i64, second: i64) -> Result<Option<i64>> {
        use I64Op::*;
        let key = || Key::Integer(IntegerValue::from_i64(first));
        let data = Arc::make_mut(&mut self.data);
        match (op, data) {
            (PushFront, Data::I64Deque(items)) => items.push_front(first),
            (PushBack, Data::I64Deque(items)) => items.push_back(first),
            (PopFront | PopBack, Data::I64Deque(items)) => {
                let value = if op == PopFront {
                    items.pop_front()
                } else {
                    items.pop_back()
                };
                return value
                    .map(Some)
                    .ok_or_else(|| Diagnostic::coded("AU4003", "cannot pop from an empty deque"));
            }
            (HeapPush, Data::I64Min(items)) => items.push(Reverse(first)),
            (HeapPush, Data::I64Max(items)) => items.push(first),
            (HeapPop, Data::I64Min(items)) => {
                return items.pop().map(|i| Some(i.0)).map_or_else(heap_empty, Ok)
            }
            (HeapPop, Data::I64Max(items)) => {
                return items.pop().map(Some).map_or_else(heap_empty, Ok)
            }
            (MapSet | SetAdd, Data::I64Map(map)) => {
                map.entries.insert(first, second);
            }
            (MapRemove, Data::I64Map(map)) => return Ok(map.entries.remove(&first)),
            (SetRemove, Data::I64Map(map)) => {
                return Ok(Some(i64::from(map.entries.remove(&first).is_some())))
            }

            (PushFront, Data::Deque(items)) => items.push_front(i64_value(first)),
            (PushBack, Data::Deque(items)) => items.push_back(i64_value(first)),
            (PopFront | PopBack, Data::Deque(items)) => {
                let value = if op == PopFront {
                    items.pop_front()
                } else {
                    items.pop_back()
                };
                return value
                    .ok_or_else(|| Diagnostic::coded("AU4003", "cannot pop from an empty deque"))
                    .and_then(integer)
                    .map(Some);
            }
            (HeapPush, Data::Min(items)) => items.push(Reverse(key())),
            (HeapPush, Data::Max(items)) => items.push(key()),
            (HeapPop, Data::Min(items)) => {
                return items
                    .pop()
                    .map(|item| item.0)
                    .map_or_else(heap_empty, |k| integer(k.into_value()).map(Some))
            }
            (HeapPop, Data::Max(items)) => {
                return items
                    .pop()
                    .map_or_else(heap_empty, |k| integer(k.into_value()).map(Some))
            }
            (MapSet, Data::Map(entries)) => {
                entries.insert(key(), i64_value(second));
            }
            (SetAdd, Data::Map(entries)) => {
                entries.insert(key(), Value::Unit);
            }
            (MapRemove, Data::Map(entries)) => {
                return entries.remove(&key()).map(integer).transpose()
            }
            (SetRemove, Data::Map(entries)) => {
                return Ok(Some(i64::from(entries.remove(&key()).is_some())))
            }
            _ => return Err(invalid()),
        }
        Ok(None)
    }
}

fn heap_empty() -> Result<Option<i64>> {
    Err(Diagnostic::coded("AU4003", "cannot pop from an empty heap"))
}

#[cfg(test)]
#[path = "ordered_collections_tests.rs"]
mod tests;
