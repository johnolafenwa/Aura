//! Values in their native layouts.
//!
//! Generated code keeps a value of a concrete type in the layout
//! [`aura_core::layout`] computes for it. This module moves a value between
//! that layout and the boxed [`Value`] the rest of the runtime reads, drops a
//! value where it lies, and copies one. Generated code reaches it only where
//! a value crosses into a runtime helper, a host call, FFI, or `aura run`.
//!
//! A handle in a layout is a box the layout owns. No task's ledger links
//! it, so a failed task frees it with whatever owns the layout. A heap
//! pointer owns one block of its target's size and alignment.

use std::alloc::{alloc, dealloc, handle_alloc_error, Layout as AllocLayout};
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
use std::sync::{OnceLock, RwLock};

use aura_core::integer::{IntegerKind, IntegerRepresentation, IntegerValue};
use aura_core::layout::{
    decode_descriptor, names_type_param, LayoutId, LayoutKind, LayoutTable, ScalarLayout,
};
use aura_core::types::Type;

use super::{
    canonical_runtime_type_name, consume_owned_untracked_value, element_boxed_value,
    is_unknown_runtime_type, release_untracked_value, retag_value, runtime_error,
    set_explicit_runtime_type_tag, shared_runtime_type_tag, value_type_name, ElementBox,
    OpaqueValue,
};
use crate::runtime_value::{
    nominal_runtime_base_name, union_value, EnumVariantValue, InstanceFields, InstanceValue,
    TupleValue, Value, DIRECT_RUNTIME_TYPE_FIELD,
};

/// The layout of one type, with every layout it reaches.
pub(crate) struct TypedLayout {
    table: LayoutTable,
    root: LayoutId,
}

impl TypedLayout {
    /// The layouts a descriptor carries, which the compiler computed.
    pub(crate) fn from_descriptor(descriptor: &str) -> Result<Self, String> {
        let (table, root) = decode_descriptor(descriptor)?;
        Ok(Self { table, root })
    }

    pub(crate) fn table(&self) -> &LayoutTable {
        &self.table
    }

    pub(crate) fn root(&self) -> LayoutId {
        self.root
    }

    pub(crate) fn size(&self) -> usize {
        self.table.get(self.root).size as usize
    }

    pub(crate) fn align(&self) -> usize {
        self.table.get(self.root).align as usize
    }

    /// Writes `value` into the uninitialized bytes at `dst`. A value whose
    /// shape does not match the layout is refused before anything is
    /// written.
    ///
    /// # Safety
    ///
    /// `dst` is valid for writes of [`Self::size`] bytes and aligned to
    /// [`Self::align`].
    pub(crate) unsafe fn write(&self, value: Value, dst: *mut u8) -> Result<(), String> {
        check(&self.table, self.root, &value)?;
        unsafe { write(&self.table, self.root, value, dst) };
        Ok(())
    }

    /// A copy of the value at `src`, which stays where it is.
    ///
    /// # Safety
    ///
    /// `src` holds a value of this layout.
    pub(crate) unsafe fn read(&self, src: *const u8) -> Value {
        unsafe { read(&self.table, self.root, src) }
    }

    /// Moves the value at `src` out. The bytes are dead afterwards.
    ///
    /// # Safety
    ///
    /// `src` holds a value of this layout, and nothing reads it again.
    pub(crate) unsafe fn take(&self, src: *mut u8) -> Value {
        unsafe { take(&self.table, self.root, src) }
    }

    /// Drops the value at `ptr`. The bytes are dead afterwards.
    ///
    /// # Safety
    ///
    /// As for [`Self::take`].
    pub(crate) unsafe fn drop_in_place(&self, ptr: *mut u8) {
        unsafe { drop_in_place(&self.table, self.root, ptr) }
    }

    /// Writes an independent copy of the value at `src` into the
    /// uninitialized bytes at `dst`.
    ///
    /// # Safety
    ///
    /// `src` holds a value of this layout, and `dst` is as for
    /// [`Self::write`] and does not overlap it.
    pub(crate) unsafe fn clone_into(&self, src: *const u8, dst: *mut u8) {
        unsafe { clone_into(&self.table, self.root, src, dst) }
    }
}

/// The layout of a type whose values hold no handle and no heap block, so
/// a value is its bytes and nothing else. Generated code registers one by
/// descriptor, and a list of such values keeps them in place, one after
/// another ([`crate::list_elements::ListElements::Typed`]), where generated
/// code reads and writes them.
pub struct PlainLayout {
    /// The list access code of a list that keeps these values in place.
    access: u32,
    layout: TypedLayout,
    /// The words one value takes in a list: its size, rounded up to whole
    /// 8-byte words. A plain layout is never empty, so it is never 0.
    stride_words: NonZeroUsize,
    /// How values move in and out of the layout. Only registration names
    /// these, so a program that registers no plain layout links none of
    /// the converters, however many list operations it has.
    ops: &'static PlainOps,
}

