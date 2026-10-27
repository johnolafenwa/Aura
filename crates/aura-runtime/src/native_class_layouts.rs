//! Class instances in their typed layouts (typed-layout step 2e).
//!
//! An instance generated code builds keeps its fields in one heap block in
//! the class's layout ([`aura_core::layout`]) rather than in a map from
//! names to values: scalars and plain records at their offsets, nested
//! classes inline, and each `str`, collection, enum, union, or resource in
//! a box of its own that the block owns ([`ElementBox`]). A field box is
//! null while its field is moved out. The instance's box carries the
//! class's access code and the block's address in its list view, so
//! generated code checks one code, then reads and writes fields at their
//! offsets and lends a field's box to a call without moving it out.
//!
//! The rest of the runtime reaches a typed instance through
//! [`crate::runtime_value::InstanceFields`], as it reaches one whose fields
//! are a map.
//!
//! The block is reached only through its instance, under the instance's
//! access, so a field box takes no lock of its own and records no thread.
//! Generated code changes the block and the field boxes only while the
//! instance is confined to its thread, and it appends to or pops from a
//! list in a field box only through the runtime, so the box's list view
//! never runs ahead of the list and a reader on another thread writes
//! nothing.

use std::alloc::{alloc_zeroed, dealloc, handle_alloc_error, Layout as AllocLayout};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
use std::sync::{OnceLock, RwLock};

use aura_core::layout::{LayoutId, LayoutKind, LayoutTable};
use aura_core::types::Type;

use super::typed_layout::{self, TypedLayout};
use super::ElementBox;
use crate::runtime_value::Value;

/// The access code of the first class layout a program registers. Plain
/// layouts take the codes from `FIRST_PLAIN_ACCESS` up to this one, so the
/// two never meet.
pub const FIRST_CLASS_ACCESS: u32 = 0x4000_0000;

/// The layout of one class whose instances keep their fields in place.
pub struct ClassLayout {
    /// The access code of an instance box that keeps its fields in this
    /// layout.
    access: u32,
    layout: TypedLayout,
    class_name: String,
    /// The fields in declaration order.
    fields: Vec<ClassField>,
    /// Indices into `fields`, in name order, which is the order a map of
    /// fields iterates in.
    by_name: Vec<usize>,
    /// The offsets of every field box the layout holds outside an enum or
    /// union payload, nested classes included.
    boxes: Vec<usize>,
    /// How values move in and out of the block. Only registration names
    /// these, so a program that registers no class layout links none of
    /// the converters, however many instances it prints, copies, or drops.
    ops: &'static ClassOps,
}

struct ClassOps {
    fits: fn(&LayoutTable, LayoutId, &Value) -> bool,
    field_box: fn(Value, &Type) -> ElementBox,
    write: unsafe fn(&LayoutTable, LayoutId, Value, *mut u8),
    read: unsafe fn(&LayoutTable, LayoutId, *const u8) -> Value,
    take: unsafe fn(&LayoutTable, LayoutId, *mut u8) -> Value,
    drop: unsafe fn(&LayoutTable, LayoutId, *mut u8),
    clone: unsafe fn(&LayoutTable, LayoutId, *const u8, *mut u8),
}

static CLASS_OPS: ClassOps = ClassOps {
    fits: typed_layout::fits,
    field_box: typed_layout::field_box_for,
    write: typed_layout::write_unchecked,
    read: typed_layout::read_at,
    take: typed_layout::take_at,
    drop: typed_layout::drop_at,
    clone: typed_layout::clone_at,
};

struct ClassField {
    name: String,
    offset: usize,
    layout: LayoutId,
}

impl ClassLayout {
    pub fn access(&self) -> u32 {
        self.access
    }

    pub fn class_name(&self) -> &str {
        &self.class_name
    }

    fn table(&self) -> &LayoutTable {
        self.layout.table()
    }

    fn block_layout(&self) -> AllocLayout {
        AllocLayout::from_size_align(self.layout.size().max(1), self.layout.align())
            .expect("layouts keep sizes and alignments an allocation accepts")
    }

