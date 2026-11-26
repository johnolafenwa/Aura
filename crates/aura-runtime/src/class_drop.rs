//! Shares one owner obligation among the runtime's transient copies. The
//! execution backend calls Aura at an instruction boundary after the last
//! copy releases its fields. A Rust destructor only transfers the fields.

use super::{current_lightweight_task_id, InstanceFields, InstanceValue, RuntimeName, Value};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub enum DropMethod {
    Interpreted(String),
    Native(
        unsafe extern "C-unwind" fn(*const i64, usize) -> *mut crate::native_runtime::OpaqueValue,
    ),
}

#[derive(Clone)]
pub struct HookFields {
    owner: Option<Arc<Owner>>,
    pub(super) fields: Box<InstanceFields>,
}

struct Owner {
    // Value destruction is linked into every native executable. Arming an
    // owner installs this callback, so only a hook user links the queue.
    release: fn(&mut HookFields),
    class_name: RuntimeName,
    method: DropMethod,
    rank: AtomicU64,
}

struct ReleasedInstance {
    task: Option<u64>,
    rank: u64,
    method: DropMethod,
    receiver: InstanceValue,
}

thread_local! {
    static RELEASED: RefCell<VecDeque<ReleasedInstance>> =
        const { RefCell::new(VecDeque::new()) };
}

impl Drop for HookFields {
    fn drop(&mut self) {
        if let Some(owner) = &self.owner {
            (owner.release)(self);
        }
    }
}

impl HookFields {
    fn release(&mut self) {
        let Some(Owner {
            class_name,
            method,
            rank,
            ..
        }) = self.owner.take().and_then(Arc::into_inner)
        else {
            return;
        };
        let fields = std::mem::take(self.fields.as_mut());
        let receiver = InstanceValue { class_name, fields };
        RELEASED.with_borrow_mut(|queue| {
            queue.push_back(ReleasedInstance {
                task: current_lightweight_task_id(),
                rank: rank.load(Ordering::Relaxed),
                method,
                receiver,
            });
        });
    }
}

impl InstanceValue {
    pub fn arm_drop(&mut self, method: DropMethod) {
        let fields = Box::new(std::mem::take(&mut self.fields));
        self.fields = InstanceFields::Hooked(HookFields {
            owner: Some(Arc::new(Owner {
                release: HookFields::release,
                class_name: self.class_name.clone(),
                method,
                rank: AtomicU64::new(NEXT_OWNER.fetch_add(1, Ordering::Relaxed)),
            })),
            fields,
        });
    }
}

pub fn next_released_instance() -> Option<(DropMethod, InstanceValue)> {
    RELEASED.with_borrow_mut(|queue| {
        if queue.is_empty() {
            return None;
        }
        let task = current_lightweight_task_id();
        let index = queue.iter().position(|entry| entry.task == task)?;
        queue
            .remove(index)
            .map(|entry| (entry.method, entry.receiver))
    })
}

/// Detaches the siblings in the current release batch. A nested call can
/// drain the fields it releases without running its caller's next sibling.
pub fn released_instances() -> Vec<(DropMethod, InstanceValue)> {
    std::iter::from_fn(next_released_instance).collect()
}

/// Unwinding can release the abandoned native frame's boxes in storage
/// order. Restore owner order before its pending hooks enter Aura.
pub fn reverse_owner_order() {
    RELEASED.with_borrow_mut(|queue| {
        queue
            .make_contiguous()
            .sort_by_key(|entry| std::cmp::Reverse(entry.rank));
    });
}

