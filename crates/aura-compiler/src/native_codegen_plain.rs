//! Plain records in their typed layouts (typed-layout step 2c).
//!
//! A plain class, one whose fields own no handle, lives in generated code
//! as words: one per scalar field, with nested plain classes flattened.
//! Where such a value meets the runtime it crosses in its typed layout (see
//! `architecture_docs/24-typed-layouts-design-note.md`). A list of them
//! keeps each value's bytes in place, and generated code loads and stores
//! the fields at their layout offsets. A box is made or read, and a field
//! of a boxed instance is read, with one call that copies the bytes rather
//! than one call per field.
//!
//! Each record type gets a cell, a data object the runtime fills with the
//! registered layout, and a list access code, `FIRST_PLAIN_ACCESS` plus the
//! cell's position. The main wrapper registers every layout under its code
//! before the program's first call, so generated code compares a list's
//! code against a constant.

use aura_core::layout::{
    encode_descriptor, LayoutBuilder, LayoutEnv, LayoutId, LayoutKind, LayoutTable, ScalarLayout,
};
use aura_runtime::native_runtime::{
    FIRST_CLASS_ACCESS, FIRST_PLAIN_ACCESS, PLAIN_LAYOUT_CELL_ACCESS_OFFSET,
    PLAIN_LAYOUT_CELL_BYTES, UNREGISTERED_PLAIN_ACCESS,
};

use super::*;

/// One scalar of a record: its byte offset in the layout and its kind, in
/// the order of the record's words.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PlainLeaf {
    pub(super) offset: i32,
    pub(super) kind: ScalarKind,
}

/// The typed layout of a plain class whose fields are all scalars or
/// nested plain classes, as generated code reads and writes it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PlainRecord {
    cell: DataHandle,
    /// The list access code of a list that keeps these values in place.
    access: i64,
    /// One leaf per word of the record's direct type.
    leaves: Vec<PlainLeaf>,
    /// The layout's size in bytes.
    size: u32,
    /// The bytes one element takes in a list: the size rounded up to whole
    /// words.
    stride: i64,
    /// The byte offsets of the stride's words that no 8-byte leaf fills,
    /// which a store into a list zeroes first.
    partial_words: Vec<i32>,
}

/// The records a module's code uses, by class name, and their cells.
pub(super) struct PlainRecords {
    pub(super) env: LayoutEnv,
    by_class: HashMap<String, Option<PlainRecord>>,
    /// The class layouts the module's code uses, by class name (see
    /// `native_codegen_classes.rs`).
    pub(super) classes: HashMap<String, Option<super::classes::ClassRecord>>,
    /// The enum layouts the module's code uses, by the type's descriptor
    /// (see `native_codegen_enums.rs`).
    pub(super) enums: HashMap<String, Option<super::enums::EnumRecord>>,
    /// The plain records made so far, which number their access codes.
    plain_count: u32,
    /// Each cell with its type's descriptor and access code, in the order
    /// they were made.
    cells: Vec<(DataHandle, String, u64)>,
}

impl PlainRecords {
    pub(super) fn new(module: &MirModule) -> Self {
        Self {
            env: crate::mir::layout::layout_env(module),
            by_class: HashMap::new(),
            classes: HashMap::new(),
            enums: HashMap::new(),
            plain_count: 0,
            cells: Vec::new(),
        }
    }

    /// The cells the main wrapper registers, with their descriptors and
    /// access codes.
    pub(super) fn cells(&self) -> &[(DataHandle, String, u64)] {
        &self.cells
    }

    /// Declares a cell for the layout of `ty`, which the main wrapper
    /// registers under `access`. The access code reads as unregistered
    /// until the runtime fills the cell.
    pub(super) fn declare_cell(
        &mut self,
        ty: &Type,
        access: u64,
        object: &mut dyn ModuleBackend,
    ) -> std::result::Result<DataHandle, String> {
        let descriptor = match encode_descriptor(ty, &self.env) {
            Ok(descriptor) => descriptor,
            Err(error) => return Err(error.0),
        };
        self.declare_cell_for(descriptor, access, object)
    }