    fn field(&self, name: &str) -> Option<&ClassField> {
        self.by_name
            .binary_search_by(|index| self.fields[*index].name.as_str().cmp(name))
            .ok()
            .map(|position| &self.fields[self.by_name[position]])
    }

    /// The offset and layout `segments` reach through fields, when every
    /// step but the last is a class or tuple held inline.
    fn path(&self, segments: &[&str]) -> Option<(usize, LayoutId)> {
        let (first, rest) = segments.split_first()?;
        let field = self.field(first)?;
        let mut offset = field.offset;
        let mut id = field.layout;
        for segment in rest {
            let LayoutKind::Struct { fields, .. } = &self.table().get(id).kind else {
                return None;
            };
            let nested = fields.iter().find(|field| field.name == *segment)?;
            offset += nested.offset as usize;
            id = nested.layout;
        }
        Some((offset, id))
    }

    /// Whether the layout `id` is one box.
    fn is_box(&self, id: LayoutId) -> bool {
        matches!(self.table().get(id).kind, LayoutKind::Handle)
    }

    fn type_of(&self, id: LayoutId) -> &Type {
        &self.table().get(id).ty
    }
}

impl fmt::Debug for ClassLayout {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClassLayout")
            .field("access", &self.access)
            .field("class_name", &self.class_name)
            .finish_non_exhaustive()
    }
}

/// The offsets of the boxes `id` holds at `base` outside every payload.
fn collect_boxes(table: &LayoutTable, id: LayoutId, base: usize, boxes: &mut Vec<usize>) {
    match &table.get(id).kind {
        LayoutKind::Handle => boxes.push(base),
        LayoutKind::Struct { fields, .. } => {
            for field in fields {
                collect_boxes(table, field.layout, base + field.offset as usize, boxes);
            }
        }
        _ => {}
    }
}

impl ClassLayout {
    fn from_descriptor(descriptor: &str, access: u32) -> Result<Self, String> {
        let layout = TypedLayout::from_descriptor(descriptor)?;
        let table = layout.table();
        let root = table.get(layout.root());
        let LayoutKind::Struct {
            class: Some(class_name),
            fields,
        } = &root.kind
        else {
            return Err(format!("`{}` is not a class laid out in place", root.ty));
        };
        if matches!(&root.ty, Type::Named(_, args) if !args.is_empty()) {
            return Err(format!(
                "generic class `{}` keeps its fields in a map",
                root.ty
            ));
        }
        // A deep recursive value is never copied or freed by recursion.
        if table.holds_block(layout.root()) {
            return Err(format!(
                "recursive class `{}` keeps its fields in a map",
                root.ty
            ));
        }
        let fields = fields
            .iter()
            .map(|field| ClassField {
                name: field.name.clone(),
                offset: field.offset as usize,
                layout: field.layout,
            })
            .collect::<Vec<_>>();
        let mut by_name = (0..fields.len()).collect::<Vec<_>>();
        by_name.sort_by(|left, right| fields[*left].name.cmp(&fields[*right].name));
        let mut boxes = Vec::new();
        collect_boxes(table, layout.root(), 0, &mut boxes);
        let class_name = class_name.clone();
        Ok(Self {
            access,
            layout,
            class_name,
            fields,
            by_name,
            boxes,
            ops: &CLASS_OPS,
        })
    }
}

#[derive(Default)]
struct ClassRegistry {
    by_descriptor: HashMap<String, &'static ClassLayout>,
    taken: HashSet<u32>,
}

fn class_registry() -> &'static RwLock<ClassRegistry> {
    static REGISTRY: OnceLock<RwLock<ClassRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

