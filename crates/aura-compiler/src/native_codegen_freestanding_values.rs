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
//!
//! A type inside itself, such as the `indirect` payload of a list or a
//! tree, has no fixed size. Its field holds the address of a block
//! instead: a reference count, then the value's words. The block
//! functions are Aura source over `mem.alloc`. For each class that owns a
//! block the pass makes functions that store, load, drop, retain, and
//! release, and it calls retain and release where the ownership pass
//! stated a reference (ADR-0160).

use super::*;
use crate::diag::Diagnostic;
use crate::mir::{MirClassField, MirFieldInit, MirLocalType, MirParam, MirReceiverKind};

/// What each class the pass makes is named. A source name holds no `:`.
const CLASS_PREFIX: &str = "aura::values::";

/// The library of boxes, in Aura.
const BOX_SOURCE: &str = include_str!("../std/freestanding_values.au");

/// The path a failed check in the library reports to the trap hook.
const BOX_PATH: &str = "aura/freestanding_values.au";

/// What each function the pass adds is named.
const FUNCTION_PREFIX: &str = "aura::values::";

/// The functions of the library.
const BOX_NEW: &str = "aura::values::value_box_new";
const BOX_RETAIN: &str = "aura::values::value_box_retain";
const BOX_RELEASE: &str = "aura::values::value_box_release";
const BOX_FREE: &str = "aura::values::value_box_free";

/// The functions the pass makes for a class, each named with its kind and
/// the class. The emitter writes the code of a call of the first two.
const STORE: &str = "box_store";
const LOAD: &str = "box_load";
const DROP_BOX: &str = "drop_box";
const RETAIN: &str = "retain";
const RELEASE: &str = "release";

/// The function of kind `kind` for the class `class`.
fn class_function(kind: &str, class: &str) -> String {
    let class = class.strip_prefix(CLASS_PREFIX).unwrap_or(class);
    format!("{FUNCTION_PREFIX}{kind}::{class}")
}

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

/// What the pass knows of one tuple or enum type.
#[derive(Clone)]
enum Known {
    /// Its class is being made, under this name. A part of this type
    /// inside itself lies in a box.
    Pending(String),
    /// Its class, or `None` for a type the pass leaves alone.
    Done(Option<String>),
}

/// A field of a class that holds the address of a box: a heap block of a
/// reference count and then the words of a value of type `content`.
#[derive(Clone)]
struct BoxField {
    field: String,
    content: Type,
}

struct Values {
    enums: HashMap<String, MirEnum>,
    /// The program's classes and the pass's, by name.
    classes: HashMap<String, MirClass>,
    /// Each tuple or enum type asked about, by the type's text.
    known: HashMap<String, Known>,
    /// The types in `known`, in the order the pass met them.
    met: Vec<String>,
    /// The variants of each enum class.
    variants: HashMap<String, Variants>,
    /// The box fields of each class the pass made that has any.
    boxes: HashMap<String, Vec<BoxField>>,
    /// The types a box holds, by text.
    boxed: HashSet<String>,
    made: Vec<MirClass>,
    /// The number of the next class.
    next: usize,
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
    /// or a class of words, whose zero value the pass can write.
    fn fits_a_field(&self, ty: &Type) -> bool {
        matches!(ty, Type::Named(..)) && super::freestanding::words_only(ty, &self.classes)
    }

    /// The type of the field that holds a part of type `part` in a class
    /// being made, and what the field's box holds when the part lies in
    /// one: a part whose own class is still being made holds the type it
    /// is part of.
    fn part(&mut self, part: &Type) -> (Type, Option<Type>) {
        if let Some(Known::Pending(_)) = self.known.get(&part.to_string()) {
            self.boxed.insert(part.to_string());
            return (Type::named("uint64"), Some(part.clone()));
        }
        (self.rewritten(part), None)
    }

