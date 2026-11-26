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

use aura_core::layout::{encode_layouts, LayoutBuilder, LayoutId, LayoutKind, LayoutTable};
use aura_runtime::native_runtime::{FIRST_CLASS_ACCESS, FIRST_INSTANCE_LIST_ACCESS};

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
    pub(super) access: i64,
    pub(super) offset: i32,
    pub(super) kind: SlotKind,
}

#[derive(Clone, Debug)]
pub(super) enum SlotKind {
    /// Scalars, one per word of the place's direct type.
    Leaves(Vec<PlainLeaf>),
    /// One box, null while the field is moved out.
    Handle,
}

/// A field box lent as it is: the variable that is 1 while the temporary
/// holds the field's own box, and the one that holds the slot's address.
#[derive(Clone, Copy, Debug)]
pub(super) struct FieldLend {
    pub(super) lent: Var,
    pub(super) slot: Var,
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
                // The descriptor of the layout itself, which a field held in
                // a box makes differ from the type's own.
                let descriptor = encode_layouts(&table, root);
                let cell = self.declare_cell_for(descriptor, access, object)?;
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

    pub(super) fn class_layout(&self, class_name: &str) -> Option<(LayoutTable, LayoutId)> {
        let class = self.env.classes.get(class_name)?;
        if !class.type_params.is_empty() {
            return None;
        }
        let ty = Type::named(class_name);
        let mut builder = LayoutBuilder::new(&self.env);
        let root = builder.layout(&ty).ok()?;
        let table = builder.into_table();
        let in_place = matches!(
            table.get(root).kind,
            LayoutKind::Struct { class: Some(_), .. }
        ) && !table.holds_block(root);
        if !in_place {
            return None;
        }
        if self.copy_classes.contains(class_name) {
            return Some((table, root));
        }
        // An enum or union field lies in a box of its own, which a read
        // copies as generated code holds it, instead of a value built from
        // the field's bytes.
        let mut builder = LayoutBuilder::new(&self.env);
        let root = builder
            .boxed_class_layout(&ty, |field| self.field_lies_inline(field))
            .ok()??;
        Some((builder.into_table(), root))
    }

    /// Whether a list may keep the instances of `class_name` in place, one
    /// block's bytes after another, as the runtime's
    /// `ClassLayout::keeps_in_list` decides for the same layout: every field
    /// is a scalar, a `str` in a box, or a class or tuple of such fields.
    pub(super) fn keeps_in_list(&self, class_name: &str) -> bool {
        let Some((table, root)) = self.class_layout(class_name) else {
            return false;
        };
        let layout = table.get(root);
        layout.size > 0 && layout.align <= 8 && fits_list(&table, root)
    }

    /// Whether a field of type `ty` of a class that is not Copy lies in the
    /// instance's block in its own layout rather than in a box. A class
    /// field stays inline, where generated code reads its scalars.
    fn field_lies_inline(&self, ty: &Type) -> bool {
        match ty {
            Type::Union(_) => false,
            Type::Named(name, _) => !self.env.enums.contains_key(name),
            _ => true,
        }
    }
}

/// Whether a list may keep values of `id` in place (see
/// [`PlainRecords::keeps_in_list`]).
fn fits_list(table: &LayoutTable, id: LayoutId) -> bool {
    let layout = table.get(id);
    match &layout.kind {
        LayoutKind::Unit | LayoutKind::Scalar(_) => true,
        LayoutKind::Handle => {
            matches!(&layout.ty, Type::Named(name, args) if name == "str" && args.is_empty())
        }
        LayoutKind::Struct { fields, .. } => {
            fields.iter().all(|field| fits_list(table, field.layout))
        }
        LayoutKind::Indirect(_) | LayoutKind::Tagged { .. } => false,
    }
}

/// The temporaries of `function` that one `Construct` of a class fills and
/// the next instruction appends to a list by moving them, and that nothing
/// else names: `list.append(Class(...))`. Generated code may build such an
/// instance straight into the list (see `push_constructed_instance`).
pub(super) fn instance_append_temps(function: &MirFunction) -> HashSet<String> {
    use crate::mir::{instruction_place_mentions, terminator_place_mentions};
    let mut mentions = HashMap::<&str, usize>::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            let mut places = Vec::new();
            instruction_place_mentions(instruction, &mut places);
            if let Instruction::Assign { target, .. } = instruction {
                places.push(target);
            }
            for place in places {
                *mentions.entry(place_root(place)).or_default() += 1;
            }
        }
        for place in terminator_place_mentions(&block.terminator) {
            *mentions.entry(place_root(place)).or_default() += 1;
        }
    }
    let mut temps = HashSet::new();
    for block in &function.blocks {
        for pair in block.instructions.windows(2) {
            let [Instruction::Assign {
                target: temp,
                value: Rvalue::Construct { .. },
            }, Instruction::Assign {
                value:
                    Rvalue::Call {
                        callee:
                            CallTarget::Runtime {
                                op:
                                    crate::runtime_ops::RuntimeOp::List(
                                        crate::runtime_ops::ListOp::Push,
                                    ),
                                receiver: Some(Operand::Place(_)),
                                ..
                            },
                        args,
                        ..
                    },
                ..
            }] = pair
            else {
                continue;
            };
            let moved = matches!(
                args.as_slice(),
                [MirArg {
                    value: Operand::MovePlace(moved),
                    writeback_place: None,
                    ..
                }] if moved == temp
            );
            if moved && !temp.contains('.') && mentions.get(temp.as_str()) == Some(&2) {
                temps.insert(temp.clone());
            }
        }
    }
    temps
}

