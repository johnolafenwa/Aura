//! Runs owner hooks after releasing runtime references. The receiver has
//! already been disarmed; releasing its fields can enqueue further hooks.

use super::*;
use aura_runtime::runtime_value::class_drop::{released_instances, DropMethod};

pub(super) use aura_runtime::runtime_value::class_drop::holds_owner;

impl MirRuntime {
    pub(super) fn drain_owner_hooks(&mut self) -> Result<()> {
        let mut first = Ok(());
        for (method, value) in released_instances() {
            let DropMethod::Interpreted(name) = method else {
                return Err(Diagnostic::new("native drop hook inside interpreted code"));
            };
            let function = self
                .functions
                .get(&name)
                .cloned()
                .ok_or_else(|| Diagnostic::new(format!("missing drop hook `{name}`")))?;
            if let Err(error) = self
                .call_function(&function, Some(Value::Instance(value)), vec![])
                .and_then(|outcome| outcome.value.into_result())
            {
                if first.is_ok() {
                    first = Err(error);
                }
            }
            if let Err(error) = self.drain_owner_hooks() {
                if first.is_ok() {
                    first = Err(error);
                }
            }
        }
        first
    }

    pub(super) fn drop_frame_owners(&mut self, env: &mut Env) -> Result<()> {
        let mut first = Ok(());
        for place in std::mem::take(&mut env.owner_order).into_iter().rev() {
            env.shared_values.remove(&place);
            env.values.remove(&place);
            if let Err(error) = self.drain_owner_hooks() {
                if first.is_ok() {
                    first = Err(error);
                }
            }
        }
        env.shared_values.clear();
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
                include_str!("../tests/fixtures/run-pass/class_drop_nested_record_copy.au"),
                include_str!("../tests/fixtures/run-pass/class_drop_nested_record_copy.stdout"),
            ),
            (
                include_str!("../tests/fixtures/run-pass/class_drop_reinitialized_local.au"),
                include_str!("../tests/fixtures/run-pass/class_drop_reinitialized_local.stdout"),
            ),
            (
                include_str!("../tests/fixtures/run-pass/class_drop_hook.au"),
                include_str!("../tests/fixtures/run-pass/class_drop_hook.stdout"),
            ),
            (
                include_str!("../tests/fixtures/run-pass/class_drop_traps.au"),
                include_str!("../tests/fixtures/run-pass/class_drop_traps.stdout"),
            ),
            (
                include_str!("../tests/fixtures/run-pass/class_drop_reassigned_order.au"),
                include_str!("../tests/fixtures/run-pass/class_drop_reassigned_order.stdout"),
            ),
            (
                include_str!("../tests/fixtures/run-pass/class_drop_lexical_bindings.au"),
                include_str!("../tests/fixtures/run-pass/class_drop_lexical_bindings.stdout"),
            ),
            (
                include_str!("../tests/fixtures/run-pass/class_drop_field_trap.au"),
                include_str!("../tests/fixtures/run-pass/class_drop_field_trap.stdout"),
            ),
            (
                include_str!("../tests/fixtures/run-pass/class_drop_nested_trap.au"),
                include_str!("../tests/fixtures/run-pass/class_drop_nested_trap.stdout"),
            ),
            (
                include_str!("../tests/fixtures/run-pass/class_drop_trait_object.au"),
                include_str!("../tests/fixtures/run-pass/class_drop_trait_object.stdout"),
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