    /// The class that stands for the tuple or enum type `ty`.
    fn class_of(&mut self, ty: &Type) -> Option<String> {
        let key = ty.to_string();
        match self.known.get(&key) {
            Some(Known::Done(class)) => return class.clone(),
            Some(Known::Pending(name)) => return Some(name.clone()),
            None => {}
        }
        let name = format!("{CLASS_PREFIX}{}", self.next);
        self.next += 1;
        let start = self.met.len();
        self.met.push(key.clone());
        self.known.insert(key.clone(), Known::Pending(name.clone()));
        let made = self.make_class(ty, &name);
        if made && !(self.boxed.contains(&key) && self.holds_a_union(&name, &mut HashSet::new())) {
            self.known.insert(key, Known::Done(Some(name.clone())));
            return Some(name);
        }
        // A class made on the way may hold a box of this type. Forget each
        // one, so a later question makes it again without this type.
        for forgotten in self.met.split_off(start) {
            if let Some(Known::Done(Some(class)) | Known::Pending(class)) =
                self.known.remove(&forgotten)
            {
                self.classes.remove(&class);
                self.variants.remove(&class);
                self.boxes.remove(&class);
                self.made.retain(|made| made.name != class);
            }
            self.boxed.remove(&forgotten);
        }
        self.known.insert(key.clone(), Known::Done(None));
        self.met.push(key);
        None
    }

    /// Makes the class `name` for `ty`. Returns whether every part fits.
    fn make_class(&mut self, ty: &Type, name: &str) -> bool {
        let mut fields = Vec::new();
        let mut boxes = Vec::new();
        match ty {
            Type::Tuple(elements) => {
                for (index, element) in elements.iter().enumerate() {
                    let (element, content) = self.part(element);
                    if !super::freestanding::words_only(&element, &self.classes) {
                        return false;
                    }
                    if let Some(content) = content {
                        boxes.push(BoxField {
                            field: element_field(index),
                            content,
                        });
                    }
                    fields.push(MirClassField {
                        name: element_field(index),
                        ty: element,
                    });
                }
            }
            Type::Named(enum_name, args) => {
                let Some(layout) = self.enums.get(enum_name).cloned() else {
                    return false;
                };
                if layout.type_params.len() != args.len() {
                    return false;
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
                        let (payload, content) =
                            self.part(&crate::sema::substitute_type(payload, &bound));
                        if !self.fits_a_field(&payload) {
                            return false;
                        }
                        if let Some(content) = content {
                            boxes.push(BoxField {
                                field: payload_field(variant, index),
                                content,
                            });
                        }
                        fields.push(MirClassField {
                            name: payload_field(variant, index),
                            ty: payload.clone(),
                        });
                        payloads.push(payload);
                    }
                    variants.push((declared.name.clone(), payloads));
                }
                self.variants.insert(name.to_string(), variants);
            }
            _ => return false,
        }
        // The class needs drop exactly when the type it stands for did, so
        // the references the ownership pass stated still add up. A class
        // of words with no box drops nothing either way.
        let copy = boxes.is_empty()
            && fields.iter().all(|field| match &field.ty {
                Type::Named(name, _) => self.classes.get(name).is_none_or(|class| class.copy),
                Type::Union(union) => union.members.iter().all(|member| match member {
                    Type::Named(name, _) => self.classes.get(name).is_none_or(|class| class.copy),
                    _ => true,
                }),
                _ => true,
            });
        let class = MirClass {
            name: name.to_string(),
            type_params: Vec::new(),
            fields,
            methods: Vec::new(),
            copy,
        };
        self.classes.insert(name.to_string(), class.clone());
        self.made.push(class);
        if !boxes.is_empty() {
            self.boxes.insert(name.to_string(), boxes);
        }
        true
    }

    /// Whether a value of class `name` holds a union at any depth outside
    /// its boxes. A box holds only scalars.
    fn holds_a_union(&self, name: &str, seen: &mut HashSet<String>) -> bool {
        if !seen.insert(name.to_string()) {
            return false;
        }
        self.classes.get(name).is_some_and(|class| {
            class.fields.iter().any(|field| match &field.ty {
                Type::Named(inner, _) => self.holds_a_union(inner, seen),
                _ => true,
            })
        })
    }

    /// Whether a value of class `name` holds a box at any depth outside
    /// its boxes, so it owns heap memory.
    fn holds_a_box(&self, name: &str) -> bool {
        fn visit(values: &Values, name: &str, seen: &mut HashSet<String>) -> bool {
            if !seen.insert(name.to_string()) {
                return false;
            }
            values.boxes.contains_key(name)
                || values.classes.get(name).is_some_and(|class| {
                    class.fields.iter().any(|field| {
                        match &field.ty {
                        Type::Named(inner, _) => visit(values, inner, seen),
                        Type::Union(union) => union.members.iter().any(|member| {
                            matches!(member, Type::Named(inner, _) if visit(values, inner, seen))
                        }),
                        _ => false,
                    }
                    })
                })
        }
        visit(self, name, &mut HashSet::new())
    }

    /// Whether a value of class `name` holds a union one of whose members
    /// owns a box. The pass counts no references through a union.
    fn holds_a_union_of_boxes(&self, name: &str) -> bool {
        fn visit(values: &Values, name: &str, seen: &mut HashSet<String>) -> bool {
            if !seen.insert(name.to_string()) {
                return false;
            }
            values.classes.get(name).is_some_and(|class| {
                class.fields.iter().any(|field| match &field.ty {
                    Type::Named(inner, _) => visit(values, inner, seen),
                    Type::Union(union) => union.members.iter().any(
                        |member| matches!(member, Type::Named(inner, _) if values.holds_a_box(inner)),
                    ),
                    _ => false,
                })
            })
        }
        visit(self, name, &mut HashSet::new())
    }

    /// How many words a value of the class or scalar type `ty` takes in a
    /// box.
    fn words(&self, ty: &Type) -> u128 {
        match ty {
            Type::Named(name, _) => match self.classes.get(name) {
                Some(class) => class.fields.iter().map(|field| self.words(&field.ty)).sum(),
                None => 1,
            },
            _ => 1,
        }
    }
}