    /// Declares a cell for the layout `descriptor` describes, which the main
    /// wrapper registers under `access`.
    pub(super) fn declare_cell_for(
        &mut self,
        descriptor: String,
        access: u64,
        object: &mut dyn ModuleBackend,
    ) -> std::result::Result<DataHandle, String> {
        let index = self.cells.len();
        let cell = try_or_string_error!(
            object.declare_data(&format!("aura_plain_layout_{index}"), Linkage::Local, true),
            "failed to declare a plain layout cell: {}"
        );
        let mut bytes = vec![0u8; PLAIN_LAYOUT_CELL_BYTES];
        let at = PLAIN_LAYOUT_CELL_ACCESS_OFFSET as usize;
        bytes[at..at + 4].copy_from_slice(&UNREGISTERED_PLAIN_ACCESS.to_le_bytes());
        let mut data = DataDescription::new(bytes);
        data.align = Some(8);
        try_or_string_error!(
            object.define_data(cell, data),
            "failed to define a plain layout cell: {}"
        );
        self.cells.push((cell, descriptor, access));
        Ok(cell)
    }

    /// The record of `class`, made on first use. A class with a union
    /// field, or one whose layout the runtime cannot keep in place, has
    /// none and keeps its boxed paths.
    fn record(
        &mut self,
        class: &PlainClassType,
        object: &mut dyn ModuleBackend,
    ) -> std::result::Result<Option<PlainRecord>, String> {
        if let Some(record) = self.by_class.get(&class.class_name) {
            return Ok(record.clone());
        }
        let record = match self.layout_of(class) {
            Some((leaves, size)) => Some(self.declare_record(class, leaves, size, object)?),
            None => None,
        };
        self.by_class
            .insert(class.class_name.clone(), record.clone());
        Ok(record)
    }

    /// The leaves and size of `class`'s layout, when it is one generated
    /// code reads in place. A layout over 4 GiB keeps the boxed paths.
    fn layout_of(&self, class: &PlainClassType) -> Option<(Vec<PlainLeaf>, u32)> {
        let ty = Type::named(&class.class_name);
        let mut builder = LayoutBuilder::new(&self.env);
        let id = builder.layout(&ty).ok()?;
        let table = builder.into_table();
        let layout = table.get(id);
        if table.needs_drop(id) || layout.size == 0 || layout.align > 8 {
            return None;
        }
        let mut leaves = Vec::new();
        collect_leaves(
            &table,
            id,
            &DirectType::PlainClass(class.clone()),
            0,
            &mut leaves,
        )
        .then_some((leaves, u32::try_from(layout.size).ok()?))
    }

    fn declare_record(
        &mut self,
        class: &PlainClassType,
        leaves: Vec<PlainLeaf>,
        size: u32,
        object: &mut dyn ModuleBackend,
    ) -> std::result::Result<PlainRecord, String> {
        let access = i64::from(FIRST_PLAIN_ACCESS) + i64::from(self.plain_count);
        if access >= i64::from(FIRST_CLASS_ACCESS) {
            return Err("direct backend made too many plain layouts".to_string());
        }
        self.plain_count += 1;
        let cell = self.declare_cell(&Type::named(&class.class_name), access as u64, object)?;
        let stride = u64::from(size).div_ceil(8) * 8;
        let partial_words = (0..stride)
            .step_by(8)
            .filter(|word| {
                !leaves
                    .iter()
                    .any(|leaf| leaf.offset as u64 == *word && leaf_bytes(leaf.kind) == 8)
            })
            .map(|word| word as i32)
            .collect();
        Ok(PlainRecord {
            cell,
            access,
            leaves,
            size,
            stride: stride as i64,
            partial_words,
        })
    }
}