/// The locals of `function` that hold an element of a list of instances
/// of a class a list keeps in place, which generated code may read where
/// the list keeps the element (see `emit_list_lend`). Each candidate,
/// with its class, is one `lents` reads from a list, and `scalar_field`
/// says whether a field of its class is a scalar, and `text_field`
/// whether it is a `str`. A local qualifies when its only assignment is
/// one read, the only other mention of the bare local is its `Release`,
/// and every other mention reads a scalar field (`local.field`) or clones
/// a `str` field (`local.field.clone()`), which borrows the field's box
/// where it lies, so nothing needs the element's box.
pub(super) fn instance_element_locals(
    function: &MirFunction,
    candidates: &HashMap<String, String>,
    scalar_field: impl Fn(&str, &str) -> bool,
    text_field: impl Fn(&str, &str) -> bool,
) -> HashSet<String> {
    use crate::mir::{instruction_place_mentions, terminator_place_mentions};
    // The clone of a `str` field of a candidate into a temporary.
    let text_clone = |instruction: &Instruction| match instruction {
        Instruction::Assign {
            target,
            value:
                Rvalue::Call {
                    callee:
                        CallTarget::Runtime {
                            op: RuntimeOp::Value(ValueOp::Clone | ValueOp::Copy),
                            receiver: Some(Operand::Place(place)),
                            receiver_place,
                        },
                    args,
                    ..
                },
        } if args.is_empty()
            && !candidates.contains_key(place_root(target))
            && receiver_place.as_deref().is_none_or(|other| other == place) =>
        {
            place.split_once('.').is_some_and(|(root, field)| {
                !field.contains('.')
                    && candidates
                        .get(root)
                        .is_some_and(|class| text_field(class, field))
            })
        }
        _ => false,
    };
    let mut refused = HashSet::<String>::new();
    let mut assigned = HashMap::<String, usize>::new();
    let check = |place: &str, refused: &mut HashSet<String>| {
        let root = place_root(place);
        let Some(class) = candidates.get(root) else {
            return;
        };
        let fine = match place.split_once('.') {
            Some((_, field)) => !field.contains('.') && scalar_field(class, field),
            None => false,
        };
        if !fine {
            refused.insert(root.to_string());
        }
    };
    for block in &function.blocks {
        for instruction in &block.instructions {
            match instruction {
                Instruction::Release { place } if candidates.contains_key(place.as_str()) => {
                    continue
                }
                Instruction::Assign {
                    target,
                    value: Rvalue::Call { callee, args, .. },
                } if candidates.contains_key(target.as_str()) => {
                    *assigned.entry(target.clone()).or_default() += 1;
                    if let CallTarget::Runtime {
                        receiver: Some(Operand::Place(place) | Operand::MovePlace(place)),
                        ..
                    } = callee
                    {
                        check(place, &mut refused);
                    }
                    for arg in args {
                        if let Operand::Place(place) | Operand::MovePlace(place) = &arg.value {
                            check(place, &mut refused);
                        }
                    }
                    continue;
                }
                _ if text_clone(instruction) => continue,
                _ => {}
            }
            let mut places = Vec::new();
            instruction_place_mentions(instruction, &mut places);
            if let Instruction::Assign { target, .. } = instruction {
                places.push(target);
            }
            for place in places {
                check(place, &mut refused);
            }
        }
        for place in terminator_place_mentions(&block.terminator) {
            check(place, &mut refused);
        }
    }
    candidates
        .keys()
        .filter(|local| {
            !refused.contains(local.as_str()) && assigned.get(local.as_str()) == Some(&1)
        })
        .cloned()
        .collect()
}

impl ClassRecord {
    pub(super) fn cell(&self) -> DataHandle {
        self.cell
    }

