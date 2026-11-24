//! Tuples and enums of plain values in a freestanding program.
//!
//! A hosted program keeps a tuple or an enum value in a box of the
//! runtime. A freestanding program has no runtime, and a tuple or an enum
//! whose parts are all machine words needs no heap either: it is Copy, so
//! the ownership pass states no reference for it.
//!
//! [`lower_plain_values`] rewrites each such type of a freestanding
//! program to a Copy class, which the emitter keeps in machine words:
//!
//! - a tuple becomes a class with one field for each element;
//! - an enum becomes a class with a `tag` field, the variant's index, and
//!   one field for each payload of each variant.
//!
//! Building a tuple or a variant becomes building the class, reading an
//! element or a payload becomes reading a field, and a `match` becomes
//! comparisons of the tag. A type the pass cannot rewrite, such as an enum
//! that holds text, is left as it is, and the subset check reports it.

use super::*;
use crate::mir::{MirClassField, MirFieldInit, MirLocalType};

/// What each class the pass makes is named. A source name holds no `:`.
const CLASS_PREFIX: &str = "aura::values::";

const TAG_FIELD: &str = "tag";
const TAG_TYPE: &str = "int64";

fn element_field(index: usize) -> String {
    format!("_{index}")
}

fn payload_field(variant: usize, index: usize) -> String {
    format!("p{variant}_{index}")
}

/// The variants of one enum type the pass rewrote, in order, each with
/// its payloads' rewritten types.
type Variants = Vec<(String, Vec<Type>)>;

struct Values {
    enums: HashMap<String, MirEnum>,
    /// The program's classes and the pass's, by name.
    classes: HashMap<String, MirClass>,
    /// The class of each tuple or enum type asked about, by the type's
    /// text: `None` for a type the pass leaves alone.
    known: HashMap<String, Option<String>>,
    /// The variants of each enum class.
    variants: HashMap<String, Variants>,
    made: Vec<MirClass>,
}

impl Values {
    /// `ty` with each tuple and enum the pass rewrites replaced by its
    /// class.
    fn rewritten(&mut self, ty: &Type) -> Type {
        match ty {
            Type::Tuple(_) => match self.class_of(ty) {
                Some(class) => Type::named(&class),
                None => ty.clone(),
            },
            Type::Named(name, _) if self.enums.contains_key(name) => match self.class_of(ty) {
                Some(class) => Type::named(&class),
                None => ty.clone(),
            },
            _ => ty.clone(),
        }
    }

    /// Whether a field of a class the pass makes can hold `ty`: a scalar
    /// or a Copy class of words, whose zero value the pass can write.
    fn fits_a_field(&self, ty: &Type) -> bool {
        matches!(ty, Type::Named(..)) && super::freestanding::words_only(ty, &self.classes)
    }

