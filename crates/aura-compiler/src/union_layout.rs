//! Shared explicit-tag layout and drop plans for unions (ADR-0052 A9).
//!
//! Both execution paths carry union values as tagged runtime values, so the
//! plan below is the internal Aura ABI both backends agree on and check,
//! encoded in the MIR so importers and native caches rebuild rather than
//! guess a tag from source order. Logical tags are dense canonical-member
//! ordinals; the tag is the smallest unsigned width holding every member;
//! the payload is aligned to the widest member alignment and sized to the
//! widest member; the total rounds up to the aggregate alignment. The plan is
//! target-dependent internal ABI, not a C ABI or a portable serialization.

use crate::sema::{Type, UnionType};
use serde::{Deserialize, Serialize};

/// Version of the layout rule set; part of every plan and of the native
/// cache identity through the MIR.
pub const UNION_LAYOUT_VERSION: u32 = 1;

/// The explicit-tag layout and drop plan of one normalized union.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirUnionLayout {
    /// The canonical union this plan describes.
    pub union_type: Type,
    /// Members in canonical order; the index is the dense tag ordinal.
    pub members: Vec<MirUnionMemberLayout>,
    /// Tag width in bytes: 1, 2, or 4.
    pub tag_width: u8,
    /// Widest member size in bytes.
    pub payload_size: u64,
    /// Widest member alignment in bytes.
    pub payload_align: u64,
    /// Payload offset from the start of the value.
    pub payload_offset: u64,
    /// Total size rounded to the aggregate alignment.
    pub size: u64,
    /// Aggregate alignment.
    pub align: u64,
    /// Whether the whole union is Copy (every member is Copy).
    pub copy: bool,
    /// Pointer width in bytes of the target this plan was computed for.
    pub pointer_width: u8,
    /// [`UNION_LAYOUT_VERSION`] at lowering time.
    pub layout_version: u32,
}

/// One member's slot in a union layout plan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirUnionMemberLayout {
    /// Dense canonical tag ordinal.
    pub tag: u32,
    /// The canonical member key.
    pub key: String,
    /// The member type.
    pub ty: Type,
    pub size: u64,
    pub align: u64,
    /// Whether the member is Copy.
    pub copy: bool,
    /// Whether the active payload owns cleanup when this member is active;
    /// only the active payload is destroyed, once.
    pub needs_drop: bool,
}

/// Size and alignment of a type under the internal ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbiLayout {
    pub size: u64,
    pub align: u64,
}

fn pointer_width() -> u64 {
    std::mem::size_of::<usize>() as u64
}

fn round_up(value: u64, align: u64) -> u64 {
    debug_assert!(align > 0);
    value.div_ceil(align) * align
}

/// The internal ABI size and alignment of a member type. Scalars use their
/// width; unit `None` is empty; every boxed aggregate, handle, callable, and
/// type parameter is one pointer; a nested union is its own plan.
pub fn abi_layout(ty: &Type) -> AbiLayout {
    let pointer = AbiLayout {
        size: pointer_width(),
        align: pointer_width(),
    };
    match ty {
        Type::Unit => AbiLayout { size: 0, align: 1 },
        Type::Union(union) => {
            let plan = plan_union_layout(union, |_| false);
            AbiLayout {
                size: plan.size,
                align: plan.align,
            }
        }
        Type::Named(name, args) if args.is_empty() => match name.as_str() {
            "bool" | "int8" | "uint8" => AbiLayout { size: 1, align: 1 },
            "int16" | "uint16" => AbiLayout { size: 2, align: 2 },
            "int32" | "uint32" | "float32" => AbiLayout { size: 4, align: 4 },
            "int64" | "uint64" | "float64" | "intsize" | "uintsize" => {
                AbiLayout { size: 8, align: 8 }
            }
            "int128" | "uint128" | "Duration" => AbiLayout {
                size: 16,
                align: 16,
            },
            _ => pointer,
        },
        _ => pointer,
    }
}

/// The tag width in bytes for a member count.
pub fn tag_width_for(member_count: usize) -> u8 {
    if member_count <= usize::from(u8::MAX) + 1 {
        1
    } else if member_count <= usize::from(u16::MAX) + 1 {
        2
    } else {
        4
    }
}

/// Computes the plan for a normalized union. `is_copy` answers member
/// copy-ness from the checked program; the plan itself never guesses.
pub fn plan_union_layout(union: &UnionType, is_copy: impl Fn(&Type) -> bool) -> MirUnionLayout {
    let members = union
        .members
        .iter()
        .zip(&union.keys)
        .enumerate()
        .map(|(tag, (ty, key))| {
            let layout = abi_layout(ty);
            let copy = matches!(ty, Type::Unit) || is_copy(ty);
            MirUnionMemberLayout {
                tag: tag as u32,
                key: key.clone(),
                ty: ty.clone(),
                size: layout.size,
                align: layout.align,
                copy,
                needs_drop: !copy,
            }
        })
        .collect::<Vec<_>>();
    let tag_width = tag_width_for(members.len());
    let payload_align = members
        .iter()
        .map(|member| member.align)
        .max()
        .unwrap_or(1)
        .max(1);
    let payload_size = members.iter().map(|member| member.size).max().unwrap_or(0);
    let payload_offset = round_up(u64::from(tag_width), payload_align);
    let align = payload_align.max(u64::from(tag_width));
    let size = round_up(payload_offset + payload_size, align);
    MirUnionLayout {
        union_type: Type::Union(Box::new(union.clone())),
        copy: members.iter().all(|member| member.copy),
        members,
        tag_width,
        payload_size,
        payload_align,
        payload_offset,
        size,
        align,
        pointer_width: pointer_width() as u8,
        layout_version: UNION_LAYOUT_VERSION,
    }
}

