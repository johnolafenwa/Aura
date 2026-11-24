//! Shares one owner obligation among the runtime's transient copies. The
//! execution backend calls Aura at an instruction boundary after the last
//! copy releases its fields. A Rust destructor only transfers the fields.

use super::{current_lightweight_task_id, InstanceFields, InstanceValue, RuntimeName, Value};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub enum DropMethod {
    Interpreted(String),
    Native(unsafe extern "C-unwind" fn(*const i64, usize) -> *mut crate::native_runtime::OpaqueValue),
}

#[derive(Clone)]
pub struct HookFields {
    owner: Option<Arc<(RuntimeName, DropMethod, u64)>>,
    pub(super) fields: Box<InstanceFields>,
}

thread_local! {
    static RELEASED: RefCell<VecDeque<(Option<u64>, u64, DropMethod, InstanceValue)>> =
        const { RefCell::new(VecDeque::new()) };
}

impl Drop for HookFields {
    fn drop(&mut self) {
        let Some((class_name, method, order)) = self.owner.take().and_then(Arc::into_inner) else {
            return;
        };
        let fields = std::mem::take(self.fields.as_mut());
        let receiver = InstanceValue { class_name, fields };
        RELEASED.with_borrow_mut(|queue| {
            queue.push_back((current_lightweight_task_id(), order, method, receiver));
        });
    }
}

impl InstanceValue {
    pub fn arm_drop(&mut self, method: DropMethod) {
        let fields = Box::new(std::mem::take(&mut self.fields));
        self.fields = InstanceFields::Hooked(HookFields {
            owner: Some(Arc::new((self.class_name.clone(), method, NEXT_OWNER.fetch_add(1, Ordering::Relaxed)))),
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
        let index = queue.iter().position(|(owner, _, _, _)| owner == &task)?;
        queue.remove(index).map(|(_, _, method, receiver)| (method, receiver))
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
        queue.make_contiguous().sort_by_key(|entry| std::cmp::Reverse(entry.1));
    });
}

/// The copy of an owned collection is a new source owner of each element.
/// Only this explicit language operation forks the runtime obligations.
pub fn renew_owners(value: &mut Value) {
    match value {
        Value::Instance(instance) => {
            if let InstanceFields::Hooked(fields) = &mut instance.fields {
                if let Some(owner) = &mut fields.owner {
                    *owner = Arc::new((**owner).clone());
                }
            }
            if !matches!(instance.fields, InstanceFields::Recovered(_)) {
                for field in instance.fields.map_mut().values_mut() { renew_owners(field); }
            }
        }
        Value::Vec(list) => {
            for index in 0..list.elements.len() {
                if let Some(mut element) = list.elements.get_mut(index) { renew_owners(&mut element); }
            }
        }
        Value::Map(map) => {
            for i in 0..map.entries.len() { renew_owners(map.entries.value_at_mut(i)); }
        }
        Value::EnumVariant(variant) => {
            for payload in variant.payloads.values_mut() { renew_owners(payload); }
        }
        Value::Tuple(tuple) => {
            for field in &mut tuple.elements { renew_owners(field); }
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
        assert_eq!(receiver.fields.get("done").as_deref(), Some(&Value::Bool(true)));
        drop(receiver);
        assert!(next_released_instance().is_none());
    }
}
