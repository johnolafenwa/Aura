//! Byte-wide masked accesses work for packed fields on strict-alignment targets.
use super::*;

impl FunctionCompiler<'_> {
    pub(super) fn store_foreign_bit_field(
        &mut self,
        slot: Val,
        offset: usize,
        start: u8,
        width: u8,
        word: Val,
    ) {
        for (index, (source, destination, mask)) in
            crate::c_layout::bit_chunks(start, width).enumerate()
        {
            let displacement = (offset + index) as i32;
            let old = self
                .builder
                .load(types::I8, MemFlags::new(), slot, displacement);
            let keep = self.builder.band_imm(old, i64::from(!mask));
            let right = self.builder.iconst(types::I64, i64::from(source));
            let part = self.builder.ushr(word, right);
            let part = self.builder.ireduce(types::I8, part);
            let left = self.builder.iconst(types::I8, i64::from(destination));
            let part = self.builder.ishl(part, left);
            let part = self.builder.band_imm(part, i64::from(mask));
            let byte = self.builder.bor(keep, part);
            self.builder
                .store(MemFlags::new(), byte, slot, displacement);
        }
    }

    pub(super) fn load_foreign_bit_field(
        &mut self,
        slot: Val,
        offset: usize,
        start: u8,
        width: u8,
    ) -> Val {
        let mut word = self.builder.iconst(types::I64, 0);
        for (index, (destination, source, mask)) in
            crate::c_layout::bit_chunks(start, width).enumerate()
        {
            let byte = self
                .builder
                .load(types::I8, MemFlags::new(), slot, (offset + index) as i32);
            let byte = self.builder.band_imm(byte, i64::from(mask));
            let right = self.builder.iconst(types::I8, i64::from(source));
            let part = self.builder.ushr(byte, right);
            let part = self.builder.uextend(types::I64, part);
            let left = self.builder.iconst(types::I64, i64::from(destination));
            let part = self.builder.ishl(part, left);
            word = self.builder.bor(word, part);
        }
        word
    }
}

impl FunctionCompiler<'_> {
    pub(super) fn store_controlled_struct(
        &mut self,
        layout: &CStruct,
        value: &ValueRef,
        slot: Val,
    ) {
        use crate::c_layout::StoragePart;
        for (part, &word) in layout.storage_parts().into_iter().zip(&value.values) {
            match part {
                StoragePart::Inline { offset } => {
                    self.builder
                        .store(MemFlags::new(), word, slot, offset as i32)
                }
                StoragePart::Scalar(leaf) => {
                    if let Some((start, width)) = leaf.bits {
                        self.store_foreign_bit_field(slot, leaf.offset, start, width, word);
                    } else {
                        let scalar =
                            foreign_scalar(leaf.scalar.ffi_type()).expect("a leaf is a scalar");
                        let narrow = self.foreign_scalar_argument(&scalar, word);
                        self.builder
                            .store(MemFlags::new(), narrow, slot, leaf.offset as i32);
                    }
                }
            }
        }
    }

    pub(super) fn load_controlled_struct(
        &mut self,
        layout: &CStruct,
        slot: Val,
        ty: DirectType,
        symbol: &str,
    ) -> std::result::Result<ValueRef, String> {
        use crate::c_layout::StoragePart;
        let mut values = Vec::new();
        for (part, storage) in layout.storage_parts().into_iter().zip(ty.abi_types()) {
            values.push(match part {
                StoragePart::Inline { offset } => {
                    self.builder
                        .load(storage, MemFlags::new(), slot, offset as i32)
                }
                StoragePart::Scalar(leaf) => {
                    if let Some((start, width)) = leaf.bits {
                        self.load_foreign_bit_field(slot, leaf.offset, start, width)
                    } else {
                        let scalar =
                            foreign_scalar(leaf.scalar.ffi_type()).expect("a leaf is a scalar");
                        let raw =
                            self.builder
                                .load(scalar.ty, MemFlags::new(), slot, leaf.offset as i32);
                        self.foreign_scalar_result(&scalar, raw, symbol)?.values[0]
                    }
                }
            });
        }
        Ok(ValueRef { values, ty })
    }
}