    pub(super) fn access(&self) -> i64 {
        self.access
    }

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
///
/// The operation may also be a call that passes the temporary to a
/// parameter its callee borrows, when `borrows` says the call only borrows
/// its arguments: the callee reads the field's box where it lies, and can
/// reach the instance through nothing it is passed.
pub(super) fn field_lend_temps(
    function: &MirFunction,
    borrows: impl Fn(&Instruction) -> bool,
) -> HashSet<String> {
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
            let receives = |instruction: &Instruction| {
                receives_lent_field(instruction, temp, root)
                    || (borrows(instruction) && passes_lent_field(instruction, temp, root))
            };
            let calls = body
                .iter()
                .filter(|instruction| receives(instruction))
                .count();
            let rest_stays_away = body.iter().all(|instruction| {
                receives(instruction)
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

/// Whether `instruction` is a call of a function by name that passes the
/// lent temporary `temp` as one argument and names neither it elsewhere
/// nor the instance `root`.
fn passes_lent_field(instruction: &Instruction, temp: &str, root: &str) -> bool {
    let Instruction::Assign {
        target,
        value:
            Rvalue::Call {
                callee: CallTarget::Name(_),
                args,
                ..
            },
    } = instruction
    else {
        return false;
    };
    let passed = args
        .iter()
        .filter(|arg| matches!(&arg.value, Operand::Place(place) if place == temp))
        .count();
    passed == 1
        && !names_root(target, temp, root)
        && args
            .iter()
            .all(|arg| arg.writeback_place.is_none() && !reaches_lent_field(&arg.value, temp, root))
}

/// Whether the argument `value` moves the lent temporary `temp` or names
/// it, or the instance `root`, as any place other than `temp` itself.
fn reaches_lent_field(value: &Operand, temp: &str, root: &str) -> bool {
    match value {
        Operand::Place(place) => place != temp && names_root(place, temp, root),
        Operand::MovePlace(place) => names_root(place, temp, root),
        _ => false,
    }
}

/// The temporaries of `temps` (see [`field_lend_temps`]) whose lend only
/// reads the list it lends: its one operation reads an element scalar or
/// the length. Such an operation neither keeps the box nor puts another in
/// the temporary, so the box comes back to its field as it went.
pub(super) fn read_only_field_lends(
    function: &MirFunction,
    temps: &HashSet<String>,
) -> HashSet<String> {
    let mut read_only = HashSet::new();
    for block in &function.blocks {
        for (index, instruction) in block.instructions.iter().enumerate() {
            let Instruction::Lend { temp, .. } = instruction else {
                continue;
            };
            if !temps.contains(temp) {
                continue;
            }
            let body = block.instructions[index + 1..].iter().take_while(
                |later| !matches!(later, Instruction::EndLend { temp: ended, .. } if ended == temp),
            );
            let reads_only = body.into_iter().all(|instruction| match instruction {
                Instruction::IndexInRange { .. } => true,
                Instruction::Assign {
                    value:
                        Rvalue::Call {
                            callee:
                                CallTarget::Runtime {
                                    op: RuntimeOp::List(op),
                                    receiver: Some(Operand::Place(receiver)),
                                    ..
                                },
                            ..
                        },
                    ..
                } => {
                    receiver == temp
                        && matches!(
                            op,
                            ListOp::IndexScalar(_)
                                | ListOp::IndexScalarUnchecked(_)
                                | ListOp::Len
                                | ListOp::IsEmpty
                        )
                }
                _ => false,
            });
            if reads_only {
                read_only.insert(temp.clone());
            }
        }
    }
    read_only
}

/// Whether `instruction` is a runtime operation on the lent temporary
/// `temp` as its receiver, which only reads the temporary or changes it in
/// place, and which names neither it elsewhere nor the instance `root`.
pub(super) fn receives_lent_field(instruction: &Instruction, temp: &str, root: &str) -> bool {
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
    pub(super) fn class_slot(
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
    pub(super) fn class_place(
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

    /// The root and slot of a static field place of a class kept in place
    /// whose field lies in a box, whatever generated code holds the field
    /// as. A callable field is such a place: its box holds a function value.
    pub(super) fn boxed_class_place(
        &mut self,
        place: &str,
    ) -> std::result::Result<Option<(String, ClassSlot)>, String> {
        let Some((root, path)) = place.split_once('.') else {
            return Ok(None);
        };
        if self.view_places.contains_key(root) {
            return Ok(None);
        }
        let Some(root_ty) = self.variable_types.get(root).cloned() else {
            return Ok(None);
        };
        let segments = path.split('.').collect::<Vec<_>>();
        let boxed = DirectType::Opaque(Type::Unit);
        Ok(self
            .class_slot(&root_ty, &segments, &boxed)?
            .filter(|(_, slot)| matches!(slot.kind, SlotKind::Handle))
            .map(|(_, slot)| (root.to_string(), slot)))
    }

    /// Branches to `fast_block` when the instance box `object` keeps its
    /// fields in the layout `access` names, which also means it is confined
    /// to this thread, and to `slow_block` otherwise.
    pub(super) fn emit_class_check(
        &mut self,
        object: Val,
        access: i64,
        slow_block: Blk,
        fast_block: Blk,
    ) {
        // An instance only this function builds and reaches through its
        // fields keeps its layout (see `confined_instances`).
        if std::mem::take(&mut self.skip_class_check) {
            self.class_check_snapshot = None;
            self.builder.jump(fast_block, &[]);
            return;
        }
        let snapshot = self.class_check_snapshot.take();
        // A box an earlier check found in the layout keeps it until a call
        // that may share it.
        let kept = self.box_keeping_calls();
        let earlier = snapshot.is_none().then(|| {
            self.layout_checks
                .iter()
                .find(|(check, taken, then, checked, _)| {
                    *checked == access
                        && self
                            .builder
                            .holds_since(*check, *taken, *then, object, &kept)
                })
                .map(|check| check.4)
        });
        let matches = match earlier.flatten() {
            Some(matches) => matches,
            None => {
                let layout = aura_runtime::native_runtime::direct_list_layout();
                let actual = self.emit_list_access(object, &layout);
                let matches = self.builder.icmp_imm(IntCC::Equal, actual, access);
                // The copy of a loop that reads a field's list once runs
                // while the instance keeps its layout.
                match snapshot {
                    Some(snapshot) => self.holds_in_copy(snapshot, matches),
                    None => {
                        let check = self.builder.current_block().expect("a check's block");
                        self.layout_checks
                            .insert(0, (check, fast_block, object, access, matches));
                        self.layout_checks.truncate(8);
                        matches
                    }
                }
            }
        };
        // An instance is out of its layout only while it is shared, so
        // the runtime's way lies out of line.
        self.builder.set_cold_block(slow_block);
        self.builder.brif(matches, fast_block, &[], slow_block, &[]);
    }

    /// The address of the block of a checked instance box.
    pub(super) fn class_block(&mut self, object: Val) -> Val {
        let layout = aura_runtime::native_runtime::direct_list_layout();
        self.builder.load(
            types::I64,
            MemFlags::trusted_in(AliasClass::BoxElements),
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
            return self.load_union_class_place(place);
        };
        if let (Some(block_var), SlotKind::Leaves(leaves)) =
            (self.instance_element_blocks.get(&root).copied(), &slot.kind)
        {
            // An element a list keeps in place is read where it lies.
            let leaves = leaves.clone();
            let element_block = self.builder.create_block();
            let boxed_block = self.builder.create_block();
            let join_block = self.builder.create_block();
            for abi in ty.abi_types() {
                self.builder.append_block_param(join_block, abi);
            }
            let address = self.builder.use_var(block_var);
            self.builder
                .brif(address, element_block, &[], boxed_block, &[]);
            self.builder.switch_to_block(element_block);
            self.builder.seal_block(element_block);
            let values = leaves
                .iter()
                .map(|leaf| self.load_leaf(*leaf, address))
                .collect::<Vec<_>>();
            self.builder.jump(join_block, &values);
            self.builder.switch_to_block(boxed_block);
            self.builder.seal_block(boxed_block);
            let loaded = self.load_boxed_class_place(place, &root, &slot, ty.clone())?;
            self.builder.jump(join_block, &loaded.values);
            self.builder.switch_to_block(join_block);
            self.builder.seal_block(join_block);
            let values = self.builder.block_params(join_block).to_vec();
            return Ok(Some(ValueRef { values, ty }));
        }
        if let (Some(block_var), SlotKind::Handle) =
            (self.instance_element_blocks.get(&root).copied(), &slot.kind)
        {
            // A `str` field of an element a list keeps in place is the box
            // the list holds at its offset, which the clone that reads it
            // borrows (see `instance_element_locals`); a box the element's
            // read lent takes the instance's way.
            if !(self.borrow_field_reads && self.field_read_copies.len() > self.field_read_count) {
                return Err(format!(
                    "direct backend copies `{place}` out of a kept element"
                ));
            }
            let element_block = self.builder.create_block();
            let boxed_block = self.builder.create_block();
            let join_block = self.builder.create_block();
            self.builder.append_block_param(join_block, types::I64);
            let address = self.builder.use_var(block_var);
            self.builder
                .brif(address, element_block, &[], boxed_block, &[]);
            self.builder.switch_to_block(element_block);
            self.builder.seal_block(element_block);
            let boxed = self
                .builder
                .load(types::I64, MemFlags::trusted(), address, slot.offset);
            self.builder.jump(join_block, &[boxed]);
            self.builder.switch_to_block(boxed_block);
            self.builder.seal_block(boxed_block);
            let loaded = self.load_boxed_class_place(place, &root, &slot, ty.clone())?;
            self.builder.jump(join_block, &loaded.values);
            self.builder.switch_to_block(join_block);
            self.builder.seal_block(join_block);
            let values = self.builder.block_params(join_block).to_vec();
            return Ok(Some(ValueRef { values, ty }));
        }
        self.load_boxed_class_place(place, &root, &slot, ty)
            .map(Some)
    }

    /// [`Self::load_class_place`] for a root that holds an instance box.
    fn load_boxed_class_place(
        &mut self,
        place: &str,
        root: &str,
        slot: &ClassSlot,
        ty: DirectType,
    ) -> std::result::Result<ValueRef, String> {
        let object = self.load_root(root)?.values[0];
        let slow_block = self.builder.create_block();
        self.skip_class_check = self.confined_instances.contains(root);
        self.class_check_snapshot = self.instance_snapshot(root);
        let loaded = self.load_class_slot(place, object, slow_block, slot, ty, false);
        self.skip_class_check = false;
        loaded
    }

    /// The list access code of a list that keeps instances of `ty` in
    /// place, and the bytes one takes there, when its class may be kept so.
    pub(super) fn instance_list_slot(
        &mut self,
        ty: &DirectType,
    ) -> std::result::Result<Option<(i64, i64)>, String> {
        let DirectType::Opaque(Type::Named(class_name, args)) = ty else {
            return Ok(None);
        };
        if !args.is_empty() || !self.plain_records.keeps_in_list(class_name) {
            return Ok(None);
        }
        let Some(record) = self
            .plain_records
            .class_record(class_name, self.builder.module())?
        else {
            return Ok(None);
        };
        let size = record.table.get(record.root).size as i64;
        let access =
            i64::from(FIRST_INSTANCE_LIST_ACCESS) + record.access - i64::from(FIRST_CLASS_ACCESS);
        Ok(Some((access, (size + 7) / 8 * 8)))
    }

    /// Whether `instruction` is the `Construct` of one of
    /// `instance_appends` for a list that may keep the class in place, which
    /// then waits for the append that follows (see
    /// [`Self::push_constructed_instance`]).
    pub(super) fn defer_instance_construct(
        &mut self,
        instructions: &[Instruction],
    ) -> std::result::Result<bool, String> {
        let [Instruction::Assign {
            target,
            value: Rvalue::Construct { class_name, fields },
        }, Instruction::Assign {
            value:
                Rvalue::Call {
                    callee:
                        CallTarget::Runtime {
                            receiver: Some(Operand::Place(list)),
                            ..
                        },
                    ..
                },
            ..
        }, ..] = instructions
        else {
            return Ok(false);
        };
        if !self.instance_appends.contains(target) {
            return Ok(false);
        }
        let element = match self.type_of_place(list)? {
            DirectType::Opaque(Type::Named(name, args)) if name == "list" && args.len() == 1 => {
                args[0].clone()
            }
            _ => return Ok(false),
        };
        let Some(class) = self.classes.get(class_name).cloned() else {
            return Ok(false);
        };
        let named_once = fields.len() == class.fields.len()
            && class
                .fields
                .iter()
                .all(|field| fields.iter().filter(|init| init.name == field.name).count() == 1);
        let built = Type::named(class_name);
        if element != built
            || !named_once
            || !matches!(self.type_of_place(target)?, DirectType::Opaque(ty) if ty == built)
            || sync::handle_class_field(&DirectType::Opaque(built.clone()), &self.classes).is_some()
            || self
                .instance_list_slot(&DirectType::Opaque(built))?
                .is_none()
        {
            return Ok(false);
        }
        self.deferred_instance = Some((target.clone(), class_name.clone(), fields.clone()));
        Ok(true)
    }

    /// Appends the instance a deferred `Construct` describes to `list`:
    /// the fields go into a buffer laid out as the instance's block, each
    /// box as the reference the instance would own, and one call hands the
    /// buffer to the list (see `aura_direct_vec_push_instance`). `false`
    /// when `temp` is not the deferred instance's temporary.
    pub(super) fn push_constructed_instance(
        &mut self,
        list: Val,
        temp: &str,
    ) -> std::result::Result<bool, String> {
        let Some((deferred, class_name, fields)) = self.deferred_instance.take() else {
            return Ok(false);
        };
        if deferred != temp {
            self.deferred_instance = Some((deferred, class_name, fields));
            return Ok(false);
        }
        let class = self
            .classes
            .get(&class_name)
            .cloned()
            .ok_or(format!("direct backend does not know class `{class_name}`"))?;
        let record = self
            .plain_records
            .class_record(&class_name, self.builder.module())?
            .ok_or_else(|| format!("direct backend has no layout for class `{class_name}`"))?;
        let size = record.table.get(record.root).size;
        let words = size.div_ceil(8) as i32;
        let slot = self.builder.create_sized_stack_slot((words * 8) as u32, 3);
        let buffer = self.builder.stack_addr(types::I64, slot, 0);
        let zero = self.builder.iconst(types::I64, 0);
        for word in 0..words {
            self.builder
                .store(MemFlags::trusted(), zero, buffer, word * 8);
        }
        for field in &fields {
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
            let slot = record
                .slot(&[field.name.as_str()], &field_ty)
                .ok_or_else(|| {
                    format!(
                        "direct backend cannot place field `{}` of class `{class_name}`",
                        field.name
                    )
                })?;
            let loaded = self.load_operand_for_target(&field.value, &field_ty)?;
            let loaded = self.coerce_value(loaded, &field_ty)?;
            match slot.kind {
                SlotKind::Leaves(leaves) => {
                    for (leaf, word) in leaves.iter().zip(&loaded.values) {
                        self.store_leaf(*leaf, *word, buffer);
                    }
                }
                SlotKind::Handle => {
                    let boxed = self.ensure_opaque(loaded)?;
                    let boxed = self.transfer_owned_opaque_value(&boxed);
                    self.builder
                        .store(MemFlags::trusted(), boxed, buffer, slot.offset);
                }
            }
        }
        let cell = self.builder.import_data(record.cell);
        let cell = self.builder.symbol_value(types::I64, cell);
        self.builder.call(
            self.runtime[RuntimeFn::VecPushInstance],
            &[list, cell, buffer],
        );
        Ok(true)
    }

    /// Reads `root.__union_payload_N.field...` where it lies when member
    /// `N` of the union `root` holds is an instance of a class kept in
    /// place: the member's payload word is the instance box, which the
    /// class check reads as any other. `None` for any other place.
    fn load_union_class_place(
        &mut self,
        place: &str,
    ) -> std::result::Result<Option<ValueRef>, String> {
        let mut segments = place.split('.');
        let (Some(root), Some(member), Some(_)) =
            (segments.next(), segments.next(), segments.clone().next())
        else {
            return Ok(None);
        };
        if self.view_places.contains_key(root) {
            return Ok(None);
        }
        let Some(DirectType::Union(union)) = self.variable_types.get(root).cloned() else {
            return Ok(None);
        };
        let Some(member_ty) = crate::mir::union_payload_projection_index(member)
            .and_then(|index| union.members.get(index))
            .map(|member| member.ty.clone())
        else {
            return Ok(None);
        };
        let Ok(ty) = self.type_of_place(place) else {
            return Ok(None);
        };
        let path = segments.collect::<Vec<_>>();
        let Some((_, slot)) = self.class_slot(&member_ty, &path, &ty)? else {
            return Ok(None);
        };
        // The payload words follow the tag; an instance is one box.
        let object = self.load_root(root)?.values[1];
        let slow_block = self.builder.create_block();
        self.load_class_slot(place, object, slow_block, &slot, ty, false)
            .map(Some)
    }

    /// Reads the field at `slot` of the instance box `object` where it lies
    /// when the box keeps its fields in place, and `place` the slow way,
    /// branching to `slow_block`, otherwise. The slow way is the view walk
    /// when `through_view`, and the runtime read of a static place when not.
    /// Other code may already branch to `slow_block`.
    pub(super) fn load_class_slot(
        &mut self,
        place: &str,
        object: Val,
        slow_block: Blk,
        slot: &ClassSlot,
        ty: DirectType,
        through_view: bool,
    ) -> std::result::Result<ValueRef, String> {
        let fast_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        for abi in ty.abi_types() {
            self.builder.append_block_param(join_block, abi);
        }
        // A borrowed read also passes the copy the slow path made, or null.
        let borrowed = if matches!(slot.kind, SlotKind::Handle) {
            self.next_field_read_copy()
        } else {
            None
        };
        if borrowed.is_some() {
            self.builder.append_block_param(join_block, types::I64);
        }
        self.emit_class_check(object, slot.access, slow_block, fast_block);

        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        let block = self.class_block(object);
        let fields = MemFlags::trusted_in(AliasClass::ClassFields);
        let values = match &slot.kind {
            SlotKind::Leaves(leaves) => leaves
                .iter()
                .map(|leaf| self.load_leaf_in(*leaf, block, fields))
                .collect::<Vec<_>>(),
            SlotKind::Handle => {
                let boxed = self.builder.load(types::I64, fields, block, slot.offset);
                // A moved-out field takes the runtime read, which reports it.
                let present_block = self.builder.create_block();
                self.builder
                    .brif(boxed, present_block, &[], slow_block, &[]);
                self.builder.switch_to_block(present_block);
                self.builder.seal_block(present_block);
                if borrowed.is_some() {
                    let null = self.builder.iconst(types::I64, 0);
                    vec![boxed, null]
                } else {
                    let copy = self
                        .builder
                        .call(self.runtime[RuntimeFn::InstanceCopyField], &[boxed]);
                    vec![copy.results[0]]
                }
            }
        };
        self.builder.jump(join_block, &values);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let caller_owned = self.layout_scratch.clone();
        let loaded = if through_view {
            self.load_view_place(place)?
        } else {
            self.load_static_place_slowly(place)?
        };
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
        let mut values = loaded.values;
        if borrowed.is_some() {
            values.push(values[0]);
        }
        self.builder.jump(join_block, &values);
        self.layout_scratch = caller_owned;

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        let mut values = self.builder.block_params(join_block).to_vec();
        if let Some(variable) = borrowed {
            let copy = values.pop().expect("a borrowed read passes its copy");
            self.builder.def_var(variable, copy);
            return Ok(ValueRef { values, ty });
        }
        let value = ValueRef { values, ty };
        if matches!(slot.kind, SlotKind::Handle) {
            self.mark_temporary_opaque_owned(&value);
        }
        Ok(value)
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
                self.skip_class_check = self.confined_instances.contains(&root);
                self.class_check_snapshot = self.instance_snapshot(&root);
                self.emit_class_check(object, slot.access, slow_block, fast_block);
                self.builder.switch_to_block(fast_block);
                self.builder.seal_block(fast_block);
                let block = self.class_block(object);
                let fields = MemFlags::trusted_in(AliasClass::ClassFields);
                for (leaf, word) in leaves.iter().zip(&value.values) {
                    self.store_leaf_in(*leaf, *word, block, fields);
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
                self.skip_class_check = self.confined_instances.contains(&root);
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
        let slow_block;
        let mut snapshot = None;
        let mut instance = None;
        let (object, slot) = if let Some((root, slot, _)) = self.class_place(place)? {
            if !matches!(slot.kind, SlotKind::Handle) {
                return Ok(false);
            }
            slow_block = self.builder.create_block();
            snapshot = self.field_snapshot(place);
            // The copy of a method's body that keeps its receiver knows
            // the receiver's layout too.
            instance = self.instance_snapshot(&root);
            (self.load_root(&root)?.values[0], slot)
        } else if mutable {
            return Ok(false);
        } else {
            match self.native_loan_projection(place)? {
                Some((slot, _)) if matches!(slot.kind, SlotKind::Handle) => {
                    slow_block = self.builder.create_block();
                    let element = self.native_loan_element(place, slow_block)?;
                    (element, slot)
                }
                _ => return Ok(false),
            }
        };
        let lent = self.declare_word_var();
        let slot_var = self.declare_word_var();
        let temp_var = self.local_vars(temp)?[0];
        self.lent_temps.insert(temp.to_string(), mutable);
        let fast_block = self.builder.create_block();
        let lent_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.class_check_snapshot = snapshot.or(instance);
        self.emit_class_check(object, slot.access, slow_block, fast_block);

        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        let block = self.class_block(object);
        let field_slot = self.builder.iadd_imm(block, i64::from(slot.offset));
        let fields = MemFlags::trusted_in(AliasClass::ClassFields);
        let mut boxed = self.builder.load(types::I64, fields, field_slot, 0);
        let mut present = self.builder.icmp_imm(IntCC::NotEqual, boxed, 0);
        if let Some(snapshot) = snapshot {
            // The loop's copy knows the box its entry found in the field.
            let field_box = self.list_snapshots[snapshot]
                .field_box
                .expect("a field's loop keeps its box");
            let kept = self.builder.use_var(field_box);
            boxed = self.keep_in_copy(snapshot, kept, boxed);
            present = self.holds_in_copy(snapshot, present);
        }
        self.builder.brif(present, lent_block, &[], slow_block, &[]);

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
        self.lend_on_runtime_path(place, temp, mutable)?;
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
        mutable: bool,
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
        // The field's box was lent as it is unless the instance is out of
        // its layout, and it comes back as it went unless the operation
        // replaced it.
        self.builder.set_cold_block(slow_block);
        self.builder.set_cold_block(restore_block);
        self.builder.brif(lent, lent_block, &[], slow_block, &[]);

        self.builder.switch_to_block(lent_block);
        self.builder.seal_block(lent_block);
        let flags = MemFlags::trusted();
        let boxed = self.builder.use_var(temp_var);
        let field_slot = self.builder.use_var(lend.slot);
        // A lend that only read the list left the box as it went.
        let unchanged = if self.read_only_field_lends.contains(temp) {
            self.builder.iconst(types::I8, 1)
        } else {
            let fields = MemFlags::trusted_in(AliasClass::ClassFields);
            let held = self.builder.load(types::I64, fields, field_slot, 0);
            let kept = self.builder.icmp(IntCC::Equal, held, boxed);
            let count = self
                .builder
                .uload32(flags, boxed, ownership.ref_count_offset);
            let alone = self.builder.icmp_imm(IntCC::Equal, count, 1);
            let mut unchanged = self.builder.band(kept, alone);
            if let Some(snapshot) = self.field_snapshot(place) {
                // The loop's copy only reads the list, which leaves the box
                // in its field with the one holder the loop's entry found.
                unchanged = self.holds_in_copy(snapshot, unchanged);
            }
            unchanged
        };
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
        self.end_lend_on_runtime_path(place, temp, mutable)?;
        self.release_temporary_owned_since(&caller_owned);
        self.builder.jump(join_block, &[]);
        self.layout_scratch = caller_owned;

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        Ok(true)
    }

    /// Whether the receiver of the runtime operation being compiled is a
    /// temporary that holds a field's own box, which generated code never
    /// pops from in place: the runtime takes in what generated code
    /// appended to a field box, but not what it popped. The operation sees
    /// no receiver place, since it changes a lent receiver in place.
    pub(super) fn holds_field_box(&self) -> bool {
        self.field_box_receiver
    }

    /// The layout of a copy-class record's class and the scalars of the
    /// whole record in its block, so a box of the record can hold the
    /// record's bytes. `None` for a record of any other type, or of a class
    /// generated code does not keep in place.
    pub(super) fn record_class_leaves(
        &mut self,
        ty: &DirectType,
    ) -> std::result::Result<Option<(ClassRecord, Vec<PlainLeaf>)>, String> {
        let DirectType::PlainClass(class) = ty else {
            return Ok(None);
        };
        let Some(record) = self
            .plain_records
            .class_record(&class.class_name, self.builder.module())?
        else {
            return Ok(None);
        };
        Ok(match record.slot(&[], ty) {
            Some(ClassSlot {
                kind: SlotKind::Leaves(leaves),
                ..
            }) => Some((record, leaves)),
            _ => None,
        })
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
            // An enum or union field lies in a box of its own, which the
            // literal puts into its slot as it would a box field.
            let slot = record.slot(&[field.name.as_str()], &field_ty).or_else(|| {
                record
                    .slot(&[field.name.as_str()], &DirectType::Opaque(Type::Unit))
                    .filter(|slot| matches!(slot.kind, SlotKind::Handle))
            });
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
    pub(super) fn declare_word_var(&mut self) -> Var {
        let variable = Var(self.next_variable_index as u32);
        self.next_variable_index += 1;
        self.builder.declare_var(variable, types::I64);
        variable
    }
}

/// The class locals of `function` whose every value this function builds
/// in place and reaches only through field places, so each keeps its
/// layout's access code and stays confined to this thread for as long as
/// the local holds it: a field read or write needs no check.
///
/// A local qualifies when each assignment of it moves in a temporary that
/// one `Construct` naming every field of its class built (or is that
/// `Construct`), the only other mention of the bare local is its `Release`,
/// and every other mention is a field place (`local.field`). Nothing then
/// passes, shares, sends, or lends the instance itself, which is what
/// changes an access code.
pub(super) fn confined_instances(
    function: &MirFunction,
    classes: &HashMap<String, MirClass>,
) -> HashSet<String> {
    use crate::mir::{Instruction, Operand, Rvalue};
    let constructs = |value: &Rvalue, ty: Option<&Type>| match value {
        Rvalue::Construct { class_name, fields } => {
            let Some(class) = classes.get(class_name) else {
                return false;
            };
            class.type_params.is_empty()
                && matches!(ty, Some(Type::Named(name, args)) if name == class_name && args.is_empty())
                && fields.len() == class.fields.len()
                && class
                    .fields
                    .iter()
                    .all(|field| fields.iter().filter(|init| init.name == field.name).count() == 1)
        }
        _ => false,
    };
    let types = function
        .local_types
        .iter()
        .map(|local| (local.name.as_str(), &local.ty))
        .collect::<HashMap<_, _>>();
    let mut candidates = function
        .local_types
        .iter()
        .filter(|local| {
            matches!(&local.ty, Type::Named(name, args)
                if args.is_empty() && classes.contains_key(name))
        })
        .map(|local| local.name.clone())
        .collect::<HashSet<_>>();
    candidates.remove("self");
    for param in &function.params {
        candidates.remove(&param.name);
    }
    if candidates.is_empty() {
        return candidates;
    }
    // Every string each instruction and terminator mentions.
    fn strings<'v>(value: &'v serde_json::Value, out: &mut Vec<&'v str>) {
        match value {
            serde_json::Value::String(text) => out.push(text),
            serde_json::Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
            serde_json::Value::Object(map) => map.values().for_each(|item| strings(item, out)),
            _ => {}
        }
    }
    let mut bare = HashMap::<String, usize>::new();
    let mut allowed = HashMap::<String, usize>::new();
    let mut built = HashMap::<String, usize>::new();
    let mut moved_in = Vec::new();
    let mut assigned = HashSet::new();
    let count = |value: serde_json::Value, bare: &mut HashMap<String, usize>| {
        let mut found = Vec::new();
        strings(&value, &mut found);
        for text in found {
            let root = text.split(['.', '[']).next().unwrap_or(text);
            if text == root {
                *bare.entry(root.to_string()).or_default() += 1;
            }
        }
    };
    for block in &function.blocks {
        for instruction in &block.instructions {
            if let Instruction::Assign { target, value } = instruction {
                if constructs(value, types.get(target.as_str()).copied()) {
                    *built.entry(target.clone()).or_default() += 1;
                }
            }
            match instruction {
                Instruction::Assign { target, value } if candidates.contains(target) => {
                    assigned.insert(target.clone());
                    *allowed.entry(target.clone()).or_default() += 1;
                    match value {
                        Rvalue::Use(Operand::MovePlace(temp)) => {
                            moved_in.push((target.clone(), temp.clone()));
                        }
                        _ if constructs(value, types.get(target.as_str()).copied()) => {}
                        _ => {
                            candidates.remove(target);
                        }
                    }
                }
                Instruction::Release { place } => {
                    *allowed.entry(place.clone()).or_default() += 1;
                }
                _ => {}
            }
            let Ok(value) = serde_json::to_value(instruction) else {
                return HashSet::new();
            };
            count(value, &mut bare);
        }
        let Ok(value) = serde_json::to_value(&block.terminator) else {
            return HashSet::new();
        };
        count(value, &mut bare);
    }
    // The moved temporary is built once and mentioned only by its build
    // and the move.
    for (local, temp) in moved_in {
        if built.get(&temp) != Some(&1) || bare.get(&temp) != Some(&2) {
            candidates.remove(&local);
        }
    }
    candidates.retain(|local| {
        assigned.contains(local) && bare.get(local).copied().unwrap_or(0) == allowed[local]
    });
    candidates
}
