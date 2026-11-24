//! Models of list operations that read or build whole lists.
//!
//! Each model states the result with defined terms, or with a fresh
//! constant and the facts that hold of it. A result no fact pins down
//! stays a made-up one: the executor reports a check that reads it as not
//! modeled.

use super::exec::{Exec, IntTy, State, Val};
use crate::mir::MirArg;
use crate::runtime_ops::ListOp;

impl Exec<'_> {
    /// The result of the list operation `op` on `list`, when this module
    /// models it.
    pub(super) fn list_op(
        &mut self,
        state: &mut State,
        op: ListOp,
        list: Val,
        args: &[MirArg],
    ) -> Option<Val> {
        match op {
            ListOp::Contains => {
                let Val::List { elem, .. } = &list else {
                    return None;
                };
                let hint = IntTy::of(elem);
                let item = self.operand(state, &args.first()?.value, hint);
                self.list_contains(&list, &item)
            }
            _ => None,
        }
    }

    /// `item in list`: some position of the list holds `item`. A contract
    /// states membership the same way, so the two agree. Floats are left
    /// out: `nan` is not equal to itself.
    fn list_contains(&mut self, list: &Val, item: &Val) -> Option<Val> {
        if !matches!(item, Val::Int(..) | Val::Bool(_) | Val::Str(_)) {
            return None;
        }
        let held = self.membership(item, list).ok()?;
        Some(Val::Bool(self.define("Bool", "contains", held)))
    }
}