/// Rewrites the tuples and enums of plain values in `module`, a
/// freestanding program, to Copy classes.
pub(super) fn lower_plain_values(module: &mut MirModule) -> Vec<Diagnostic> {
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
        met: Vec::new(),
        variants: HashMap::new(),
        boxes: HashMap::new(),
        boxed: HashSet::new(),
        made: Vec::new(),
        next: 1,
    };
    // The classes first, so a function sees which of them hold a box.
    for class in &mut module.classes {
        if class.type_params.is_empty() {
            for field in &mut class.fields {
                field.ty = values.rewritten(&field.ty);
            }
            values.classes.insert(class.name.clone(), class.clone());
        }
    }
    let mut problems = Vec::new();
    for function in &mut module.functions {
        lower_function(function, &mut values, &mut problems);
    }
    if !values.boxed.is_empty() {
        let made = box_functions(&mut values);
        // A retain or a release reads its value and leaves it with the
        // caller.
        module.borrowing.extend(
            made.iter()
                .filter(|function| {
                    function
                        .params
                        .iter()
                        .all(|param| param.passing == MirReceiverKind::Borrow)
                })
                .map(|function| function.name.clone()),
        );
        module.functions.extend(made);
        module
            .functions
            .extend(super::freestanding_heap::library_functions(
                BOX_SOURCE,
                "value_box_",
                FUNCTION_PREFIX,
                BOX_PATH,
            ));
    }
    module.classes.extend(values.made);
    problems
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

/// Adds `target = function(args)` to `out` and returns `target`, a new
/// temporary of type `returns`.
fn call(
    fresh: &mut Fresh,
    out: &mut Vec<Instruction>,
    function: String,
    args: Vec<Operand>,
    returns: Type,
) -> String {
    let target = fresh.local(returns);
    out.push(Instruction::Assign {
        target: target.clone(),
        value: Rvalue::Call {
            callee: CallTarget::Name(function),
            args: args
                .into_iter()
                .map(|value| MirArg {
                    name: None,
                    value,
                    writeback_place: None,
                })
                .collect(),
            type_args: Default::default(),
        },
    });
    target
}

/// The box field `field` of the class `class`, if it is one.
fn box_field(values: &Values, class: &str, field: &str) -> Option<BoxField> {
    values
        .boxes
        .get(class)?
        .iter()
        .find(|known| known.field == field)
        .cloned()
}

/// The operand that fills the field `field` of a new value of the class
/// `class` with `operand`: the address of a new box that holds it when the
/// field is a box, and `operand` itself otherwise.
fn into_field(
    values: &mut Values,
    fresh: &mut Fresh,
    out: &mut Vec<Instruction>,
    class: &str,
    field: &str,
    operand: Operand,
) -> Operand {
    let Some(found) = box_field(values, class, field) else {
        return operand;
    };
    let content = values.rewritten(&found.content);
    let Type::Named(content_class, _) = &content else {
        return operand;
    };
    let address = call(
        fresh,
        out,
        BOX_NEW.to_string(),
        vec![Operand::Int(values.words(&content))],
        Type::named("uint64"),
    );
    call(
        fresh,
        out,
        class_function(STORE, content_class),
        vec![Operand::Place(address.clone()), operand],
        Type::Unit,
    );
    Operand::Place(address)
}