struct PlainOps {
    check: fn(&LayoutTable, LayoutId, &Value) -> Result<(), String>,
    write: unsafe fn(&LayoutTable, LayoutId, &Value, *mut u8),
    read: unsafe fn(&LayoutTable, LayoutId, *const u8) -> Value,
}

static PLAIN_OPS: PlainOps = PlainOps {
    check,
    write: write_plain,
    read,
};

/// The access code of the first plain layout registered. The codes below
/// it are the fixed list forms of `DirectListAccess`.
pub const FIRST_PLAIN_ACCESS: u32 = 0x100;

/// The access code a typed cell holds before its layout is registered.
/// No list carries it, so generated code that compares a list's code
/// against an unregistered cell takes the slow path.
pub const UNREGISTERED_PLAIN_ACCESS: u32 = u32::MAX;

impl PlainLayout {
    pub fn access(&self) -> u32 {
        self.access
    }

    pub fn stride_words(&self) -> usize {
        self.stride_words.get()
    }

    /// [`Self::stride_words`], which dividing by needs no zero check.
    #[inline]
    pub fn stride(&self) -> NonZeroUsize {
        self.stride_words
    }

    /// The bytes one value takes, without the padding to whole words.
    pub fn size(&self) -> usize {
        self.layout.table.get(self.layout.root).size as usize
    }

    /// Whether `value` has this layout's shape, all the way down.
    pub fn fits(&self, value: &Value) -> bool {
        (self.ops.check)(&self.layout.table, self.layout.root, value).is_ok()
    }

    /// Writes `value` into one element's words, padding included.
    ///
    /// # Safety
    ///
    /// [`Self::fits`] accepted `value`, and `dst` is valid for writes of
    /// [`Self::stride_words`] words.
    pub unsafe fn encode(&self, value: &Value, dst: *mut u64) {
        unsafe {
            std::ptr::write_bytes(dst, 0, self.stride_words.get());
            (self.ops.write)(&self.layout.table, self.layout.root, value, dst.cast());
        }
    }

    /// The value one element's words hold.
    ///
    /// # Safety
    ///
    /// `src` holds a value [`Self::encode`] wrote.
    pub unsafe fn decode(&self, src: *const u64) -> Value {
        unsafe { self.read(src.cast()) }
    }

    /// Writes `value` into the layout's bytes at `dst`, as generated code
    /// keeps a value of this type. A value of another shape is refused
    /// before anything is written.
    ///
    /// # Safety
    ///
    /// `dst` is valid for writes of the layout's size, at its alignment.
    pub unsafe fn write_checked(&self, value: &Value, dst: *mut u8) -> Result<(), String> {
        (self.ops.check)(&self.layout.table, self.layout.root, value)?;
        unsafe { (self.ops.write)(&self.layout.table, self.layout.root, value, dst) };
        Ok(())
    }

    /// The value the layout's bytes at `src` hold.
    ///
    /// # Safety
    ///
    /// `src` holds a value of this layout.
    pub unsafe fn read(&self, src: *const u8) -> Value {
        unsafe { (self.ops.read)(&self.layout.table, self.layout.root, src) }
    }
}

#[derive(Default)]
struct PlainRegistry {
    by_descriptor: HashMap<String, &'static PlainLayout>,
    /// The layouts of classes and tuples, by [`plain_type_key`], so a list
    /// can keep values of a registered type in place however it was built.
    by_key: HashMap<String, &'static PlainLayout>,
    /// Every access code a layout holds.
    taken: HashSet<u32>,
    /// The first code a layout registered without one may take.
    next: u32,
}

/// The lookup behind [`plain_layout_for_value`], installed when a layout
/// registers under a key. A program that registered none skips the
/// registry's lock and links neither the registry nor the shape check.
static PLAIN_LOOKUP: OnceLock<fn(&Value) -> Option<&'static PlainLayout>> = OnceLock::new();

fn plain_registry() -> &'static RwLock<PlainRegistry> {
    static REGISTRY: OnceLock<RwLock<PlainRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

/// The key a class or tuple type registers its plain layout under: the
/// class's base name, or the tuple's canonical type descriptor.
fn plain_type_key(ty: &Type) -> Option<String> {
    match ty {
        Type::Named(name, args) if args.is_empty() => Some(name.clone()),
        Type::Tuple(_) => Some(format!("({})", aura_core::type_descriptor::encode(ty))),
        _ => None,
    }
}

fn plain_value_key(value: &Value) -> Option<String> {
    match value {
        Value::Instance(instance) => {
            Some(nominal_runtime_base_name(&instance.class_name).to_string())
        }
        Value::Tuple(tuple) => plain_type_key(&Type::Tuple(tuple.element_types.clone())),
        _ => None,
    }
}

/// The plain layout of the type `descriptor` names, registered on first
/// use under the next free access code. Every call with the same
/// descriptor returns the same layout. A type whose values hold a handle
/// or a heap block, take no bytes, or need more than 8-byte alignment has
/// no plain layout.
pub fn plain_layout_for_descriptor(descriptor: &str) -> Result<&'static PlainLayout, String> {
    register_plain_layout(descriptor, None)
}

