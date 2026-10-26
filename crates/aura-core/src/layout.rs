//! Native memory layouts of Aura types.
//!
//! Native code keeps each value of a concrete type in one fixed layout, as
//! Rust does. This module computes that layout. The compiler reads it to
//! place fields and elements, and the runtime reads it to convert a value
//! between its layout and the boxed `Value` that `aura run`, host calls, and
//! FFI use. Both compute layouts with this one module, so they agree on
//! every offset. See `architecture_docs/24-typed-layouts-design-note.md`.
//!
//! The rules:
//!
//! - **Scalars** take their width: one byte for `bool`, `int8`, and
//!   `uint8`, up to sixteen for `int128`, `uint128`, and `Duration`, and a
//!   pointer for `intsize` and `uintsize`. `None` takes no bytes.
//! - **Classes and tuples** are structs. Each field sits at the next offset
//!   its alignment allows, in declaration order, and the size rounds up to
//!   the largest alignment.
//! - **Enums and unions** are a tag followed by a payload area. The tag is
//!   the variant's declaration index, or the member's index in canonical
//!   order, in the smallest unsigned width that holds every tag. Each
//!   variant lays its payloads out as a struct at the start of the payload
//!   area, so the variants overlap.
//! - **Handles.** A runtime object, such as `str`, a collection, a file, a
//!   task, or a callable, is one pointer to its box. So is any type that
//!   names a type parameter, which generic code keeps erased.
//! - **Recursion.** Classes and enums that reach each other through fields,
//!   payloads, tuple elements, or union members form a cycle: a strongly
//!   connected component of the graph of instantiated classes and enums.
//!   Inside the definition of a type in a cycle, each place that holds
//!   another type of the same cycle holds a pointer to a heap block with
//!   that value instead, as `Box` does in Rust. The cycles depend only on
//!   the program, so a type's layout does not depend on where a walk
//!   starts. A tuple or union inside such a definition gets its own storage
//!   layout with the pointer, and every other use keeps the plain layout.
//! - **Budget.** A class or enum that reaches more instantiated classes
//!   and enums than [`MAX_LAYOUT_INSTANTIATIONS`], or one whose type
//!   arguments nest deeper than [`MAX_LAYOUT_TYPE_DEPTH`], stays a handle.
//!   Only a type that names itself with an ever larger type argument
//!   reaches either limit.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::type_descriptor::{self, Reader};
use crate::types::{substitute_type_in_context, Type};

/// The version of the layout rules. A descriptor names it, and it changes
/// whenever a rule does.
pub const LAYOUT_VERSION: u32 = 1;

/// The prefix of every layout descriptor.
pub const LAYOUT_DESCRIPTOR_PREFIX: &str = "aura-layout-v1:";

/// The instantiated classes and enums a class or enum may reach before it
/// stays a handle.
pub const MAX_LAYOUT_INSTANTIATIONS: usize = 512;

/// How deeply the type arguments of a reached class or enum may nest before
/// the class or enum that reaches it stays a handle. A program only writes
/// shallow types, so only a type that names itself with a growing argument
/// reaches the limit, and it does so before the types grow large.
pub const MAX_LAYOUT_TYPE_DEPTH: usize = 32;

/// The size and alignment of a pointer on every native target Aura builds
/// for.
pub const POINTER_BYTES: u64 = 8;

/// A scalar's width and kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScalarLayout {
    Bool,
    Int8,
    Int16,
    Int32,
    Int64,
    Int128,
    IntSize,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Uint128,
    UintSize,
    Float32,
    Float64,
    /// A `Duration`: signed nanoseconds in sixteen bytes.
    Duration,
}

impl ScalarLayout {
    /// The scalar a type name denotes.
    pub fn of_name(name: &str) -> Option<Self> {
        Some(match name {
            "bool" => Self::Bool,
            "int8" => Self::Int8,
            "int16" => Self::Int16,
            "int32" => Self::Int32,
            "int64" => Self::Int64,
            "int128" => Self::Int128,
            "intsize" => Self::IntSize,
            "uint8" => Self::Uint8,
            "uint16" => Self::Uint16,
            "uint32" => Self::Uint32,
            "uint64" => Self::Uint64,
            "uint128" => Self::Uint128,
            "uintsize" => Self::UintSize,
            "float32" => Self::Float32,
            "float64" => Self::Float64,
            "Duration" => Self::Duration,
            _ => return None,
        })
    }

    /// The type name, as Aura spells it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int8 => "int8",
            Self::Int16 => "int16",
            Self::Int32 => "int32",
            Self::Int64 => "int64",
            Self::Int128 => "int128",
            Self::IntSize => "intsize",
            Self::Uint8 => "uint8",
            Self::Uint16 => "uint16",
            Self::Uint32 => "uint32",
            Self::Uint64 => "uint64",
            Self::Uint128 => "uint128",
            Self::UintSize => "uintsize",
            Self::Float32 => "float32",
            Self::Float64 => "float64",
            Self::Duration => "Duration",
        }
    }

    /// The size in bytes, which is also the alignment.
    pub fn size(self) -> u64 {
        match self {
            Self::Bool | Self::Int8 | Self::Uint8 => 1,
            Self::Int16 | Self::Uint16 => 2,
            Self::Int32 | Self::Uint32 | Self::Float32 => 4,
            Self::Int64 | Self::Uint64 | Self::Float64 => 8,
            Self::IntSize | Self::UintSize => POINTER_BYTES,
            Self::Int128 | Self::Uint128 | Self::Duration => 16,
        }
    }

    /// Whether the scalar is an integer kind.
    pub fn is_integer(self) -> bool {
        !matches!(
            self,
            Self::Bool | Self::Float32 | Self::Float64 | Self::Duration
        )
    }
}

