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

use aura_core::integer::{IntegerKind, IntegerRepresentation, IntegerValue};
use aura_core::layout::{
    decode_descriptor, names_type_param, LayoutBuilder, LayoutId, LayoutKind, LayoutTable,
    ScalarLayout,
};
use aura_core::types::Type;

use super::{
    canonical_runtime_type_name, consume_owned_untracked_value, explicit_runtime_type_tag,
    is_unknown_runtime_type, release_untracked_value, retag_value, set_explicit_runtime_type_tag,
    shared_runtime_type_tag, untracked_boxed_value, value_ref, value_type_name, OpaqueValue,
};
use crate::runtime_value::{
    nominal_runtime_base_name, union_value, EnumVariantValue, InstanceValue, TupleValue, Value,
    DIRECT_RUNTIME_TYPE_FIELD,
};

/// The layout of one type, with every layout it reaches.
pub(crate) struct TypedLayout {
    table: LayoutTable,
    root: LayoutId,
}

impl TypedLayout {
    /// Lays out the type a descriptor names, with the definitions it
    /// carries.
    pub(crate) fn from_descriptor(descriptor: &str) -> Result<Self, String> {
        let (ty, env) = decode_descriptor(descriptor)?;
        let mut builder = LayoutBuilder::new(&env);
        let root = builder.layout(&ty).map_err(|error| error.0)?;
        Ok(Self {
            table: builder.into_table(),
            root,
        })
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

/// A new box for a handle, which no ledger links: the layout that stores
/// it owns it. It carries the handle's type, as a box generated code makes
/// for a value of that static type does. An erased handle carries none.
fn untracked_box(value: Value, ty: &Type) -> *mut OpaqueValue {
    let boxed = untracked_boxed_value(value, None);
    if !names_type_param(ty) && !is_unknown_runtime_type(ty) {
        let tag = shared_runtime_type_tag(canonical_runtime_type_name(ty).as_bytes());
        // SAFETY: the box is new and confined to this thread.
        unsafe { set_explicit_runtime_type_tag(boxed, &tag) };
    }
    boxed
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
                .filter(|name| name.as_str() != DIRECT_RUNTIME_TYPE_FIELD)
                .count();
            if named != fields.len() {
                return mismatch();
            }
            for field in fields {
                let Some(field_value) = instance.fields.get(&field.name) else {
                    return mismatch();
                };
                check(table, field.layout, field_value)?;
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
                match mode {
                    Visit::Copy => value_ref(boxed),
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
                    fields: fields
                        .iter()
                        .map(|field| {
                            (
                                field.name.clone(),
                                visit(table, field.layout, src.add(field.offset as usize), mode),
                            )
                        })
                        .collect(),
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
                let copy =
                    untracked_boxed_value(value_ref(boxed), explicit_runtime_type_tag(boxed));
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