/// [`plain_layout_for_descriptor`], under access code `access` when one is
/// given. A program registers its layouts under the codes its generated
/// code compares lists against, so a code another layout holds, or a
/// descriptor registered under another code, is refused.
pub fn register_plain_layout(
    descriptor: &str,
    access: Option<u32>,
) -> Result<&'static PlainLayout, String> {
    fn same_code(
        layout: &'static PlainLayout,
        access: Option<u32>,
    ) -> Result<&'static PlainLayout, String> {
        match access {
            Some(code) if code != layout.access => Err(format!(
                "the layout is registered under access code {}, not {code}",
                layout.access
            )),
            _ => Ok(layout),
        }
    }
    let registry = plain_registry();
    if let Some(layout) = registry
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .by_descriptor
        .get(descriptor)
    {
        return same_code(layout, access);
    }
    let layout = TypedLayout::from_descriptor(descriptor)?;
    let root = layout.table.get(layout.root);
    // An empty value has no stride, so it keeps the boxed paths too.
    let stride_words = NonZeroUsize::new((root.size as usize).div_ceil(8))
        .filter(|_| !layout.table.needs_drop(layout.root) && root.align <= 8)
        .ok_or_else(|| format!("`{}` has no plain layout", root.ty))?;
    let key = plain_type_key(&root.ty);
    let mut registry = registry
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(layout) = registry.by_descriptor.get(descriptor) {
        return same_code(layout, access);
    }
    let access = match access {
        Some(code) if !(FIRST_PLAIN_ACCESS..super::FIRST_CLASS_ACCESS).contains(&code) => {
            return Err(format!("{code} is not a plain layout access code"));
        }
        Some(code) if registry.taken.contains(&code) => {
            return Err(format!("access code {code} names another layout"));
        }
        Some(code) => code,
        None => {
            let mut code = registry.next.max(FIRST_PLAIN_ACCESS);
            while registry.taken.contains(&code) {
                code = match code.checked_add(1) {
                    Some(next) if next < super::FIRST_CLASS_ACCESS => next,
                    _ => return Err("too many plain layouts".to_string()),
                };
            }
            registry.next = code.saturating_add(1);
            code
        }
    };
    registry.taken.insert(access);
    let plain: &'static PlainLayout = Box::leak(Box::new(PlainLayout {
        access,
        layout,
        stride_words,
        ops: &PLAIN_OPS,
    }));
    registry.by_descriptor.insert(descriptor.to_string(), plain);
    if let Some(key) = key {
        registry.by_key.entry(key).or_insert(plain);
        PLAIN_LOOKUP.get_or_init(|| registered_plain_layout_for_value);
    }
    Ok(plain)
}

/// The registered plain layout `value` fits, when its class or tuple type
/// registered one.
pub fn plain_layout_for_value(value: &Value) -> Option<&'static PlainLayout> {
    if !matches!(value, Value::Instance(_) | Value::Tuple(_)) {
        return None;
    }
    PLAIN_LOOKUP.get()?(value)
}

fn registered_plain_layout_for_value(value: &Value) -> Option<&'static PlainLayout> {
    let registry = plain_registry()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let layout = *registry.by_key.get(&plain_value_key(value)?)?;
    layout.fits(value).then_some(layout)
}

/// The cell generated code keeps for one plain layout's descriptor: the
/// layout's access code and the layout itself, which the runtime helpers
/// generated code calls read. Both start empty.
#[repr(C)]
pub struct PlainLayoutCell {
    access: AtomicU32,
    layout: AtomicPtr<PlainLayout>,
}

impl PlainLayoutCell {
    /// A cell with no layout yet.
    pub const fn new() -> Self {
        Self {
            access: AtomicU32::new(UNREGISTERED_PLAIN_ACCESS),
            layout: AtomicPtr::new(std::ptr::null_mut()),
        }
    }
}

impl Default for PlainLayoutCell {
    fn default() -> Self {
        Self::new()
    }
}

/// The offset of the access code in a [`PlainLayoutCell`], which
/// generated code reads, and the cell's size.
pub const PLAIN_LAYOUT_CELL_ACCESS_OFFSET: i32 =
    std::mem::offset_of!(PlainLayoutCell, access) as i32;
pub const PLAIN_LAYOUT_CELL_BYTES: usize = std::mem::size_of::<PlainLayoutCell>();

/// The layout in `cell`, registered from `descriptor` under access code
/// `access` and stored there on first use.
///
/// # Safety
///
/// `cell` is null or points to a [`PlainLayoutCell`] that lives for the
/// rest of the process.
pub unsafe fn celled_plain_layout(
    cell: *const PlainLayoutCell,
    descriptor: &[u8],
    access: u32,
) -> Result<&'static PlainLayout, String> {
    let cell = unsafe { cell.as_ref() }.ok_or("direct runtime received a null layout cell")?;
    let stored = cell.layout.load(Ordering::Acquire);
    if !stored.is_null() {
        // SAFETY: only this function stores into the cell, and it stores a
        // registered layout, which lives for the rest of the process.
        return Ok(unsafe { &*stored });
    }
    let descriptor =
        std::str::from_utf8(descriptor).map_err(|_| "a layout descriptor is not UTF-8")?;
    let layout = register_plain_layout(descriptor, Some(access))?;
    cell.access.store(layout.access, Ordering::Release);
    cell.layout
        .store(std::ptr::from_ref(layout).cast_mut(), Ordering::Release);
    Ok(layout)
}