    /// The class that stands for the tuple or enum type `ty`.
    fn class_of(&mut self, ty: &Type) -> Option<String> {
        let key = ty.to_string();
        if let Some(known) = self.known.get(&key) {
            return known.clone();
        }
        // A type that holds itself is not plain.
        self.known.insert(key.clone(), None);
        let name = format!("{CLASS_PREFIX}{}", self.known.len());
        let mut fields = Vec::new();
        match ty {
            Type::Tuple(elements) => {
                for (index, element) in elements.iter().enumerate() {
                    let element = self.rewritten(element);
                    if !super::freestanding::words_only(&element, &self.classes) {
                        return None;
                    }
                    fields.push(MirClassField {
                        name: element_field(index),
                        ty: element,
                    });
                }
            }
            Type::Named(enum_name, args) => {
                let layout = self.enums.get(enum_name)?.clone();
                if layout.type_params.len() != args.len() {
                    return None;
                }
                let bound = layout
                    .type_params
                    .iter()
                    .cloned()
                    .zip(args.iter().cloned())
                    .collect::<HashMap<_, _>>();
                fields.push(MirClassField {
                    name: TAG_FIELD.to_string(),
                    ty: Type::named(TAG_TYPE),
                });
                let mut variants = Vec::new();
                for (variant, declared) in layout.variants.iter().enumerate() {
                    let mut payloads = Vec::new();
                    for (index, payload) in declared.payloads.iter().enumerate() {
                        let payload =
                            self.rewritten(&crate::sema::substitute_type(payload, &bound));
                        if !self.fits_a_field(&payload) {
                            return None;
                        }
                        fields.push(MirClassField {
                            name: payload_field(variant, index),
                            ty: payload.clone(),
                        });
                        payloads.push(payload);
                    }
                    variants.push((declared.name.clone(), payloads));
                }
                self.variants.insert(name.clone(), variants);
            }
            _ => return None,
        }
        // The class needs drop exactly when the type it stands for did, so
        // the references the ownership pass stated still add up. A class
        // of words drops nothing either way.
        let copy = fields.iter().all(|field| match &field.ty {
            Type::Named(name, _) => self.classes.get(name).is_none_or(|class| class.copy),
            Type::Union(union) => union.members.iter().all(|member| match member {
                Type::Named(name, _) => self.classes.get(name).is_none_or(|class| class.copy),
                _ => true,
            }),
            _ => true,
        });
        let class = MirClass {
            name: name.clone(),
            type_params: Vec::new(),
            fields,
            methods: Vec::new(),
            copy,
        };
        self.classes.insert(name.clone(), class.clone());
        self.made.push(class);
        self.known.insert(key, Some(name.clone()));
        Some(name)
    }
}

/// Rewrites the tuples and enums of plain values in `module`, a
/// freestanding program, to Copy classes.
pub(super) fn lower_plain_values(module: &mut MirModule) {
    let mut values = Values {
        enums: module
            .enums
            .iter()
            .map(|layout| (layout.name.clone(), layout.clone()))
            .collect(),
        classes: module
            .classes
            .iter()
            .map(|class| (class.name.clone(), class.clone()))
            .collect(),
        known: HashMap::new(),
        variants: HashMap::new(),
        made: Vec::new(),
    };
    for function in &mut module.functions {
        lower_function(function, &mut values);
    }
    for class in &mut module.classes {
        if class.type_params.is_empty() {
            for field in &mut class.fields {
                field.ty = values.rewritten(&field.ty);
            }
        }
    }
    module.classes.extend(values.made);
}

/// The temporaries a function gains.
struct Fresh {
    /// The number of the next temporary.
    count: usize,
    locals: Vec<MirLocalType>,
}

impl Fresh {
    fn local(&mut self, ty: Type) -> String {
        let name = format!("%t{}", self.count);
        self.count += 1;
        self.locals.push(MirLocalType {
            name: name.clone(),
            ty,
        });
        name
    }
}

/// The zero value of a field of type `ty`, which a variant's class holds
/// in the fields of its other variants. `out` gains what builds it.
fn zero(ty: &Type, values: &Values, fresh: &mut Fresh, out: &mut Vec<Instruction>) -> Operand {
    let Type::Named(name, _) = ty else {
        return Operand::Int(0);
    };
    if let Some(class) = values.classes.get(name) {
        let fields = class
            .fields
            .iter()
            .map(|field| MirFieldInit {
                name: field.name.clone(),
                value: zero(&field.ty, values, fresh, out),
            })
            .collect();
        let local = fresh.local(ty.clone());
        out.push(Instruction::Assign {
            target: local.clone(),
            value: Rvalue::Construct {
                class_name: name.clone(),
                fields,
            },
        });
        // The class it goes into takes it whole.
        return Operand::MovePlace(local);
    }
    match name.as_str() {
        "bool" => Operand::Bool(false),
        "float32" | "float64" => Operand::Float(0.0),
        _ => Operand::Int(0),
    }
}