/// The class layout of the type `descriptor` names, registered under
/// access code `access` on first use. A code another layout holds, or a
/// descriptor registered under another code, is refused.
pub fn register_class_layout(
    descriptor: &str,
    access: u32,
) -> Result<&'static ClassLayout, String> {
    if access < FIRST_CLASS_ACCESS || access == super::UNREGISTERED_PLAIN_ACCESS {
        return Err(format!("{access} is not a class layout access code"));
    }
    let registry = class_registry();
    let same_code = |layout: &'static ClassLayout| {
        if layout.access == access {
            Ok(layout)
        } else {
            Err(format!(
                "the layout is registered under access code {}, not {access}",
                layout.access
            ))
        }
    };
    if let Some(layout) = registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .by_descriptor
        .get(descriptor)
    {
        return same_code(layout);
    }
    let layout = ClassLayout::from_descriptor(descriptor, access)?;
    let mut registry = registry
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(layout) = registry.by_descriptor.get(descriptor) {
        return same_code(layout);
    }
    if !registry.taken.insert(access) {
        return Err(format!("access code {access} names another layout"));
    }
    let layout: &'static ClassLayout = Box::leak(Box::new(layout));
    registry
        .by_descriptor
        .insert(descriptor.to_string(), layout);
    Ok(layout)
}

/// The cell generated code keeps for one class layout: its access code and
/// the layout, which the runtime helpers generated code calls read. It is
/// laid out as a plain layout's cell, so one table registers both.
#[repr(C)]
pub struct ClassLayoutCell {
    access: AtomicU32,
    layout: AtomicPtr<ClassLayout>,
}

impl ClassLayoutCell {
    pub const fn new() -> Self {
        Self {
            access: AtomicU32::new(super::UNREGISTERED_PLAIN_ACCESS),
            layout: AtomicPtr::new(std::ptr::null_mut()),
        }
    }
}

impl Default for ClassLayoutCell {
    fn default() -> Self {
        Self::new()
    }
}

/// The layout in `cell`, registered from `descriptor` under `access` and
/// stored there on first use.
///
/// # Safety
///
/// `cell` is null or points to a [`ClassLayoutCell`] that lives for the
/// rest of the process.
pub unsafe fn celled_class_layout(
    cell: *const ClassLayoutCell,
    descriptor: &[u8],
    access: u32,
) -> Result<&'static ClassLayout, String> {
    let cell = unsafe { cell.as_ref() }.ok_or("direct runtime received a null layout cell")?;
    let stored = cell.layout.load(Ordering::Acquire);
    if !stored.is_null() {
        // SAFETY: only this function stores into the cell, and it stores a
        // registered layout, which lives for the rest of the process.
        return Ok(unsafe { &*stored });
    }
    let descriptor =
        std::str::from_utf8(descriptor).map_err(|_| "a layout descriptor is not UTF-8")?;
    let layout = register_class_layout(descriptor, access)?;
    cell.access.store(layout.access, Ordering::Release);
    cell.layout
        .store(std::ptr::from_ref(layout).cast_mut(), Ordering::Release);
    Ok(layout)
}

/// The layout registered into `cell`.
///
/// # Safety
///
/// As for [`celled_class_layout`].
pub unsafe fn registered_class_layout(
    cell: *const ClassLayoutCell,
) -> Result<&'static ClassLayout, String> {
    let cell = unsafe { cell.as_ref() }.ok_or("direct runtime received a null layout cell")?;
    let stored = cell.layout.load(Ordering::Acquire);
    // SAFETY: as in `celled_class_layout`.
    unsafe { stored.as_ref() }
        .ok_or_else(|| "a class layout was used before it was registered".into())
}

/// The fields of one instance, in its class's layout.
pub struct TypedFields {
    layout: &'static ClassLayout,
    data: NonNull<u8>,
}

// SAFETY: the block is reached only through its instance, under the
// instance's access (see the module documentation).
unsafe impl Send for TypedFields {}
unsafe impl Sync for TypedFields {}

/// A place in the block: a field, or a field of a class or tuple held
/// inline in one, at its offset.
#[derive(Clone, Copy)]
struct Place {
    offset: usize,
    layout: LayoutId,
}

