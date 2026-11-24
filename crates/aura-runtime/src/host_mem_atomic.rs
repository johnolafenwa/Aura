use super::*;
use aura_core::atomic_order::AtomicOperation;
use std::sync::atomic::{fence, AtomicU8, AtomicU16, AtomicU32, AtomicU64};

pub(super) fn execute(name: &str, op: AtomicOperation, args: &[Value]) -> Result<Value> {
    let arity = op.arity();
    let max = arity + if op == AtomicOperation::CompareExchange { 2 } else { 1 };
    if !(arity..=max).contains(&args.len()) {
        return Err(Diagnostic::coded("AU4034", format!("{name} expects {arity} to {max} arguments")));
    }
    let order = |index| match args.get(index) {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.as_str())),
        _ => Err(Diagnostic::coded("AU4034", "atomic ordering expects a string")),
    };
    // A missing failure order is represented by the empty default in checked MIR.
    let (success, failure) = op.orders(order(arity)?, order(arity + 1)?.filter(|s| !s.is_empty()))
        .map_err(|msg| Diagnostic::coded("AU4034", msg))?;
    if op == AtomicOperation::Fence {
        fence(success);
        return Ok(Value::Unit);
    }
    let word = |index: usize| match &args[index] {
        Value::Int(n) => n.as_i128().and_then(|n| u64::try_from(n).ok()).ok_or_else(|| mem_argument_error(name)),
        _ => Err(mem_argument_error(name)),
    };
    let address = word(0)? as usize;
    macro_rules! access {
        ($atomic:ty, $word:ty, $kind:ident) => {{
            // SAFETY: the package with FFI permission guarantees lifetime, alignment and atomic access.
            let cell = unsafe { <$atomic>::from_ptr(address as *mut $word) };
            let value = match op {
                AtomicOperation::Load => cell.load(success),
                AtomicOperation::Store => {
                    cell.store(word(1)? as $word, success);
                    return Ok(Value::Unit);
                }
                AtomicOperation::Add => cell.fetch_add(word(1)? as $word, success),
                AtomicOperation::Swap => cell.swap(word(1)? as $word, success),
                AtomicOperation::CompareExchange => cell.compare_exchange(
                    word(1)? as $word, word(2)? as $word, success, failure,
                ).unwrap_or_else(|current| current),
                AtomicOperation::Fence => unreachable!("handled above"),
            };
            Ok(Value::Int(IntegerValue::from_typed_unsigned(u128::from(value), IntegerKind::$kind)
                .expect("value came from the matching atomic width")))
        }};
    }
    match name.rsplit_once("_uint").map(|(_, width)| width) {
        Some("8") => access!(AtomicU8, u8, Uint8),
        Some("16") => access!(AtomicU16, u16, Uint16),
        Some("32") => access!(AtomicU32, u32, Uint32),
        Some("64") => access!(AtomicU64, u64, Uint64),
        _ => unreachable!("registered atomic width"),
    }
}