/// A class as layouts read it: its type parameters and its fields in
/// declaration order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClassDefinition {
    pub type_params: Vec<String>,
    pub fields: Vec<(String, Type)>,
}

/// An enum as layouts read it: its type parameters and its variants in
/// declaration order, each with its payload types.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnumDefinition {
    pub type_params: Vec<String>,
    pub variants: Vec<(String, Vec<Type>)>,
}

/// The classes and enums a program defines, by the name its types use.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayoutEnv {
    pub classes: BTreeMap<String, ClassDefinition>,
    pub enums: BTreeMap<String, EnumDefinition>,
}

/// One layout in a [`LayoutTable`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LayoutId(u32);

impl LayoutId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// The layout of one type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The type laid out. A storage layout inside a cycle names the type it
    /// stores.
    pub ty: Type,
    pub size: u64,
    pub align: u64,
    pub kind: LayoutKind,
}

/// What a layout holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutKind {
    /// `None`: no bytes.
    Unit,
    Scalar(ScalarLayout),
    /// One pointer to the value's box.
    Handle,
    /// One pointer to a heap block that holds a value of the target layout.
    Indirect(LayoutId),
    /// A class, named, or a tuple, unnamed: its fields in order.
    Struct {
        class: Option<String>,
        fields: Vec<FieldLayout>,
    },
    /// An enum, named, or a union, unnamed: a tag of `tag_width` bytes at
    /// offset 0, and one variant per tag. A union member is a variant named
    /// by its canonical key, with one field.
    Tagged {
        enum_name: Option<String>,
        tag_width: u8,
        payload_offset: u64,
        variants: Vec<VariantLayout>,
    },
}

/// A field, element, or payload: its offset from the start of the value
/// that holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldLayout {
    pub name: String,
    pub offset: u64,
    pub layout: LayoutId,
}

/// One variant of a tagged layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariantLayout {
    pub name: String,
    pub fields: Vec<FieldLayout>,
}

/// Why a type has no layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutError(pub String);

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Every layout one builder computed. Layouts name each other by
/// [`LayoutId`], so a cycle through [`LayoutKind::Indirect`] needs no
/// shared ownership.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayoutTable {
    layouts: Vec<Layout>,
}

impl LayoutTable {
    pub fn get(&self, id: LayoutId) -> &Layout {
        &self.layouts[id.index()]
    }

    pub fn len(&self) -> usize {
        self.layouts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.layouts.is_empty()
    }

    pub fn ids(&self) -> impl Iterator<Item = LayoutId> {
        (0..self.layouts.len() as u32).map(LayoutId)
    }

    /// Whether dropping a value of `id` must release something: a handle,
    /// a heap block, or a value holding one.
    pub fn needs_drop(&self, id: LayoutId) -> bool {
        let mut seen = BTreeSet::new();
        self.needs_drop_inner(id, &mut seen)
    }

    fn needs_drop_inner(&self, id: LayoutId, seen: &mut BTreeSet<LayoutId>) -> bool {
        if !seen.insert(id) {
            return false;
        }
        match &self.get(id).kind {
            LayoutKind::Unit | LayoutKind::Scalar(_) => false,
            LayoutKind::Handle | LayoutKind::Indirect(_) => true,
            LayoutKind::Struct { fields, .. } => fields
                .iter()
                .any(|field| self.needs_drop_inner(field.layout, seen)),
            LayoutKind::Tagged { variants, .. } => variants.iter().any(|variant| {
                variant
                    .fields
                    .iter()
                    .any(|field| self.needs_drop_inner(field.layout, seen))
            }),
        }
    }

