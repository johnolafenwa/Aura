//! MIR also represents field access as a dotted operand, such as a class
//! field or the payload of a narrowed union. Give those reads ordinary typed
//! temporaries before replacing a frozen aggregate's type with an address.
use super::*;

pub(super) fn expand(
    function: &mut MirFunction,
    addresses: &HashSet<String>,
    types: &mut HashMap<String, Type>,
    classes: &HashMap<&str, &MirClass>,
) -> bool {
    if addresses.is_empty() {
        return false;
    }
    let mut added = Vec::new();
    let mut next = types
        .keys()
        .filter_map(|n| n.strip_prefix("%t")?.parse::<usize>().ok())
        .max()
        .map_or(0, |n| n + 1);
    for block in &mut function.blocks {
        for mut instruction in std::mem::take(&mut block.instructions) {
            let mut operands = Vec::new();
            match &mut instruction {
                Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. } => {
                    crate::mir::ownership::rvalue_operands_mut(value, &mut operands)
                }
                Instruction::Eval { value } => operands.push(value),
                _ => {}
            }
            for operand in operands {
                split(
                    operand,
                    addresses,
                    types,
                    classes,
                    &mut next,
                    &mut block.instructions,
                    &mut added,
                );
            }
            block.instructions.push(instruction);
        }
        let operand = match &mut block.terminator {
            Terminator::Return(value)
            | Terminator::Branch {
                condition: value, ..
            }
            | Terminator::ForRange {
                iterable: value, ..
            }
            | Terminator::Match {
                scrutinee: value, ..
            } => Some(value),
            _ => None,
        };
        if let Some(operand) = operand {
            split(
                operand,
                addresses,
                types,
                classes,
                &mut next,
                &mut block.instructions,
                &mut added,
            );
        }
    }
    let changed = !added.is_empty();
    for local in added {
        types.insert(local.name.clone(), local.ty.clone());
        function.local_types.push(local);
    }
    changed
}

fn split(
    operand: &mut Operand,
    addresses: &HashSet<String>,
    types: &HashMap<String, Type>,
    classes: &HashMap<&str, &MirClass>,
    next: &mut usize,
    output: &mut Vec<Instruction>,
    added: &mut Vec<crate::mir::MirLocalType>,
) {
    let (Operand::Place(path) | Operand::MovePlace(path)) = operand else {
        return;
    };
    let Some((root, tail)) = path.split_once('.') else {
        return;
    };
    if !addresses.contains(root) {
        return;
    }
    let mut ty = types[root].clone();
    let mut place = root.to_owned();
    let mut steps = Vec::new();
    for segment in tail.split('.') {
        let Some((field, value)) = field_read(&place, &ty, segment, classes) else {
            return;
        };
        place = format!("%t{}", *next);
        *next += 1;
        ty = field;
        steps.push((
            crate::mir::MirLocalType {
                name: place.clone(),
                ty: ty.clone(),
            },
            value,
        ));
    }
    for (local, value) in steps {
        output.push(Instruction::Assign {
            target: local.name.clone(),
            value,
        });
        added.push(local);
    }
    *path = place;
}

fn field_read(
    place: &str,
    ty: &Type,
    segment: &str,
    classes: &HashMap<&str, &MirClass>,
) -> Option<(Type, Rvalue)> {
    let receiver = Operand::Place(place.into());
    match ty {
        Type::Union(union) => {
            let index = segment.strip_prefix("__union_payload_")?.parse().ok()?;
            let member: &Type = union.members.get(index)?;
            Some((
                member.clone(),
                Rvalue::UnionTakePayload {
                    place: place.into(),
                    union_type: ty.clone(),
                    member_type: member.clone(),
                    member_index: index,
                },
            ))
        }
        Type::Tuple(elements) => {
            let index = segment.parse().ok()?;
            let element: &Type = elements.get(index)?;
            Some((
                element.clone(),
                Rvalue::TupleElement {
                    tuple: receiver,
                    index,
                    element_type: element.clone(),
                },
            ))
        }
        Type::Named(name, args) => {
            let class = classes.get(name.as_str())?;
            let field = class.fields.iter().find(|field| field.name == segment)?;
            let bindings = class
                .type_params
                .iter()
                .cloned()
                .zip(args.iter().cloned())
                .collect();
            Some((
                substitute_type(&field.ty, &bindings),
                Rvalue::Member {
                    object: receiver,
                    field: field.name.clone(),
                },
            ))
        }
        _ => None,
    }
}