/// The box a slot of the block holds, null while its value is moved out.
///
/// # Safety
///
/// `slot` is the address of a box slot in a live block, and nothing else
/// borrows the slot for as long as the result lives.
unsafe fn field_box<'a>(slot: *mut u8) -> &'a mut Option<ElementBox> {
    // SAFETY: a box slot holds a pointer that is null or owns its box, and
    // `ElementBox` is a transparent `NonNull`.
    unsafe { &mut *slot.cast::<Option<ElementBox>>() }
}

impl TypedFields {
    /// An instance whose every field is moved out: scalars are zero and
    /// every box is null.
    pub fn new(layout: &'static ClassLayout) -> Self {
        let block = layout.block_layout();
        // SAFETY: the size is at least one byte.
        let data = unsafe { alloc_zeroed(block) };
        let Some(data) = NonNull::new(data) else {
            handle_alloc_error(block);
        };
        Self { layout, data }
    }

    /// The instance whose fields `fields` holds, when every field is there
    /// and fits its layout. Otherwise the map comes back.
    pub fn from_map(
        layout: &'static ClassLayout,
        mut fields: BTreeMap<String, Value>,
    ) -> Result<Self, BTreeMap<String, Value>> {
        let fits = fields.len() == layout.fields.len()
            && layout.fields.iter().all(|field| {
                fields
                    .get(&field.name)
                    .is_some_and(|value| (layout.ops.fits)(layout.table(), field.layout, value))
            });
        if !fits {
            return Err(fields);
        }
        let mut typed = Self::new(layout);
        for field in &layout.fields {
            let value = fields.remove(&field.name).expect("checked to be present");
            let place = Place {
                offset: field.offset,
                layout: field.layout,
            };
            // SAFETY: the place's bytes are dead, and the value fits.
            unsafe { typed.write_dead(place, value) };
        }
        Ok(typed)
    }

