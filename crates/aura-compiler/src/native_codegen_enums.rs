//! Enum variants in their typed layouts (typed-layout step 2f).
//!
//! A variant generated code builds keeps its payloads in one heap block in
//! the layout the compiler computes for its enum in a box
//! ([`aura_core::layout::LayoutBuilder::boxed_enum_layout`]), which the
//! runtime registers from a cell under an enum access code,
//! `FIRST_ENUM_ACCESS` plus the layout's position (see
//! `native_enum_layouts.rs` in the runtime). A scalar payload lies in the
//! block, and every other payload, a recursive one included, in a box the
//! block owns. The variant's box shows the code, the variant's index, and
//! the block's address in its list view while it is confined to its thread
//! and keeps its payloads in place, so generated code checks one code and
//! then:
//!
//! - tests a variant by comparing its index;
//! - reads a scalar payload where it lies;
//! - reads any other payload by taking a new reference to its box, so a
//!   recursive enum's subtree is shared, not copied;
//! - builds a variant with one call and stores each payload.
//!
//! Any other variant, such as a shared one or one the runtime built with its
//! payloads as values, takes the runtime call the code made before.
//!
//! The enums a program defines and `Result` are kept this way. A `Result`
//! the runtime returns keeps its payloads as values, and generated code
//! reads it through the runtime. The runtime builds and reads the
//! compiler's other enums, such as `Lookup`, and the library's, such as
//! `json.Value`, with their payloads as values.

use std::rc::Rc;

use aura_core::layout::{encode_layouts, LayoutBuilder, LayoutId, LayoutKind, LayoutTable};
use aura_runtime::native_runtime::FIRST_ENUM_ACCESS;

use super::classes::{ClassSlot, SlotKind};
use super::plain::{collect_leaves, PlainRecords};
use super::*;

/// Each variant's name and its payloads' offsets in the block and layouts.
type VariantPayloadPlaces = Vec<(String, Vec<(u64, LayoutId)>)>;

/// The layout of one enum type whose variants generated code keeps in
/// place.
#[derive(Clone, Debug)]
pub(super) struct EnumRecord {
    cell: DataHandle,
    pub(super) access: i64,
    table: Rc<LayoutTable>,
    variants: Rc<VariantPayloadPlaces>,
}

impl EnumRecord {
    /// The index of variant `name`.
    pub(super) fn variant_index(&self, name: &str) -> Option<usize> {
        self.variants
            .iter()
            .position(|(variant, _)| variant == name)
    }

    /// How many payloads variant `variant` takes.
    pub(super) fn payload_count(&self, variant: usize) -> usize {
        self.variants[variant].1.len()
    }

    /// The type of payload `index` of variant `variant`.
    pub(super) fn payload_type(&self, variant: usize, index: usize) -> &Type {
        &self.table.get(self.variants[variant].1[index].1).ty
    }

    /// Where payload `index` of variant `variant` lies in the block, and its
    /// generated-code form for a place of direct type `ty`: scalars, or
    /// one box.
    pub(super) fn payload_slot(
        &self,
        variant: usize,
        index: usize,
        ty: &DirectType,
    ) -> Option<ClassSlot> {
        let (offset, id) = *self.variants.get(variant)?.1.get(index)?;
        let kind = match (&self.table.get(id).kind, ty) {
            (LayoutKind::Handle, _) => SlotKind::Handle,
            (_, DirectType::Scalar(_) | DirectType::PlainClass(_)) => {
                let mut leaves = Vec::new();
                if !collect_leaves(&self.table, id, ty, offset, &mut leaves) {
                    return None;
                }
                SlotKind::Leaves(leaves)
            }
            _ => return None,
        };
        Some(ClassSlot {
            access: self.access,
            offset: i32::try_from(offset).ok()?,
            kind,
        })
    }
}

/// Whether the variants of `enum_name` may keep their payloads in a layout:
/// an enum the program defines, or `Result`, not one only the runtime
/// builds or reads.
fn program_defines_enum(enum_name: &str) -> bool {
    if matches!(
        enum_name,
        "Lookup"
            | "Poll"
            | "SendError"
            | "QueueReceive"
            | "TaskResult"
            | "WaitAny"
            | "WaitAll"
            | "SelectOutcome"
    ) {
        return false;
    }
    match enum_name.rsplit_once('.') {
        Some((module, _)) => {
            let path = module.split('.').map(str::to_string).collect::<Vec<_>>();
            crate::builtin_modules::builtin_module_namespace(&path).is_none()
        }
        None => true,
    }
}

