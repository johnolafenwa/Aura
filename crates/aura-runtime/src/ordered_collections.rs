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
}

impl PartialEq for Data {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Deque(a), Self::Deque(b)) => a == b,
            (Self::Min(a), Self::Min(b)) => a.as_slice() == b.as_slice(),
            (Self::Max(a), Self::Max(b)) => a.as_slice() == b.as_slice(),
            (Self::Map(a), Self::Map(b)) => a == b,
            _ => false,
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
    };
    Ok(Value::Int(IntegerValue::from_i64(count as i64)))
}

pub fn deque_push(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state, value, front] = arguments(args)?;
    let mut state = store(state)?;
    let Data::Deque(items) = Arc::make_mut(&mut state.data) else {
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
    let Data::Deque(items) = Arc::make_mut(&mut state.data) else {
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
        Data::Min(items) => items.peek().map(|it| &it.0),
        Data::Max(items) => items.peek(),
        _ => return Err(invalid()),
    };
    Ok(lookup(key.cloned().map(Key::into_value)))
}

pub fn map_put(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state, key, value] = arguments(args)?;
    let mut state = store(state)?;
    let Data::Map(entries) = Arc::make_mut(&mut state.data) else {
        return Err(invalid());
    };
    entries.insert(Key::extract(key)?, value);
    Ok(Value::CollectionStore(state))
}

pub fn map_get(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state, key] = arguments(args)?;
    let state = store(state)?;
    let Data::Map(entries) = state.data.as_ref() else {
        return Err(invalid());
    };
    Ok(lookup(entries.get(&Key::extract(key)?).cloned()))
}

pub fn map_has(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state, key] = arguments(args)?;
    let state = store(state)?;
    let Data::Map(entries) = state.data.as_ref() else {
        return Err(invalid());
    };
    Ok(Value::Bool(entries.contains_key(&Key::extract(key)?)))
}

pub fn map_take(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state, key] = arguments(args)?;
    let mut state = store(state)?;
    let Data::Map(entries) = Arc::make_mut(&mut state.data) else {
        return Err(invalid());
    };
    let value = entries.remove(&Key::extract(key)?);
    Ok(pair(Value::CollectionStore(state), lookup(value)))
}

pub fn map_edge(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let [state, last] = arguments(args)?;
    let state = store(state)?;
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
    let key = Key::extract(key)?;
    let state = store(state)?;
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
    let low = Key::extract(low)?;
    let high = Key::extract(high)?;
    let state = store(state)?;
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
    let Data::Map(entries) = Arc::unwrap_or_clone(store(state)?.data) else {
        return Err(invalid());
    };
    Ok(list(
        entries.into_iter().map(|(k, v)| entry(k, v)).collect(),
    ))
}

#[cfg(test)]
#[path = "ordered_collections_tests.rs"]
mod tests;