/// The read of the field `field` of `object`, a value of the class
/// `class`: the value in the box when the field is one.
fn out_of_field(
    values: &mut Values,
    fresh: &mut Fresh,
    out: &mut Vec<Instruction>,
    class: &str,
    field: String,
    object: Operand,
) -> Rvalue {
    // A part moved out is read as a part copied out: the caller counts a
    // reference for it, and the release of the whole drops the first.
    let object = match object {
        Operand::MovePlace(place) => Operand::Place(place),
        other => other,
    };
    let content = box_field(values, class, &field).map(|found| values.rewritten(&found.content));
    let Some(Type::Named(content_class, _)) = content else {
        return Rvalue::Member { object, field };
    };
    let address = fresh.local(Type::named("uint64"));
    out.push(Instruction::Assign {
        target: address.clone(),
        value: Rvalue::Member { object, field },
    });
    Rvalue::Call {
        callee: CallTarget::Name(class_function(LOAD, &content_class)),
        args: vec![MirArg {
            name: None,
            value: Operand::Place(address),
            writeback_place: None,
        }],
        type_args: Default::default(),
    }
}

/// The class of the value in `place`, when a value of it owns a box.
fn owner(place: &str, types: &HashMap<String, Type>, values: &mut Values) -> Option<String> {
    let ty = types.get(place)?.clone();
    match values.rewritten(&ty) {
        Type::Named(name, _) if values.holds_a_box(&name) => Some(name),
        _ => None,
    }
}