/// Whether a payload of type `ty` lies in its variant's block rather than
/// in a box of its own: a scalar.
fn payload_lies_inline(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Named(name, args) if args.is_empty() && matches!(
            name.as_str(),
            "int8" | "int16" | "int32" | "int64" | "uint8" | "uint16" | "uint32" | "uint64"
                | "float32" | "float64" | "bool"
        )
    )
}

impl PlainRecords {
    /// The record of the enum type `ty`, made on first use. An enum the
    /// runtime builds, or a type that names a type parameter, has none.
    pub(super) fn enum_record(
        &mut self,
        ty: &Type,
        object: &mut dyn ModuleBackend,
    ) -> std::result::Result<Option<EnumRecord>, String> {
        let key = aura_core::type_descriptor::encode(ty);
        if let Some(record) = self.enums.get(&key) {
            return Ok(record.clone());
        }
        let record = match self.enum_layout(ty) {
            Some((table, root)) => {
                let index = self
                    .enums
                    .values()
                    .filter(|record| record.is_some())
                    .count();
                let access = u64::from(FIRST_ENUM_ACCESS) + index as u64;
                let descriptor = encode_layouts(&table, root);
                let cell = self.declare_cell_for(descriptor, access, object)?;
                let LayoutKind::Tagged {
                    payload_offset,
                    variants,
                    ..
                } = &table.get(root).kind
                else {
                    unreachable!("an enum in a box is laid out tagged");
                };
                let variants = variants
                    .iter()
                    .map(|variant| {
                        let payloads = variant
                            .fields
                            .iter()
                            .map(|field| (field.offset - payload_offset, field.layout))
                            .collect();
                        (variant.name.clone(), payloads)
                    })
                    .collect();
                Some(EnumRecord {
                    cell,
                    access: access as i64,
                    table: Rc::new(table),
                    variants: Rc::new(variants),
                })
            }
            None => None,
        };
        self.enums.insert(key, record.clone());
        Ok(record)
    }

    fn enum_layout(&self, ty: &Type) -> Option<(LayoutTable, LayoutId)> {
        let Type::Named(enum_name, _) = ty else {
            return None;
        };
        if !program_defines_enum(enum_name) {
            return None;
        }
        let mut builder = LayoutBuilder::new(&self.env);
        let root = builder.boxed_enum_layout(ty, payload_lies_inline).ok()??;
        let table = builder.into_table();
        (!table.holds_block(root)).then_some((table, root))
    }
}