/// The layout registered into `cell`.
///
/// # Safety
///
/// As for [`celled_plain_layout`].
pub unsafe fn registered_plain_layout(
    cell: *const PlainLayoutCell,
) -> Result<&'static PlainLayout, String> {
    let cell = unsafe { cell.as_ref() }.ok_or("direct runtime received a null layout cell")?;
    let stored = cell.layout.load(Ordering::Acquire);
    // SAFETY: as in `celled_plain_layout`.
    unsafe { stored.as_ref() }
        .ok_or_else(|| "a plain layout was used before it was registered".into())
}

/// Writes `value` into a plain layout without consuming it.
///
/// # Safety
///
/// [`check`] accepted `value` for `id`, `id`'s layout holds no handle and
/// no heap block, and `dst` is valid for its bytes at its alignment.
unsafe fn write_plain(table: &LayoutTable, id: LayoutId, value: &Value, dst: *mut u8) {
    let layout = table.get(id);
    unsafe {
        match (&layout.kind, value) {
            (LayoutKind::Unit, _) => {}
            (LayoutKind::Scalar(ScalarLayout::Bool), Value::Bool(flag)) => {
                dst.write(u8::from(*flag))
            }
            (LayoutKind::Scalar(ScalarLayout::Float32), Value::Float(float)) => {
                dst.cast::<f32>().write(*float as f32)
            }
            (LayoutKind::Scalar(ScalarLayout::Float64), Value::Float(float)) => {
                dst.cast::<f64>().write(*float)
            }
            (LayoutKind::Scalar(ScalarLayout::Duration), Value::Duration(nanos)) => {
                dst.cast::<i128>().write_unaligned(*nanos)
            }
            (LayoutKind::Scalar(scalar), Value::Int(integer)) => {
                let bits = integer_bits(*scalar, *integer).expect("checked to fit");
                match scalar.size() {
                    1 => dst.write(bits as u8),
                    2 => dst.cast::<u16>().write(bits as u16),
                    4 => dst.cast::<u32>().write(bits as u32),
                    8 => dst.cast::<u64>().write(bits as u64),
                    _ => dst.cast::<u128>().write_unaligned(bits),
                }
            }
            (
                LayoutKind::Struct {
                    class: Some(_),
                    fields,
                },
                Value::Instance(instance),
            ) => {
                for field in fields {
                    let field_value = instance
                        .fields
                        .get(&field.name)
                        .expect("checked to hold every field");
                    write_plain(
                        table,
                        field.layout,
                        &field_value,
                        dst.add(field.offset as usize),
                    );
                }
            }
            (
                LayoutKind::Struct {
                    class: None,
                    fields,
                },
                Value::Tuple(tuple),
            ) => {
                for (field, element) in fields.iter().zip(&tuple.elements) {
                    write_plain(table, field.layout, element, dst.add(field.offset as usize));
                }
            }
            (
                LayoutKind::Tagged {
                    enum_name: Some(_),
                    tag_width,
                    variants,
                    ..
                },
                Value::EnumVariant(variant),
            ) => {
                let index = variants
                    .iter()
                    .position(|laid| laid.name == *variant.variant_name)
                    .expect("checked to name a variant");
                write_tag(dst, *tag_width, index);
                for (field, payload) in variants[index].fields.iter().zip(&variant.payloads) {
                    write_plain(table, field.layout, payload, dst.add(field.offset as usize));
                }
            }
            (
                LayoutKind::Tagged {
                    enum_name: None,
                    tag_width,
                    variants,
                    ..
                },
                Value::Union(union),
            ) => {
                write_tag(dst, *tag_width, union.member_index);
                let field = &variants[union.member_index].fields[0];
                write_plain(
                    table,
                    field.layout,
                    &union.payload,
                    dst.add(field.offset as usize),
                );
            }
            _ => unreachable!("checked plain values fit their layout"),
        }
    }
}

/// A new box for a handle, which no ledger links: the layout that stores
/// it owns it. It carries the handle's type, as a box generated code makes
/// for a value of that static type does. An erased handle carries none.
/// Like a list element's box, it is reached only through what holds it, so
/// it records no thread.
fn untracked_box(value: Value, ty: &Type) -> *mut OpaqueValue {
    let boxed = element_boxed_value(value);
    if !names_type_param(ty) && !is_unknown_runtime_type(ty) {
        let tag = shared_runtime_type_tag(canonical_runtime_type_name(ty).as_bytes());
        // SAFETY: the box is new and confined to this thread.
        unsafe { set_explicit_runtime_type_tag(boxed, &tag) };
    }
    boxed
}

