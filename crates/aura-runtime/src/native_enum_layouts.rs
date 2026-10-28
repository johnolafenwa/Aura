//! Enum variants in their typed layouts (typed-layout step 2f).
//!
//! A variant generated code builds keeps its payloads in one heap block in
//! the layout the compiler computed for the enum in a box
//! ([`aura_core::layout::LayoutBuilder::boxed_enum_layout`]) rather than in
//! a vector of values: each payload generated code holds unboxed, a scalar
//! or a plain record, at its offset, and every other payload, a recursive
//! one included, in a box of its own that the block owns ([`ElementBox`]).
//! A payload box is null while its payload is moved out, and a variant with
//! no payloads has no block. The variant's box carries the enum's access
//! code, the variant's index, and the block's address in its list view, so
//! generated code tests a variant with two compares, reads a scalar payload
//! where it lies, and takes a new reference to a payload's box instead of
//! copying the payload.
//!
//! The rest of the runtime reaches a typed variant through
//! [`crate::runtime_value::VariantPayloads`], as it reaches one whose
//! payloads are values.
//!
//! The block is reached only through its variant, under the variant's
//! access. Generated code never changes a payload in place: a payload box
//! that another reference shares is copied before the runtime changes it
//! ([`ElementBox::value_mut`]), so a reference generated code took keeps
//! the payload it read.

use std::alloc::{alloc, alloc_zeroed, dealloc, handle_alloc_error, Layout as AllocLayout};
use std::any::Any;
use std::borrow::Cow;
use std::fmt;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use aura_core::layout::{LayoutId, LayoutKind, LayoutTable};
use aura_core::types::Type;

use super::typed_layout::{self, FieldOps, TypedLayout};
use super::{canonical_runtime_type_name, shared_runtime_type_tag, ElementBox};
use crate::runtime_value::{RuntimeName, Value};

/// The access code of the first enum layout a program registers. Class
/// layouts take the codes from `FIRST_CLASS_ACCESS` up to this one.
pub const FIRST_ENUM_ACCESS: u32 = 0x6000_0000;

/// The layout of one enum type whose variants keep their payloads in
/// place. A generic enum has one per type it is used at.
pub struct EnumLayout {
    /// The access code of a variant box that keeps its payloads in this
    /// layout.
    access: u32,
    layout: TypedLayout,
    /// The name every variant of the layout carries: the enum's name, and
    /// for a generic enum the type it is used at, as tagging a variant
    /// gives it.
    enum_name: RuntimeName,
    /// The type tag of a box that holds a variant of the layout.
    runtime_type: Arc<str>,
    variants: Vec<VariantPlan>,
    /// How values move in and out of a variant's block.
    ops: &'static FieldOps,
}

/// Where one variant keeps its payloads.
struct VariantPlan {
    name: RuntimeName,
    payloads: Vec<Place>,
    /// The block of a variant with payloads.
    block: Option<AllocLayout>,
    /// Whether every payload is held in place and needs no drop, so the
    /// block copies as bytes and drops by freeing it.
    bytes_only: bool,
}

/// A payload's offset in the block and its layout.
#[derive(Clone, Copy)]
struct Place {
    offset: usize,
    layout: LayoutId,
}

impl EnumLayout {
    pub fn access(&self) -> u32 {
        self.access
    }

    pub fn enum_name(&self) -> &RuntimeName {
        &self.enum_name
    }

    pub fn runtime_type(&self) -> &Arc<str> {
        &self.runtime_type
    }

    /// The variant named `name`, by its index.
    pub fn variant_index(&self, name: &str) -> Option<usize> {
        self.variants
            .iter()
            .position(|variant| variant.name.as_str() == name)
    }

    /// The name of variant `index`.
    pub fn variant_name(&self, index: usize) -> Option<&RuntimeName> {
        self.variants.get(index).map(|variant| &variant.name)
    }

    fn table(&self) -> &LayoutTable {
        self.layout.table()
    }

    fn is_box(&self, place: Place) -> bool {
        matches!(self.table().get(place.layout).kind, LayoutKind::Handle)
    }

    fn type_of(&self, place: Place) -> &Type {
        &self.table().get(place.layout).ty
    }