/// Assigning a new value to an older local does not change when that
/// local leaves its scope. Nested values inherit the rank of their owner.
pub fn stamp_owner(value: &Value, rank: u64) -> u64 {
    let rank = if rank == 0 {
        NEXT_OWNER.fetch_add(1, Ordering::Relaxed)
    } else {
        rank
    };
    match value {
        Value::Instance(instance) => match &instance.fields {
            InstanceFields::Hooked(fields) => {
                if let Some(owner) = &fields.owner {
                    owner.rank.store(rank, Ordering::Relaxed);
                }
            }
            InstanceFields::Recovered(_) => {}
            fields => {
                for field in fields.values() {
                    stamp_owner(&field, rank);
                }
            }
        },
        Value::Vec(list) => {
            for field in &list.elements {
                stamp_owner(&field, rank);
            }
        }
        Value::Set(set) => {
            for element in set.elements.iter() {
                stamp_owner(element, rank);
            }
        }
        Value::Map(map) => {
            for (key, field) in map.entries.iter() {
                stamp_owner(key, rank);
                stamp_owner(field, rank);
            }
        }
        Value::EnumVariant(variant) => {
            for field in variant.payloads.iter() {
                stamp_owner(&field, rank);
            }
        }
        Value::Tuple(tuple) => {
            for field in &tuple.elements {
                stamp_owner(field, rank);
            }
        }
        Value::Union(union) => {
            stamp_owner(&union.payload, rank);
        }
        Value::TraitObject(object) => {
            stamp_owner(&object.value, rank);
        }
        _ => {}
    }
    rank
}

pub fn holds_owner(value: &Value) -> bool {
    match value {
        Value::Instance(instance) => {
            matches!(instance.fields, InstanceFields::Hooked(_))
                || (!matches!(instance.fields, InstanceFields::Recovered(_))
                    && instance.fields.values().any(|field| holds_owner(&field)))
        }
        Value::Vec(list) => list.elements.iter().any(|element| holds_owner(&element)),
        Value::Set(set) => set.elements.iter().any(holds_owner),
        Value::Map(map) => map
            .entries
            .iter()
            .any(|(key, value)| holds_owner(key) || holds_owner(value)),
        Value::Tuple(tuple) => tuple.elements.iter().any(holds_owner),
        Value::Union(union) => holds_owner(&union.payload),
        Value::EnumVariant(variant) => {
            !matches!(variant.payloads, super::VariantPayloads::Json(_))
                && variant.payloads.iter().any(|v| holds_owner(&v))
        }
        Value::TraitObject(object) => holds_owner(&object.value),
        _ => false,
    }
}