/// Collects the leaves of `direct`, laid out as `id` at `base`, or returns
/// false when the two do not match leaf for leaf.
pub(super) fn collect_leaves(
    table: &LayoutTable,
    id: LayoutId,
    direct: &DirectType,
    base: u64,
    leaves: &mut Vec<PlainLeaf>,
) -> bool {
    let Ok(offset) = i32::try_from(base) else {
        return false;
    };
    match (direct, &table.get(id).kind) {
        (DirectType::Scalar(ScalarKind::Unit), LayoutKind::Unit) => {
            leaves.push(PlainLeaf {
                offset,
                kind: ScalarKind::Unit,
            });
            true
        }
        (DirectType::Scalar(kind), LayoutKind::Scalar(scalar))
            if scalar_layout(*kind) == Some(*scalar) =>
        {
            leaves.push(PlainLeaf {
                offset,
                kind: *kind,
            });
            true
        }
        (
            DirectType::PlainClass(class),
            LayoutKind::Struct {
                class: Some(_),
                fields,
            },
        ) if fields.len() == class.fields.len() => {
            class.fields.iter().zip(fields).all(|(direct, field)| {
                direct.name == field.name
                    && collect_leaves(table, field.layout, &direct.ty, base + field.offset, leaves)
            })
        }
        _ => false,
    }
}

/// The layout scalar a direct scalar kind is kept as.
fn scalar_layout(kind: ScalarKind) -> Option<ScalarLayout> {
    Some(match kind {
        ScalarKind::Int8 => ScalarLayout::Int8,
        ScalarKind::Int16 => ScalarLayout::Int16,
        ScalarKind::Int32 => ScalarLayout::Int32,
        ScalarKind::Int64 => ScalarLayout::Int64,
        ScalarKind::Uint8 => ScalarLayout::Uint8,
        ScalarKind::Uint16 => ScalarLayout::Uint16,
        ScalarKind::Uint32 => ScalarLayout::Uint32,
        ScalarKind::Uint64 => ScalarLayout::Uint64,
        ScalarKind::Float32 => ScalarLayout::Float32,
        ScalarKind::Float64 => ScalarLayout::Float64,
        ScalarKind::Bool => ScalarLayout::Bool,
        ScalarKind::Unit => return None,
    })
}

/// The bytes a leaf of `kind` takes.
fn leaf_bytes(kind: ScalarKind) -> u64 {
    scalar_layout(kind).map_or(0, ScalarLayout::size)
}