    fn from_descriptor(descriptor: &str, access: u32) -> Result<Self, String> {
        let layout = TypedLayout::from_descriptor(descriptor)?;
        let table = layout.table();
        let root = table.get(layout.root());
        let LayoutKind::Tagged {
            enum_name: Some(base),
            payload_offset,
            variants,
            ..
        } = &root.kind
        else {
            return Err(format!("`{}` is not an enum laid out in a box", root.ty));
        };
        if aura_core::layout::names_type_param(&root.ty) {
            return Err(format!("`{}` names a type parameter", root.ty));
        }
        // Every recursive payload is a box, so dropping or copying a
        // variant never recurses through the block.
        if table.holds_block(layout.root()) {
            return Err(format!("enum `{}` holds a heap block", root.ty));
        }
        let runtime_type = canonical_runtime_type_name(&root.ty);
        let tag = shared_runtime_type_tag(runtime_type.as_bytes());
        let enum_name = match (&root.ty, &tag.tagged_enum_name) {
            (Type::Named(_, args), Some(tagged)) if !args.is_empty() => tagged.clone(),
            _ => RuntimeName::from(base.as_str()),
        };
        let variants = variants
            .iter()
            .map(|variant| {
                let payloads = variant
                    .fields
                    .iter()
                    // The block holds the payloads without the tag, which
                    // the variant keeps beside it.
                    .map(|field| Place {
                        offset: (field.offset - payload_offset) as usize,
                        layout: field.layout,
                    })
                    .collect::<Vec<_>>();
                let end = payloads
                    .iter()
                    .map(|place| place.offset + table.get(place.layout).size as usize)
                    .max();
                let block = end.map(|end| {
                    AllocLayout::from_size_align(end.max(1), layout.align())
                        .expect("layouts keep sizes and alignments an allocation accepts")
                        .pad_to_align()
                });
                let bytes_only = payloads.iter().all(|place| !table.needs_drop(place.layout));
                VariantPlan {
                    name: RuntimeName::from(variant.name.as_str()),
                    payloads,
                    block,
                    bytes_only,
                }
            })
            .collect();
        Ok(Self {
            access,
            enum_name,
            runtime_type: Arc::clone(&tag.name),
            layout,
            variants,
            ops: &typed_layout::FIELD_OPS,
        })
    }
}

impl fmt::Debug for EnumLayout {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EnumLayout")
            .field("access", &self.access)
            .field("runtime_type", &self.runtime_type)
            .finish_non_exhaustive()
    }
}

/// The enum layout `descriptor` describes, registered under access code
/// `access` on first use. A code another layout holds, or a descriptor
/// registered under another code, is refused.
pub fn register_enum_layout(descriptor: &str, access: u32) -> Result<&'static EnumLayout, String> {
    if access < FIRST_ENUM_ACCESS || access == super::UNREGISTERED_PLAIN_ACCESS {
        return Err(format!("{access} is not an enum layout access code"));
    }
    static REGISTRY: OnceLock<RwLock<typed_layout::LayoutRegistry>> = OnceLock::new();
    let build = || {
        let layout: &'static EnumLayout =
            Box::leak(Box::new(EnumLayout::from_descriptor(descriptor, access)?));
        Ok(layout as &'static (dyn Any + Send + Sync))
    };
    let layout = typed_layout::register_layout(
        REGISTRY.get_or_init(Default::default),
        descriptor,
        access,
        &build,
    )?;
    Ok(layout
        .downcast_ref()
        .expect("the enum layout registry holds enum layouts"))
}

/// The cell generated code keeps for one enum layout: its access code and
/// the layout. It is laid out as a plain layout's cell, so one table
/// registers both.
#[repr(C)]
pub struct EnumLayoutCell {
    access: AtomicU32,
    layout: AtomicPtr<EnumLayout>,
}

impl EnumLayoutCell {
    pub const fn new() -> Self {
        Self {
            access: AtomicU32::new(super::UNREGISTERED_PLAIN_ACCESS),
            layout: AtomicPtr::new(std::ptr::null_mut()),
        }
    }
}

impl Default for EnumLayoutCell {
    fn default() -> Self {
        Self::new()
    }
}