/// Adds the call that retains or releases the boxes of the value in
/// `place`, when it owns any. Returns false for a place the pass cannot
/// count references for: a part of a value that owns a box.
fn count_references(
    kind: &str,
    place: &str,
    types: &HashMap<String, Type>,
    values: &mut Values,
    fresh: &mut Fresh,
    out: &mut Vec<Instruction>,
) -> bool {
    let root = place.split(['.', '[']).next().unwrap_or(place);
    let Some(class) = owner(root, types, values) else {
        return true;
    };
    if root.len() != place.len() || values.holds_a_union_of_boxes(&class) {
        return false;
    }
    call(
        fresh,
        out,
        class_function(kind, &class),
        vec![Operand::Place(place.to_string())],
        Type::Unit,
    );
    true
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

fn lower_function(function: &mut MirFunction, values: &mut Values, problems: &mut Vec<Diagnostic>) {
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
    // Whether the pass counted every reference to a box.
    let mut counted = true;
    let excluded = excluded_variants(&function.blocks);
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
                Instruction::Retain { place } => {
                    counted &= count_references(
                        RETAIN,
                        &place,
                        &types,
                        values,
                        &mut fresh,
                        &mut current.instructions,
                    );
                    current.instructions.push(Instruction::Retain { place });
                }
                Instruction::Release { place } => {
                    counted &= count_references(
                        RELEASE,
                        &place,
                        &types,
                        values,
                        &mut fresh,
                        &mut current.instructions,
                    );
                    current.instructions.push(Instruction::Release { place });
                }
                // A lend moves a part out for one call and back after it.
                // The hosted planner would not lend a class of words, so
                // the call gets a reference of its own instead.
                Instruction::Lend {
                    place,
                    temp,
                    mutable,
                } if types
                    .get(&temp)
                    .is_some_and(|ty| values.rewritten(ty) != *ty) =>
                {
                    counted &= !mutable;
                    let value = match place.rsplit_once('.') {
                        Some((object, field)) if !field.contains('[') => Rvalue::Member {
                            object: Operand::Place(object.to_string()),
                            field: field.to_string(),
                        },
                        _ => Rvalue::Use(Operand::Place(place)),
                    };
                    current.instructions.push(Instruction::Assign {
                        target: temp.clone(),
                        value,
                    });
                    counted &= count_references(
                        RETAIN,
                        &temp,
                        &types,
                        values,
                        &mut fresh,
                        &mut current.instructions,
                    );
                }
                Instruction::EndLend { temp, .. }
                    if types
                        .get(&temp)
                        .is_some_and(|ty| values.rewritten(ty) != *ty) =>
                {
                    counted &= count_references(
                        RELEASE,
                        &temp,
                        &types,
                        values,
                        &mut fresh,
                        &mut current.instructions,
                    );
                    current
                        .instructions
                        .push(Instruction::Release { place: temp });
                }
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
                    // A read of a part is a new reference to what the
                    // part owns, and so is a part moved out. A write of a part
                    // is not counted yet.
                    let reads = matches!(
                        value,
                        Rvalue::TupleElement { .. }
                            | Rvalue::TupleTakeElement { .. }
                            | Rvalue::VariantPayload { .. }
                            | Rvalue::Member { .. }
                    );
                    let root = target.split(['.', '[']).next().unwrap_or(&target);
                    if root.len() != target.len() && owner(root, &types, values).is_some() {
                        counted = false;
                    }
                    let value = lower_value(
                        value,
                        types.get(&target),
                        &types,
                        values,
                        &mut fresh,
                        &mut current.instructions,
                    );
                    current.instructions.push(Instruction::Assign {
                        target: target.clone(),
                        value,
                    });
                    if reads {
                        counted &= count_references(
                            RETAIN,
                            &target,
                            &types,
                            values,
                            &mut fresh,
                            &mut current.instructions,
                        );
                    }
                }
                other => current.instructions.push(other),
            }
        }
        current.terminator = terminator;
        lower_match(
            current,
            excluded.get(&label),
            &types,
            values,
            &mut fresh,
            &mut blocks,
        );
    }
    function.blocks = blocks;
    if !counted {
        let mut problem = Diagnostic::coded_at(
            super::freestanding::UNSUPPORTED,
            function.span,
            format!(
                "`{}` changes a part of a value that owns heap memory, which a freestanding build cannot compile",
                public_direct_function_name(&function.name)
            ),
        )
        .with_help("build a new value instead, or keep the part in a local");
        if let Some(path) = &function.source_path {
            problem.render_path = Some(path.clone());
        }
        problems.push(problem);
    }
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
    // The error path releases these, and the pass counts no references
    // through a union.
    if releases_on_error.iter().any(|place| {
        owner(place, types, values).is_some_and(|class| values.holds_a_union_of_boxes(&class))
    }) {
        return false;
    }
    let (Some(operand_class), Some(return_class)) =
        (values.class_of(&operand_type), values.class_of(returns))
    else {
        return false;
    };
    let copy =
        |values: &Values, class: &str| values.classes.get(class).is_some_and(|class| class.copy);
    // A result that owns heap memory is consumed: each path takes a
    // reference to the part it keeps and releases the whole.
    let consumed = !copy(values, &operand_class);
    let moved = |values: &Values, ty: &Type, place: String| match ty {
        Type::Named(name, _) if values.classes.contains_key(name) && !copy(values, name) => {
            Operand::MovePlace(place)
        }
        _ => Operand::Place(place),
    };
    let mut wide = types.clone();
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
    let mut kept = vec![Instruction::Assign {
        target: target.to_string(),
        value: Rvalue::Member {
            object: Operand::Place(result.to_string()),
            field: payload_field(ok, 0),
        },
    }];
    if consumed {
        count_references(RETAIN, target, types, values, fresh, &mut kept);
        count_references(RELEASE, result, types, values, fresh, &mut kept);
        kept.push(Instruction::Release {
            place: result.to_string(),
        });
    }
    let next = BasicBlock {
        label: ok_label,
        instructions: kept,
        terminator: Terminator::Unreachable,
    };
    blocks.push(std::mem::replace(current, next));

    let mut instructions = Vec::new();
    let carried = fresh.local(error_type.clone());
    instructions.push(Instruction::Assign {
        target: carried.clone(),
        value: Rvalue::Member {
            object: Operand::Place(result.to_string()),
            field: payload_field(error, 0),
        },
    });
    if consumed {
        wide.insert(carried.clone(), error_type.clone());
        count_references(RETAIN, &carried, &wide, values, fresh, &mut instructions);
        count_references(RELEASE, result, types, values, fresh, &mut instructions);
        instructions.push(Instruction::Release {
            place: result.to_string(),
        });
    }
    let carried_type = if from.is_some() {
        returned_error_type.clone()
    } else {
        error_type.clone()
    };
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
                        value: moved(values, &error_type, carried),
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
    let returned_type = Type::named(&return_class);
    let returned = fresh.local(returned_type.clone());
    let mut fields = vec![MirFieldInit {
        name: TAG_FIELD.to_string(),
        value: Operand::Int(return_error as u128),
    }];
    // The fields lie in the order of the variants.
    let mut payloads = [
        (return_ok, ok_zero),
        (return_error, moved(values, &carried_type, carried)),
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
    for place in releases_on_error {
        count_references(RELEASE, place, types, values, fresh, &mut instructions);
        instructions.push(Instruction::Release {
            place: place.clone(),
        });
    }
    blocks.push(BasicBlock {
        label: error_label,
        instructions,
        terminator: Terminator::Return(moved(values, &returned_type, returned)),
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
                Some(class_name) => {
                    let fields = elements
                        .into_iter()
                        .enumerate()
                        .map(|(index, value)| {
                            let name = element_field(index);
                            let value = into_field(values, fresh, out, &class_name, &name, value);
                            MirFieldInit { name, value }
                        })
                        .collect();
                    Rvalue::Construct { class_name, fields }
                }
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
            Some(class) => out_of_field(values, fresh, out, &class, element_field(index), tuple),
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
                            into_field(
                                values,
                                fresh,
                                out,
                                &class_name,
                                &payload_field(variant, index),
                                payloads[index].clone(),
                            )
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
            let found = operand_class(&scrutinee, types, values).and_then(|class| {
                let variants = values.variants.get(&class)?;
                let variant = variants
                    .iter()
                    .position(|(name, declared)| *name == variant_name && index < declared.len())?;
                Some((class, payload_field(variant, index)))
            });
            match found {
                Some((class, field)) => out_of_field(values, fresh, out, &class, field, scrutinee),
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

/// For each block a `match` reaches only when none of its arms hold, the
/// variants the value is then known not to be. Lowering tests one arm per
/// block, each going on to the next.
fn excluded_variants(blocks: &[BasicBlock]) -> HashMap<String, HashSet<String>> {
    let mut entries = HashMap::<&str, usize>::new();
    for block in blocks {
        for label in super::elements::successors(&block.terminator) {
            *entries.entry(label).or_default() += 1;
        }
    }
    let tested = |block: &BasicBlock| match &block.terminator {
        Terminator::Match {
            scrutinee: Operand::Place(place) | Operand::MovePlace(place),
            arms,
            otherwise,
        } => {
            let names = arms
                .iter()
                .map(|arm| arm.variant_name.clone().filter(|_| !arm.wildcard))
                .collect::<Option<Vec<_>>>()?;
            Some((place.clone(), names, otherwise.clone()))
        }
        _ => None,
    };
    let mut excluded = HashMap::<String, HashSet<String>>::new();
    // A chain is at most as long as the function has blocks.
    for _ in 0..blocks.len() {
        let mut changed = false;
        for block in blocks {
            let Some((place, names, otherwise)) = tested(block) else {
                continue;
            };
            let next = blocks.iter().find(|next| next.label == otherwise);
            let follows = next.is_some_and(|next| {
                next.instructions.is_empty()
                    && entries.get(otherwise.as_str()) == Some(&1)
                    && tested(next).is_some_and(|(same, _, _)| same == place)
            });
            if !follows {
                continue;
            }
            let mut known = excluded.get(&block.label).cloned().unwrap_or_default();
            known.extend(names);
            if excluded.get(&otherwise) != Some(&known) {
                excluded.insert(otherwise, known);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    excluded
}

/// Pushes `block` on `blocks`. A `match` on an enum the pass rewrote
/// becomes a comparison of the tag for each arm, each in a block of its
/// own after the first.
fn lower_match(
    mut block: BasicBlock,
    excluded: Option<&HashSet<String>>,
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
    // How many variants the value can still be when the block starts.
    let mut open = 0;
    let tests = operand_class(scrutinee, types, values).and_then(|class| {
        let variants = values.variants.get(&class)?;
        open = variants
            .iter()
            .filter(|(variant, _)| excluded.is_none_or(|names| !names.contains(variant)))
            .count();
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
        // The last variant left needs no test: the checker saw the
        // `match` cover every variant, and the tests alone do not tell
        // it that.
        open = open.saturating_sub(1);
        let (Some(variant), true) = (test, open > 0) else {
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

/// A function the pass makes, with its parameters among its locals.
fn made_function(
    name: String,
    params: Vec<(&str, MirReceiverKind, Type)>,
    return_type: Type,
    mut locals: Vec<MirLocalType>,
    blocks: Vec<BasicBlock>,
) -> MirFunction {
    let params = params
        .into_iter()
        .map(|(name, passing, ty)| MirParam {
            name: name.to_string(),
            passing,
            ty,
            default_function: None,
            keyword_only: false,
        })
        .collect::<Vec<_>>();
    locals.extend(params.iter().map(|param| MirLocalType {
        name: param.name.clone(),
        ty: param.ty.clone(),
    }));
    locals.sort_by(|left, right| left.name.cmp(&right.name));
    MirFunction {
        name,
        module_name: "aura".to_string(),
        source_path: Some(BOX_PATH.to_string()),
        span: crate::diag::Span::new(1, 1),
        receiver: None,
        params,
        local_types: locals,
        return_type,
        entry: "entry".to_string(),
        blocks,
    }
}

fn one_block(instructions: Vec<Instruction>, terminator: Terminator) -> Vec<BasicBlock> {
    vec![BasicBlock {
        label: "entry".to_string(),
        instructions,
        terminator,
    }]
}

/// The functions that keep the boxes of a program: for each type a box
/// holds, the store, the load, and the drop of a box of it; and for each
/// class that owns a box, its retain and its release.
fn box_functions(values: &mut Values) -> Vec<MirFunction> {
    let word = || Type::named("uint64");
    let new_fresh = || Fresh {
        count: 0,
        locals: Vec::new(),
    };
    let mut functions = Vec::new();
    let mut contents = values.boxed.iter().cloned().collect::<Vec<_>>();
    contents.sort();
    for content in contents {
        let Some(Known::Done(Some(class))) = values.known.get(&content).cloned() else {
            continue;
        };
        let ty = Type::named(&class);
        // The emitter writes each call of the store and the load, so the
        // bodies only keep the module whole: the store drops its value,
        // and the load returns a value of zeros.
        functions.push(made_function(
            class_function(STORE, &class),
            vec![
                ("address", MirReceiverKind::Borrow, word()),
                ("value", MirReceiverKind::Value, ty.clone()),
            ],
            Type::Unit,
            Vec::new(),
            one_block(
                vec![Instruction::Release {
                    place: "value".to_string(),
                }],
                Terminator::Return(Operand::Unit),
            ),
        ));
        let mut fresh = new_fresh();
        let mut instructions = Vec::new();
        let empty = zero(&ty, values, &mut fresh, &mut instructions);
        functions.push(made_function(
            class_function(LOAD, &class),
            vec![("address", MirReceiverKind::Borrow, word())],
            ty.clone(),
            fresh.locals,
            one_block(instructions, Terminator::Return(empty)),
        ));
        // The drop of a box: one reference fewer, and at the last one
        // the value in it is released and the block freed.
        let mut fresh = new_fresh();
        let mut entry = Vec::new();
        let last = call(
            &mut fresh,
            &mut entry,
            BOX_RELEASE.to_string(),
            vec![Operand::Place("address".to_string())],
            Type::named("bool"),
        );
        let mut free = Vec::new();
        let held = call(
            &mut fresh,
            &mut free,
            class_function(LOAD, &class),
            vec![Operand::Place("address".to_string())],
            ty.clone(),
        );
        if values.holds_a_box(&class) {
            call(
                &mut fresh,
                &mut free,
                class_function(RELEASE, &class),
                vec![Operand::Place(held.clone())],
                Type::Unit,
            );
        }
        free.push(Instruction::Release { place: held });
        call(
            &mut fresh,
            &mut free,
            BOX_FREE.to_string(),
            vec![
                Operand::Place("address".to_string()),
                Operand::Int(values.words(&ty)),
            ],
            Type::Unit,
        );
        functions.push(made_function(
            class_function(DROP_BOX, &class),
            vec![("address", MirReceiverKind::Borrow, word())],
            Type::Unit,
            fresh.locals,
            vec![
                BasicBlock {
                    label: "entry".to_string(),
                    instructions: entry,
                    terminator: Terminator::Branch {
                        condition: Operand::Place(last),
                        then_label: "free".to_string(),
                        else_label: "done".to_string(),
                    },
                },
                BasicBlock {
                    label: "free".to_string(),
                    instructions: free,
                    terminator: Terminator::Return(Operand::Unit),
                },
                BasicBlock {
                    label: "done".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Unit),
                },
            ],
        ));
    }
    let mut owners = values
        .classes
        .keys()
        .filter(|name| values.holds_a_box(name))
        .cloned()
        .collect::<Vec<_>>();
    owners.sort();
    for class in owners {
        let fields = values.classes[&class].fields.clone();
        for kind in [RETAIN, RELEASE] {
            let mut fresh = new_fresh();
            let mut instructions = Vec::new();
            for field in &fields {
                let Type::Named(inner, _) = &field.ty else {
                    continue;
                };
                let boxed = box_field(values, &class, &field.name);
                if boxed.is_none() && !values.holds_a_box(inner) {
                    continue;
                }
                let part = fresh.local(field.ty.clone());
                instructions.push(Instruction::Assign {
                    target: part.clone(),
                    value: Rvalue::Member {
                        object: Operand::Place("value".to_string()),
                        field: field.name.clone(),
                    },
                });
                match boxed {
                    Some(found) => {
                        let function = if kind == RETAIN {
                            BOX_RETAIN.to_string()
                        } else {
                            match values.rewritten(&found.content) {
                                Type::Named(content, _) => class_function(DROP_BOX, &content),
                                _ => continue,
                            }
                        };
                        call(
                            &mut fresh,
                            &mut instructions,
                            function,
                            vec![Operand::Place(part)],
                            Type::Unit,
                        );
                    }
                    None => {
                        call(
                            &mut fresh,
                            &mut instructions,
                            class_function(kind, inner),
                            vec![Operand::Place(part.clone())],
                            Type::Unit,
                        );
                        instructions.push(Instruction::Release { place: part });
                    }
                }
            }
            functions.push(made_function(
                class_function(kind, &class),
                vec![("value", MirReceiverKind::Borrow, Type::named(&class))],
                Type::Unit,
                fresh.locals,
                one_block(instructions, Terminator::Return(Operand::Unit)),
            ));
        }
    }
    functions
}

/// The scalars of a value of type `ty`, in the order of its words.
fn leaf_kinds(ty: &DirectType, kinds: &mut Vec<ScalarKind>) {
    match ty {
        DirectType::Scalar(kind) => kinds.push(*kind),
        DirectType::PlainClass(class) => {
            for field in &class.fields {
                leaf_kinds(&field.ty, kinds);
            }
        }
        DirectType::Union(_) | DirectType::Callable(_) | DirectType::Opaque(_) => {}
    }
}

/// Where word `index` of the value in a box lies: after the reference
/// count.
fn box_leaf(index: usize, kind: ScalarKind) -> super::plain::PlainLeaf {
    super::plain::PlainLeaf {
        offset: 8 + 8 * index as i32,
        kind,
    }
}

impl FunctionCompiler<'_> {
    /// A call of the store or the load of a box in a freestanding object:
    /// one store or one load for each word of the value.
    pub(super) fn compile_box_call(
        &mut self,
        callee: &CallTarget,
        args: &[MirArg],
        target: &DirectType,
    ) -> std::result::Result<Option<ValueRef>, String> {
        let CallTarget::Name(name) = callee else {
            return Ok(None);
        };
        let Some(kind) = name.strip_prefix(FUNCTION_PREFIX) else {
            return Ok(None);
        };
        if self.freestanding.is_none() {
            return Ok(None);
        }
        let mut kinds = Vec::new();
        if kind.starts_with(STORE) {
            let address = self.load_operand(&args[0].value)?.values[0];
            let value = self.load_operand(&args[1].value)?;
            leaf_kinds(&value.ty, &mut kinds);
            for (index, (kind, word)) in kinds.into_iter().zip(&value.values).enumerate() {
                self.store_leaf(box_leaf(index, kind), *word, address);
            }
            return Ok(Some(unit_value(self.builder)));
        }
        if kind.starts_with(LOAD) {
            let address = self.load_operand(&args[0].value)?.values[0];
            leaf_kinds(target, &mut kinds);
            let values = kinds
                .into_iter()
                .enumerate()
                .map(|(index, kind)| self.load_leaf(box_leaf(index, kind), address))
                .collect();
            return Ok(Some(ValueRef {
                values,
                ty: target.clone(),
            }));
        }
        Ok(None)
    }
}

#[cfg(test)]
#[path = "native_codegen_freestanding_values_tests.rs"]
mod tests;
