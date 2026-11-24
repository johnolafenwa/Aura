//! Backing storage for addressed C locals and aggregate pointer operations.
use super::*;

impl FunctionCompiler<'_> {
    pub(in super::super) fn compile_memory_bool(
        &mut self,
        access: crate::native_backend::MemoryAccess,
        words: &[Val],
    ) -> std::result::Result<ValueRef, String> {
        let Some(byte) = self.builder.memory_access(access, types::I8, words) else {
            return Ok(unit_value(self.builder));
        };
        self.trap_on_bad_bool(byte, "mem.read_bool")?;
        Ok(ValueRef {
            values: vec![byte],
            ty: DirectType::Scalar(ScalarKind::Bool),
        })
    }

    pub(in super::super) fn read_raw_class(
        &mut self,
        layout: &CStruct,
        slot: Val,
        ty: DirectType,
    ) -> std::result::Result<ValueRef, String> {
        if layout.is_plain() {
            return self.load_foreign_struct(layout, slot, ty, "mem.address_of");
        }
        let (descriptor, length) = self.foreign_layout_descriptor(layout)?;
        let (label, label_len) = self.string_constant(b"mem.address_of")?;
        let call = self.builder.call(
            self.runtime[RuntimeFn::FfiUnpackStruct],
            &[descriptor, length, slot, label, label_len],
        );
        Ok(self.owned_opaque_result(call.results, direct_type_to_type(&ty)))
    }

    pub(in super::super) fn write_raw_class(
        &mut self,
        layout: &CStruct,
        value: &ValueRef,
        slot: Val,
    ) -> std::result::Result<(), String> {
        self.store_foreign_struct(layout, value, slot)
    }

    pub(in super::super) fn prepare_raw_cells(
        &mut self,
        function: &MirFunction,
    ) -> std::result::Result<(), String> {
        for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
            let Instruction::Assign {
                value: Rvalue::CFieldAddress { root, layout, .. },
                ..
            } = instruction
            else {
                continue;
            };
            if self.raw_cells.contains_key(root) {
                continue;
            }
            let size =
                u32::try_from(layout.size()).map_err(|_| "C local storage exceeds target limit")?;
            let align_shift = layout.align().trailing_zeros() as u8;
            let slot = self.builder.create_sized_stack_slot(size, align_shift);
            let address = self.builder.stack_addr(types::I64, slot, 0);
            if function.params.iter().any(|param| param.name == *root) {
                let value = self.load_root(root)?;
                self.store_foreign_struct(layout, &value, address)?;
            }
            self.raw_cells
                .insert(root.clone(), (layout.clone(), address));
        }
        Ok(())
    }
}

impl FunctionCompiler<'_> {
    pub(in super::super) fn compile_pointer_class(
        &mut self,
        address: &Operand,
        store: Option<&Operand>,
        layout: &CStruct,
        volatile: bool,
        target: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        let address = self.load_operand(address)?.values[0];
        // The temporary has the class's target alignment, so a field inside
        // a packed containing class need not be naturally aligned.
        let size = u32::try_from(layout.size()).map_err(|_| "C aggregate exceeds target size")?;
        let slot = self
            .builder
            .create_sized_stack_slot(size, layout.align().trailing_zeros() as u8);
        let scratch = self.builder.stack_addr(types::I64, slot, 0);
        if let Some(value) = store {
            let value = self.load_operand(value)?;
            self.copy_raw_bytes(None, scratch, size, false, false);
            self.store_foreign_struct(layout, &value, scratch)?;
            self.copy_raw_bytes(Some(scratch), address, size, false, volatile);
            Ok(unit_value(self.builder))
        } else {
            self.copy_raw_bytes(Some(address), scratch, size, volatile, false);
            self.read_raw_class(layout, scratch, target.clone())
        }
    }

    fn copy_raw_bytes(
        &mut self,
        from: Option<Val>,
        to: Val,
        size: u32,
        volatile_read: bool,
        volatile_write: bool,
    ) {
        use crate::native_backend::MemoryAccess;
        if size == 0 {
            return;
        }
        let read = if volatile_read {
            MemoryAccess::VolatileRead
        } else {
            MemoryAccess::Read
        };
        let write = if volatile_write {
            MemoryAccess::VolatileWrite
        } else {
            MemoryAccess::Write
        };
        let zero = self.builder.iconst(types::I64, 0);
        let body = self.builder.create_block();
        let done = self.builder.create_block();
        let index = self.builder.append_block_param(body, types::I64);
        self.builder.jump(body, &[zero]);
        self.builder.switch_to_block(body);
        let target = self.builder.iadd(to, index);
        let byte = if let Some(from) = from {
            let source = self.builder.iadd(from, index);
            self.builder
                .memory_access(read, types::I8, &[source])
                .expect("read produces one byte")
        } else {
            zero
        };
        self.builder
            .memory_access(write, types::I8, &[target, byte]);
        let next = self.builder.iadd_imm(index, 1);
        let complete = self.builder.icmp_imm(IntCC::Equal, next, i64::from(size));
        self.builder.brif(complete, done, &[], body, &[next]);
        self.builder.seal_block(body);
        self.builder.switch_to_block(done);
        self.builder.seal_block(done);
    }
}