/// The layout in `cell`, registered from `descriptor` under `access` and
/// stored there on first use.
///
/// # Safety
///
/// `cell` is null or points to an [`EnumLayoutCell`] that lives for the
/// rest of the process.
pub unsafe fn celled_enum_layout(
    cell: *const EnumLayoutCell,
    descriptor: &[u8],
    access: u32,
) -> Result<&'static EnumLayout, String> {
    let cell = unsafe { cell.as_ref() }.ok_or("direct runtime received a null layout cell")?;
    let stored = cell.layout.load(Ordering::Acquire);
    if !stored.is_null() {
        // SAFETY: only this function stores into the cell, and it stores a
        // registered layout, which lives for the rest of the process.
        return Ok(unsafe { &*stored });
    }
    let descriptor =
        std::str::from_utf8(descriptor).map_err(|_| "a layout descriptor is not UTF-8")?;
    let layout = register_enum_layout(descriptor, access)?;
    cell.access.store(layout.access, Ordering::Release);
    cell.layout
        .store(std::ptr::from_ref(layout).cast_mut(), Ordering::Release);
    Ok(layout)
}

/// The layout registered into `cell`.
///
/// # Safety
///
/// As for [`celled_enum_layout`].
pub unsafe fn registered_enum_layout(
    cell: *const EnumLayoutCell,
) -> Result<&'static EnumLayout, String> {
    let cell = unsafe { cell.as_ref() }.ok_or("direct runtime received a null layout cell")?;
    let stored = cell.layout.load(Ordering::Acquire);
    // SAFETY: as in `celled_enum_layout`.
    unsafe { stored.as_ref() }
        .ok_or_else(|| "an enum layout was used before it was registered".into())
}

/// The payloads of one variant, in its enum's layout.
pub struct TypedPayloads {
    layout: &'static EnumLayout,
    variant: u32,
    data: Option<NonNull<u8>>,
}

// SAFETY: the block is reached only through its variant, under the
// variant's access (see the module documentation).
unsafe impl Send for TypedPayloads {}
unsafe impl Sync for TypedPayloads {}

/// The box a payload slot holds, null while its payload is moved out.
///
/// # Safety
///
/// `slot` is the address of a box slot in a live block, and nothing else
/// borrows the slot for as long as the result lives.
unsafe fn payload_box<'a>(slot: *mut u8) -> &'a mut Option<ElementBox> {
    // SAFETY: a box slot holds a pointer that is null or owns its box, and
    // `ElementBox` is a transparent `NonNull`.
    unsafe { &mut *slot.cast::<Option<ElementBox>>() }
}

impl TypedPayloads {
    /// Variant `variant` with every payload moved out: scalars are zero and
    /// every box is null.
    pub fn new(layout: &'static EnumLayout, variant: usize) -> Self {
        let plan = &layout.variants[variant];
        let data = plan.block.map(|block| {
            // SAFETY: a block is at least one byte.
            let data = unsafe { alloc_zeroed(block) };
            NonNull::new(data).unwrap_or_else(|| handle_alloc_error(block))
        });
        Self {
            layout,
            variant: variant as u32,
            data,
        }
    }

    /// The variant `variant_name` of `layout` holding `payloads`, when the
    /// variant takes that many and each fits its place. Otherwise the
    /// payloads come back.
    pub fn from_values(
        layout: &'static EnumLayout,
        variant_name: &str,
        payloads: Vec<Value>,
    ) -> Result<Self, Vec<Value>> {
        let Some(variant) = layout.variant_index(variant_name) else {
            return Err(payloads);
        };
        let plan = &layout.variants[variant];
        let fits = plan.payloads.len() == payloads.len()
            && plan.payloads.iter().zip(&payloads).all(|(place, value)| {
                layout.is_box(*place) || (layout.ops.fits)(layout.table(), place.layout, value)
            });
        if !fits {
            return Err(payloads);
        }
        let mut typed = Self::new(layout, variant);
        for (index, value) in payloads.into_iter().enumerate() {
            // SAFETY: the place is dead, and the value fits it.
            unsafe { typed.write_dead(index, value) };
        }
        Ok(typed)
    }