/// The copy of an owned collection is a new source owner of each element.
/// Only this explicit language operation forks the runtime obligations.
pub fn renew_owners(value: &mut Value) {
    if !holds_owner(value) {
        return;
    }
    match value {
        Value::Instance(instance) => {
            if let InstanceFields::Hooked(fields) = &mut instance.fields {
                if let Some(owner) = &mut fields.owner {
                    *owner = Arc::new(Owner {
                        release: owner.release,
                        class_name: owner.class_name.clone(),
                        method: owner.method.clone(),
                        rank: AtomicU64::new(NEXT_OWNER.fetch_add(1, Ordering::Relaxed)),
                    });
                }
            }
            if !matches!(instance.fields, InstanceFields::Recovered(_)) {
                let names: Vec<_> = instance.fields.keys().map(str::to_owned).collect();
                for name in names {
                    let mut field = instance
                        .fields
                        .get(&name)
                        .expect("a declared field")
                        .into_owned();
                    if holds_owner(&field) {
                        renew_owners(&mut field);
                        instance
                            .fields
                            .insert(&name, field)
                            .expect("the same field type");
                    }
                }
            }
        }
        Value::Vec(list) => {
            for index in 0..list.elements.len() {
                if let Some(mut element) = list.elements.get_mut(index) {
                    renew_owners(&mut element);
                }
            }
        }
        Value::Set(set) => {
            let mut contents = std::mem::take(&mut set.elements).into_vec();
            for element in &mut contents {
                renew_owners(element);
            }
            set.elements = contents.into();
        }
        Value::Map(map) => {
            let mut contents = std::mem::take(&mut map.entries).into_vec();
            for (key, field) in &mut contents {
                renew_owners(key);
                renew_owners(field);
            }
            map.entries = contents.into();
        }
        Value::EnumVariant(variant) => {
            use super::VariantPayloads;
            match &mut variant.payloads {
                VariantPayloads::Values(payloads) => {
                    for payload in payloads {
                        renew_owners(payload);
                    }
                }
                VariantPayloads::Typed(typed) => {
                    let mut values: Vec<_> = (0..typed.len())
                        .map(|i| typed.get(i).unwrap().into_owned())
                        .collect();
                    for payload in &mut values {
                        renew_owners(payload);
                    }
                    *typed = super::TypedPayloads::from_values(
                        typed.layout(),
                        typed.variant_name(),
                        values,
                    )
                    .expect("unchanged payload types");
                }
                VariantPayloads::Json(_) => {}
            }
        }
        Value::Tuple(tuple) => {
            for field in &mut tuple.elements {
                renew_owners(field);
            }
        }
        Value::Union(union) => renew_owners(&mut union.payload),
        Value::TraitObject(object) => renew_owners(&mut object.value),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_value::Value;

    #[test]
    fn nested_owner_copies_keep_independent_obligations_and_lexical_order() {
        use crate::runtime_value::{
            EnumVariantValue, MapValue, SetValue, TraitObjectValue, TupleValue, UnionValue,
            VecValue,
        };
        use aura_core::types::Type;
        let ty = Type::named("Ticket");
        let mut ticket = InstanceValue {
            class_name: "Ticket".into(),
            fields: Default::default(),
        };
        ticket.arm_drop(DropMethod::Interpreted("ticket".into()));
        let erased = Value::TraitObject(Box::new(TraitObjectValue {
            held: Arc::new(ty.clone()),
            value: Value::Instance(ticket),
        }));
        let choice = Value::Union(Box::new(UnionValue {
            union_type: Arc::new(ty.clone()),
            member_index: 0,
            payload: erased,
        }));
        let tagged = Value::EnumVariant(EnumVariantValue {
            enum_name: "Choice".into(),
            variant_name: "Item".into(),
            payloads: vec![choice].into(),
        });
        let tuple = Value::Tuple(TupleValue {
            element_types: vec![ty.clone()],
            elements: vec![tagged],
        });
        let list = Value::Vec(VecValue::new(ty.clone(), vec![tuple].into()));
        let set = Value::Set(SetValue {
            element_type: ty.clone().into(),
            elements: vec![list].into(),
        });
        let map = Value::Map(MapValue {
            key_type: Type::named("bool").into(),
            value_type: ty.into(),
            entries: vec![(Value::Bool(true), set)].into(),
        });
        let mut envelope = InstanceValue {
            class_name: "Envelope".into(),
            fields: Default::default(),
        };
        envelope.fields.insert("contents", map).unwrap();
        let original = Value::Instance(envelope);
        let mut copied = original.clone();
        assert!(holds_owner(&original));
        renew_owners(&mut copied);
        stamp_owner(&original, 100);
        stamp_owner(&copied, 200);
        drop(original);
        assert_eq!(RELEASED.with_borrow(|q| q.len()), 1);
        drop(copied);
        reverse_owner_order();
        assert_eq!(
            RELEASED.with_borrow(|q| q.iter().map(|entry| entry.rank).collect::<Vec<_>>()),
            [200, 100]
        );
        assert_eq!(released_instances().len(), 2);
    }

    #[test]
    fn transient_copies_share_the_obligation_and_the_hook_gets_updated_fields() {
        let mut value = InstanceValue {
            class_name: "Resource".into(),
            fields: InstanceFields::default(),
        };
        value.arm_drop(DropMethod::Interpreted("release".into()));
        let mut updated = value.clone();
        drop(value);
        assert!(next_released_instance().is_none());
        updated.fields.insert("done", Value::Bool(true)).unwrap();
        drop(updated);
        let (DropMethod::Interpreted(method), receiver) = next_released_instance().unwrap() else {
            panic!("expected interpreted hook");
        };
        assert_eq!(method, "release");
        assert_eq!(
            receiver.fields.get("done").as_deref(),
            Some(&Value::Bool(true))
        );
        drop(receiver);
        assert!(next_released_instance().is_none());
    }
}