/// The box of a field an instance keeps in its layout (see
/// `native_class_layouts.rs`).
pub(super) fn field_box_for(value: Value, ty: &Type) -> ElementBox {
    // SAFETY: the box is new, and its one reference is the field's.
    unsafe { ElementBox::from_raw(untracked_box(value, ty)) }
}

/// Whether `value` has the shape of `id`'s layout, all the way down.
pub(super) fn fits(table: &LayoutTable, id: LayoutId, value: &Value) -> bool {
    check(table, id, value).is_ok()
}

/// Writes `value` into the dead bytes at `dst`.
///
/// # Safety
///
/// [`fits`] accepted `value` for `id`, and `dst` is valid for the layout's
/// bytes at its alignment.
pub(super) unsafe fn write_unchecked(
    table: &LayoutTable,
    id: LayoutId,
    value: Value,
    dst: *mut u8,
) {
    unsafe { write(table, id, value, dst) }
}

/// A copy of the value of `id` at `src`, which stays where it is.
///
/// # Safety
///
/// `src` holds a value of `id`'s layout.
pub(super) unsafe fn read_at(table: &LayoutTable, id: LayoutId, src: *const u8) -> Value {
    unsafe { read(table, id, src) }
}

/// Moves the value of `id` at `src` out. Its bytes are dead afterwards.
///
/// # Safety
///
/// `src` holds a value of `id`'s layout, and nothing reads it again.
pub(super) unsafe fn take_at(table: &LayoutTable, id: LayoutId, src: *mut u8) -> Value {
    unsafe { take(table, id, src) }
}

/// Drops the value of `id` at `ptr`. Its bytes are dead afterwards.
///
/// # Safety
///
/// As for [`take_at`].
pub(super) unsafe fn drop_at(table: &LayoutTable, id: LayoutId, ptr: *mut u8) {
    unsafe { drop_in_place(table, id, ptr) }
}

/// Writes a copy of the value of `id` at `src` into the dead bytes at `dst`.
///
/// # Safety
///
/// `src` holds a value of `id`'s layout, and `dst` is valid for its bytes
/// at its alignment and does not overlap `src`.
pub(super) unsafe fn clone_at(table: &LayoutTable, id: LayoutId, src: *const u8, dst: *mut u8) {
    unsafe { clone_into(table, id, src, dst) }
}

/// Gives a class or enum value read out of a layout the type arguments its
/// type names, as tagging a box of that type does.
fn retag_generic(value: &mut Value, ty: &Type) {
    if matches!(ty, Type::Named(_, args) if !args.is_empty()) {
        retag_value(
            value,
            &shared_runtime_type_tag(canonical_runtime_type_name(ty).as_bytes()),
        );
    }
}

fn block_layout(table: &LayoutTable, target: LayoutId) -> AllocLayout {
    let layout = table.get(target);
    AllocLayout::from_size_align(layout.size.max(1) as usize, layout.align as usize)
        .expect("layouts keep sizes and alignments an allocation accepts")
}

fn allocate_block(table: &LayoutTable, target: LayoutId) -> *mut u8 {
    let layout = block_layout(table, target);
    // SAFETY: the size is at least one byte.
    let block = unsafe { alloc(layout) };
    if block.is_null() {
        handle_alloc_error(layout);
    }
    block
}

/// # Safety
///
/// `block` came from [`allocate_block`] for `target`, and its value is
/// dead.
unsafe fn free_block(table: &LayoutTable, target: LayoutId, block: *mut u8) {
    unsafe { dealloc(block, block_layout(table, target)) };
}

fn integer_kind(scalar: ScalarLayout) -> Option<IntegerKind> {
    Some(match scalar {
        ScalarLayout::Int8 => IntegerKind::Int8,
        ScalarLayout::Int16 => IntegerKind::Int16,
        ScalarLayout::Int32 => IntegerKind::Int32,
        ScalarLayout::Int64 => IntegerKind::Int64,
        ScalarLayout::Int128 => IntegerKind::Int128,
        ScalarLayout::IntSize => IntegerKind::IntSize,
        ScalarLayout::Uint8 => IntegerKind::Uint8,
        ScalarLayout::Uint16 => IntegerKind::Uint16,
        ScalarLayout::Uint32 => IntegerKind::Uint32,
        ScalarLayout::Uint64 => IntegerKind::Uint64,
        ScalarLayout::Uint128 => IntegerKind::Uint128,
        ScalarLayout::UintSize => IntegerKind::UintSize,
        ScalarLayout::Bool
        | ScalarLayout::Float32
        | ScalarLayout::Float64
        | ScalarLayout::Duration => return None,
    })
}

/// The integer a scalar layout keeps, as the bits of its width, when the
/// integer fits.
fn integer_bits(scalar: ScalarLayout, value: IntegerValue) -> Option<u128> {
    let kind = integer_kind(scalar)?;
    value
        .fits_bounds(kind.bounds())
        .then_some(match value.representation() {
            IntegerRepresentation::Signed(bits) => bits as u128,
            IntegerRepresentation::Unsigned(bits) => bits,
        })
}

