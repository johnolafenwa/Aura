//! Class instances in their typed layouts (typed-layout step 2e).
//!
//! An instance generated code builds keeps its fields in one heap block in
//! the class's layout, which the runtime registers from a cell under a
//! class access code, `FIRST_CLASS_ACCESS` plus the class's position (see
//! `native_class_layouts.rs` in the runtime). The instance's box shows that
//! code and the block's address in its list view while the instance is
//! confined to its thread and keeps its fields in place, so generated code
//! checks one code and then reads or writes a field at its offset:
//!
//! - a scalar or plain-record field, at any path through classes and tuples
//!   held inline, is loaded or stored where it lies;
//! - a field that is a box is copied with one call when it is read, and a
//!   new box is put into its slot with one call when it is written;
//! - a field box lent to a runtime operation is lent as it is, with no
//!   take, no new box, and no store back, unless the operation left it
//!   shared or replaced it;
//! - a class literal builds the block with one call and stores each field.
//!
//! Any other instance, such as a shared one or one the runtime built with
//! its fields in a map, takes the runtime call the code made before, and so
//! does any other place.

use std::rc::Rc;

use aura_core::layout::{LayoutBuilder, LayoutId, LayoutKind, LayoutTable};
use aura_runtime::native_runtime::FIRST_CLASS_ACCESS;

use super::plain::{collect_leaves, PlainLeaf, PlainRecords};
use super::*;
use crate::mir::ownership::{instruction_accesses, Access};

/// The layout of one class whose instances generated code keeps in place.
#[derive(Clone, Debug)]
pub(super) struct ClassRecord {
    cell: DataHandle,
    access: i64,
    table: Rc<LayoutTable>,
    root: LayoutId,
}

/// A place inside an instance that generated code reads and writes where it
/// lies.
#[derive(Clone, Debug)]
pub(super) struct ClassSlot {
    access: i64,
    offset: i32,
    kind: SlotKind,
}

#[derive(Clone, Debug)]
enum SlotKind {
    /// Scalars, one per word of the place's direct type.
    Leaves(Vec<PlainLeaf>),
    /// One box, null while the field is moved out.
    Handle,
}

/// A field box lent as it is: the variable that is 1 while the temporary
/// holds the field's own box, and the one that holds the slot's address.
#[derive(Clone, Copy, Debug)]
pub(super) struct FieldLend {
    lent: Var,
    slot: Var,
}

impl PlainRecords {
    /// The record of `class_name`, made on first use. A generic or
    /// recursive class keeps its fields in a map and has none.
    pub(super) fn class_record(
        &mut self,
        class_name: &str,
        object: &mut dyn ModuleBackend,
    ) -> std::result::Result<Option<ClassRecord>, String> {
        if let Some(record) = self.classes.get(class_name) {
            return Ok(record.clone());
        }
        let record = match self.class_layout(class_name) {
            Some((table, root)) => {
                let index = self
                    .classes
                    .values()
                    .filter(|record| record.is_some())
                    .count();
                let access = u64::from(FIRST_CLASS_ACCESS) + index as u64;
                let cell = self.declare_cell(&Type::named(class_name), access, object)?;
                Some(ClassRecord {
                    cell,
                    access: access as i64,
                    table: Rc::new(table),
                    root,
                })
            }
            None => None,
        };
        self.classes.insert(class_name.to_string(), record.clone());
        Ok(record)
    }

    fn class_layout(&self, class_name: &str) -> Option<(LayoutTable, LayoutId)> {
        if !self.env.classes.get(class_name)?.type_params.is_empty() {
            return None;
        }
        let mut builder = LayoutBuilder::new(&self.env);
        let root = builder.layout(&Type::named(class_name)).ok()?;
        let table = builder.into_table();
        let in_place = matches!(
            table.get(root).kind,
            LayoutKind::Struct { class: Some(_), .. }
        ) && !table.holds_block(root);
        in_place.then_some((table, root))
    }
}