impl FunctionCompiler<'_> {
    /// The record of `ty`, when it is a plain class generated code keeps in
    /// its typed layout.
    pub(super) fn plain_record(
        &mut self,
        ty: &DirectType,
    ) -> std::result::Result<Option<PlainRecord>, String> {
        let DirectType::PlainClass(class) = ty else {
            return Ok(None);
        };
        self.plain_records.record(class, self.builder.module())
    }

    /// The address of `record`'s cell.
    fn plain_cell(&mut self, record: &PlainRecord) -> Val {
        let cell = self.builder.import_data(record.cell);
        self.builder.symbol_value(types::I64, cell)
    }

    /// A stack slot for one value of `record`'s layout, and its address.
    fn plain_slot(&mut self, record: &PlainRecord) -> Val {
        // A whole stride, so the slot can hold a record as a list element.
        let slot = self
            .builder
            .create_sized_stack_slot(record.stride as u32, 3);
        self.builder.stack_addr(types::I64, slot, 0)
    }

    /// The words of a `ty` value whose layout bytes are at `address`.
    fn load_plain_record(
        &mut self,
        record: &PlainRecord,
        address: Val,
        ty: &DirectType,
    ) -> ValueRef {
        let values = record
            .leaves
            .iter()
            .map(|leaf| self.load_leaf(*leaf, address))
            .collect();
        ValueRef {
            values,
            ty: ty.clone(),
        }
    }

    /// The word of the scalar `leaf` names, read from its bytes at
    /// `address` plus its offset.
    pub(super) fn load_leaf(&mut self, leaf: PlainLeaf, address: Val) -> Val {
        let flags = MemFlags::trusted();
        match leaf.kind {
            // Integers narrower than a word widen by their signedness.
            ScalarKind::Int8 => self.builder.sload8(flags, address, leaf.offset),
            ScalarKind::Int16 => self.builder.sload16(flags, address, leaf.offset),
            ScalarKind::Int32 => self.builder.sload32(flags, address, leaf.offset),
            ScalarKind::Uint8 => self.builder.uload8(types::I64, flags, address, leaf.offset),
            ScalarKind::Uint16 => self.builder.uload16(flags, address, leaf.offset),
            ScalarKind::Uint32 => self.builder.uload32(flags, address, leaf.offset),
            ScalarKind::Int64 | ScalarKind::Uint64 => {
                self.builder.load(types::I64, flags, address, leaf.offset)
            }
            ScalarKind::Float64 => self.builder.load(types::F64, flags, address, leaf.offset),
            ScalarKind::Float32 => {
                let narrow = self.builder.load(types::F32, flags, address, leaf.offset);
                self.builder.fpromote(types::F64, narrow)
            }
            ScalarKind::Bool => self.builder.uload8(types::I64, flags, address, leaf.offset),
            ScalarKind::Unit => self.builder.iconst(types::I64, 0),
        }
    }

    /// Stores `word` as the scalar bytes `leaf` names at `address` plus its
    /// offset.
    pub(super) fn store_leaf(&mut self, leaf: PlainLeaf, word: Val, address: Val) {
        let stored = match leaf.kind {
            ScalarKind::Int8 | ScalarKind::Uint8 => self.builder.ireduce(types::I8, word),
            ScalarKind::Int16 | ScalarKind::Uint16 => self.builder.ireduce(types::I16, word),
            ScalarKind::Int32 | ScalarKind::Uint32 => self.builder.ireduce(types::I32, word),
            ScalarKind::Int64 | ScalarKind::Uint64 | ScalarKind::Float64 => word,
            ScalarKind::Float32 => self.builder.fdemote(types::F32, word),
            // A `bool` is one byte that holds 0 or 1.
            ScalarKind::Bool => self.builder.icmp_imm(IntCC::NotEqual, word, 0),
            ScalarKind::Unit => return,
        };
        self.builder
            .store(MemFlags::trusted(), stored, address, leaf.offset);
    }

    /// Stores the words of a record value as its layout bytes at `address`.
    /// For a list element, or a slot the runtime copies into a list word for
    /// word, the words that no leaf fills are zeroed first, so a list never
    /// holds uninitialized padding.
    fn store_plain_record(
        &mut self,
        record: &PlainRecord,
        words: &[Val],
        address: Val,
        element: bool,
    ) {
        let flags = MemFlags::trusted();
        if element && !record.partial_words.is_empty() {
            let zero = self.builder.iconst(types::I64, 0);
            for offset in &record.partial_words {
                self.builder.store(flags, zero, address, *offset);
            }
        }
        for (leaf, word) in record.leaves.iter().zip(words) {
            self.store_leaf(*leaf, *word, address);
        }
    }

    /// Boxes a record value with one call that reads its layout bytes.
    pub(super) fn box_plain_record(
        &mut self,
        record: &PlainRecord,
        value: &ValueRef,
        class_name: &str,
    ) -> ValueRef {
        let slot = self.plain_slot(record);
        self.store_plain_record(record, &value.values, slot, false);
        let cell = self.plain_cell(record);
        let inst = self
            .builder
            .call(self.runtime[RuntimeFn::BoxPlain], &[slot, cell]);
        let boxed = ValueRef {
            values: inst.results,
            ty: DirectType::Opaque(Type::named(class_name)),
        };
        self.mark_temporary_opaque_owned(&boxed);
        boxed
    }

    /// The words of the record a box holds, written by one call.
    pub(super) fn unbox_plain_record(
        &mut self,
        record: &PlainRecord,
        boxed: Val,
        ty: &DirectType,
    ) -> ValueRef {
        let slot = self.plain_slot(record);
        let cell = self.plain_cell(record);
        self.builder
            .call(self.runtime[RuntimeFn::UnboxPlain], &[boxed, slot, cell]);
        self.load_plain_record(record, slot, ty)
    }

    /// The words of record field `field` of the boxed value `object`,
    /// written by one call with no clone and no box.
    pub(super) fn read_plain_field(
        &mut self,
        record: &PlainRecord,
        object: Val,
        field: &str,
        ty: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        let (field_ptr, field_len) = self.string_constant(field.as_bytes())?;
        let slot = self.plain_slot(record);
        let cell = self.plain_cell(record);
        self.builder.call(
            self.runtime[RuntimeFn::InstanceReadPlain],
            &[object, field_ptr, field_len, slot, cell],
        );
        Ok(self.load_plain_record(record, slot, ty))
    }

    /// Branches to `fast_block` when `list` keeps `record`'s values in
    /// place, which also means the list is confined to this thread, and to
    /// `slow_block` otherwise.
    fn emit_plain_list_check(
        &mut self,
        list: Val,
        record: &PlainRecord,
        layout: &aura_runtime::native_runtime::DirectListLayout,
        slow_block: Blk,
        fast_block: Blk,
    ) {
        let actual = self.emit_list_access(list, layout);
        let matches = self.builder.icmp_imm(IntCC::Equal, actual, record.access);
        self.builder.brif(matches, fast_block, &[], slow_block, &[]);
    }

    /// Reads element `index` of a list of records. A confined list that
    /// keeps the records in place is read where the element lies, with one
    /// bounds check, or none when the loop proves the index. Any other list
    /// writes the element into a stack slot with one call, which reports
    /// the list-index diagnostic for an index out of range.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_plain_list_read(
        &mut self,
        record: &PlainRecord,
        list: Val,
        index: Val,
        proven: bool,
        line: Val,
        column: Val,
        ty: &DirectType,
    ) -> ValueRef {
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let slow_block = self.builder.create_block();
        let fast_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder.append_block_param(join_block, types::I64);
        self.emit_plain_list_check(list, record, &layout, slow_block, fast_block);

        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        let elements = self.builder.load(
            types::I64,
            MemFlags::trusted(),
            list,
            layout.elements_offset,
        );
        let position = if proven {
            index
        } else {
            self.emit_list_view_bounds_check(list, index, &layout, slow_block)
        };
        let offset = self.builder.imul_imm(position, record.stride);
        let element = self.builder.iadd(elements, offset);
        self.builder.jump(join_block, &[element]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let slot = self.plain_slot(record);
        let cell = self.plain_cell(record);
        self.builder.call(
            self.runtime[RuntimeFn::VecReadPlain],
            &[list, index, line, column, slot, cell],
        );
        self.builder.jump(join_block, &[slot]);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        let address = self.builder.block_params(join_block)[0];
        self.load_plain_record(record, address, ty)
    }

    /// Appends a record to `list`. A confined list that keeps the records
    /// in place and has room stores the element past the end and raises
    /// the view's length, and the runtime takes the element in at its next
    /// access. Any other list, or a full one, takes one call that reads the
    /// record from a stack slot.
    pub(super) fn emit_plain_list_push(
        &mut self,
        record: &PlainRecord,
        list: Val,
        value: &ValueRef,
    ) {
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let slow_block = self.builder.create_block();
        let fast_block = self.builder.create_block();
        let room_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.emit_plain_list_check(list, record, &layout, slow_block, fast_block);

        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        let flags = MemFlags::trusted();
        let len = self
            .builder
            .load(types::I64, flags, list, layout.len_offset);
        let capacity = self.builder.uload32(flags, list, layout.capacity_offset);
        let full = self
            .builder
            .icmp(IntCC::UnsignedGreaterThanOrEqual, len, capacity);
        self.builder.brif(full, slow_block, &[], room_block, &[]);

        self.builder.switch_to_block(room_block);
        self.builder.seal_block(room_block);
        let elements = self
            .builder
            .load(types::I64, flags, list, layout.elements_offset);
        let offset = self.builder.imul_imm(len, record.stride);
        let element = self.builder.iadd(elements, offset);
        self.store_plain_record(record, &value.values, element, true);
        let grown = self.builder.iadd_imm(len, 1);
        self.builder.store(flags, grown, list, layout.len_offset);
        self.builder.jump(join_block, &[]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let slot = self.plain_slot(record);
        self.store_plain_record(record, &value.values, slot, true);
        let cell = self.plain_cell(record);
        self.builder
            .call(self.runtime[RuntimeFn::VecPushPlain], &[list, slot, cell]);
        self.builder.jump(join_block, &[]);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
    }

    /// Replaces element `index` of a list of records. A confined list that
    /// keeps the records in place and shares its storage with no other list
    /// is written where the element lies. Any other list, or an index out
    /// of range, takes one call, which reports the list-index diagnostic.
    pub(super) fn emit_plain_list_set(
        &mut self,
        record: &PlainRecord,
        list: Val,
        index: Val,
        value: &ValueRef,
        (line, column): (Val, Val),
    ) {
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let slow_block = self.builder.create_block();
        let fast_block = self.builder.create_block();
        let unshared_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.emit_plain_list_check(list, record, &layout, slow_block, fast_block);

        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        let flags = MemFlags::trusted();
        // A view's room is 0 while another list shares the storage.
        let capacity = self.builder.uload32(flags, list, layout.capacity_offset);
        let shared = self.builder.icmp_imm(IntCC::Equal, capacity, 0);
        self.builder
            .brif(shared, slow_block, &[], unshared_block, &[]);

        self.builder.switch_to_block(unshared_block);
        self.builder.seal_block(unshared_block);
        let elements = self
            .builder
            .load(types::I64, flags, list, layout.elements_offset);
        let position = self.emit_list_view_bounds_check(list, index, &layout, slow_block);
        let offset = self.builder.imul_imm(position, record.stride);
        let element = self.builder.iadd(elements, offset);
        self.store_plain_record(record, &value.values, element, true);
        self.builder.jump(join_block, &[]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let slot = self.plain_slot(record);
        self.store_plain_record(record, &value.values, slot, true);
        let cell = self.plain_cell(record);
        self.builder.call(
            self.runtime[RuntimeFn::VecSetPlain],
            &[list, index, line, column, slot, cell],
        );
        self.builder.jump(join_block, &[]);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
    }
}

/// Declares a table the main wrapper registers, under the symbol `name`:
/// one entry per cell, with the cell's address, its descriptor's address
/// and length, and its access code, as
/// `aura_runtime::native_runtime::DirectPlainLayoutEntry` lays them out.
pub(super) fn declare_plain_layout_table(
    object: &mut dyn ModuleBackend,
    string_data: &mut HashMap<Vec<u8>, DataHandle>,
    name: &str,
    cells: &[&(DataHandle, String, u64)],
) -> std::result::Result<DataHandle, String> {
    use aura_runtime::native_runtime::DirectPlainLayoutEntry as Entry;
    const ENTRY_BYTES: usize = std::mem::size_of::<Entry>();
    let mut entries = Vec::new();
    for (cell, descriptor, access) in cells {
        let text = string_data_id(object, string_data, descriptor.as_bytes())?;
        entries.push((*cell, text, descriptor.len(), *access));
    }
    let table = try_or_string_error!(
        object.declare_data(name, Linkage::Local, false),
        "failed to declare a layout table: {}"
    );
    let mut bytes = vec![0u8; ENTRY_BYTES * entries.len().max(1)];
    for (index, (_, _, len, access)) in entries.iter().enumerate() {
        let entry = ENTRY_BYTES * index;
        let at = entry + std::mem::offset_of!(Entry, len);
        bytes[at..at + 8].copy_from_slice(&(*len as u64).to_le_bytes());
        let at = entry + std::mem::offset_of!(Entry, access);
        bytes[at..at + 8].copy_from_slice(&access.to_le_bytes());
    }
    let mut data = DataDescription::new(bytes);
    data.align = Some(8);
    for (index, (cell, text, _, _)) in entries.iter().enumerate() {
        let entry = ENTRY_BYTES * index;
        data.write_data_addr((entry + std::mem::offset_of!(Entry, cell)) as u32, *cell);
        data.write_data_addr(
            (entry + std::mem::offset_of!(Entry, descriptor)) as u32,
            *text,
        );
    }
    try_or_string_error!(
        object.define_data(table, data),
        "failed to define a layout table: {}"
    );
    Ok(table)
}