    pub fn layout(&self) -> &'static EnumLayout {
        self.layout
    }

    /// The variant's index in its enum.
    pub fn variant(&self) -> usize {
        self.variant as usize
    }

    pub fn variant_name(&self) -> &'static RuntimeName {
        &self.layout.variants[self.variant()].name
    }

    /// The block's address, which generated code reads payloads at, or 0
    /// for a variant with no payloads.
    pub fn data(&self) -> usize {
        self.data.map_or(0, |data| data.as_ptr() as usize)
    }

    fn places(&self) -> &'static [Place] {
        &self.layout.variants[self.variant()].payloads
    }

    fn place(&self, index: usize) -> Option<Place> {
        self.places().get(index).copied()
    }

    fn address(&self, place: Place) -> *mut u8 {
        let data = self
            .data
            .expect("a variant with payloads has a block")
            .as_ptr();
        // SAFETY: every place lies inside the block.
        unsafe { data.add(place.offset) }
    }

    /// Writes `value` into the dead bytes of payload `index`.
    ///
    /// # Safety
    ///
    /// The payload's bytes are dead, and `value` fits its place.
    unsafe fn write_dead(&mut self, index: usize, value: Value) {
        let place = self.places()[index];
        let address = self.address(place);
        if self.layout.is_box(place) {
            let boxed = (self.layout.ops.field_box)(value, self.layout.type_of(place));
            // SAFETY: the slot's pointer is dead.
            unsafe { std::ptr::write(address.cast::<Option<ElementBox>>(), Some(boxed)) };
        } else {
            // SAFETY: the caller's contract.
            unsafe { (self.layout.ops.write)(self.layout.table(), place.layout, value, address) };
        }
    }

    /// How many payloads the variant takes.
    pub fn len(&self) -> usize {
        self.places().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The box that holds payload `index`, when the payload is kept in one
    /// and is not moved out.
    pub fn payload_box(&self, index: usize) -> Option<&ElementBox> {
        let place = self.place(index)?;
        if !self.layout.is_box(place) {
            return None;
        }
        // SAFETY: the slot holds a box pointer, and `&self` keeps the block.
        unsafe { payload_box(self.address(place)) }.as_ref()
    }

    /// Whether `leaf` holds for the value of every payload box that is not
    /// moved out. A payload held in place is a scalar or a plain record,
    /// which copies as bytes.
    pub fn boxed_payloads_all(&self, leaf: impl Fn(&Value) -> bool) -> bool {
        // A variant whose payloads need no drop holds no box.
        self.layout.variants[self.variant()].bytes_only
            || (0..self.len()).all(|index| {
                self.payload_box(index)
                    .is_none_or(|boxed| leaf(boxed.value()))
            })
    }

    /// A copy of the variant whose payload boxes hold `values`, in order,
    /// one for each box that is not moved out, under the tags of the boxes
    /// they copy. The payloads held in place are copied from this block.
    /// A walk that copies a deep variant one box at a time builds the copy
    /// this way.
    ///
    /// # Panics
    ///
    /// When `values` runs out before the boxes do.
    pub fn copy_with_boxes(&self, mut values: impl Iterator<Item = Value>) -> Self {
        let copy = Self::new(self.layout, self.variant());
        let table = self.layout.table();
        for place in self.places() {
            let (source, target) = (self.address(*place), copy.address(*place));
            if !self.layout.is_box(*place) {
                // SAFETY: the place holds a value of its layout, the copy's
                // bytes are dead, and the two blocks are distinct.
                unsafe { (self.layout.ops.clone)(table, place.layout, source, target) };
                continue;
            }
            // SAFETY: both slots hold box pointers, and the copy's is null.
            if let Some(boxed) = unsafe { payload_box(source) }.as_ref() {
                let value = values.next().expect("one value for each payload box");
                *unsafe { payload_box(target) } = Some(boxed.copy_with(value));
            }
        }
        copy
    }

    /// Payload `index`: borrowed from its box, or read out of the block. A
    /// moved-out payload reads as `None`, the unit value a moved-out payload
    /// of a vector of values holds.
    pub fn get(&self, index: usize) -> Option<Cow<'_, Value>> {
        let place = self.place(index)?;
        let address = self.address(place);
        if self.layout.is_box(place) {
            // SAFETY: the slot holds a box pointer.
            return Some(match unsafe { payload_box(address) }.as_ref() {
                Some(boxed) => Cow::Borrowed(boxed.value()),
                None => Cow::Owned(Value::Unit),
            });
        }
        // SAFETY: the place holds a value of its layout.
        Some(Cow::Owned(unsafe {
            (self.layout.ops.read)(self.layout.table(), place.layout, address)
        }))
    }

    /// Every payload, in order.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = Cow<'_, Value>> + '_ {
        (0..self.len()).map(|index| self.get(index).expect("index in range"))
    }

    /// Moves payload `index` out, leaving its place moved out: a null box,
    /// or zeroed bytes. A moved-out payload gives the unit value.
    pub fn take(&mut self, index: usize) -> Option<Value> {
        let place = self.place(index)?;
        let address = self.address(place);
        if self.layout.is_box(place) {
            // SAFETY: the slot holds a box pointer.
            return Some(
                unsafe { payload_box(address) }
                    .take()
                    .map_or(Value::Unit, ElementBox::into_value),
            );
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

    /// Moves the box that holds payload `index` out, leaving its place
    /// moved out. `None` when the payload is held in place or already
    /// moved out.
    pub fn take_box(&mut self, index: usize) -> Option<ElementBox> {
        let place = self.place(index)?;
        if !self.layout.is_box(place) {
            return None;
        }
        // SAFETY: the slot holds a box pointer, and `&mut self` keeps the
        // block.
        unsafe { payload_box(self.address(place)) }.take()
    }

    /// The payloads as values, which [`Self::from_values`] turns back.
    pub fn into_vec(self) -> Vec<Value> {
        let mut this = std::mem::ManuallyDrop::new(self);
        let payloads = (0..this.len())
            .map(|index| this.take(index).expect("index in range"))
            .collect();
        if let (Some(data), Some(block)) = (this.data, this.layout.variants[this.variant()].block) {
            // SAFETY: every payload is moved out, so the block holds nothing
            // to drop, and it came from `alloc_zeroed` with this layout.
            unsafe { dealloc(data.as_ptr(), block) };
        }
        payloads
    }

    /// Brings the list view of every payload box up to date after the
    /// runtime changed the variant.
    pub fn refresh_views(&self) {
        for index in 0..self.len() {
            if let Some(boxed) = self.payload_box(index) {
                boxed.refresh_view();
            }
        }
    }
}

impl Clone for TypedPayloads {
    fn clone(&self) -> Self {
        let plan = &self.layout.variants[self.variant()];
        if plan.bytes_only {
            let data = self.data.zip(plan.block).map(|(data, block)| {
                // SAFETY: a block is at least one byte.
                let copy = unsafe { alloc(block) };
                let copy = NonNull::new(copy).unwrap_or_else(|| handle_alloc_error(block));
                // SAFETY: both blocks have this layout's size, every byte of
                // the source is initialized, and the blocks are distinct.
                unsafe {
                    std::ptr::copy_nonoverlapping(data.as_ptr(), copy.as_ptr(), block.size())
                };
                copy
            });
            return Self {
                layout: self.layout,
                variant: self.variant,
                data,
            };
        }
        let copy = Self::new(self.layout, self.variant());
        let table = self.layout.table();
        for place in self.places() {
            // SAFETY: each place of the block holds a value of its layout,
            // the copy's bytes are dead, and the two blocks are distinct.
            unsafe {
                (self.layout.ops.clone)(
                    table,
                    place.layout,
                    self.address(*place),
                    copy.address(*place),
                )
            };
        }
        copy
    }
}

impl Drop for TypedPayloads {
    fn drop(&mut self) {
        let plan = &self.layout.variants[self.variant()];
        let (Some(data), Some(block)) = (self.data, plan.block) else {
            return;
        };
        if plan.bytes_only {
            // SAFETY: the block holds nothing to drop and came from this
            // layout.
            unsafe { dealloc(data.as_ptr(), block) };
            return;
        }
        let table = self.layout.table();
        // SAFETY: each place holds a value of its layout, with null boxes
        // where payloads are moved out, and nothing reads the block again.
        unsafe {
            for place in self.places() {
                (self.layout.ops.drop)(table, place.layout, self.address(*place));
            }
            dealloc(data.as_ptr(), block);
        }
    }
}

impl fmt::Debug for TypedPayloads {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}

#[cfg(test)]
#[path = "native_enum_layouts_tests.rs"]
mod tests;