/// Whether `value` has the shape of `id`'s layout, all the way down.
fn check(table: &LayoutTable, id: LayoutId, value: &Value) -> Result<(), String> {
    let layout = table.get(id);
    let mismatch = || {
        Err(format!(
            "a `{}` value does not fit the layout of `{}`",
            value_type_name(value),
            layout.ty
        ))
    };
    match (&layout.kind, value) {
        (LayoutKind::Unit, Value::Unit) | (LayoutKind::Handle, _) => Ok(()),
        (LayoutKind::Scalar(ScalarLayout::Bool), Value::Bool(_)) => Ok(()),
        (LayoutKind::Scalar(ScalarLayout::Float32 | ScalarLayout::Float64), Value::Float(_)) => {
            Ok(())
        }
        (LayoutKind::Scalar(ScalarLayout::Duration), Value::Duration(_)) => Ok(()),
        (LayoutKind::Scalar(scalar), Value::Int(integer)) if scalar.is_integer() => {
            if integer_bits(*scalar, *integer).is_some() {
                Ok(())
            } else {
                Err(format!(
                    "integer value `{integer}` does not fit in `{}`",
                    scalar.name()
                ))
            }
        }
        (LayoutKind::Indirect(target), _) => check(table, *target, value),
        (
            LayoutKind::Struct {
                class: Some(class),
                fields,
            },
            Value::Instance(instance),
        ) if nominal_runtime_base_name(&instance.class_name) == class => {
            // A generic class's value also names its type arguments.
            let named = instance
                .fields
                .keys()
                .filter(|name| *name != DIRECT_RUNTIME_TYPE_FIELD)
                .count();
            if named != fields.len() {
                return mismatch();
            }
            for field in fields {
                let Some(field_value) = instance.fields.get(&field.name) else {
                    return mismatch();
                };
                check(table, field.layout, &field_value)?;
            }
            Ok(())
        }
        (
            LayoutKind::Struct {
                class: None,
                fields,
            },
            Value::Tuple(tuple),
        ) if tuple.elements.len() == fields.len() => {
            for (field, element) in fields.iter().zip(&tuple.elements) {
                check(table, field.layout, element)?;
            }
            Ok(())
        }
        (
            LayoutKind::Tagged {
                enum_name: Some(name),
                variants,
                ..
            },
            Value::EnumVariant(variant),
        ) if nominal_runtime_base_name(&variant.enum_name) == name => {
            let Some(laid) = variants
                .iter()
                .find(|laid| laid.name == *variant.variant_name)
            else {
                return mismatch();
            };
            if laid.fields.len() != variant.payloads.len() {
                return mismatch();
            }
            for (field, payload) in laid.fields.iter().zip(&variant.payloads) {
                check(table, field.layout, payload)?;
            }
            Ok(())
        }
        (
            LayoutKind::Tagged {
                enum_name: None,
                variants,
                ..
            },
            Value::Union(union),
        ) => match variants.get(union.member_index) {
            Some(laid) => check(table, laid.fields[0].layout, &union.payload),
            None => mismatch(),
        },
        _ => mismatch(),
    }
}

unsafe fn write_tag(dst: *mut u8, width: u8, tag: usize) {
    unsafe {
        match width {
            1 => dst.write(tag as u8),
            2 => dst.cast::<u16>().write(tag as u16),
            _ => dst.cast::<u32>().write(tag as u32),
        }
    }
}

unsafe fn read_tag(src: *const u8, width: u8) -> usize {
    unsafe {
        match width {
            1 => usize::from(src.read()),
            2 => usize::from(src.cast::<u16>().read()),
            _ => src.cast::<u32>().read() as usize,
        }
    }
}