    /// Checks the rules every layout keeps: sizes are multiples of their
    /// alignment, alignments are powers of two, every field fits inside its
    /// value at an offset its alignment allows, a struct's fields do not
    /// overlap, a tag fits before the payload area, and every id names a
    /// layout of this table.
    pub fn validate(&self) -> Result<(), LayoutError> {
        for id in self.ids() {
            let layout = self.get(id);
            let fail =
                |reason: &str| Err(LayoutError(format!("layout of `{}` {reason}", layout.ty)));
            if !layout.align.is_power_of_two() {
                return fail("has an alignment that is not a power of two");
            }
            if !layout.size.is_multiple_of(layout.align) {
                return fail("has a size that is not a multiple of its alignment");
            }
            let check_fields = |fields: &[FieldLayout], start: u64, disjoint: bool| {
                let mut end = start;
                for field in fields {
                    let Some(inner) = self.layouts.get(field.layout.index()) else {
                        return Err(LayoutError(format!(
                            "layout of `{}` names a missing layout",
                            layout.ty
                        )));
                    };
                    if field.offset % inner.align != 0 || inner.align > layout.align {
                        return Err(LayoutError(format!(
                            "layout of `{}` misaligns field `{}`",
                            layout.ty, field.name
                        )));
                    }
                    if disjoint && field.offset < end {
                        return Err(LayoutError(format!(
                            "layout of `{}` overlaps field `{}`",
                            layout.ty, field.name
                        )));
                    }
                    if field.offset < start || field.offset + inner.size > layout.size {
                        return Err(LayoutError(format!(
                            "layout of `{}` puts field `{}` outside the value",
                            layout.ty, field.name
                        )));
                    }
                    end = field.offset + inner.size;
                }
                Ok(())
            };
            match &layout.kind {
                LayoutKind::Unit => {
                    if layout.size != 0 {
                        return fail("is a unit with bytes");
                    }
                }
                LayoutKind::Scalar(scalar) => {
                    if layout.size != scalar.size() || layout.align != scalar.size() {
                        return fail("has the wrong scalar size");
                    }
                }
                LayoutKind::Handle => {
                    if layout.size != POINTER_BYTES || layout.align != POINTER_BYTES {
                        return fail("is a handle that is not one pointer");
                    }
                }
                LayoutKind::Indirect(target) => {
                    if layout.size != POINTER_BYTES || layout.align != POINTER_BYTES {
                        return fail("is a heap pointer that is not one pointer");
                    }
                    if target.index() >= self.layouts.len() {
                        return fail("points to a missing layout");
                    }
                }
                LayoutKind::Struct { fields, .. } => check_fields(fields, 0, true)?,
                LayoutKind::Tagged {
                    tag_width,
                    payload_offset,
                    variants,
                    ..
                } => {
                    if !matches!(tag_width, 1 | 2 | 4) {
                        return fail("has a tag that is not 1, 2, or 4 bytes");
                    }
                    if *payload_offset < u64::from(*tag_width) {
                        return fail("overlaps its tag with its payload");
                    }
                    if tag_width_for(variants.len().max(1)) != *tag_width {
                        return fail("has a tag too narrow or wide for its variants");
                    }
                    for variant in variants {
                        check_fields(&variant.fields, *payload_offset, true)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// A rendering of `id`'s layout that names no ids, so layouts from two
    /// tables compare equal when they lay the type out the same way.
    pub fn render(&self, id: LayoutId) -> String {
        let mut out = String::new();
        // A pointer back to the root shows as `^1` too.
        let mut path = vec![id];
        self.render_into(id, &mut out, &mut path);
        out
    }

    fn render_into(&self, id: LayoutId, out: &mut String, path: &mut Vec<LayoutId>) {
        use std::fmt::Write as _;
        let layout = self.get(id);
        let _ = write!(out, "{}@{}/{}:", layout.ty, layout.size, layout.align);
        match &layout.kind {
            LayoutKind::Unit => out.push_str("unit"),
            LayoutKind::Scalar(scalar) => out.push_str(scalar.name()),
            LayoutKind::Handle => out.push_str("handle"),
            LayoutKind::Indirect(target) => {
                out.push_str("indirect(");
                if let Some(depth) = path.iter().position(|seen| seen == target) {
                    let _ = write!(out, "^{}", path.len() - depth);
                } else {
                    path.push(*target);
                    self.render_into(*target, out, path);
                    path.pop();
                }
                out.push(')');
            }
            LayoutKind::Struct { class, fields } => {
                let _ = write!(out, "struct {}{{", class.as_deref().unwrap_or("()"));
                for field in fields {
                    let _ = write!(out, "{}+{}=", field.name, field.offset);
                    self.render_into(field.layout, out, path);
                    out.push(';');
                }
                out.push('}');
            }
            LayoutKind::Tagged {
                enum_name,
                tag_width,
                payload_offset,
                variants,
            } => {
                let _ = write!(
                    out,
                    "tagged {} tag{} payload+{}{{",
                    enum_name.as_deref().unwrap_or("|"),
                    tag_width,
                    payload_offset
                );
                for variant in variants {
                    let _ = write!(out, "{}(", variant.name);
                    for field in &variant.fields {
                        let _ = write!(out, "{}+{}=", field.name, field.offset);
                        self.render_into(field.layout, out, path);
                        out.push(';');
                    }
                    out.push(')');
                }
                out.push('}');
            }
        }
    }
}

/// The tag width in bytes for a variant count: the smallest unsigned width
/// that holds every tag.
pub fn tag_width_for(variant_count: usize) -> u8 {
    if variant_count <= 1 << 8 {
        1
    } else if variant_count <= 1 << 16 {
        2
    } else {
        4
    }
}

fn round_up(value: u64, align: u64) -> u64 {
    value.div_ceil(align) * align
}

/// What the cycle walk learned about one instantiated class or enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cycle {
    /// The type reaches more instantiations than the budget allows, so it
    /// stays a handle.
    OverBudget,
    /// The type belongs to the cycle numbered so. `recursive` is false for
    /// a component of one type that does not hold itself.
    Member { cycle: usize, recursive: bool },
}

/// Computes layouts into one [`LayoutTable`], reusing each layout it
/// computed before.
pub struct LayoutBuilder<'env> {
    env: &'env LayoutEnv,
    table: LayoutTable,
    /// Plain layouts, by type key.
    plain: HashMap<String, LayoutId>,
    /// Storage layouts of tuples and unions inside one cycle's definitions,
    /// by cycle and type key.
    stored: HashMap<(usize, String), LayoutId>,
    /// Heap pointers, by the layout they point to.
    indirect: HashMap<LayoutId, LayoutId>,
    /// Each instantiated class and enum the cycle walk reached, by type key.
    cycles: HashMap<String, Cycle>,
    next_cycle: usize,
}

impl<'env> LayoutBuilder<'env> {
    pub fn new(env: &'env LayoutEnv) -> Self {
        Self {
            env,
            table: LayoutTable::default(),
            plain: HashMap::new(),
            stored: HashMap::new(),
            indirect: HashMap::new(),
            cycles: HashMap::new(),
            next_cycle: 0,
        }
    }

    pub fn table(&self) -> &LayoutTable {
        &self.table
    }

    pub fn into_table(self) -> LayoutTable {
        self.table
    }

    /// The layout of a value of `ty`.
    pub fn layout(&mut self, ty: &Type) -> Result<LayoutId, LayoutError> {
        self.layout_in(ty, None)
    }

    fn push(&mut self, layout: Layout) -> LayoutId {
        let id = LayoutId(self.table.layouts.len() as u32);
        self.table.layouts.push(layout);
        id
    }

    fn leaf(&mut self, ty: &Type, kind: LayoutKind, size: u64) -> LayoutId {
        let key = type_key(ty);
        if let Some(id) = self.plain.get(&key) {
            return *id;
        }
        let id = self.push(Layout {
            ty: ty.clone(),
            size,
            align: size.max(1),
            kind,
        });
        self.plain.insert(key, id);
        id
    }

    /// `ty`'s layout where it sits inside the definition of a type of
    /// `cycle`, or anywhere else when `cycle` is `None`.
    fn layout_in(&mut self, ty: &Type, cycle: Option<usize>) -> Result<LayoutId, LayoutError> {
        if names_type_param(ty) {
            return Ok(self.leaf(ty, LayoutKind::Handle, POINTER_BYTES));
        }
        match ty {
            Type::Unit => Ok(self.leaf(ty, LayoutKind::Unit, 0)),
            Type::ReturnedView(_) => Err(LayoutError(format!(
                "a returned view `{ty}` is not a value and has no layout"
            ))),
            Type::Named(name, args) if args.is_empty() && ScalarLayout::of_name(name).is_some() => {
                let scalar = ScalarLayout::of_name(name).expect("matched a scalar name");
                Ok(self.leaf(ty, LayoutKind::Scalar(scalar), scalar.size()))
            }
            Type::Named(..) if self.is_nominal(ty) => {
                let membership = self.cycle_of(ty);
                match membership {
                    Cycle::OverBudget => Ok(self.leaf(ty, LayoutKind::Handle, POINTER_BYTES)),
                    Cycle::Member {
                        cycle: own_cycle,
                        recursive,
                    } => {
                        if recursive && cycle == Some(own_cycle) {
                            let target = self.nominal_layout(ty, own_cycle, recursive)?;
                            return Ok(self.indirect_to(ty, target));
                        }
                        self.nominal_layout(ty, own_cycle, recursive)
                    }
                }
            }
            Type::Tuple(elements) => self.structural_layout(ty, cycle, |builder, cycle| {
                let mut fields = Vec::with_capacity(elements.len());
                for (index, element) in elements.iter().enumerate() {
                    fields.push((index.to_string(), builder.layout_in(element, cycle)?));
                }
                Ok(StructuralShape::Struct(fields))
            }),
            Type::Union(union) => self.structural_layout(ty, cycle, |builder, cycle| {
                let mut members = Vec::with_capacity(union.members.len());
                for (key, member) in union.keys.iter().zip(&union.members) {
                    members.push((key.clone(), builder.layout_in(member, cycle)?));
                }
                Ok(StructuralShape::Union(members))
            }),
            _ => Ok(self.leaf(ty, LayoutKind::Handle, POINTER_BYTES)),
        }
    }

    fn indirect_to(&mut self, ty: &Type, target: LayoutId) -> LayoutId {
        if let Some(id) = self.indirect.get(&target) {
            return *id;
        }
        let id = self.push(Layout {
            ty: ty.clone(),
            size: POINTER_BYTES,
            align: POINTER_BYTES,
            kind: LayoutKind::Indirect(target),
        });
        self.indirect.insert(target, id);
        id
    }

    /// A tuple's or union's layout. Inside a cycle's definitions, one that
    /// holds a type of that cycle gets a storage layout of its own; any
    /// other shares the plain layout.
    fn structural_layout(
        &mut self,
        ty: &Type,
        cycle: Option<usize>,
        shape: impl FnOnce(&mut Self, Option<usize>) -> Result<StructuralShape, LayoutError>,
    ) -> Result<LayoutId, LayoutError> {
        let key = type_key(ty);
        let cycle = cycle.filter(|cycle| self.holds_cycle_member(ty, *cycle));
        let cached = match cycle {
            Some(cycle) => self.stored.get(&(cycle, key.clone())).copied(),
            None => self.plain.get(&key).copied(),
        };
        if let Some(id) = cached {
            return Ok(id);
        }
        let layout = match shape(self, cycle)? {
            StructuralShape::Struct(fields) => self.struct_layout(ty, None, fields),
            StructuralShape::Union(members) => {
                let variants = members
                    .into_iter()
                    .map(|(key, member)| (key, vec![("0".to_string(), member)]))
                    .collect();
                self.tagged_layout(ty, None, variants)
            }
        };
        let id = self.push(layout);
        match cycle {
            Some(cycle) => self.stored.insert((cycle, key), id),
            None => self.plain.insert(key, id),
        };
        Ok(id)
    }

    /// Whether `ty` holds a class or enum of `cycle` where a tuple or union
    /// keeps its members inline.
    fn holds_cycle_member(&mut self, ty: &Type, cycle: usize) -> bool {
        let mut nominals = Vec::new();
        self.inline_nominals(ty, &mut nominals);
        nominals.iter().any(|nominal| {
            matches!(
                self.cycles.get(&type_key(nominal)),
                Some(Cycle::Member { cycle: member, recursive: true }) if *member == cycle
            )
        })
    }

    /// The plain layout of an instantiated class or enum.
    fn nominal_layout(
        &mut self,
        ty: &Type,
        cycle: usize,
        recursive: bool,
    ) -> Result<LayoutId, LayoutError> {
        let key = type_key(ty);
        if let Some(id) = self.plain.get(&key) {
            return Ok(*id);
        }
        // Reserve the id first, so a pointer back to this type from one of
        // its own fields names it.
        let id = self.push(Layout {
            ty: ty.clone(),
            size: 0,
            align: 1,
            kind: LayoutKind::Unit,
        });
        self.plain.insert(key, id);
        let inner = recursive.then_some(cycle);
        let Type::Named(name, _) = ty else {
            unreachable!("nominal types are named");
        };
        let layout = if let Some(fields) = self.class_fields(ty) {
            let mut laid = Vec::with_capacity(fields.len());
            for (field, field_ty) in fields {
                laid.push((field, self.layout_in(&field_ty, inner)?));
            }
            self.struct_layout(ty, Some(name.clone()), laid)
        } else {
            let variants = self.enum_variants(ty).unwrap_or_default();
            let mut laid = Vec::with_capacity(variants.len());
            for (variant, payloads) in variants {
                let mut fields = Vec::with_capacity(payloads.len());
                for (index, payload) in payloads.iter().enumerate() {
                    fields.push((index.to_string(), self.layout_in(payload, inner)?));
                }
                laid.push((variant, fields));
            }
            self.tagged_layout(ty, Some(name.clone()), laid)
        };
        self.table.layouts[id.index()] = layout;
        Ok(id)
    }

    fn struct_layout(
        &self,
        ty: &Type,
        class: Option<String>,
        fields: Vec<(String, LayoutId)>,
    ) -> Layout {
        let mut offset = 0;
        let mut align = 1;
        let mut laid = Vec::with_capacity(fields.len());
        for (name, layout) in fields {
            let inner = self.table.get(layout);
            offset = round_up(offset, inner.align);
            laid.push(FieldLayout {
                name,
                offset,
                layout,
            });
            offset += inner.size;
            align = align.max(inner.align);
        }
        Layout {
            ty: ty.clone(),
            size: round_up(offset, align),
            align,
            kind: LayoutKind::Struct {
                class,
                fields: laid,
            },
        }
    }

    fn tagged_layout(
        &self,
        ty: &Type,
        enum_name: Option<String>,
        variants: Vec<(String, Vec<(String, LayoutId)>)>,
    ) -> Layout {
        // An enum with no variants still takes a tag, so every tagged value
        // has one to read.
        let tag_width = tag_width_for(variants.len().max(1));
        let payload_align = variants
            .iter()
            .flat_map(|(_, fields)| fields.iter())
            .map(|(_, layout)| self.table.get(*layout).align)
            .max()
            .unwrap_or(1);
        let payload_offset = round_up(u64::from(tag_width), payload_align);
        let align = payload_align.max(u64::from(tag_width));
        let mut end = u64::from(tag_width);
        let mut laid = Vec::with_capacity(variants.len());
        for (name, fields) in variants {
            let mut offset = payload_offset;
            let mut placed = Vec::with_capacity(fields.len());
            for (field, layout) in fields {
                let inner = self.table.get(layout);
                offset = round_up(offset, inner.align);
                placed.push(FieldLayout {
                    name: field,
                    offset,
                    layout,
                });
                offset += inner.size;
            }
            end = end.max(offset);
            laid.push(VariantLayout {
                name,
                fields: placed,
            });
        }
        Layout {
            ty: ty.clone(),
            size: round_up(end, align),
            align,
            kind: LayoutKind::Tagged {
                enum_name,
                tag_width,
                payload_offset,
                variants: laid,
            },
        }
    }

    /// Whether `ty` is an instantiated class or enum of the environment.
    fn is_nominal(&self, ty: &Type) -> bool {
        let Type::Named(name, args) = ty else {
            return false;
        };
        if let Some(class) = self.env.classes.get(name) {
            return class.type_params.len() == args.len();
        }
        if let Some(enumeration) = self.env.enums.get(name) {
            return enumeration.type_params.len() == args.len();
        }
        false
    }

    /// The fields of an instantiated class, with type arguments
    /// substituted.
    fn class_fields(&self, ty: &Type) -> Option<Vec<(String, Type)>> {
        let Type::Named(name, args) = ty else {
            return None;
        };
        let class = self.env.classes.get(name)?;
        let substitutions = substitutions(&class.type_params, args);
        Some(
            class
                .fields
                .iter()
                .map(|(field, field_ty)| {
                    (
                        field.clone(),
                        substitute_type_in_context(field_ty, &substitutions, None),
                    )
                })
                .collect(),
        )
    }

    /// The variants of an instantiated enum, with type arguments
    /// substituted.
    fn enum_variants(&self, ty: &Type) -> Option<Vec<(String, Vec<Type>)>> {
        let Type::Named(name, args) = ty else {
            return None;
        };
        let enumeration = self.env.enums.get(name)?;
        let substitutions = substitutions(&enumeration.type_params, args);
        Some(
            enumeration
                .variants
                .iter()
                .map(|(variant, payloads)| {
                    (
                        variant.clone(),
                        payloads
                            .iter()
                            .map(|payload| {
                                substitute_type_in_context(payload, &substitutions, None)
                            })
                            .collect(),
                    )
                })
                .collect(),
        )
    }

    /// The instantiated classes and enums `ty` holds inline: itself when it
    /// is one, and the members of the tuples and unions it is made of.
    fn inline_nominals(&self, ty: &Type, out: &mut Vec<Type>) {
        if names_type_param(ty) {
            return;
        }
        match ty {
            Type::Named(..) if self.is_nominal(ty) => out.push(ty.clone()),
            Type::Tuple(elements) => {
                for element in elements {
                    self.inline_nominals(element, out);
                }
            }
            Type::Union(union) => {
                for member in &union.members {
                    self.inline_nominals(member, out);
                }
            }
            _ => {}
        }
    }

    /// The instantiated classes and enums the definition of `ty` holds
    /// inline.
    fn successors(&self, ty: &Type) -> Vec<Type> {
        let mut out = Vec::new();
        if let Some(fields) = self.class_fields(ty) {
            for (_, field_ty) in fields {
                self.inline_nominals(&field_ty, &mut out);
            }
        } else if let Some(variants) = self.enum_variants(ty) {
            for (_, payloads) in variants {
                for payload in payloads {
                    self.inline_nominals(&payload, &mut out);
                }
            }
        }
        out
    }

    /// The cycle of an instantiated class or enum. The first query of a
    /// type walks every instantiation it reaches and numbers the strongly
    /// connected components no earlier walk numbered.
    ///
    /// A type stays a handle when the instantiations it reaches, itself
    /// included, number more than the budget or one of them nests too
    /// deeply. That depends only on the type, never on which types earlier
    /// walks numbered, so the order a program's types are laid out in
    /// changes no layout.
    fn cycle_of(&mut self, ty: &Type) -> Cycle {
        let key = type_key(ty);
        if let Some(cycle) = self.cycles.get(&key) {
            return *cycle;
        }
        if type_depth(ty) > MAX_LAYOUT_TYPE_DEPTH {
            self.cycles.insert(key, Cycle::OverBudget);
            return Cycle::OverBudget;
        }
        // The instantiations `ty` reaches that no earlier walk numbered.
        let mut reached: Vec<Type> = vec![ty.clone()];
        let mut index: HashMap<String, usize> = HashMap::from([(key.clone(), 0)]);
        let mut edges: Vec<Vec<usize>> = Vec::new();
        // The numbered ones it reaches, which count toward the budget too.
        // Everything a numbered type reaches is numbered and within the
        // depth limit.
        let mut numbered: HashSet<String> = HashSet::new();
        let mut numbered_pending: Vec<Type> = Vec::new();
        let mut next = 0;
        while next < reached.len() {
            let current = reached[next].clone();
            let mut targets = Vec::new();
            for successor in self.successors(&current) {
                let successor_key = type_key(&successor);
                match self.cycles.get(&successor_key) {
                    Some(Cycle::OverBudget) => {
                        // A type that reaches an over-budget type reaches as
                        // much.
                        self.cycles.insert(key.clone(), Cycle::OverBudget);
                        return Cycle::OverBudget;
                    }
                    Some(Cycle::Member { .. }) => {
                        if numbered.insert(successor_key) {
                            numbered_pending.push(successor);
                        }
                        continue;
                    }
                    None => {}
                }
                let position = match index.get(&successor_key) {
                    Some(position) => *position,
                    None => {
                        if reached.len() + numbered.len() >= MAX_LAYOUT_INSTANTIATIONS
                            || type_depth(&successor) > MAX_LAYOUT_TYPE_DEPTH
                        {
                            self.cycles.insert(key.clone(), Cycle::OverBudget);
                            return Cycle::OverBudget;
                        }
                        index.insert(successor_key, reached.len());
                        reached.push(successor);
                        reached.len() - 1
                    }
                };
                targets.push(position);
            }
            edges.push(targets);
            next += 1;
        }
        while let Some(current) = numbered_pending.pop() {
            for successor in self.successors(&current) {
                if numbered.insert(type_key(&successor)) {
                    if reached.len() + numbered.len() > MAX_LAYOUT_INSTANTIATIONS {
                        self.cycles.insert(key.clone(), Cycle::OverBudget);
                        return Cycle::OverBudget;
                    }
                    numbered_pending.push(successor);
                }
            }
        }
        for component in strongly_connected_components(&edges) {
            let recursive = component.len() > 1 || edges[component[0]].contains(&component[0]);
            let cycle = self.next_cycle;
            self.next_cycle += 1;
            for member in component {
                self.cycles.insert(
                    type_key(&reached[member]),
                    Cycle::Member { cycle, recursive },
                );
            }
        }
        self.cycles[&key]
    }
}

enum StructuralShape {
    Struct(Vec<(String, LayoutId)>),
    Union(Vec<(String, LayoutId)>),
}

fn substitutions(params: &[String], args: &[Type]) -> HashMap<String, Type> {
    params.iter().cloned().zip(args.iter().cloned()).collect()
}

/// A key that tells types apart exactly: their descriptor.
fn type_key(ty: &Type) -> String {
    type_descriptor::encode(ty)
}

/// How deeply `ty`'s tuples, unions, and type arguments nest.
pub fn type_depth(ty: &Type) -> usize {
    1 + match ty {
        Type::Named(_, args) => args.iter().map(type_depth).max().unwrap_or(0),
        Type::Tuple(elements) => elements.iter().map(type_depth).max().unwrap_or(0),
        Type::Union(union) => union.members.iter().map(type_depth).max().unwrap_or(0),
        _ => 0,
    }
}

/// Whether `ty` names a type parameter anywhere.
pub fn names_type_param(ty: &Type) -> bool {
    match ty {
        Type::TypeParam(_) => true,
        Type::Named(_, args) => args.iter().any(names_type_param),
        Type::Tuple(elements) => elements.iter().any(names_type_param),
        Type::Union(union) => union.members.iter().any(names_type_param),
        Type::Function {
            params,
            return_type,
        } => {
            params.iter().any(|param| names_type_param(&param.ty)) || names_type_param(return_type)
        }
        Type::Closure {
            params,
            return_type,
            captures,
            ..
        } => {
            params.iter().any(|param| names_type_param(&param.ty))
                || names_type_param(return_type)
                || captures.iter().any(|capture| names_type_param(&capture.ty))
        }
        Type::Callable(callable) => {
            callable
                .params
                .iter()
                .any(|param| names_type_param(&param.ty))
                || names_type_param(&callable.return_type)
        }
        Type::ReturnedView(view) => names_type_param(&view.pointee),
        Type::Unit | Type::Module(_) => false,
    }
}

/// Tarjan's strongly connected components of a graph given as successor
/// lists, without recursion. Components come out in reverse topological
/// order, each listing its nodes.
fn strongly_connected_components(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    const UNVISITED: usize = usize::MAX;
    let count = edges.len();
    let mut index = vec![UNVISITED; count];
    let mut low = vec![0; count];
    let mut on_stack = vec![false; count];
    let mut stack = Vec::new();
    let mut components = Vec::new();
    let mut next_index = 0;
    for root in 0..count {
        if index[root] != UNVISITED {
            continue;
        }
        // Each frame is a node and the position of its next successor.
        let mut frames = vec![(root, 0usize)];
        index[root] = next_index;
        low[root] = next_index;
        next_index += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(frame) = frames.last_mut() {
            let (node, position) = *frame;
            if let Some(&successor) = edges[node].get(position) {
                frame.1 += 1;
                if index[successor] == UNVISITED {
                    index[successor] = next_index;
                    low[successor] = next_index;
                    next_index += 1;
                    stack.push(successor);
                    on_stack[successor] = true;
                    frames.push((successor, 0));
                } else if on_stack[successor] {
                    low[node] = low[node].min(index[successor]);
                }
                continue;
            }
            frames.pop();
            if let Some(parent) = frames.last() {
                low[parent.0] = low[parent.0].min(low[node]);
            }
            if low[node] == index[node] {
                let mut component = Vec::new();
                loop {
                    let member = stack.pop().expect("a component's nodes are on the stack");
                    on_stack[member] = false;
                    component.push(member);
                    if member == node {
                        break;
                    }
                }
                component.sort_unstable();
                components.push(component);
            }
        }
    }
    components
}

/// The descriptor of `ty`'s layout: the type and every class and enum
/// definition the layout can reach, so the runtime computes the same
/// layout with this module.
pub fn encode_descriptor(ty: &Type, env: &LayoutEnv) -> String {
    let mut names = BTreeSet::new();
    let mut pending = Vec::new();
    collect_named(ty, &mut pending);
    while let Some(name) = pending.pop() {
        if !names.insert(name.clone()) {
            continue;
        }
        if let Some(class) = env.classes.get(&name) {
            for (_, field_ty) in &class.fields {
                collect_named(field_ty, &mut pending);
            }
        }
        if let Some(enumeration) = env.enums.get(&name) {
            for (_, payloads) in &enumeration.variants {
                for payload in payloads {
                    collect_named(payload, &mut pending);
                }
            }
        }
    }
    let mut out = String::from(LAYOUT_DESCRIPTOR_PREFIX);
    type_descriptor::push_type(&mut out, ty);
    let classes = names
        .iter()
        .filter_map(|name| env.classes.get(name).map(|class| (name, class)))
        .collect::<Vec<_>>();
    type_descriptor::push_count(&mut out, classes.len());
    for (name, class) in classes {
        type_descriptor::push_string(&mut out, name);
        push_strings(&mut out, &class.type_params);
        type_descriptor::push_count(&mut out, class.fields.len());
        for (field, field_ty) in &class.fields {
            type_descriptor::push_string(&mut out, field);
            type_descriptor::push_type(&mut out, field_ty);
        }
    }
    let enums = names
        .iter()
        .filter_map(|name| env.enums.get(name).map(|enumeration| (name, enumeration)))
        .collect::<Vec<_>>();
    type_descriptor::push_count(&mut out, enums.len());
    for (name, enumeration) in enums {
        type_descriptor::push_string(&mut out, name);
        push_strings(&mut out, &enumeration.type_params);
        type_descriptor::push_count(&mut out, enumeration.variants.len());
        for (variant, payloads) in &enumeration.variants {
            type_descriptor::push_string(&mut out, variant);
            type_descriptor::push_count(&mut out, payloads.len());
            for payload in payloads {
                type_descriptor::push_type(&mut out, payload);
            }
        }
    }
    out
}

fn push_strings(out: &mut String, strings: &[String]) {
    type_descriptor::push_count(out, strings.len());
    for string in strings {
        type_descriptor::push_string(out, string);
    }
}

/// Every name a type spells, including type arguments.
fn collect_named(ty: &Type, out: &mut Vec<String>) {
    match ty {
        Type::Named(name, args) => {
            out.push(name.clone());
            for arg in args {
                collect_named(arg, out);
            }
        }
        Type::Tuple(elements) => {
            for element in elements {
                collect_named(element, out);
            }
        }
        Type::Union(union) => {
            for member in &union.members {
                collect_named(member, out);
            }
        }
        _ => {}
    }
}

/// Reads a descriptor [`encode_descriptor`] wrote: the type and the
/// definitions to lay it out with.
pub fn decode_descriptor(descriptor: &str) -> Result<(Type, LayoutEnv), String> {
    let body = descriptor
        .strip_prefix(LAYOUT_DESCRIPTOR_PREFIX)
        .ok_or_else(|| "layout descriptor has an unknown version".to_string())?;
    let mut reader = Reader {
        bytes: body.as_bytes(),
        position: 0,
    };
    let ty = reader.read_type(0)?;
    let mut env = LayoutEnv::default();
    for _ in 0..reader.count()? {
        let name = reader.string()?;
        let type_params = read_strings(&mut reader)?;
        let mut fields = Vec::new();
        for _ in 0..reader.count()? {
            let field = reader.string()?;
            fields.push((field, reader.read_type(0)?));
        }
        if env
            .classes
            .insert(
                name.clone(),
                ClassDefinition {
                    type_params,
                    fields,
                },
            )
            .is_some()
        {
            return Err(format!("layout descriptor defines class `{name}` twice"));
        }
    }
    for _ in 0..reader.count()? {
        let name = reader.string()?;
        let type_params = read_strings(&mut reader)?;
        let mut variants = Vec::new();
        for _ in 0..reader.count()? {
            let variant = reader.string()?;
            let mut payloads = Vec::new();
            for _ in 0..reader.count()? {
                payloads.push(reader.read_type(0)?);
            }
            variants.push((variant, payloads));
        }
        if env.classes.contains_key(&name)
            || env
                .enums
                .insert(
                    name.clone(),
                    EnumDefinition {
                        type_params,
                        variants,
                    },
                )
                .is_some()
        {
            return Err(format!("layout descriptor defines `{name}` twice"));
        }
    }
    if reader.position != reader.bytes.len() {
        return Err("layout descriptor has trailing bytes".to_string());
    }
    Ok((ty, env))
}

fn read_strings(reader: &mut Reader<'_>) -> Result<Vec<String>, String> {
    let mut strings = Vec::new();
    for _ in 0..reader.count()? {
        strings.push(reader.string()?);
    }
    Ok(strings)
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