impl FunctionCompiler<'_> {
    /// The record of `ty`, when it is an enum generated code keeps in place.
    pub(super) fn enum_record_of(
        &mut self,
        ty: &DirectType,
    ) -> std::result::Result<Option<EnumRecord>, String> {
        let DirectType::Opaque(ty @ Type::Named(..)) = ty else {
            return Ok(None);
        };
        self.plain_records.enum_record(ty, self.builder.module())
    }

    /// The index of a checked variant box, which its view shows as a
    /// length.
    pub(super) fn variant_index_of(&mut self, variant: Val) -> Val {
        let layout = aura_runtime::native_runtime::direct_list_layout();
        self.builder
            .load(types::I64, MemFlags::trusted(), variant, layout.len_offset)
    }

    /// Branches to `fast_block` when the variant box `variant` keeps its
    /// payloads in `record`'s layout and is variant `index`, and to
    /// `slow_block` otherwise.
    fn emit_variant_check(
        &mut self,
        variant: Val,
        record: &EnumRecord,
        index: usize,
        slow_block: Blk,
        fast_block: Blk,
    ) {
        let checked = self.builder.create_block();
        self.emit_class_check(variant, record.access, slow_block, checked);
        self.builder.switch_to_block(checked);
        self.builder.seal_block(checked);
        let actual = self.variant_index_of(variant);
        let matches = self.builder.icmp_imm(IntCC::Equal, actual, index as i64);
        self.builder.brif(matches, fast_block, &[], slow_block, &[]);
    }

    /// Builds variant `variant_name` of the enum type `ty` in its layout:
    /// one call makes the block, then each scalar payload is stored at its
    /// offset and each box is put into its slot. `None` when the enum is not
    /// kept in place.
    pub(super) fn construct_variant_in_place(
        &mut self,
        ty: &Type,
        variant_name: &str,
        payloads: &[Operand],
    ) -> std::result::Result<Option<ValueRef>, String> {
        let Some(record) = self.enum_record_of(&DirectType::Opaque(ty.clone()))? else {
            return Ok(None);
        };
        let Some(variant) = record.variant_index(variant_name) else {
            return Ok(None);
        };
        if record.payload_count(variant) != payloads.len() {
            return Ok(None);
        }
        let mut slots = Vec::with_capacity(payloads.len());
        for index in 0..payloads.len() {
            let payload_ty = record.payload_type(variant, index).clone();
            let direct =
                direct_type(&payload_ty, &self.classes).unwrap_or(DirectType::Opaque(payload_ty));
            let Some(slot) = record.payload_slot(variant, index, &direct) else {
                return Ok(None);
            };
            slots.push((direct, slot));
        }
        let cell = self.builder.import_data(record.cell);
        let cell = self.builder.symbol_value(types::I64, cell);
        let index = self.builder.iconst(types::I64, variant as i64);
        let init = self
            .builder
            .call(self.runtime[RuntimeFn::EnumNewTyped], &[cell, index]);
        let mut last_call = init.inst;
        let built = self.owned_opaque_result(init.results, ty.clone());
        let object = built.values[0];
        // A new variant is confined and keeps its payloads in place.
        let block = self.class_block(object);
        for (payload, (direct, slot)) in payloads.iter().zip(slots) {
            let loaded = self.load_operand_for_target(payload, &direct)?;
            let loaded = self.coerce_value(loaded, &direct)?;
            match slot.kind {
                SlotKind::Leaves(leaves) => {
                    for (leaf, word) in leaves.iter().zip(&loaded.values) {
                        self.store_leaf(*leaf, *word, block);
                    }
                }
                SlotKind::Handle => {
                    let boxed = self.ensure_opaque(loaded)?;
                    let boxed = self.transfer_owned_opaque_value(&boxed);
                    let payload_slot = self.builder.iadd_imm(block, i64::from(slot.offset));
                    let put = self.builder.call(
                        self.runtime[RuntimeFn::InstancePutField],
                        &[payload_slot, boxed],
                    );
                    last_call = put.inst;
                }
            }
        }
        // The box already carries the enum's type, so tagging it with that
        // type again does nothing.
        self.last_runtime_type_tag = Some((
            last_call,
            object,
            crate::native_runtime::canonical_runtime_type_name(ty),
        ));
        Ok(Some(built))
    }

    /// Tests whether the variant box `variant`, of static type `ty`, is
    /// variant `variant_name`: by its index when it keeps its payloads in
    /// place, and with `slow` otherwise. `None` when the enum is not kept in
    /// place.
    pub(super) fn typed_variant_test(
        &mut self,
        variant: Val,
        ty: &DirectType,
        variant_name: &str,
        slow: impl FnOnce(&mut Self) -> std::result::Result<Val, String>,
    ) -> std::result::Result<Option<Val>, String> {
        let Some(record) = self.enum_record_of(ty)? else {
            return Ok(None);
        };
        let Some(index) = record.variant_index(variant_name) else {
            return Ok(None);
        };
        let checked = self.builder.create_block();
        let slow_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder.append_block_param(join_block, types::I64);
        self.emit_class_check(variant, record.access, slow_block, checked);

        self.builder.switch_to_block(checked);
        self.builder.seal_block(checked);
        let actual = self.variant_index_of(variant);
        let matches = self.builder.icmp_imm(IntCC::Equal, actual, index as i64);
        let matches = self.builder.uextend(types::I64, matches);
        self.builder.jump(join_block, &[matches]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let matched = slow(self)?;
        self.builder.jump(join_block, &[matched]);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        Ok(Some(self.builder.block_params(join_block)[0]))
    }

    /// Reads payload `index` of variant `variant_name` from the variant box
    /// `scrutinee` into a value of direct type `target`: a scalar where it
    /// lies, or a new reference to the payload's box, when the variant keeps
    /// its payloads in place, and with `slow` otherwise. `None` when the
    /// enum is not kept in place or generated code cannot read the payload
    /// as `target`. A read for a lent payload (see `lend_element`) holds the
    /// payload's box without a reference and sets `lent_element_flag`.
    pub(super) fn typed_variant_payload(
        &mut self,
        scrutinee: &ValueRef,
        variant_name: &str,
        index: usize,
        target: &DirectType,
        slow: impl FnOnce(&mut Self) -> std::result::Result<ValueRef, String>,
    ) -> std::result::Result<Option<ValueRef>, String> {
        let Some(record) = self.enum_record_of(&scrutinee.ty)? else {
            return Ok(None);
        };
        let Some(variant) = record.variant_index(variant_name) else {
            return Ok(None);
        };
        let Some(slot) = record.payload_slot(variant, index, target) else {
            return Ok(None);
        };
        let handle = matches!(slot.kind, SlotKind::Handle);
        // A box read as the type its variant declares carries that type's
        // tag from when the variant was built, so it keeps the static type
        // and is not tagged again.
        let declared = record.payload_type(variant, index);
        let shaped = handle
            && matches!(target, DirectType::Opaque(ty) if !runtime_type_is_wildcard(ty)
                && crate::native_runtime::canonical_runtime_type_name(ty)
                    == crate::native_runtime::canonical_runtime_type_name(declared));
        let object = scrutinee.values[0];
        let slow_block = self.builder.create_block();
        let fast_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        let result_ty = if handle && !shaped {
            DirectType::Opaque(Type::named("Unknown"))
        } else {
            target.clone()
        };
        for abi in result_ty.abi_types() {
            self.builder.append_block_param(join_block, abi);
        }
        // The join's last parameter is the lent flag: 1 for the variant's
        // own box, 0 for a box the slow read made.
        let lend = shaped && self.lend_element && !self.temporary_owns_opaque(scrutinee);
        if lend {
            self.builder.append_block_param(join_block, types::I64);
        }
        self.emit_variant_check(object, &record, variant, slow_block, fast_block);

        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        let block = self.class_block(object);
        let values = match &slot.kind {
            SlotKind::Leaves(leaves) => leaves
                .iter()
                .map(|leaf| self.load_leaf(*leaf, block))
                .collect::<Vec<_>>(),
            SlotKind::Handle => {
                let boxed = self
                    .builder
                    .load(types::I64, MemFlags::trusted(), block, slot.offset);
                // A moved-out payload takes the runtime read, which reports
                // it.
                let present_block = self.builder.create_block();
                self.builder
                    .brif(boxed, present_block, &[], slow_block, &[]);
                self.builder.switch_to_block(present_block);
                self.builder.seal_block(present_block);
                if lend {
                    vec![boxed, self.builder.iconst(types::I64, 1)]
                } else {
                    vec![self.retain_opaque_handle(boxed)]
                }
            }
        };
        self.builder.jump(join_block, &values);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let caller_owned = self.layout_scratch.clone();
        let loaded = slow(self)?;
        let loaded = if handle && !shaped {
            loaded
        } else {
            self.coerce_value(loaded, target)?
        };
        let loaded = if handle && !self.temporary_owns_opaque(&loaded) {
            // The join owns the payload's reference on both paths.
            let retained = self.retain_opaque_handle(loaded.values[0]);
            ValueRef {
                values: vec![retained],
                ty: loaded.ty,
            }
        } else {
            loaded
        };
        self.clear_temporary_opaque_owned(&loaded);
        self.release_temporary_owned_since(&caller_owned);
        let mut values = loaded.values;
        if lend {
            values.push(self.builder.iconst(types::I64, 0));
        }
        self.builder.jump(join_block, &values);
        self.layout_scratch = caller_owned;

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        let mut values = self.builder.block_params(join_block).to_vec();
        if lend {
            self.lent_element_flag = values.pop();
        }
        let value = ValueRef {
            values,
            ty: result_ty,
        };
        if handle && !lend {
            self.mark_temporary_opaque_owned(&value);
        }
        if shaped {
            self.shaped_boxes.insert(value.values[0]);
        }
        Ok(Some(value))
    }
}