/// # Safety
///
/// [`check`] accepted `value` for `id`, and `dst` is valid for the
/// layout's bytes at its alignment.
unsafe fn write(table: &LayoutTable, id: LayoutId, value: Value, dst: *mut u8) {
    let layout = table.get(id);
    unsafe {
        match (&layout.kind, value) {
            (LayoutKind::Unit, _) => {}
            (LayoutKind::Scalar(ScalarLayout::Bool), Value::Bool(flag)) => {
                dst.write(u8::from(flag))
            }
            (LayoutKind::Scalar(ScalarLayout::Float32), Value::Float(float)) => {
                dst.cast::<f32>().write(float as f32)
            }
            (LayoutKind::Scalar(ScalarLayout::Float64), Value::Float(float)) => {
                dst.cast::<f64>().write(float)
            }
            (LayoutKind::Scalar(ScalarLayout::Duration), Value::Duration(nanos)) => {
                dst.cast::<i128>().write(nanos)
            }
            (LayoutKind::Scalar(scalar), Value::Int(integer)) => {
                let bits = integer_bits(*scalar, integer).expect("checked to fit");
                match scalar.size() {
                    1 => dst.write(bits as u8),
                    2 => dst.cast::<u16>().write(bits as u16),
                    4 => dst.cast::<u32>().write(bits as u32),
                    8 => dst.cast::<u64>().write(bits as u64),
                    _ => dst.cast::<u128>().write(bits),
                }
            }
            (LayoutKind::Handle, value) => {
                dst.cast::<*mut OpaqueValue>()
                    .write(untracked_box(value, &layout.ty));
            }
            (LayoutKind::Indirect(target), value) => {
                let block = allocate_block(table, *target);
                write(table, *target, value, block);
                dst.cast::<*mut u8>().write(block);
            }
            (
                LayoutKind::Struct {
                    class: Some(_),
                    fields,
                },
                Value::Instance(mut instance),
            ) => {
                for field in fields {
                    let field_value = instance
                        .fields
                        .remove(&field.name)
                        .expect("checked to hold every field");
                    write(
                        table,
                        field.layout,
                        field_value,
                        dst.add(field.offset as usize),
                    );
                }
            }
            (
                LayoutKind::Struct {
                    class: None,
                    fields,
                },
                Value::Tuple(tuple),
            ) => {
                for (field, element) in fields.iter().zip(tuple.elements) {
                    write(table, field.layout, element, dst.add(field.offset as usize));
                }
            }
            (
                LayoutKind::Tagged {
                    enum_name: Some(_),
                    tag_width,
                    variants,
                    ..
                },
                Value::EnumVariant(variant),
            ) => {
                let index = variants
                    .iter()
                    .position(|laid| laid.name == *variant.variant_name)
                    .expect("checked to name a variant");
                write_tag(dst, *tag_width, index);
                for (field, payload) in variants[index].fields.iter().zip(variant.payloads) {
                    write(table, field.layout, payload, dst.add(field.offset as usize));
                }
            }
            (
                LayoutKind::Tagged {
                    enum_name: None,
                    tag_width,
                    variants,
                    ..
                },
                Value::Union(union),
            ) => {
                let union = *union;
                write_tag(dst, *tag_width, union.member_index);
                let field = &variants[union.member_index].fields[0];
                write(
                    table,
                    field.layout,
                    union.payload,
                    dst.add(field.offset as usize),
                );
            }
            _ => unreachable!("checked values fit their layout"),
        }
    }
}

/// How [`visit`] leaves the value it reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Visit {
    /// Copies the value and leaves it in place.
    Copy,
    /// Moves the value out, freeing what it owned.
    Take,
}

/// # Safety
///
/// `src` holds a value of `id`'s layout.
unsafe fn read(table: &LayoutTable, id: LayoutId, src: *const u8) -> Value {
    unsafe { visit(table, id, src.cast_mut(), Visit::Copy) }
}

/// # Safety
///
/// `src` holds a value of `id`'s layout, and nothing reads it again.
unsafe fn take(table: &LayoutTable, id: LayoutId, src: *mut u8) -> Value {
    unsafe { visit(table, id, src, Visit::Take) }
}

unsafe fn visit(table: &LayoutTable, id: LayoutId, src: *mut u8, mode: Visit) -> Value {
    let layout = table.get(id);
    unsafe {
        match &layout.kind {
            LayoutKind::Unit => Value::Unit,
            LayoutKind::Scalar(ScalarLayout::Bool) => Value::Bool(src.read() != 0),
            LayoutKind::Scalar(ScalarLayout::Float32) => {
                Value::Float(f64::from(src.cast::<f32>().read()))
            }
            LayoutKind::Scalar(ScalarLayout::Float64) => Value::Float(src.cast::<f64>().read()),
            LayoutKind::Scalar(ScalarLayout::Duration) => {
                Value::Duration(src.cast::<i128>().read())
            }
            LayoutKind::Scalar(scalar) => {
                let kind = integer_kind(*scalar).expect("the other scalars are integers");
                let integer = if kind.is_signed() {
                    let bits = match scalar.size() {
                        1 => i128::from(src.cast::<i8>().read()),
                        2 => i128::from(src.cast::<i16>().read()),
                        4 => i128::from(src.cast::<i32>().read()),
                        8 => i128::from(src.cast::<i64>().read()),
                        _ => src.cast::<i128>().read(),
                    };
                    IntegerValue::from_typed_signed(bits, kind)
                } else {
                    let bits = match scalar.size() {
                        1 => u128::from(src.read()),
                        2 => u128::from(src.cast::<u16>().read()),
                        4 => u128::from(src.cast::<u32>().read()),
                        8 => u128::from(src.cast::<u64>().read()),
                        _ => src.cast::<u128>().read(),
                    };
                    IntegerValue::from_typed_unsigned(bits, kind)
                };
                Value::Int(integer.expect("a scalar of a width fits its own kind"))
            }
            LayoutKind::Handle => {
                let boxed = src.cast::<*mut OpaqueValue>().read();
                if boxed.is_null() {
                    runtime_error("a value was read after it moved out");
                }
                match mode {
                    // The layout's holder covers the box.
                    Visit::Copy => (*boxed).held_value().clone(),
                    Visit::Take => consume_owned_untracked_value(boxed),
                }
            }
            LayoutKind::Indirect(target) => {
                let block = src.cast::<*mut u8>().read();
                let value = visit(table, *target, block, mode);
                if mode == Visit::Take {
                    free_block(table, *target, block);
                }
                value
            }
            LayoutKind::Struct {
                class: Some(class),
                fields,
            } => {
                let mut value = Value::Instance(InstanceValue {
                    class_name: class.clone(),
                    fields: InstanceFields::Map(
                        fields
                            .iter()
                            .map(|field| {
                                (
                                    field.name.clone(),
                                    visit(
                                        table,
                                        field.layout,
                                        src.add(field.offset as usize),
                                        mode,
                                    ),
                                )
                            })
                            .collect(),
                    ),
                });
                retag_generic(&mut value, &layout.ty);
                value
            }
            LayoutKind::Struct {
                class: None,
                fields,
            } => {
                let Type::Tuple(element_types) = &layout.ty else {
                    unreachable!("an unnamed struct lays out a tuple");
                };
                Value::Tuple(TupleValue {
                    element_types: element_types.clone(),
                    elements: fields
                        .iter()
                        .map(|field| {
                            visit(table, field.layout, src.add(field.offset as usize), mode)
                        })
                        .collect(),
                })
            }
            LayoutKind::Tagged {
                enum_name,
                tag_width,
                variants,
                ..
            } => {
                let index = read_tag(src, *tag_width);
                let laid = &variants[index];
                let mut payloads = laid
                    .fields
                    .iter()
                    .map(|field| visit(table, field.layout, src.add(field.offset as usize), mode))
                    .collect::<Vec<_>>();
                match enum_name {
                    Some(name) => {
                        let mut value = Value::EnumVariant(EnumVariantValue {
                            enum_name: name.as_str().into(),
                            variant_name: laid.name.as_str().into(),
                            payloads,
                        });
                        retag_generic(&mut value, &layout.ty);
                        value
                    }
                    None => union_value(
                        layout.ty.clone(),
                        index,
                        payloads.pop().expect("a union member has one payload"),
                    ),
                }
            }
        }
    }
}