/// Plans every union a module holds or operates on, replacing its plan
/// table. Copy-ness comes from `is_copy`; a consumer without the checked
/// program may pass a conservative answer, because a plan may understate
/// Copy but never claim it for a definitely non-Copy member.
pub fn plan_module_unions(module: &mut crate::mir::MirModule, is_copy: impl Fn(&Type) -> bool) {
    use crate::mir::{Instruction, Rvalue};
    use std::collections::BTreeMap;
    fn collect(ty: &Type, unions: &mut BTreeMap<String, UnionType>) {
        match ty {
            Type::Union(union) => {
                unions
                    .entry(union_plan_key(ty))
                    .or_insert_with(|| (**union).clone());
                for member in &union.members {
                    collect(member, unions);
                }
            }
            Type::Named(_, args) => args.iter().for_each(|arg| collect(arg, unions)),
            Type::Tuple(elements) => elements.iter().for_each(|element| collect(element, unions)),
            Type::Function {
                params,
                return_type,
            } => {
                params.iter().for_each(|param| collect(&param.ty, unions));
                collect(return_type, unions);
            }
            Type::Closure {
                params,
                return_type,
                captures,
                ..
            } => {
                params.iter().for_each(|param| collect(&param.ty, unions));
                captures
                    .iter()
                    .for_each(|capture| collect(&capture.ty, unions));
                collect(return_type, unions);
            }
            Type::ReturnedView(view) => collect(&view.pointee, unions),
            Type::Callable(callable) => {
                callable
                    .params
                    .iter()
                    .for_each(|param| collect(&param.ty, unions));
                collect(&callable.return_type, unions);
            }
            Type::Unit | Type::Module(_) | Type::TypeParam(_) => {}
        }
    }
    let mut unions = BTreeMap::new();
    for function in module.functions.iter().chain(module.top_level.iter()) {
        for param in &function.params {
            collect(&param.ty, &mut unions);
        }
        collect(&function.return_type, &mut unions);
        for local in &function.local_types {
            collect(&local.ty, &mut unions);
        }
        for block in &function.blocks {
            for instruction in &block.instructions {
                if let Instruction::Assign {
                    value:
                        Rvalue::UnionTagTest { union_type, .. }
                        | Rvalue::UnionTakePayload { union_type, .. }
                        | Rvalue::UnionInject { union_type, .. },
                    ..
                } = instruction
                {
                    collect(union_type, &mut unions);
                }
            }
        }
    }
    for class in &module.classes {
        for field in &class.fields {
            collect(&field.ty, &mut unions);
        }
    }
    for enum_layout in &module.enums {
        for variant in &enum_layout.variants {
            variant
                .payloads
                .iter()
                .for_each(|payload| collect(payload, &mut unions));
        }
    }
    module.unions = unions
        .into_values()
        .map(|union| plan_union_layout(&union, &is_copy))
        .collect();
}

/// The stable key a plan table uses for a union type.
pub fn union_plan_key(ty: &Type) -> String {
    serde_json::to_string(ty).expect("Aura semantic types must serialize")
}

/// Structural checks a plan must pass regardless of who produced it:
/// members and keys equal the canonical union in order, tags are dense
/// ordinals, widths and sizes follow the layout rule, and the version and
/// pointer width match this compiler.
pub fn validate_union_layout(plan: &MirUnionLayout) -> Result<(), String> {
    let Type::Union(union) = &plan.union_type else {
        return Err("union layout plan does not describe a union type".to_string());
    };
    if plan.layout_version != UNION_LAYOUT_VERSION {
        return Err(format!(
            "union layout plan for `{}` uses layout version {}, expected {UNION_LAYOUT_VERSION}",
            plan.union_type, plan.layout_version
        ));
    }
    if u64::from(plan.pointer_width) != pointer_width() {
        return Err(format!(
            "union layout plan for `{}` targets a {}-byte pointer width, expected {}",
            plan.union_type,
            plan.pointer_width,
            pointer_width()
        ));
    }
    let expected = plan_union_layout(union, |ty| {
        plan.members
            .iter()
            .find(|member| &member.ty == ty)
            .is_some_and(|member| member.copy)
    });
    if plan.members.len() != union.members.len() {
        return Err(format!(
            "union layout plan for `{}` lists {} members, expected {}",
            plan.union_type,
            plan.members.len(),
            union.members.len()
        ));
    }
    for (index, (member, expected_member)) in plan.members.iter().zip(&expected.members).enumerate()
    {
        if member.tag != index as u32
            || member.key != expected_member.key
            || member.ty != expected_member.ty
            || member.size != expected_member.size
            || member.align != expected_member.align
            || member.needs_drop == member.copy
        {
            return Err(format!(
                "union layout plan for `{}` disagrees with the canonical member {index}",
                plan.union_type
            ));
        }
    }
    if plan.tag_width != expected.tag_width
        || plan.payload_size != expected.payload_size
        || plan.payload_align != expected.payload_align
        || plan.payload_offset != expected.payload_offset
        || plan.size != expected.size
        || plan.align != expected.align
        || plan.copy != plan.members.iter().all(|member| member.copy)
    {
        return Err(format!(
            "union layout plan for `{}` disagrees with the layout rule",
            plan.union_type
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "union_layout_tests.rs"]
mod tests;
