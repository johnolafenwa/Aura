use super::*;
use aura_core::atomic_order::AtomicOperation;
use std::sync::atomic::Ordering;

fn llvm_order(order: Ordering) -> AtomicOrdering {
    match order {
        Ordering::Relaxed => AtomicOrdering::Monotonic,
        Ordering::Acquire => AtomicOrdering::Acquire,
        Ordering::Release => AtomicOrdering::Release,
        Ordering::AcqRel => AtomicOrdering::AcquireRelease,
        Ordering::SeqCst => AtomicOrdering::SequentiallyConsistent,
        _ => unreachable!("checked atomic ordering"),
    }
}

impl<'ctx> FunctionLowerer<'ctx, '_, '_> {
    pub(super) fn ordered_atomic(
        &self,
        op: AtomicOperation,
        success: Ordering,
        failure: Ordering,
        width: ScalarType,
        operands: &[Val],
    ) -> Result<Option<IntValue<'ctx>>, String> {
        let success = llvm_order(success);
        if op == AtomicOperation::Fence {
            built(self.builder.build_fence(success, false, ""))?;
            return Ok(None);
        }
        let ty = self.int_type(width);
        let ptr = built(self.builder.build_int_to_ptr(
            self.int(operands[0])?,
            self.context.ptr_type(AddressSpace::default()),
            "",
        ))?;
        let word = |i: usize| built(self.builder.build_int_cast(self.int(operands[i])?, ty, ""));
        let value = match op {
            AtomicOperation::Load => {
                let loaded = built(self.builder.build_load(ty, ptr, ""))?.into_int_value();
                let inst = loaded.as_instruction_value().expect("load instruction");
                inst.set_alignment(ty.get_bit_width() / 8)
                    .map_err(|e| e.to_string())?;
                inst.set_atomic_ordering(success)
                    .map_err(|e| e.to_string())?;
                loaded
            }
            AtomicOperation::Store => {
                let store = built(self.builder.build_store(ptr, word(1)?))?;
                store
                    .set_alignment(ty.get_bit_width() / 8)
                    .map_err(|e| e.to_string())?;
                store
                    .set_atomic_ordering(success)
                    .map_err(|e| e.to_string())?;
                return Ok(None);
            }
            AtomicOperation::Swap | AtomicOperation::Add => {
                let kind = if op == AtomicOperation::Swap {
                    AtomicRMWBinOp::Xchg
                } else {
                    AtomicRMWBinOp::Add
                };
                built(self.builder.build_atomicrmw(kind, ptr, word(1)?, success))?
            }
            AtomicOperation::CompareExchange => {
                let pair = built(self.builder.build_cmpxchg(
                    ptr,
                    word(1)?,
                    word(2)?,
                    success,
                    llvm_order(failure),
                ))?;
                built(self.builder.build_extract_value(pair, 0, ""))?.into_int_value()
            }
            AtomicOperation::Fence => unreachable!("handled above"),
        };
        built(
            self.builder
                .build_int_z_extend_or_bit_cast(value, self.context.i64_type(), ""),
        )
        .map(Some)
    }
}