/// # Safety
///
/// `ptr` holds a value of `id`'s layout, and nothing reads it again.
unsafe fn drop_in_place(table: &LayoutTable, id: LayoutId, ptr: *mut u8) {
    let layout = table.get(id);
    unsafe {
        match &layout.kind {
            LayoutKind::Unit | LayoutKind::Scalar(_) => {}
            LayoutKind::Handle => release_untracked_value(ptr.cast::<*mut OpaqueValue>().read()),
            LayoutKind::Indirect(target) => {
                let block = ptr.cast::<*mut u8>().read();
                drop_in_place(table, *target, block);
                free_block(table, *target, block);
            }
            LayoutKind::Struct { fields, .. } => {
                for field in fields {
                    drop_in_place(table, field.layout, ptr.add(field.offset as usize));
                }
            }
            LayoutKind::Tagged {
                tag_width,
                variants,
                ..
            } => {
                let index = read_tag(ptr, *tag_width);
                for field in &variants[index].fields {
                    drop_in_place(table, field.layout, ptr.add(field.offset as usize));
                }
            }
        }
    }
}

/// # Safety
///
/// `src` holds a value of `id`'s layout, and `dst` is valid for its bytes
/// at its alignment and does not overlap `src`.
unsafe fn clone_into(table: &LayoutTable, id: LayoutId, src: *const u8, dst: *mut u8) {
    let layout = table.get(id);
    unsafe {
        match &layout.kind {
            LayoutKind::Unit => {}
            LayoutKind::Scalar(_) => std::ptr::copy_nonoverlapping(src, dst, layout.size as usize),
            LayoutKind::Handle => {
                let boxed = src.cast::<*mut OpaqueValue>().read();
                // A moved-out handle stays moved out in the copy.
                let copy = if boxed.is_null() {
                    boxed
                } else {
                    // The layout's holder covers the box, so neither read
                    // takes an access of its own.
                    let copy = element_boxed_value((*boxed).held_value().clone());
                    let tag = (*(*boxed).runtime_type_name.get()).clone();
                    // SAFETY: the copy is new and confined to this thread.
                    *(*copy).runtime_type_name.get() = tag;
                    copy
                };
                dst.cast::<*mut OpaqueValue>().write(copy);
            }
            LayoutKind::Indirect(target) => {
                let block = allocate_block(table, *target);
                clone_into(table, *target, src.cast::<*const u8>().read(), block);
                dst.cast::<*mut u8>().write(block);
            }
            LayoutKind::Struct { fields, .. } => {
                for field in fields {
                    let offset = field.offset as usize;
                    clone_into(table, field.layout, src.add(offset), dst.add(offset));
                }
            }
            LayoutKind::Tagged {
                tag_width,
                variants,
                ..
            } => {
                let index = read_tag(src, *tag_width);
                write_tag(dst, *tag_width, index);
                for field in &variants[index].fields {
                    let offset = field.offset as usize;
                    clone_into(table, field.layout, src.add(offset), dst.add(offset));
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "native_typed_layout_tests.rs"]
mod tests;
