//! Runs owner hooks after releasing runtime references. The receiver has
//! already been disarmed; releasing its fields can enqueue further hooks.

use super::*;
use aura_runtime::runtime_value::class_drop::{released_instances, DropMethod};

pub(super) fn holds_owner(value: &Value) -> bool {
    match value {
        Value::Instance(instance) => matches!(instance.fields, crate::runtime_value::InstanceFields::Hooked(_))
            || (!matches!(instance.fields, crate::runtime_value::InstanceFields::Recovered(_))
                && instance.fields.values().any(|field| holds_owner(&field))),
        Value::Vec(list) => list.elements.iter().any(|element| holds_owner(&element)),
        Value::Map(map) => map.entries.iter().any(|(key, value)| holds_owner(key) || holds_owner(value)),
        Value::Tuple(tuple) => tuple.elements.iter().any(holds_owner),
        Value::Union(union) => holds_owner(&union.payload),
        Value::EnumVariant(variant) => variant.payloads.iter().any(|v| holds_owner(&v)),
        Value::TraitObject(object) => holds_owner(&object.value),
        _ => false,
    }
}

impl MirRuntime {
    pub(super) fn drain_owner_hooks(&mut self) -> Result<()> {
        let mut first = Ok(());
        for (method, value) in released_instances() {
            let DropMethod::Interpreted(name) = method else {
                return Err(Diagnostic::new("native drop hook inside interpreted code"));
            };
            let function = self.functions.get(&name).cloned()
                .ok_or_else(|| Diagnostic::new(format!("missing drop hook `{name}`")))?;
            if let Err(error) = self.call_function(&function, Some(Value::Instance(value)), vec![])
                .and_then(|outcome| outcome.value.into_result()) {
                if first.is_ok() {
                    first = Err(error);
                }
            }
            if let Err(error) = self.drain_owner_hooks() {
                if first.is_ok() { first = Err(error); }
            }
        }
        first
    }

    pub(super) fn drop_frame_owners(&mut self, env: &mut Env) -> Result<()> {
        let mut first = Ok(());
        for place in std::mem::take(&mut env.owner_order).into_iter().rev() {
            env.values.remove(&place);
            if let Err(error) = self.drain_owner_hooks() {
                if first.is_ok() { first = Err(error); }
            }
        }
        env.values.clear();
        let rest = self.drain_owner_hooks();
        first.and(rest)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn class_drop_hook_fixtures_release_owners_on_normal_and_trapping_exits() {
        for (source, expected) in [
            (
                include_str!("../tests/fixtures/run-pass/class_drop_hook.au"),
                include_str!("../tests/fixtures/run-pass/class_drop_hook.stdout"),
            ),
            (
                include_str!("../tests/fixtures/run-pass/class_drop_traps.au"),
                include_str!("../tests/fixtures/run-pass/class_drop_traps.stdout"),
            ),
            (
                include_str!("../tests/fixtures/run-pass/class_drop_copy_collection.au"),
                include_str!("../tests/fixtures/run-pass/class_drop_copy_collection.stdout"),
            ),
        ] {
            assert_eq!(crate::run_source(source).unwrap().stdout, expected);
        }
    }
}