fn lower_function(function: &mut MirFunction, values: &mut Values) {
    // The type of each place before the rewrite, which names the enum a
    // variant or a `match` belongs to.
    let mut types = HashMap::new();
    for param in &function.params {
        types.insert(param.name.clone(), param.ty.clone());
    }
    for local in &function.local_types {
        types.insert(local.name.clone(), local.ty.clone());
    }
    complete_enum_types(function, &mut types, values);
    for param in &mut function.params {
        param.ty = values.rewritten(&param.ty);
    }
    for local in &mut function.local_types {
        local.ty = values.rewritten(types.get(&local.name).unwrap_or(&local.ty));
    }
    let returns = function.return_type.clone();
    function.return_type = values.rewritten(&function.return_type);

    // A temporary is named `%t` and a number, and the function's own end
    // at the largest in use.
    let mut fresh = Fresh {
        count: function
            .local_types
            .iter()
            .filter_map(|local| local.name.strip_prefix("%t")?.parse::<usize>().ok())
            .max()
            .map_or(0, |largest| largest + 1),
        locals: Vec::new(),
    };
    let mut blocks = Vec::with_capacity(function.blocks.len());
    for block in std::mem::take(&mut function.blocks) {
        let BasicBlock {
            label,
            instructions: before,
            terminator,
        } = block;
        // The block being filled. A `try` ends it and starts another.
        let mut current = BasicBlock {
            label: label.clone(),
            instructions: Vec::with_capacity(before.len()),
            terminator: Terminator::Unreachable,
        };
        let mut tries = 0;
        for instruction in before {
            match instruction {
                Instruction::Assign {
                    target,
                    value:
                        Rvalue::Try {
                            value: Operand::Place(result) | Operand::MovePlace(result),
                            releases_on_error,
                            from,
                        },
                } if lower_try(
                    &target,
                    &result,
                    &releases_on_error,
                    from.as_deref(),
                    &returns,
                    format!("{label}_value_try_{tries}"),
                    &types,
                    values,
                    &mut fresh,
                    &mut current,
                    &mut blocks,
                ) =>
                {
                    tries += 1;
                }
                Instruction::Assign { target, value } => {
                    let value = lower_value(
                        value,
                        types.get(&target),
                        &types,
                        values,
                        &mut fresh,
                        &mut current.instructions,
                    );
                    current
                        .instructions
                        .push(Instruction::Assign { target, value });
                }
                other => current.instructions.push(other),
            }
        }
        current.terminator = terminator;
        lower_match(current, &types, values, &mut fresh, &mut blocks);
    }
    function.blocks = blocks;
    if !fresh.locals.is_empty() {
        function.local_types.extend(fresh.locals);
        function
            .local_types
            .sort_by(|left, right| left.name.cmp(&right.name));
    }
}