    pub fn layout(&self) -> &'static ClassLayout {
        self.layout
    }

    /// The block's address, which generated code reads fields at.
    pub fn data(&self) -> *mut u8 {
        self.data.as_ptr()
    }

    fn address(&self, place: Place) -> *mut u8 {
        // SAFETY: every place lies inside the block.
        unsafe { self.data.as_ptr().add(place.offset) }
    }

    fn is_box(&self, place: Place) -> bool {
        self.layout.is_box(place.layout)
    }

    fn field_place(&self, name: &str) -> Option<Place> {
        self.layout.field(name).map(|field| Place {
            offset: field.offset,
            layout: field.layout,
        })
    }

    /// The place `segments` reach through fields of classes and tuples
    /// held inline, as `self.meta.name` reaches `name` inside `meta`.
    fn path_place(&self, segments: &[&str]) -> Option<Place> {
        self.layout
            .path(segments)
            .map(|(offset, layout)| Place { offset, layout })
    }

    fn is_present(&self, place: Place) -> bool {
        // SAFETY: a box place is a box slot.
        !self.is_box(place) || unsafe { field_box(self.address(place)) }.is_some()
    }

    /// Writes `value` into the place's dead bytes.
    ///
    /// # Safety
    ///
    /// The place's bytes are dead, and `value` fits its layout.
    unsafe fn write_dead(&mut self, place: Place, value: Value) {
        let address = self.address(place);
        let table = self.layout.table();
        if self.is_box(place) {
            let boxed = (self.layout.ops.field_box)(value, self.layout.type_of(place.layout));
            // SAFETY: the slot's pointer is dead.
            unsafe { std::ptr::write(address.cast::<Option<ElementBox>>(), Some(boxed)) };
        } else {
            // SAFETY: the caller's contract.
            unsafe { (self.layout.ops.write)(table, place.layout, value, address) };
        }
    }

    fn read(&self, place: Place) -> Option<Cow<'_, Value>> {
        let address = self.address(place);
        if self.is_box(place) {
            // SAFETY: the slot holds a box pointer.
            return unsafe { field_box(address) }
                .as_ref()
                .map(|boxed| Cow::Borrowed(boxed.value()));
        }
        // SAFETY: the place holds a value of its layout.
        Some(Cow::Owned(unsafe {
            (self.layout.ops.read)(self.layout.table(), place.layout, address)
        }))
    }

    fn fits(&self, place: Place, value: &Value) -> Result<(), String> {
        if self.is_box(place) || (self.layout.ops.fits)(self.layout.table(), place.layout, value) {
            return Ok(());
        }
        Err(format!(
            "a `{}` value does not fit a field of type `{}` in class `{}`",
            super::value_type_name(value),
            self.layout.type_of(place.layout),
            self.layout.class_name
        ))
    }

    /// Replaces the value at `place`, dropping the one it held, or fills
    /// it when it is moved out. A value that does not fit is refused, and
    /// the place keeps its value.
    fn set(&mut self, place: Place, value: Value) -> Result<(), String> {
        self.fits(place, &value)?;
        let address = self.address(place);
        if self.is_box(place) {
            // SAFETY: the slot holds a box pointer.
            match unsafe { field_box(address) } {
                Some(boxed) => *boxed.value_mut() = value,
                // SAFETY: the slot is moved out, so its bytes are dead.
                None => unsafe { self.write_dead(place, value) },
            }
            return Ok(());
        }
        // SAFETY: the place holds a value of its layout, dead once dropped,
        // and the new value fits.
        unsafe {
            (self.layout.ops.drop)(self.layout.table(), place.layout, address);
            self.write_dead(place, value);
        }
        Ok(())
    }

    /// Moves the value at `place` out. A place held inline is left zeroed,
    /// so dropping the instance drops nothing of it again.
    fn take(&mut self, place: Place) -> Option<Value> {
        let address = self.address(place);
        if self.is_box(place) {
            // SAFETY: the slot holds a box pointer.
            return unsafe { field_box(address) }
                .take()
                .map(ElementBox::into_value);
        }
        let table = self.layout.table();
        // SAFETY: the place holds a value of its layout, and its bytes are
        // zeroed once it is taken.
        unsafe {
            let value = (self.layout.ops.take)(table, place.layout, address);
            std::ptr::write_bytes(address, 0, table.get(place.layout).size as usize);
            Some(value)
        }
    }

    /// Runs `edit` on the value at `place`. A box is changed where it is,
    /// and its list view follows the change. A value held inline is moved
    /// out for the edit and written back after it; one the edit leaves
    /// `None`, as a move does, stays zeroed.
    fn edit<R>(&mut self, place: Place, edit: impl FnOnce(&mut Value) -> R) -> Result<R, String> {
        let address = self.address(place);
        if self.is_box(place) {
            // SAFETY: the slot holds a box pointer.
            let boxed = unsafe { field_box(address) }.as_mut().ok_or_else(|| {
                format!(
                    "a field of class `{}` was used after it moved out",
                    self.layout.class_name
                )
            })?;
            let mut value = boxed.value_mut();
            return Ok(edit(&mut value));
        }
        let mut value = self
            .take(place)
            .expect("an inline place always holds a value");
        let result = edit(&mut value);
        let unit = matches!(self.layout.table().get(place.layout).kind, LayoutKind::Unit);
        if matches!(value, Value::Unit) && !unit {
            return Ok(result);
        }
        self.set(place, value)?;
        Ok(result)
    }

    fn missing(&self, path: &str) -> String {
        format!("class `{}` has no field `{path}`", self.layout.class_name)
    }

    /// The fields that are not moved out.
    pub fn len(&self) -> usize {
        self.layout
            .fields
            .iter()
            .filter(|field| {
                self.is_present(Place {
                    offset: field.offset,
                    layout: field.layout,
                })
            })
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.field_place(name)
            .is_some_and(|place| self.is_present(place))
    }

    /// The names of the fields that are not moved out, in name order.
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &str> + '_ {
        self.layout.by_name.iter().filter_map(|index| {
            let field = &self.layout.fields[*index];
            self.is_present(Place {
                offset: field.offset,
                layout: field.layout,
            })
            .then_some(field.name.as_str())
        })
    }

    /// Field `name`: borrowed from its box, or read out of the block.
    pub fn get(&self, name: &str) -> Option<Cow<'_, Value>> {
        self.read(self.field_place(name)?)
    }

    /// Every field not moved out, with its value, in name order.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&str, Cow<'_, Value>)> + '_ {
        self.layout.by_name.iter().filter_map(|index| {
            let field = &self.layout.fields[*index];
            self.read(Place {
                offset: field.offset,
                layout: field.layout,
            })
            .map(|value| (field.name.as_str(), value))
        })
    }

    /// Sets field `name`. A name the class has no field for, or a value
    /// that does not fit the field, is refused.
    pub fn insert(&mut self, name: &str, value: Value) -> Result<(), String> {
        let place = self.field_place(name).ok_or_else(|| self.missing(name))?;
        self.set(place, value)
    }

    /// Moves field `name` out.
    pub fn remove(&mut self, name: &str) -> Option<Value> {
        let place = self.field_place(name)?;
        self.take(place)
    }

    /// Runs `edit` on field `name` where it lies.
    pub fn with_field_mut<R>(
        &mut self,
        name: &str,
        edit: impl FnOnce(&mut Value) -> R,
    ) -> Result<R, String> {
        let place = self.field_place(name).ok_or_else(|| self.missing(name))?;
        self.edit(place, edit)
    }

    /// Moves out the value the field path `segments` reaches through
    /// classes and tuples held inline, as `take` of `meta.name` does, or
    /// `None` when the path leaves them.
    pub fn take_path(&mut self, segments: &[&str]) -> Option<Option<Value>> {
        let place = self.path_place(segments)?;
        Some(self.take(place))
    }

    /// Sets the value the field path `segments` reaches, as
    /// [`Self::take_path`] finds it, or gives the value back when the path
    /// leaves the block.
    pub fn set_path(
        &mut self,
        segments: &[&str],
        value: Value,
    ) -> Result<Result<(), String>, Value> {
        match self.path_place(segments) {
            Some(place) => Ok(self.set(place, value)),
            None => Err(value),
        }
    }

    /// The fields as a map, which [`Self::from_map`] turns back.
    pub fn into_map(self) -> BTreeMap<String, Value> {
        let mut this = std::mem::ManuallyDrop::new(self);
        let layout = this.layout;
        let mut fields = BTreeMap::new();
        for field in &layout.fields {
            if let Some(value) = this.remove(&field.name) {
                fields.insert(field.name.clone(), value);
            }
        }
        // SAFETY: every field is moved out, so the block holds nothing to
        // drop, and it came from `alloc_zeroed` with this layout.
        unsafe { dealloc(this.data.as_ptr(), layout.block_layout()) };
        fields
    }

    /// Brings the list view of every field box, nested classes included,
    /// up to date after the runtime changed the instance.
    pub fn refresh_views(&self) {
        for offset in &self.layout.boxes {
            // SAFETY: each offset is a box slot of the block.
            if let Some(boxed) = unsafe { field_box(self.data.as_ptr().add(*offset)) } {
                boxed.refresh_view();
            }
        }
    }
}

impl Clone for TypedFields {
    fn clone(&self) -> Self {
        let copy = Self::new(self.layout);
        // SAFETY: the block holds a value of the layout, the copy's bytes
        // are dead, and the two blocks are distinct.
        unsafe {
            (self.layout.ops.clone)(
                self.layout.table(),
                self.layout.layout.root(),
                self.data.as_ptr(),
                copy.data.as_ptr(),
            )
        };
        copy
    }
}

impl Drop for TypedFields {
    fn drop(&mut self) {
        // SAFETY: the block holds a value of the layout, with null boxes
        // where fields are moved out, and nothing reads it again.
        unsafe {
            (self.layout.ops.drop)(
                self.layout.table(),
                self.layout.layout.root(),
                self.data.as_ptr(),
            );
            dealloc(self.data.as_ptr(), self.layout.block_layout());
        }
    }
}

impl fmt::Debug for TypedFields {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_map().entries(self.iter()).finish()
    }
}

#[cfg(test)]
#[path = "native_class_layouts_tests.rs"]
mod tests;