impl ClassRecord {
    /// The place `segments` reach through fields of classes and tuples held
    /// inline, as `meta.count` reaches `count` inside `meta`, and the
    /// generated-code form of a place of direct type `ty` there.
    fn slot(&self, segments: &[&str], ty: &DirectType) -> Option<ClassSlot> {
        let mut offset = 0u64;
        let mut id = self.root;
        for segment in segments {
            let LayoutKind::Struct { fields, .. } = &self.table.get(id).kind else {
                return None;
            };
            let field = fields.iter().find(|field| field.name == *segment)?;
            offset += field.offset;
            id = field.layout;
        }
        let kind = match (&self.table.get(id).kind, ty) {
            (LayoutKind::Handle, DirectType::Opaque(_)) => SlotKind::Handle,
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

/// The temporaries of `function`'s lends of a field to one runtime
/// operation, which may lend the field's box as it is. Between the lend
/// and its end, only that operation, with the temporary as its receiver,
/// and facts about the temporary may name the temporary or the instance.
pub(super) fn field_lend_temps(function: &MirFunction) -> HashSet<String> {
    let mut temps = HashSet::new();
    for block in &function.blocks {
        for (index, instruction) in block.instructions.iter().enumerate() {
            let Instruction::Lend { place, temp, .. } = instruction else {
                continue;
            };
            let Some((root, _)) = place.split_once('.') else {
                continue;
            };
            let end = block.instructions[index + 1..].iter().position(
                |later| matches!(later, Instruction::EndLend { temp: ended, .. } if ended == temp),
            );
            let Some(end) = end else {
                continue;
            };
            let body = &block.instructions[index + 1..index + 1 + end];
            let calls = body
                .iter()
                .filter(|instruction| receives_lent_field(instruction, temp, root))
                .count();
            let rest_stays_away = body.iter().all(|instruction| {
                receives_lent_field(instruction, temp, root)
                    || matches!(
                        instruction,
                        Instruction::IndexInRange { list, index }
                            if list == temp && !names_root(index, temp, root)
                    )
            });
            if calls == 1 && rest_stays_away {
                temps.insert(temp.clone());
            }
        }
    }
    temps
}

fn place_root(place: &str) -> &str {
    place.split('.').next().unwrap_or(place)
}

fn names_root(place: &str, temp: &str, root: &str) -> bool {
    let named = place_root(place);
    named == temp || named == root
}

/// Whether `instruction` is a runtime operation on the lent temporary
/// `temp` as its receiver, which only reads the temporary or changes it in
/// place, and which names neither it elsewhere nor the instance `root`.
fn receives_lent_field(instruction: &Instruction, temp: &str, root: &str) -> bool {
    let Instruction::Assign {
        target,
        value:
            Rvalue::Call {
                callee:
                    CallTarget::Runtime {
                        receiver: Some(Operand::Place(receiver)),
                        receiver_place,
                        ..
                    },
                args,
                ..
            },
    } = instruction
    else {
        return false;
    };
    receiver == temp
        && receiver_place.as_deref().is_none_or(|place| place == temp)
        && !names_root(target, temp, root)
        && args.iter().all(|arg| {
            arg.writeback_place.is_none()
                && !matches!(
                    &arg.value,
                    Operand::Place(place) | Operand::MovePlace(place)
                        if names_root(place, temp, root)
                )
        })
        && instruction_accesses(instruction)
            .into_iter()
            .filter(|(place, _)| names_root(place, temp, root))
            .all(|(place, access)| match access {
                Access::Read | Access::Writeback => place == temp,
                _ => false,
            })
}

impl FunctionCompiler<'_> {
    /// The slot of field path `segments` of a `root_ty` instance, when the
    /// class is kept in place and generated code reaches the place there.
    fn class_slot(
        &mut self,
        root_ty: &DirectType,
        segments: &[&str],
        ty: &DirectType,
    ) -> std::result::Result<Option<(ClassRecord, ClassSlot)>, String> {
        let DirectType::Opaque(Type::Named(class_name, args)) = root_ty else {
            return Ok(None);
        };
        if !args.is_empty() || !self.classes.contains_key(class_name) {
            return Ok(None);
        }
        let Some(record) = self
            .plain_records
            .class_record(class_name, self.builder.module())?
        else {
            return Ok(None);
        };
        Ok(record.slot(segments, ty).map(|slot| (record, slot)))
    }

    /// The root, path, and slot of a static field place of a root that
    /// holds a class kept in place.
    fn class_place(
        &mut self,
        place: &str,
    ) -> std::result::Result<Option<(String, ClassSlot, DirectType)>, String> {
        let Some((root, path)) = place.split_once('.') else {
            return Ok(None);
        };
        // A view's place lies in what it views, which the view code reaches.
        if self.view_places.contains_key(root) {
            return Ok(None);
        }
        let Some(root_ty) = self.variable_types.get(root).cloned() else {
            return Ok(None);
        };
        if !matches!(&root_ty, DirectType::Opaque(Type::Named(class_name, args))
            if args.is_empty() && self.classes.contains_key(class_name))
        {
            return Ok(None);
        }
        // A place this code cannot type takes the runtime path, which
        // reports it.
        let Ok(ty) = self.type_of_place(place) else {
            return Ok(None);
        };
        let segments = path.split('.').collect::<Vec<_>>();
        Ok(self
            .class_slot(&root_ty, &segments, &ty)?
            .map(|(_, slot)| (root.to_string(), slot, ty)))
    }

    /// Branches to `fast_block` when the instance box `object` keeps its
    /// fields in the layout `access` names, which also means it is confined
    /// to this thread, and to `slow_block` otherwise.
    fn emit_class_check(&mut self, object: Val, access: i64, slow_block: Blk, fast_block: Blk) {
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let actual = self.emit_list_access(object, &layout);
        let matches = self.builder.icmp_imm(IntCC::Equal, actual, access);
        self.builder.brif(matches, fast_block, &[], slow_block, &[]);
    }

    /// The address of the block of a checked instance box.
    fn class_block(&mut self, object: Val) -> Val {
        let layout = aura_runtime::native_runtime::direct_list_layout();
        self.builder.load(
            types::I64,
            MemFlags::trusted(),
            object,
            layout.elements_offset,
        )
    }

    /// Reads a static field place of an instance kept in place where it
    /// lies, with the runtime read for any other instance. `None` when
    /// generated code cannot reach the place in the block.
    pub(super) fn load_class_place(
        &mut self,
        place: &str,
    ) -> std::result::Result<Option<ValueRef>, String> {
        let Some((root, slot, ty)) = self.class_place(place)? else {
            return Ok(None);
        };
        let object = self.load_root(&root)?.values[0];
        let slow_block = self.builder.create_block();
        let fast_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        for abi in ty.abi_types() {
            self.builder.append_block_param(join_block, abi);
        }
        self.emit_class_check(object, slot.access, slow_block, fast_block);

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
                // A moved-out field takes the runtime read, which reports it.
                let present_block = self.builder.create_block();
                self.builder
                    .brif(boxed, present_block, &[], slow_block, &[]);
                self.builder.switch_to_block(present_block);
                self.builder.seal_block(present_block);
                let copy = self
                    .builder
                    .call(self.runtime[RuntimeFn::InstanceCopyField], &[boxed]);
                vec![copy.results[0]]
            }
        };
        self.builder.jump(join_block, &values);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let caller_owned = self.layout_scratch.clone();
        let loaded = self.load_static_place_slowly(place)?;
        let loaded = self.coerce_value(loaded, &ty)?;
        let loaded =
            if matches!(slot.kind, SlotKind::Handle) && !self.temporary_owns_opaque(&loaded) {
                // The join owns the field's copy on both paths.
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
        self.builder.jump(join_block, &loaded.values);
        self.layout_scratch = caller_owned;

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        let value = ValueRef {
            values: self.builder.block_params(join_block).to_vec(),
            ty,
        };
        if matches!(slot.kind, SlotKind::Handle) {
            self.mark_temporary_opaque_owned(&value);
        }
        Ok(Some(value))
    }

    /// Writes a static field place of an instance kept in place where it
    /// lies, with the runtime write for any other instance. The value comes
    /// back when generated code cannot reach the place in the block.
    pub(super) fn store_class_place(
        &mut self,
        place: &str,
        value: ValueRef,
    ) -> std::result::Result<Option<ValueRef>, String> {
        let Some((root, slot, ty)) = self.class_place(place)? else {
            return Ok(Some(value));
        };
        let path = place
            .split_once('.')
            .map_or("", |(_, path)| path)
            .to_string();
        let object = self.load_root(&root)?;
        let object = self.ensure_opaque(object)?.values[0];
        let value = self.coerce_value(value, &ty)?;
        let slow_block = self.builder.create_block();
        let fast_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        match &slot.kind {
            SlotKind::Leaves(leaves) => {
                self.emit_class_check(object, slot.access, slow_block, fast_block);
                self.builder.switch_to_block(fast_block);
                self.builder.seal_block(fast_block);
                let block = self.class_block(object);
                for (leaf, word) in leaves.iter().zip(&value.values) {
                    self.store_leaf(*leaf, *word, block);
                }
                self.builder.jump(join_block, &[]);

                self.builder.switch_to_block(slow_block);
                self.builder.seal_block(slow_block);
                let caller_owned = self.layout_scratch.clone();
                let boxed = self.ensure_opaque(value)?;
                let boxed = self.transfer_owned_opaque_value(&boxed);
                self.emit_set_field_owned(object, &path, boxed)?;
                self.release_temporary_owned_since(&caller_owned);
                self.builder.jump(join_block, &[]);
                self.layout_scratch = caller_owned;
            }
            SlotKind::Handle => {
                let boxed = self.ensure_opaque(value)?;
                let boxed = self.transfer_owned_opaque_value(&boxed);
                self.emit_class_check(object, slot.access, slow_block, fast_block);
                self.builder.switch_to_block(fast_block);
                self.builder.seal_block(fast_block);
                let block = self.class_block(object);
                let field_slot = self.builder.iadd_imm(block, i64::from(slot.offset));
                self.builder.call(
                    self.runtime[RuntimeFn::InstancePutField],
                    &[field_slot, boxed],
                );
                self.builder.jump(join_block, &[]);

                self.builder.switch_to_block(slow_block);
                self.builder.seal_block(slow_block);
                self.emit_set_field_owned(object, &path, boxed)?;
                self.builder.jump(join_block, &[]);
            }
        }
        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        Ok(None)
    }

    /// Hands the owned box `value` to field path `path` of `object` through
    /// the runtime.
    pub(super) fn emit_set_field_owned(
        &mut self,
        object: Val,
        path: &str,
        value: Val,
    ) -> std::result::Result<(), String> {
        let (field_ptr, field_len) = self.string_constant(path.as_bytes())?;
        self.builder.call(
            self.runtime[RuntimeFn::InstanceSetFieldOwned],
            &[object, field_ptr, field_len, value],
        );
        Ok(())
    }

    /// Lowers MIR `Lend` of a field box to one runtime operation: an
    /// instance kept in place lends the box in its slot as it is, and any
    /// other instance takes the field out as before. Returns false when the
    /// lend is not one of these.
    pub(super) fn lend_class_field(
        &mut self,
        place: &str,
        temp: &str,
        mutable: bool,
    ) -> std::result::Result<bool, String> {
        if !self.field_lend_temps.contains(temp) {
            return Ok(false);
        }
        let Some((root, slot, _)) = self.class_place(place)? else {
            return Ok(false);
        };
        if !matches!(slot.kind, SlotKind::Handle) {
            return Ok(false);
        }
        let lent = self.declare_word_var();
        let slot_var = self.declare_word_var();
        let temp_var = self.local_vars(temp)?[0];
        self.lent_temps.insert(temp.to_string(), mutable);
        let object = self.load_root(&root)?.values[0];
        let slow_block = self.builder.create_block();
        let fast_block = self.builder.create_block();
        let lent_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.emit_class_check(object, slot.access, slow_block, fast_block);

        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        let block = self.class_block(object);
        let field_slot = self.builder.iadd_imm(block, i64::from(slot.offset));
        let boxed = self
            .builder
            .load(types::I64, MemFlags::trusted(), field_slot, 0);
        self.builder.brif(boxed, lent_block, &[], slow_block, &[]);

        self.builder.switch_to_block(lent_block);
        self.builder.seal_block(lent_block);
        self.builder.def_var(temp_var, boxed);
        let one = self.builder.iconst(types::I64, 1);
        self.builder.def_var(lent, one);
        self.builder.def_var(slot_var, field_slot);
        self.builder.jump(join_block, &[]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let caller_owned = self.layout_scratch.clone();
        let value = self.take_lent_place(place)?;
        self.mark_emptied_store_root(temp);
        self.store_root(temp, value)?;
        self.release_temporary_owned_since(&caller_owned);
        let zero = self.builder.iconst(types::I64, 0);
        self.builder.def_var(lent, zero);
        self.builder.def_var(slot_var, zero);
        self.builder.jump(join_block, &[]);
        self.layout_scratch = caller_owned;

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        self.field_lends.insert(
            temp.to_string(),
            FieldLend {
                lent,
                slot: slot_var,
            },
        );
        Ok(true)
    }

    /// Lowers MIR `EndLend` of a field lend [`Self::lend_class_field`]
    /// lowered. A box lent as it is stays in its slot when the slot still
    /// holds it and nothing else does; otherwise the runtime puts the
    /// temporary's box back. A field taken out is stored back as before.
    pub(super) fn end_class_field_lend(
        &mut self,
        place: &str,
        temp: &str,
    ) -> std::result::Result<bool, String> {
        let Some(lend) = self.field_lends.remove(temp) else {
            return Ok(false);
        };
        let ownership = ownership_layout();
        let temp_var = self.local_vars(temp)?[0];
        let lent_block = self.builder.create_block();
        let restore_block = self.builder.create_block();
        let slow_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        let lent = self.builder.use_var(lend.lent);
        self.builder.brif(lent, lent_block, &[], slow_block, &[]);

        self.builder.switch_to_block(lent_block);
        self.builder.seal_block(lent_block);
        let flags = MemFlags::trusted();
        let boxed = self.builder.use_var(temp_var);
        let field_slot = self.builder.use_var(lend.slot);
        let held = self.builder.load(types::I64, flags, field_slot, 0);
        let kept = self.builder.icmp(IntCC::Equal, held, boxed);
        let count = self
            .builder
            .uload32(flags, boxed, ownership.ref_count_offset);
        let alone = self.builder.icmp_imm(IntCC::Equal, count, 1);
        let unchanged = self.builder.band(kept, alone);
        // The temporary never owned the box.
        let zero = self.builder.iconst(types::I64, 0);
        self.builder.def_var(temp_var, zero);
        self.builder
            .brif(unchanged, join_block, &[], restore_block, &[]);

        self.builder.switch_to_block(restore_block);
        self.builder.seal_block(restore_block);
        self.builder.call(
            self.runtime[RuntimeFn::InstanceRestoreField],
            &[field_slot, boxed],
        );
        self.builder.jump(join_block, &[]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let caller_owned = self.layout_scratch.clone();
        let value = self.take_place(temp)?;
        self.store_place(place, value)?;
        self.release_temporary_owned_since(&caller_owned);
        self.builder.jump(join_block, &[]);
        self.layout_scratch = caller_owned;

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        Ok(true)
    }

    /// Whether the receiver of the runtime operation being compiled is a
    /// temporary that holds a field's own box, which generated code never
    /// appends to or pops from in place: a field box never runs its list
    /// view ahead of its list. The operation sees no receiver place, since
    /// it changes a lent receiver in place.
    pub(super) fn holds_field_box(&self) -> bool {
        self.field_box_receiver
    }

    /// Builds an instance of `class_name` in its layout: one call makes the
    /// block, then each scalar or plain-record field is stored at its
    /// offset and each box is put into its slot. `None` when the class is
    /// not kept in place or the literal does not name every field once.
    pub(super) fn construct_class_in_place(
        &mut self,
        class_name: &str,
        fields: &[crate::mir::MirFieldInit],
        target_ty: &Type,
        class: &MirClass,
    ) -> std::result::Result<Option<ValueRef>, String> {
        if !matches!(target_ty, Type::Named(name, args) if name == class_name && args.is_empty()) {
            return Ok(None);
        }
        let named_once = fields.len() == class.fields.len()
            && class
                .fields
                .iter()
                .all(|field| fields.iter().filter(|init| init.name == field.name).count() == 1);
        if !named_once {
            return Ok(None);
        }
        let Some(record) = self
            .plain_records
            .class_record(class_name, self.builder.module())?
        else {
            return Ok(None);
        };
        let mut slots = Vec::with_capacity(fields.len());
        for field in fields {
            let field_ty = class
                .fields
                .iter()
                .find(|candidate| candidate.name == field.name)
                .map(|candidate| candidate.ty.clone())
                .expect("every field is named");
            let field_ty = ensure_direct_type(
                &field_ty,
                &self.classes,
                &format!("field `{}` on class `{}`", field.name, class_name),
            )?;
            let slot = record.slot(&[field.name.as_str()], &field_ty);
            slots.push((field, field_ty, slot));
        }
        let cell = self.builder.import_data(record.cell);
        let cell = self.builder.symbol_value(types::I64, cell);
        let init = self
            .builder
            .call(self.runtime[RuntimeFn::InstanceNewTyped], &[cell]);
        let current = self.owned_opaque_result(init.results, target_ty.clone());
        let object = current.values[0];
        // A new instance is confined and keeps its fields in place.
        let block = self.class_block(object);
        for (field, field_ty, slot) in slots {
            let loaded = self.load_operand_for_target(&field.value, &field_ty)?;
            let loaded = self.coerce_value(loaded, &field_ty)?;
            match slot.map(|slot| (slot.offset, slot.kind)) {
                Some((_, SlotKind::Leaves(leaves))) => {
                    for (leaf, word) in leaves.iter().zip(&loaded.values) {
                        self.store_leaf(*leaf, *word, block);
                    }
                }
                Some((offset, SlotKind::Handle)) => {
                    let boxed = self.ensure_opaque(loaded)?;
                    let boxed = self.transfer_owned_opaque_value(&boxed);
                    let field_slot = self.builder.iadd_imm(block, i64::from(offset));
                    self.builder.call(
                        self.runtime[RuntimeFn::InstancePutField],
                        &[field_slot, boxed],
                    );
                }
                None => {
                    let boxed = self.ensure_opaque(loaded)?;
                    let boxed = self.transfer_owned_opaque_value(&boxed);
                    self.emit_set_field_owned(object, &field.name, boxed)?;
                }
            }
        }
        Ok(Some(current))
    }

    /// A new `i64` variable.
    fn declare_word_var(&mut self) -> Var {
        let variable = Var(self.next_variable_index as u32);
        self.next_variable_index += 1;
        self.builder.declare_var(variable, types::I64);
        variable
    }
}