/// Gives each local whose type names a generic enum without its type
/// arguments the type of the place its value goes to. Lowering leaves
/// the arguments out for a variant with no payload, such as the `None2`
/// of an `Option2[int64]`.
fn complete_enum_types(function: &MirFunction, types: &mut HashMap<String, Type>, values: &Values) {
    let incomplete = |ty: &Type| {
        matches!(ty, Type::Named(name, args) if args.is_empty()
            && values.enums.get(name).is_some_and(|layout| !layout.type_params.is_empty()))
    };
    let same_enum = |left: &Type, right: &Type| matches!((left, right), (Type::Named(left, _), Type::Named(right, _)) if left == right);
    // A value reaches its place through at most a few copies.
    for _ in 0..4 {
        let mut found = Vec::new();
        for block in &function.blocks {
            for instruction in &block.instructions {
                if let Instruction::Assign {
                    target,
                    value: Rvalue::Use(Operand::Place(source) | Operand::MovePlace(source)),
                } = instruction
                {
                    found.push((source.clone(), types.get(target).cloned()));
                }
            }
            if let Terminator::Return(Operand::Place(source) | Operand::MovePlace(source)) =
                &block.terminator
            {
                found.push((source.clone(), Some(function.return_type.clone())));
            }
        }
        let mut changed = false;
        for (source, ty) in found {
            let (Some(ty), Some(current)) = (ty, types.get(&source)) else {
                continue;
            };
            if incomplete(current) && !incomplete(&ty) && same_enum(current, &ty) {
                types.insert(source, ty);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

/// Rewrites `target = try result` when `result` and the function's
/// return value are both results the pass rewrote and neither needs drop.
/// `from` names the function that converts the error when the two error
/// types differ. The block being filled, `current`,
/// ends in a test of the tag. On `Err` a new block returns the error in
/// the function's own result type. On `Ok` the block `ok_label` goes on
/// with the payload in `target`, and becomes `current`. Returns whether
/// it rewrote the `try`.
#[allow(clippy::too_many_arguments)]
fn lower_try(
    target: &str,
    result: &str,
    releases_on_error: &[String],
    from: Option<&str>,
    returns: &Type,
    ok_label: String,
    types: &HashMap<String, Type>,
    values: &mut Values,
    fresh: &mut Fresh,
    current: &mut BasicBlock,
    blocks: &mut Vec<BasicBlock>,
) -> bool {
    let is_result =
        |ty: &Type| matches!(ty, Type::Named(name, args) if name == "Result" && args.len() == 2);
    let Some(operand_type) = types.get(result).filter(|ty| is_result(ty)).cloned() else {
        return false;
    };
    if !is_result(returns) {
        return false;
    }
    let (Some(operand_class), Some(return_class)) =
        (values.class_of(&operand_type), values.class_of(returns))
    else {
        return false;
    };
    let copy =
        |values: &Values, class: &str| values.classes.get(class).is_some_and(|class| class.copy);
    if !copy(values, &operand_class) || !copy(values, &return_class) {
        return false;
    }
    let place = |variants: &Variants, name: &str| {
        variants
            .iter()
            .position(|(variant, payloads)| variant == name && payloads.len() == 1)
    };
    let operand_variants = values.variants[&operand_class].clone();
    let return_variants = values.variants[&return_class].clone();
    let (Some(ok), Some(error), Some(return_ok), Some(return_error)) = (
        place(&operand_variants, "Ok"),
        place(&operand_variants, "Err"),
        place(&return_variants, "Ok"),
        place(&return_variants, "Err"),
    ) else {
        return false;
    };
    let error_type = operand_variants[error].1[0].clone();
    let returned_error_type = return_variants[return_error].1[0].clone();
    if from.is_none() && error_type != returned_error_type {
        return false;
    }
    let tag = fresh.local(Type::named(TAG_TYPE));
    let holds = fresh.local(Type::named("bool"));
    current.instructions.push(Instruction::Assign {
        target: tag.clone(),
        value: Rvalue::Member {
            object: Operand::Place(result.to_string()),
            field: TAG_FIELD.to_string(),
        },
    });
    current.instructions.push(Instruction::Assign {
        target: holds.clone(),
        value: Rvalue::Binary {
            op: BinaryOp::Eq,
            left: Operand::Place(tag),
            right: Operand::Int(ok as u128),
            span: crate::diag::Span::new(0, 0),
            trap: Default::default(),
        },
    });
    let error_label = format!("{ok_label}_error");
    current.terminator = Terminator::Branch {
        condition: Operand::Place(holds),
        then_label: ok_label.clone(),
        else_label: error_label.clone(),
    };
    let next = BasicBlock {
        label: ok_label,
        instructions: vec![Instruction::Assign {
            target: target.to_string(),
            value: Rvalue::Member {
                object: Operand::Place(result.to_string()),
                field: payload_field(ok, 0),
            },
        }],
        terminator: Terminator::Unreachable,
    };
    blocks.push(std::mem::replace(current, next));

    let mut instructions = Vec::new();
    let carried = fresh.local(error_type);
    instructions.push(Instruction::Assign {
        target: carried.clone(),
        value: Rvalue::Member {
            object: Operand::Place(result.to_string()),
            field: payload_field(error, 0),
        },
    });
    // A `From` impl turns the error into the function's own.
    let carried = match from {
        Some(function) => {
            let converted = fresh.local(returned_error_type);
            instructions.push(Instruction::Assign {
                target: converted.clone(),
                value: Rvalue::Call {
                    callee: CallTarget::Name(function.to_string()),
                    args: vec![MirArg {
                        name: None,
                        value: Operand::Place(carried),
                        writeback_place: None,
                    }],
                    type_args: Default::default(),
                },
            });
            converted
        }
        None => carried,
    };
    let ok_zero = zero(
        &return_variants[return_ok].1[0],
        values,
        fresh,
        &mut instructions,
    );
    let returned = fresh.local(Type::named(&return_class));
    let mut fields = vec![MirFieldInit {
        name: TAG_FIELD.to_string(),
        value: Operand::Int(return_error as u128),
    }];
    // The fields lie in the order of the variants.
    let mut payloads = [
        (return_ok, ok_zero),
        (return_error, Operand::Place(carried)),
    ];
    payloads.sort_by_key(|(variant, _)| *variant);
    for (variant, value) in payloads {
        fields.push(MirFieldInit {
            name: payload_field(variant, 0),
            value,
        });
    }
    instructions.push(Instruction::Assign {
        target: returned.clone(),
        value: Rvalue::Construct {
            class_name: return_class,
            fields,
        },
    });
    instructions.extend(releases_on_error.iter().map(|place| Instruction::Release {
        place: place.clone(),
    }));
    blocks.push(BasicBlock {
        label: error_label,
        instructions,
        terminator: Terminator::Return(Operand::Place(returned)),
    });
    true
}

/// The class of the enum or tuple that `operand` names, by its type
/// before the rewrite.
fn operand_class(
    operand: &Operand,
    types: &HashMap<String, Type>,
    values: &mut Values,
) -> Option<String> {
    let (Operand::Place(place) | Operand::MovePlace(place)) = operand else {
        return None;
    };
    let ty = types.get(place)?.clone();
    values.class_of(&ty)
}

fn lower_value(
    value: Rvalue,
    target: Option<&Type>,
    types: &HashMap<String, Type>,
    values: &mut Values,
    fresh: &mut Fresh,
    out: &mut Vec<Instruction>,
) -> Rvalue {
    match value {
        Rvalue::TupleLiteral {
            elements,
            element_types,
        } => {
            let ty = match target {
                Some(target @ Type::Tuple(_)) => target.clone(),
                _ => Type::Tuple(element_types.clone()),
            };
            match values.class_of(&ty) {
                Some(class_name) => Rvalue::Construct {
                    class_name,
                    fields: elements
                        .into_iter()
                        .enumerate()
                        .map(|(index, value)| MirFieldInit {
                            name: element_field(index),
                            value,
                        })
                        .collect(),
                },
                None => Rvalue::TupleLiteral {
                    elements,
                    element_types,
                },
            }
        }
        Rvalue::TupleElement {
            tuple,
            index,
            element_type,
        } => match operand_class(&tuple, types, values) {
            Some(_) => Rvalue::Member {
                object: tuple,
                field: element_field(index),
            },
            None => Rvalue::TupleElement {
                tuple,
                index,
                element_type,
            },
        },
        Rvalue::TupleTakeElement {
            place,
            index,
            element_type,
        } => match operand_class(&Operand::Place(place.clone()), types, values) {
            Some(_) => Rvalue::Member {
                object: Operand::Place(place),
                field: element_field(index),
            },
            None => Rvalue::TupleTakeElement {
                place,
                index,
                element_type,
            },
        },
        Rvalue::EnumVariant {
            enum_name,
            variant_name,
            payloads,
        } => {
            let class_name = target
                .filter(|target| matches!(target, Type::Named(name, _) if *name == enum_name))
                .cloned()
                .and_then(|target| values.class_of(&target));
            let built = class_name.and_then(|class_name| {
                let variants = values.variants.get(&class_name)?.clone();
                let chosen = variants.iter().position(|(name, declared)| {
                    *name == variant_name && declared.len() == payloads.len()
                })?;
                let mut fields = vec![MirFieldInit {
                    name: TAG_FIELD.to_string(),
                    value: Operand::Int(chosen as u128),
                }];
                for (variant, (_, declared)) in variants.iter().enumerate() {
                    for (index, ty) in declared.iter().enumerate() {
                        let value = if variant == chosen {
                            payloads[index].clone()
                        } else {
                            zero(ty, values, fresh, out)
                        };
                        fields.push(MirFieldInit {
                            name: payload_field(variant, index),
                            value,
                        });
                    }
                }
                Some(Rvalue::Construct { class_name, fields })
            });
            built.unwrap_or(Rvalue::EnumVariant {
                enum_name,
                variant_name,
                payloads,
            })
        }
        Rvalue::VariantPayload {
            scrutinee,
            variant_name,
            index,
        } => {
            let field = operand_class(&scrutinee, types, values).and_then(|class| {
                let variants = values.variants.get(&class)?;
                let variant = variants
                    .iter()
                    .position(|(name, declared)| *name == variant_name && index < declared.len())?;
                Some(payload_field(variant, index))
            });
            match field {
                Some(field) => Rvalue::Member {
                    object: scrutinee,
                    field,
                },
                None => Rvalue::VariantPayload {
                    scrutinee,
                    variant_name,
                    index,
                },
            }
        }
        other => other,
    }
}

/// Pushes `block` on `blocks`. A `match` on an enum the pass rewrote
/// becomes a comparison of the tag for each arm, each in a block of its
/// own after the first.
fn lower_match(
    mut block: BasicBlock,
    types: &HashMap<String, Type>,
    values: &mut Values,
    fresh: &mut Fresh,
    blocks: &mut Vec<BasicBlock>,
) {
    let Terminator::Match {
        scrutinee,
        arms,
        otherwise,
    } = &block.terminator
    else {
        blocks.push(block);
        return;
    };
    let tests = operand_class(scrutinee, types, values).and_then(|class| {
        let variants = values.variants.get(&class)?;
        // Each arm's variant index, or `None` for a wildcard.
        arms.iter()
            .map(|arm| {
                if arm.wildcard {
                    return Some(None);
                }
                let name = arm.variant_name.as_ref()?;
                variants
                    .iter()
                    .position(|(variant, _)| variant == name)
                    .map(Some)
            })
            .collect::<Option<Vec<_>>>()
    });
    let Some(tests) = tests else {
        blocks.push(block);
        return;
    };
    let object = match scrutinee {
        Operand::MovePlace(place) => Operand::Place(place.clone()),
        other => other.clone(),
    };
    let labels = arms.iter().map(|arm| arm.label.clone()).collect::<Vec<_>>();
    let otherwise = otherwise.clone();
    let tag = fresh.local(Type::named(TAG_TYPE));
    block.instructions.push(Instruction::Assign {
        target: tag.clone(),
        value: Rvalue::Member {
            object,
            field: TAG_FIELD.to_string(),
        },
    });
    let base = block.label.clone();
    let mut current = block;
    let mut finished = false;
    for (position, (test, label)) in tests.into_iter().zip(labels).enumerate() {
        let Some(variant) = test else {
            current.terminator = Terminator::Goto(label);
            finished = true;
            break;
        };
        let holds = fresh.local(Type::named("bool"));
        current.instructions.push(Instruction::Assign {
            target: holds.clone(),
            value: Rvalue::Binary {
                op: BinaryOp::Eq,
                left: Operand::Place(tag.clone()),
                right: Operand::Int(variant as u128),
                span: crate::diag::Span::new(0, 0),
                trap: Default::default(),
            },
        });
        let next = format!("{base}_value_test_{position}");
        current.terminator = Terminator::Branch {
            condition: Operand::Place(holds),
            then_label: label,
            else_label: next.clone(),
        };
        blocks.push(std::mem::replace(
            &mut current,
            BasicBlock {
                label: next,
                instructions: Vec::new(),
                terminator: Terminator::Unreachable,
            },
        ));
    }
    if !finished {
        current.terminator = Terminator::Goto(otherwise);
    }
    blocks.push(current);
}

#[cfg(test)]
#[path = "native_codegen_freestanding_values_tests.rs"]
mod tests;
