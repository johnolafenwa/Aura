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
use crate::mir::{
    MethodDispatch, MirClassField, MirFieldInit, MirLocalType, MirParam, MirReceiverKind,
};
use crate::runtime_ops::{ListOp, RuntimeOp, StrOp};

/// What each class the pass makes is named. A source name holds no `:`.
const CLASS_PREFIX: &str = "aura::values::";

/// The library of boxes, in Aura.
const BOX_SOURCE: &str = include_str!("../std/freestanding_values.au");

/// The path a failed check in the library reports to the trap hook.
const BOX_PATH: &str = "aura/freestanding_values.au";

/// The path a failed check in the text library reports.
const TEXT_PATH: &str = "aura/freestanding_text.au";

/// What each function the pass adds is named.
const FUNCTION_PREFIX: &str = "aura::values::";

/// The functions of the library.
const BOX_NEW: &str = "aura::values::value_box_new";
const BOX_RETAIN: &str = "aura::values::value_box_retain";
const BOX_RELEASE: &str = "aura::values::value_box_release";
const BOX_FREE: &str = "aura::values::value_box_free";

/// The library of text, in Aura.
const TEXT_SOURCE: &str = include_str!("../std/freestanding_text.au");

/// The class that stands for `str`: the address of a text block.
const TEXT_CLASS: &str = "aura::values::str";
const TEXT_FIELD: &str = "address";

/// The address of a literal's bytes in the program's data. The emitter
/// writes each call.
const TEXT_LITERAL: &str = "aura::values::text_literal";

/// The functions of the text library.
const TEXT_NEW: &str = "aura::values::value_text_new";
const TEXT_BYTES: &str = "aura::values::value_text_bytes";
const TEXT_BYTE_LENGTH: &str = "aura::values::value_text_byte_length";
const TEXT_RELEASE: &str = "aura::values::value_text_release";
const TEXT_CONCAT: &str = "aura::values::value_text_concat";
const TEXT_EQUAL: &str = "aura::values::value_text_equal";
const TEXT_COMPARE: &str = "aura::values::value_text_compare";
const TEXT_LENGTH: &str = "aura::values::value_text_length";
const TEXT_BYTE_COUNT: &str = "aura::values::value_text_byte_count";
const TEXT_STARTS_WITH: &str = "aura::values::value_text_starts_with";
const TEXT_ENDS_WITH: &str = "aura::values::value_text_ends_with";
const TEXT_CONTAINS: &str = "aura::values::value_text_contains";
const TEXT_FROM_INT: &str = "aura::values::value_text_from_int";
const TEXT_FROM_UINT: &str = "aura::values::value_text_from_uint";
const TEXT_FROM_BOOL: &str = "aura::values::value_text_from_bool";

/// The library of lists, in Aura.
const LIST_SOURCE: &str = include_str!("../std/freestanding_list.au");
const LIST_PATH: &str = "aura/freestanding_list.au";

/// What the class of each list type is named, before its element type.
const LIST_CLASS_PREFIX: &str = "aura::values::list::";

/// The functions of the list library.
const LIST_NEW: &str = "aura::values::value_list_new";
const LIST_RELEASE: &str = "aura::values::value_list_release";
const LIST_LENGTH: &str = "aura::values::value_list_length";
const LIST_PUSH_SLOT: &str = "aura::values::value_list_push_slot";
const LIST_SLOT: &str = "aura::values::value_list_slot";
const LIST_SLOT_UNCHECKED: &str = "aura::values::value_list_slot_unchecked";
const LIST_CLEAR: &str = "aura::values::value_list_clear";
const LIST_FREE: &str = "aura::values::value_list_free";
const LIST_TAKE_SLOT: &str = "aura::values::value_list_take_slot";
const LIST_TAKE_LAST_SLOT: &str = "aura::values::value_list_take_last_slot";
const LIST_INSERT_SLOT: &str = "aura::values::value_list_insert_slot";
const LIST_SWAP: &str = "aura::values::value_list_swap";
const LIST_REVERSE: &str = "aura::values::value_list_reverse";
const LIST_RESERVE: &str = "aura::values::value_list_reserve";

/// The function the pass makes for a list class whose elements own heap
/// memory: it releases each element and leaves the list empty.
const CLEAR_LIST: &str = "clear_list";

/// The function the pass makes for a list class whose elements own heap
/// memory: one reference fewer, and at the last one each element is
/// released and the list freed.
const DROP_LIST: &str = "drop_list";

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

/// The field of a union's class that holds the member `index`.
fn member_field(index: usize) -> String {
    format!("{MEMBER_FIELD_PREFIX}{index}")
}

const MEMBER_FIELD_PREFIX: &str = "member_";

/// How a place in MIR names the payload of a union.
const UNION_PAYLOAD_PREFIX: &str = "__union_payload_";

/// Renames each payload of a union the pass rewrote, in every place of
/// `function`, to the field of the union's class. The validators read the
/// MIR name as a union's payload, which needs a tag test the pass turns
/// into a comparison.
fn rename_union_payloads(
    function: &mut MirFunction,
    types: &HashMap<String, Type>,
    values: &mut Values,
) {
    fn rename(value: &mut serde_json::Value, types: &HashMap<String, Type>, values: &mut Values) {
        match value {
            serde_json::Value::String(text) if text.contains(UNION_PAYLOAD_PREFIX) => {
                let mut parts = text.split('.');
                let Some(root) = parts.next() else {
                    return;
                };
                let mut renamed = root.to_string();
                let mut ty = types.get(root).map(|ty| values.rewritten(ty));
                for part in parts {
                    let fields = match &ty {
                        Some(Type::Named(class, _)) => values
                            .classes
                            .get(class)
                            .map(|class| class.fields.clone())
                            .unwrap_or_default(),
                        _ => Vec::new(),
                    };
                    let field = part
                        .strip_prefix(UNION_PAYLOAD_PREFIX)
                        .map(|index| format!("{MEMBER_FIELD_PREFIX}{index}"))
                        .filter(|field| fields.iter().any(|known| known.name == *field))
                        .unwrap_or_else(|| part.to_string());
                    ty = fields
                        .iter()
                        .find(|known| known.name == field)
                        .map(|known| known.ty.clone());
                    renamed.push('.');
                    renamed.push_str(&field);
                }
                *text = renamed;
            }
            serde_json::Value::Array(items) => {
                items
                    .iter_mut()
                    .for_each(|item| rename(item, types, values));
            }
            serde_json::Value::Object(fields) => {
                fields
                    .values_mut()
                    .for_each(|item| rename(item, types, values));
            }
            _ => {}
        }
    }
    let named = function.blocks.iter().any(|block| {
        serde_json::to_string(block).is_ok_and(|text| text.contains(UNION_PAYLOAD_PREFIX))
    });
    if !named {
        return;
    }
    let mut value = serde_json::to_value(&function.blocks).expect("MIR blocks are JSON values");
    rename(&mut value, types, values);
    function.blocks = serde_json::from_value(value).expect("renamed MIR blocks read back");
}

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
    /// Whether the program holds a `str`.
    text: bool,
    /// The element type of each list class, after the rewrite.
    lists: HashMap<String, Type>,
    /// Each dictionary class.
    dicts: HashMap<String, dicts::Dict>,
    /// The lists of keys, values, and entries the program asks a
    /// dictionary class for.
    views: HashSet<(String, dicts::View)>,
    /// The functions whose shared parameters stay with the caller.
    borrowing: HashSet<String>,
    /// How each function takes its parameters, with their names.
    passing: HashMap<String, Vec<(String, MirReceiverKind)>>,
}

impl Values {
    /// `ty` with each tuple and enum the pass rewrites replaced by its
    /// class.
    fn rewritten(&mut self, ty: &Type) -> Type {
        match ty {
            Type::Named(name, args) if name == "str" && args.is_empty() => {
                Type::named(self.text_class())
            }
            Type::Named(name, args) if name == "list" && args.len() == 1 => {
                match self.list_class(&args[0]) {
                    Some(class) => Type::named(&class),
                    None => ty.clone(),
                }
            }
            Type::Named(name, args) if name == "dict" && args.len() == 2 => {
                match self.dict_class(&args[0], &args[1]) {
                    Some(class) => Type::named(&class),
                    None => ty.clone(),
                }
            }
            Type::Tuple(_) => match self.class_of(ty) {
                Some(class) => Type::named(&class),
                None => ty.clone(),
            },
            Type::Named(name, _) if self.enums.contains_key(name) => match self.class_of(ty) {
                Some(class) => Type::named(&class),
                None => ty.clone(),
            },
            Type::Union(_) => match self.class_of(ty) {
                Some(class) => Type::named(&class),
                None => ty.clone(),
            },
            _ => ty.clone(),
        }
    }

    /// The class that stands for `str`, made at its first use: one field,
    /// the address of a text block.
    fn text_class(&mut self) -> &'static str {
        if !self.text {
            self.text = true;
            let class = MirClass {
                c_layout: None,
                name: TEXT_CLASS.to_string(),
                type_params: Vec::new(),
                fields: vec![MirClassField {
                    name: TEXT_FIELD.to_string(),
                    ty: Type::named("uint64"),
                }],
                methods: Vec::new(),
                copy: false,
            };
            self.classes.insert(TEXT_CLASS.to_string(), class.clone());
            self.made.push(class);
            self.boxes.insert(
                TEXT_CLASS.to_string(),
                vec![BoxField {
                    field: TEXT_FIELD.to_string(),
                    content: Type::named("str"),
                }],
            );
        }
        TEXT_CLASS
    }

    /// The class that stands for a list of `element`, made at its first
    /// use: one field, the address of the list's header. Only a list of
    /// scalars has one.
    fn list_class(&mut self, element: &Type) -> Option<String> {
        let (element, suffix) = self.element(element)?;
        let element = &element;
        let name = format!("{LIST_CLASS_PREFIX}{suffix}");
        if !self.lists.contains_key(&name) {
            self.lists.insert(name.clone(), element.clone());
            let class = MirClass {
                c_layout: None,
                name: name.clone(),
                type_params: Vec::new(),
                fields: vec![MirClassField {
                    name: TEXT_FIELD.to_string(),
                    ty: Type::named("uint64"),
                }],
                methods: Vec::new(),
                copy: false,
            };
            self.classes.insert(name.clone(), class.clone());
            self.made.push(class);
            self.boxes.insert(
                name.clone(),
                vec![BoxField {
                    field: TEXT_FIELD.to_string(),
                    content: Type::Named("list".to_string(), vec![element.clone()]),
                }],
            );
        }
        Some(name)
    }

    /// The type a list or a dictionary holds `element` as, and the part
    /// of a class name that stands for it: a scalar, or a class of words
    /// with no union in it.
    fn element(&mut self, element: &Type) -> Option<(Type, String)> {
        let scalar = matches!(
            element,
            Type::Named(name, args) if args.is_empty() && matches!(
                name.as_str(),
                "bool" | "int8" | "int16" | "int32" | "int64" | "uint8" | "uint16" | "uint32"
                    | "uint64" | "float32" | "float64"
            )
        );
        // Any other element is a class of words with no union in it: text,
        // a tuple or an enum the pass rewrote, or a class of the program.
        let element = &self.rewritten(element);
        let suffix = match element {
            _ if scalar => element.to_string(),
            Type::Named(class, args)
                if args.is_empty()
                    && self
                        .classes
                        .get(class)
                        .is_some_and(|known| known.type_params.is_empty())
                    && super::freestanding::words_only(element, &self.classes)
                    && !self.holds_a_union(class, &mut HashSet::new()) =>
            {
                class
                    .strip_prefix(CLASS_PREFIX)
                    .unwrap_or(class)
                    .to_string()
            }
            _ => return None,
        };
        Some((element.clone(), suffix))
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
            // A union with a member the pass rewrites: a tag, the index of
            // the member it holds, and a field for each member but `None`.
            // A union of words stays with the emitter.
            Type::Union(union) => {
                fields.push(MirClassField {
                    name: TAG_FIELD.to_string(),
                    ty: Type::named(TAG_TYPE),
                });
                let mut changed = false;
                for (index, member) in union.members.iter().enumerate() {
                    if *member == Type::Unit {
                        continue;
                    }
                    let (field, content) = self.part(member);
                    changed |= field != *member;
                    if !self.fits_a_field(&field) {
                        return false;
                    }
                    if let Some(content) = content {
                        boxes.push(BoxField {
                            field: member_field(index),
                            content,
                        });
                    }
                    fields.push(MirClassField {
                        name: member_field(index),
                        ty: field,
                    });
                }
                if !changed {
                    return false;
                }
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
            c_layout: None,
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
        text: false,
        lists: HashMap::new(),
        dicts: HashMap::new(),
        views: HashSet::new(),
        borrowing: module.borrowing.iter().cloned().collect(),
        passing: module
            .functions
            .iter()
            .map(|function| {
                let params = function
                    .params
                    .iter()
                    .map(|param| (param.name.clone(), param.passing))
                    .collect();
                (function.name.clone(), params)
            })
            .collect(),
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
    if !values.boxed.is_empty()
        || values.text
        || !values.lists.is_empty()
        || !values.dicts.is_empty()
    {
        // The functions of the dictionaries first: one may make a list
        // class.
        let of_dicts = dicts::dict_functions(&mut values);
        let mut made = box_functions(&mut values);
        made.extend(of_dicts);
        // The store and the load of an element. The emitter writes each
        // call, so the bodies only keep the module whole.
        let mut elements = values.lists.values().cloned().collect::<Vec<_>>();
        for dict in values.dicts.values() {
            elements.push(dict.key.clone());
            elements.push(dict.value.clone());
        }
        elements.sort_by_key(|element| element.to_string());
        elements.dedup();
        for element in elements {
            let name = element.to_string();
            // A box of the same type has already made the two.
            if made
                .iter()
                .any(|function| function.name == class_function(STORE, &name))
            {
                continue;
            }
            // The store takes a value that needs drop, and drops it.
            let drops = matches!(
                &element,
                Type::Named(class, _) if values.classes.get(class).is_some_and(|class| !class.copy)
            );
            made.push(made_function(
                class_function(STORE, &name),
                vec![
                    ("address", MirReceiverKind::Borrow, Type::named("uint64")),
                    (
                        "value",
                        if drops {
                            MirReceiverKind::Value
                        } else {
                            MirReceiverKind::Borrow
                        },
                        element.clone(),
                    ),
                ],
                Type::Unit,
                Vec::new(),
                one_block(
                    if drops {
                        vec![Instruction::Release {
                            place: "value".to_string(),
                        }]
                    } else {
                        Vec::new()
                    },
                    Terminator::Return(Operand::Unit),
                ),
            ));
            let mut fresh = Fresh {
                count: 0,
                locals: Vec::new(),
            };
            let mut instructions = Vec::new();
            let empty = zero(&element, &values, &mut fresh, &mut instructions);
            made.push(made_function(
                class_function(LOAD, &name),
                vec![("address", MirReceiverKind::Borrow, Type::named("uint64"))],
                element,
                fresh.locals,
                one_block(instructions, Terminator::Return(empty)),
            ));
        }
        if values.text {
            // The emitter writes each call, so the body only keeps the
            // module whole.
            made.push(made_function(
                TEXT_LITERAL.to_string(),
                vec![("text", MirReceiverKind::Borrow, Type::named("uint64"))],
                Type::named("uint64"),
                Vec::new(),
                one_block(Vec::new(), Terminator::Return(Operand::Int(0))),
            ));
        }
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
        if !values.lists.is_empty() {
            module
                .functions
                .extend(super::freestanding_heap::library_functions(
                    LIST_SOURCE,
                    "value_list_",
                    FUNCTION_PREFIX,
                    LIST_PATH,
                ));
        }
        if !values.dicts.is_empty() {
            module
                .functions
                .extend(super::freestanding_heap::library_functions(
                    dicts::DICT_SOURCE,
                    dicts::DICT_LIBRARY_PREFIX,
                    FUNCTION_PREFIX,
                    dicts::DICT_SOURCE_PATH,
                ));
        }
        if values.text {
            module
                .functions
                .extend(super::freestanding_heap::library_functions(
                    TEXT_SOURCE,
                    "value_text_",
                    FUNCTION_PREFIX,
                    TEXT_PATH,
                ));
        }
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
    /// Whether `name` is a temporary this pass made.
    fn made(&self, name: &str) -> bool {
        self.locals.iter().any(|local| local.name == name)
    }

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
    rename_union_payloads(function, &types, values);

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
    // The indexes whose range fact the pass drops.
    let mut unproven = HashSet::new();
    // The shared loans of a whole element of a list the pass rewrote.
    let mut loans = HashSet::new();
    for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
        if let Instruction::BeginElementLoan {
            loan,
            source,
            projection,
            mutable: false,
            ..
        } = instruction
        {
            let listed = matches!(
                place_type(source, &types, values),
                Some(Type::Named(class, _)) if values.lists.contains_key(&class)
            );
            let whole = matches!(
                (place_type(source, &types, values), types.get(loan)),
                (Some(Type::Named(class, _)), Some(ty))
                    if values.lists.get(&class).cloned() == Some(values.rewritten(ty))
            );
            if listed && whole && projection.is_empty() {
                loans.insert(loan.clone());
            }
            // A shared loan of a whole value of a dictionary.
            let valued = matches!(
                (place_type(source, &types, values), types.get(loan)),
                (Some(Type::Named(class, _)), Some(ty))
                    if values.dicts.get(&class).map(|dict| dict.value.clone())
                        == Some(values.rewritten(ty))
            );
            if valued && projection.is_empty() {
                loans.insert(loan.clone());
            }
        }
        // A shared loan of the payload of a union the pass rewrote.
        if let Instruction::BeginLoan {
            loan,
            source,
            mutable: false,
        } = instruction
        {
            let payload = source.rsplit_once('.').is_some_and(|(object, field)| {
                field.starts_with(MEMBER_FIELD_PREFIX)
                    && types
                        .get(object)
                        .is_some_and(|ty| values.rewritten(ty) != *ty)
            });
            if payload {
                loans.insert(loan.clone());
            }
        }
    }
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
                // A shared loan of an element is a reference to it, held
                // until the loan ends.
                Instruction::BeginElementLoan {
                    loan,
                    source,
                    selector,
                    checked,
                    ..
                } if loans.contains(&loan) => {
                    let element = types.get(&loan).map(|ty| values.rewritten(ty));
                    let keyed = match place_type(&source, &types, values) {
                        Some(Type::Named(class, _)) if values.dicts.contains_key(&class) => {
                            Some(class)
                        }
                        _ => None,
                    };
                    let address = fresh.local(Type::named("uint64"));
                    current.instructions.push(Instruction::Assign {
                        target: address.clone(),
                        value: Rvalue::Member {
                            object: Operand::Place(source),
                            field: TEXT_FIELD.to_string(),
                        },
                    });
                    let mut after = Vec::new();
                    let slot = match keyed {
                        Some(class) => dicts::value_slot(
                            &class,
                            &address,
                            selector,
                            values,
                            &mut fresh,
                            &mut current.instructions,
                            &mut after,
                        ),
                        None => call(
                            &mut fresh,
                            &mut current.instructions,
                            if checked {
                                LIST_SLOT.to_string()
                            } else {
                                LIST_SLOT_UNCHECKED.to_string()
                            },
                            vec![Operand::Place(address), selector],
                            Type::named("uint64"),
                        ),
                    };
                    let name = element.map(|ty| ty.to_string()).unwrap_or_default();
                    current.instructions.push(Instruction::Assign {
                        target: loan.clone(),
                        value: Rvalue::Call {
                            callee: CallTarget::Name(class_function(LOAD, &name)),
                            args: vec![MirArg {
                                name: None,
                                value: Operand::Place(slot),
                                writeback_place: None,
                            }],
                            type_args: Default::default(),
                        },
                    });
                    counted &= count_references(
                        RETAIN,
                        &loan,
                        &types,
                        values,
                        &mut fresh,
                        &mut current.instructions,
                    );
                    current.instructions.extend(after);
                }
                Instruction::BeginLoan { loan, source, .. } if loans.contains(&loan) => {
                    let (object, field) = source.rsplit_once('.').unwrap_or((&source, ""));
                    current.instructions.push(Instruction::Assign {
                        target: loan.clone(),
                        value: Rvalue::Member {
                            object: Operand::Place(object.to_string()),
                            field: field.to_string(),
                        },
                    });
                    counted &= count_references(
                        RETAIN,
                        &loan,
                        &types,
                        values,
                        &mut fresh,
                        &mut current.instructions,
                    );
                }
                // A read through the loan is one more reference.
                Instruction::ReadLoan { target, loan } if loans.contains(&loan) => {
                    counted &= count_references(
                        RETAIN,
                        &loan,
                        &types,
                        values,
                        &mut fresh,
                        &mut current.instructions,
                    );
                    let drops = matches!(
                        types.get(&loan).map(|ty| values.rewritten(ty)),
                        Some(Type::Named(class, _))
                            if values.classes.get(&class).is_some_and(|class| !class.copy)
                    );
                    if drops {
                        current.instructions.push(Instruction::Retain {
                            place: loan.clone(),
                        });
                    }
                    current.instructions.push(Instruction::Assign {
                        target,
                        value: Rvalue::Use(Operand::Place(loan)),
                    });
                }
                Instruction::EndLoan { loan } if loans.contains(&loan) => {
                    counted &= count_references(
                        RELEASE,
                        &loan,
                        &types,
                        values,
                        &mut fresh,
                        &mut current.instructions,
                    );
                    let drops = matches!(
                        types.get(&loan).map(|ty| values.rewritten(ty)),
                        Some(Type::Named(class, _))
                            if values.classes.get(&class).is_some_and(|class| !class.copy)
                    );
                    if drops {
                        current
                            .instructions
                            .push(Instruction::Release { place: loan });
                    }
                }
                // The fact that an index is in range served the element
                // read the pass replaces with a library call.
                Instruction::IndexInRange { list, index }
                    if matches!(
                        place_type(&list, &types, values),
                        Some(Type::Named(class, _)) if values.lists.contains_key(&class)
                    ) =>
                {
                    unproven.insert(index);
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
                    // A list or a dictionary is the address of a header
                    // that never moves, so a change through the copy is a
                    // change of the part.
                    let shared = matches!(
                        types.get(&temp).map(|ty| values.rewritten(ty)),
                        Some(Type::Named(class, _))
                            if values.lists.contains_key(&class)
                                || values.dicts.contains_key(&class)
                    );
                    counted &= !mutable || shared;
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
                            | Rvalue::UnionTakePayload { .. }
                            | Rvalue::Member { .. }
                            | Rvalue::Call {
                                callee: CallTarget::Runtime {
                                    op: RuntimeOp::List(ListOp::Index | ListOp::IndexUnchecked)
                                        | RuntimeOp::Dict(crate::runtime_ops::DictOp::Index),
                                    ..
                                },
                                ..
                            }
                    );
                    let root = target.split(['.', '[']).next().unwrap_or(&target);
                    if root.len() != target.len() && owner(root, &types, values).is_some() {
                        counted = false;
                    }
                    let mut after = Vec::new();
                    let value = lower_text(
                        value,
                        &types,
                        values,
                        &mut fresh,
                        &mut current.instructions,
                        &mut after,
                    );
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
                    current.instructions.extend(after);
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
        current.terminator = match terminator {
            Terminator::Return(literal @ Operand::String(_)) => Terminator::Return(owned_text(
                literal,
                values,
                &mut fresh,
                &mut current.instructions,
            )),
            other => other,
        };
        lower_match(
            current,
            excluded.get(&label),
            &types,
            values,
            &mut fresh,
            &mut blocks,
        );
    }
    // The fact also showed that stepping the index cannot overflow, so
    // without it the step is checked again.
    if !unproven.is_empty() {
        for instruction in blocks.iter_mut().flat_map(|block| &mut block.instructions) {
            if let Instruction::Assign {
                target,
                value: Rvalue::Binary { trap, .. },
            } = instruction
            {
                if unproven.contains(target) && *trap == crate::mir::ArithmeticTrap::None {
                    *trap = crate::mir::ArithmeticTrap::Overflow;
                }
            }
        }
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

/// The type of the value in `place`, a local or a field of one, after
/// the rewrite.
fn place_type(place: &str, types: &HashMap<String, Type>, values: &mut Values) -> Option<Type> {
    let mut parts = place.split('.');
    let mut ty = values.rewritten(types.get(parts.next()?)?);
    for field in parts {
        let Type::Named(class, _) = &ty else {
            return None;
        };
        ty = values
            .classes
            .get(class)?
            .fields
            .iter()
            .find(|known| known.name == field)?
            .ty
            .clone();
    }
    Some(ty)
}

/// Whether `operand` is text: a literal, or a place that holds a `str`.
fn is_text(operand: &Operand, types: &HashMap<String, Type>, values: &mut Values) -> bool {
    match operand {
        Operand::String(_) => true,
        Operand::Place(place) | Operand::MovePlace(place) => {
            matches!(place_type(place, types, values), Some(Type::Named(name, _)) if name == TEXT_CLASS)
        }
        _ => false,
    }
}

/// The address of the first byte of the text `operand` and its length in
/// bytes. A literal lies in the program's data and needs no block.
fn text_view(
    operand: Operand,
    fresh: &mut Fresh,
    out: &mut Vec<Instruction>,
) -> (Operand, Operand) {
    let word = || Type::named("uint64");
    match operand {
        Operand::String(text) => {
            let length = Operand::Int(text.len() as u128);
            let bytes = call(
                fresh,
                out,
                TEXT_LITERAL.to_string(),
                vec![Operand::String(text)],
                word(),
            );
            (Operand::Place(bytes), length)
        }
        Operand::Place(place) | Operand::MovePlace(place) => {
            let address = fresh.local(word());
            out.push(Instruction::Assign {
                target: address.clone(),
                value: Rvalue::Member {
                    object: Operand::Place(place),
                    field: TEXT_FIELD.to_string(),
                },
            });
            block_view(&address, fresh, out)
        }
        other => (other, Operand::Int(0)),
    }
}

/// The address of the first byte and the length in bytes of the text in
/// the block whose address the local `address` holds.
fn block_view(address: &str, fresh: &mut Fresh, out: &mut Vec<Instruction>) -> (Operand, Operand) {
    let word = || Type::named("uint64");
    let bytes = call(
        fresh,
        out,
        TEXT_BYTES.to_string(),
        vec![Operand::Place(address.to_string())],
        word(),
    );
    let length = call(
        fresh,
        out,
        TEXT_BYTE_LENGTH.to_string(),
        vec![Operand::Place(address.to_string())],
        word(),
    );
    (Operand::Place(bytes), Operand::Place(length))
}

/// The library function that writes a value of type `ty` as text, and
/// the type the value is cast to first when it is narrower.
fn text_writer(ty: &Type) -> Option<(&'static str, Option<&'static str>)> {
    let Type::Named(name, args) = ty else {
        return None;
    };
    if !args.is_empty() {
        return None;
    }
    Some(match name.as_str() {
        "int64" => (TEXT_FROM_INT, None),
        "int8" | "int16" | "int32" => (TEXT_FROM_INT, Some("int64")),
        "uint64" => (TEXT_FROM_UINT, None),
        "uint8" | "uint16" | "uint32" => (TEXT_FROM_UINT, Some("uint64")),
        "bool" => (TEXT_FROM_BOOL, None),
        _ => return None,
    })
}

/// The `str` an f-string builds: each part appended to the text so far.
/// Returns `None`, with nothing added, when a part is neither text, an
/// integer, nor a `bool`, or has a format of its own.
fn lower_format(
    parts: &[crate::mir::MirFormatPart],
    types: &HashMap<String, Type>,
    values: &mut Values,
    fresh: &mut Fresh,
    out: &mut Vec<Instruction>,
) -> Option<Rvalue> {
    use crate::mir::MirFormatPart;
    let word = || Type::named("uint64");
    // What writes each part: `None` for text.
    let mut writers = Vec::with_capacity(parts.len());
    for part in parts {
        writers.push(match part {
            MirFormatPart::Literal(_) => None,
            MirFormatPart::Int64(_) => Some((TEXT_FROM_INT, None)),
            MirFormatPart::Value(operand) => match operand {
                Operand::String(_) => None,
                Operand::Bool(_) => Some((TEXT_FROM_BOOL, None)),
                Operand::Int(_) => Some((TEXT_FROM_INT, None)),
                Operand::Place(place) | Operand::MovePlace(place) => {
                    if is_text(operand, types, values) {
                        None
                    } else {
                        Some(text_writer(&place_type(place, types, values)?)?)
                    }
                }
                _ => return None,
            },
            MirFormatPart::Formatted { .. } => return None,
        });
    }
    let release = |address: String, fresh: &mut Fresh, out: &mut Vec<Instruction>| {
        call(
            fresh,
            out,
            TEXT_RELEASE.to_string(),
            vec![Operand::Place(address)],
            Type::Unit,
        );
    };
    let mut so_far: Option<String> = None;
    for (part, writer) in parts.iter().zip(writers) {
        let operand = match part {
            MirFormatPart::Literal(text) => Operand::String(text.clone()),
            MirFormatPart::Int64(operand) | MirFormatPart::Value(operand) => operand.clone(),
            MirFormatPart::Formatted { value, .. } => value.clone(),
        };
        let mut written = None;
        let (bytes, length) = match writer {
            None => text_view(operand, fresh, out),
            Some((function, cast)) => {
                let operand = match cast {
                    Some(wide) => {
                        let local = fresh.local(Type::named(wide));
                        out.push(Instruction::Assign {
                            target: local.clone(),
                            value: Rvalue::Cast {
                                value: operand,
                                ty: Type::named(wide),
                                span: crate::diag::Span::new(0, 0),
                            },
                        });
                        Operand::Place(local)
                    }
                    None => operand,
                };
                let address = call(fresh, out, function.to_string(), vec![operand], word());
                let view = block_view(&address, fresh, out);
                written = Some(address);
                view
            }
        };
        let (start, start_length) = match &so_far {
            Some(address) => block_view(address, fresh, out),
            None => (Operand::Int(0), Operand::Int(0)),
        };
        let joined = call(
            fresh,
            out,
            TEXT_CONCAT.to_string(),
            vec![start, start_length, bytes, length],
            word(),
        );
        for spent in [so_far.replace(joined), written].into_iter().flatten() {
            release(spent, fresh, out);
        }
    }
    let address = match so_far {
        Some(address) => address,
        None => call(
            fresh,
            out,
            TEXT_NEW.to_string(),
            vec![Operand::Int(0), Operand::Int(0)],
            word(),
        ),
    };
    Some(Rvalue::Construct {
        class_name: values.text_class().to_string(),
        fields: vec![MirFieldInit {
            name: TEXT_FIELD.to_string(),
            value: Operand::Place(address),
        }],
    })
}

/// The `str` value whose block is at the address `function(args)` gives.
fn text_from(
    function: &str,
    args: Vec<Operand>,
    values: &mut Values,
    fresh: &mut Fresh,
    out: &mut Vec<Instruction>,
) -> Rvalue {
    let address = call(
        fresh,
        out,
        function.to_string(),
        args,
        Type::named("uint64"),
    );
    Rvalue::Construct {
        class_name: values.text_class().to_string(),
        fields: vec![MirFieldInit {
            name: TEXT_FIELD.to_string(),
            value: Operand::Place(address),
        }],
    }
}

/// `operand`, with a literal replaced by a new `str` value that holds a
/// copy of it, which whatever takes the operand then owns.
fn owned_text(
    operand: Operand,
    values: &mut Values,
    fresh: &mut Fresh,
    out: &mut Vec<Instruction>,
) -> Operand {
    let Operand::String(_) = operand else {
        return operand;
    };
    let (bytes, length) = text_view(operand, fresh, out);
    let value = text_from(TEXT_NEW, vec![bytes, length], values, fresh, out);
    let local = fresh.local(Type::named(values.text_class()));
    out.push(Instruction::Assign {
        target: local.clone(),
        value,
    });
    Operand::MovePlace(local)
}

/// Rewrites the text operations of `value` to calls of the text library.
/// `out` gains what runs before the value, and `after` what runs after
/// it is assigned.
fn lower_text(
    value: Rvalue,
    types: &HashMap<String, Type>,
    values: &mut Values,
    fresh: &mut Fresh,
    out: &mut Vec<Instruction>,
    after: &mut Vec<Instruction>,
) -> Rvalue {
    let bool_type = || Type::named("bool");
    let word = || Type::named("uint64");
    let plain = |value| MirArg {
        name: None,
        value,
        writeback_place: None,
    };
    let value = dicts::lower_dict(value, types, values, fresh, out, after);
    match value {
        Rvalue::VecLiteral {
            elements,
            element_type,
        } => {
            let Some(class_name) = values.list_class(&element_type) else {
                return Rvalue::VecLiteral {
                    elements,
                    element_type,
                };
            };
            let element_type = values.lists[&class_name].clone();
            let address = call(
                fresh,
                out,
                LIST_NEW.to_string(),
                vec![
                    Operand::Int(elements.len() as u128),
                    Operand::Int(values.words(&element_type)),
                ],
                word(),
            );
            for element in elements {
                let element = owned_text(element, values, fresh, out);
                let slot = call(
                    fresh,
                    out,
                    LIST_PUSH_SLOT.to_string(),
                    vec![Operand::Place(address.clone())],
                    word(),
                );
                call(
                    fresh,
                    out,
                    class_function(STORE, &element_type.to_string()),
                    vec![Operand::Place(slot), element],
                    Type::Unit,
                );
            }
            Rvalue::Construct {
                class_name,
                fields: vec![MirFieldInit {
                    name: TEXT_FIELD.to_string(),
                    value: Operand::Place(address),
                }],
            }
        }
        Rvalue::Call {
            callee:
                CallTarget::Runtime {
                    op: RuntimeOp::List(op),
                    receiver: Some(Operand::Place(receiver)),
                    receiver_place,
                },
            args,
            type_args,
        } => {
            let element = match place_type(&receiver, types, values) {
                Some(Type::Named(class, _)) => values.lists.get(&class).cloned(),
                _ => None,
            };
            // Whether an element owns heap memory, and the class it is.
            let owner = match &element {
                Some(Type::Named(inner, _)) if values.holds_a_box(inner) => Some(inner.clone()),
                _ => None,
            };
            let known = matches!(
                (op, args.len()),
                (ListOp::Len | ListOp::IsEmpty, 0)
                    | (ListOp::Push | ListOp::PushScalar(_), 1)
                    | (
                        ListOp::Index
                            | ListOp::IndexUnchecked
                            | ListOp::IndexScalar(_)
                            | ListOp::IndexScalarUnchecked(_),
                        3
                    )
                    | (ListOp::SetIndex | ListOp::SetIndexScalar(_), 4)
            );
            let known = known
                || matches!(
                    (op, args.len()),
                    (ListOp::Clear | ListOp::Reverse, 0)
                        | (ListOp::Pop | ListOp::Reserve, 1)
                        | (ListOp::Insert | ListOp::Swap, 2)
                );
            let (Some(element), true) = (element, known) else {
                return Rvalue::Call {
                    callee: CallTarget::Runtime {
                        op: RuntimeOp::List(op),
                        receiver: Some(Operand::Place(receiver)),
                        receiver_place,
                    },
                    args,
                    type_args,
                };
            };
            let receiver_name = receiver.clone();
            let address = fresh.local(word());
            out.push(Instruction::Assign {
                target: address.clone(),
                value: Rvalue::Member {
                    object: Operand::Place(receiver),
                    field: TEXT_FIELD.to_string(),
                },
            });
            let list = Operand::Place(address);
            let mut args = args.into_iter().map(|arg| arg.value);
            let named = |function: String, args: Vec<Operand>| Rvalue::Call {
                callee: CallTarget::Name(function),
                args: args.into_iter().map(plain).collect(),
                type_args: Default::default(),
            };
            let store = class_function(STORE, &element.to_string());
            let load = class_function(LOAD, &element.to_string());
            match op {
                ListOp::Len => named(LIST_LENGTH.to_string(), vec![list]),
                ListOp::Clear => match place_type(&receiver_name, types, values) {
                    Some(Type::Named(class, _)) if owner.is_some() => {
                        named(class_function(CLEAR_LIST, &class), vec![list])
                    }
                    _ => named(LIST_CLEAR.to_string(), vec![list]),
                },
                ListOp::Reverse => named(LIST_REVERSE.to_string(), vec![list]),
                ListOp::Reserve => named(
                    LIST_RESERVE.to_string(),
                    vec![list, args.next().unwrap_or(Operand::Unit)],
                ),
                ListOp::Swap => {
                    let left = args.next().unwrap_or(Operand::Unit);
                    let right = args.next().unwrap_or(Operand::Unit);
                    named(LIST_SWAP.to_string(), vec![list, left, right])
                }
                // The element moves out with the reference the list held.
                ListOp::Pop => {
                    let slot = match args.next() {
                        Some(Operand::Default(_)) | None => call(
                            fresh,
                            out,
                            LIST_TAKE_LAST_SLOT.to_string(),
                            vec![list],
                            word(),
                        ),
                        Some(index) => call(
                            fresh,
                            out,
                            LIST_TAKE_SLOT.to_string(),
                            vec![list, index],
                            word(),
                        ),
                    };
                    named(load, vec![Operand::Place(slot)])
                }
                ListOp::Insert => {
                    let index = args.next().unwrap_or(Operand::Unit);
                    let value = args.next().unwrap_or(Operand::Unit);
                    let value = owned_text(value, values, fresh, out);
                    let slot = call(
                        fresh,
                        out,
                        LIST_INSERT_SLOT.to_string(),
                        vec![list, index],
                        word(),
                    );
                    named(store, vec![Operand::Place(slot), value])
                }
                ListOp::IsEmpty => {
                    let length = call(
                        fresh,
                        out,
                        LIST_LENGTH.to_string(),
                        vec![list],
                        Type::named("int64"),
                    );
                    Rvalue::Binary {
                        op: BinaryOp::Eq,
                        left: Operand::Place(length),
                        right: Operand::Int(0),
                        span: crate::diag::Span::new(0, 0),
                        trap: Default::default(),
                    }
                }
                ListOp::Push | ListOp::PushScalar(_) => {
                    let value = args.next().unwrap_or(Operand::Unit);
                    let value = owned_text(value, values, fresh, out);
                    let slot = call(fresh, out, LIST_PUSH_SLOT.to_string(), vec![list], word());
                    named(store, vec![Operand::Place(slot), value])
                }
                ListOp::SetIndex | ListOp::SetIndexScalar(_) => {
                    let index = args.next().unwrap_or(Operand::Unit);
                    let value = args.next().unwrap_or(Operand::Unit);
                    let value = owned_text(value, values, fresh, out);
                    let slot = call(fresh, out, LIST_SLOT.to_string(), vec![list, index], word());
                    // The element the write replaces is released first.
                    if let Some(inner) = &owner {
                        let old = call(
                            fresh,
                            out,
                            load,
                            vec![Operand::Place(slot.clone())],
                            element.clone(),
                        );
                        call(
                            fresh,
                            out,
                            class_function(RELEASE, inner),
                            vec![Operand::Place(old.clone())],
                            Type::Unit,
                        );
                        out.push(Instruction::Release { place: old });
                    }
                    named(store, vec![Operand::Place(slot), value])
                }
                _ => {
                    let function = if matches!(op, ListOp::Index | ListOp::IndexScalar(_)) {
                        LIST_SLOT
                    } else {
                        LIST_SLOT_UNCHECKED
                    };
                    let index = args.next().unwrap_or(Operand::Unit);
                    let slot = call(fresh, out, function.to_string(), vec![list, index], word());
                    named(load, vec![Operand::Place(slot)])
                }
            }
        }
        Rvalue::FormatString { parts } => match lower_format(&parts, types, values, fresh, out) {
            Some(text) => text,
            None => Rvalue::FormatString { parts },
        },
        Rvalue::Use(literal @ Operand::String(_)) => {
            let (bytes, length) = text_view(literal, fresh, out);
            text_from(TEXT_NEW, vec![bytes, length], values, fresh, out)
        }
        Rvalue::Binary {
            op,
            left,
            right,
            span,
            trap: _,
        } if matches!(
            op,
            BinaryOp::Add
                | BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Less
                | BinaryOp::LessEq
                | BinaryOp::Greater
                | BinaryOp::GreaterEq
        ) && is_text(&left, types, values)
            && is_text(&right, types, values) =>
        {
            let (left, left_length) = text_view(left, fresh, out);
            let (right, right_length) = text_view(right, fresh, out);
            let args = vec![left, left_length, right, right_length];
            match op {
                BinaryOp::Add => text_from(TEXT_CONCAT, args, values, fresh, out),
                BinaryOp::Eq | BinaryOp::NotEq => {
                    let equal = call(fresh, out, TEXT_EQUAL.to_string(), args, bool_type());
                    Rvalue::Binary {
                        op: BinaryOp::Eq,
                        left: Operand::Place(equal),
                        right: Operand::Bool(op == BinaryOp::Eq),
                        span,
                        trap: Default::default(),
                    }
                }
                _ => {
                    let order = call(
                        fresh,
                        out,
                        TEXT_COMPARE.to_string(),
                        args,
                        Type::named("int64"),
                    );
                    Rvalue::Binary {
                        op,
                        left: Operand::Place(order),
                        right: Operand::Int(0),
                        span,
                        trap: Default::default(),
                    }
                }
            }
        }
        // A clone of a text is one more reference to its block, which
        // nothing changes.
        Rvalue::Call {
            callee:
                CallTarget::Runtime {
                    op: RuntimeOp::Value(crate::runtime_ops::ValueOp::Clone),
                    receiver: Some(Operand::Place(receiver)),
                    ..
                },
            ref args,
            ..
        } if args.is_empty() && is_text(&Operand::Place(receiver.clone()), types, values) => {
            let address = fresh.local(word());
            out.push(Instruction::Assign {
                target: address.clone(),
                value: Rvalue::Member {
                    object: Operand::Place(receiver),
                    field: TEXT_FIELD.to_string(),
                },
            });
            call(
                fresh,
                out,
                BOX_RETAIN.to_string(),
                vec![Operand::Place(address.clone())],
                Type::Unit,
            );
            Rvalue::Construct {
                class_name: values.text_class().to_string(),
                fields: vec![MirFieldInit {
                    name: TEXT_FIELD.to_string(),
                    value: Operand::Place(address),
                }],
            }
        }
        Rvalue::Call {
            callee:
                CallTarget::Runtime {
                    op: RuntimeOp::Str(op),
                    receiver: Some(receiver),
                    receiver_place,
                },
            args,
            type_args,
        } => {
            let function = match (op, args.as_slice()) {
                (StrOp::Len, []) => Some((TEXT_LENGTH, Type::named("int64"))),
                (StrOp::ByteLen, []) => Some((TEXT_BYTE_COUNT, Type::named("int64"))),
                (StrOp::StartsWith, [_]) => Some((TEXT_STARTS_WITH, bool_type())),
                (StrOp::EndsWith, [_]) => Some((TEXT_ENDS_WITH, bool_type())),
                (StrOp::Contains, [_]) => Some((TEXT_CONTAINS, bool_type())),
                _ => None,
            };
            let Some((function, returns)) = function else {
                return Rvalue::Call {
                    callee: CallTarget::Runtime {
                        op: RuntimeOp::Str(op),
                        receiver: Some(receiver),
                        receiver_place,
                    },
                    args,
                    type_args,
                };
            };
            let (bytes, length) = text_view(receiver, fresh, out);
            let mut operands = vec![bytes, length];
            for arg in args {
                let (bytes, length) = text_view(arg.value, fresh, out);
                operands.extend([bytes, length]);
            }
            let result = call(fresh, out, function.to_string(), operands, returns);
            Rvalue::Use(Operand::Place(result))
        }
        // A `str` value given to a foreign function crosses as the
        // address of its bytes, which end in a zero byte, and as the
        // address and the length where the declaration asks for both.
        Rvalue::Call {
            callee: CallTarget::Extern(mut foreign),
            args,
            type_args,
        } if foreign.params.len() == args.len() => {
            let word = |name: String, passing| crate::mir::MirExternParam {
                name,
                passing,
                ty: Type::named("uint64"),
                c_string: false,
                pointer: false,
                c_struct: None,
            };
            let mut params = Vec::with_capacity(args.len());
            let mut operands = Vec::with_capacity(args.len());
            for (param, arg) in std::mem::take(&mut foreign.params).into_iter().zip(args) {
                let text = matches!(&param.ty, Type::Named(name, _) if name == "str")
                    && matches!(arg.value, Operand::Place(_) | Operand::MovePlace(_))
                    && is_text(&arg.value, types, values);
                if !text {
                    params.push(param);
                    operands.push(arg);
                    continue;
                }
                let (bytes, length) = text_view(arg.value, fresh, out);
                let plain = |value| MirArg {
                    name: None,
                    value,
                    writeback_place: None,
                };
                params.push(word(param.name.clone(), param.passing));
                operands.push(plain(bytes));
                if !param.c_string {
                    params.push(word(format!("{}_length", param.name), param.passing));
                    operands.push(plain(length));
                }
            }
            foreign.params = params;
            Rvalue::Call {
                callee: CallTarget::Extern(foreign),
                args: operands,
                type_args,
            }
        }
        Rvalue::Call {
            callee,
            mut args,
            type_args,
        } => {
            let name = match &callee {
                CallTarget::Name(name) => Some(name.clone()),
                CallTarget::Member {
                    dispatch: Some(MethodDispatch::Static { function }),
                    ..
                } => Some(function.clone()),
                _ => None,
            };
            if let Some(params) = name
                .as_ref()
                .and_then(|name| values.passing.get(name))
                .cloned()
            {
                let lends = name
                    .as_ref()
                    .is_some_and(|name| values.borrowing.contains(name));
                for (position, arg) in args.iter_mut().enumerate() {
                    if !matches!(arg.value, Operand::String(_)) {
                        continue;
                    }
                    let passing = match &arg.name {
                        Some(name) => params.iter().find(|(param, _)| param == name),
                        None => params.get(position),
                    }
                    .map(|(_, passing)| *passing);
                    let literal = std::mem::replace(&mut arg.value, Operand::Unit);
                    let Operand::MovePlace(local) = owned_text(literal, values, fresh, out) else {
                        continue;
                    };
                    // A callee that borrows leaves the value here, to be
                    // released after the call. Any other takes it.
                    if lends && passing == Some(MirReceiverKind::Borrow) {
                        arg.value = Operand::Place(local.clone());
                        call(
                            fresh,
                            after,
                            class_function(RELEASE, TEXT_CLASS),
                            vec![Operand::Place(local.clone())],
                            Type::Unit,
                        );
                        after.push(Instruction::Release { place: local });
                    } else {
                        arg.value = Operand::MovePlace(local);
                    }
                }
            }
            Rvalue::Call {
                callee,
                args,
                type_args,
            }
        }
        Rvalue::TupleLiteral {
            elements,
            element_types,
        } => Rvalue::TupleLiteral {
            elements: elements
                .into_iter()
                .map(|element| owned_text(element, values, fresh, out))
                .collect(),
            element_types,
        },
        Rvalue::EnumVariant {
            enum_name,
            variant_name,
            payloads,
        } => Rvalue::EnumVariant {
            enum_name,
            variant_name,
            payloads: payloads
                .into_iter()
                .map(|payload| owned_text(payload, values, fresh, out))
                .collect(),
        },
        Rvalue::Construct { class_name, fields } => Rvalue::Construct {
            class_name,
            fields: fields
                .into_iter()
                .map(|field| MirFieldInit {
                    name: field.name,
                    value: owned_text(field.value, values, fresh, out),
                })
                .collect(),
        },
        other => other,
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
        Rvalue::UnionInject {
            value,
            union_type,
            member_type,
            member_index,
        } => match values.class_of(&union_type) {
            Some(class_name) => {
                let members = values.classes[&class_name]
                    .fields
                    .iter()
                    .skip(1)
                    .map(|field| (field.name.clone(), field.ty.clone()))
                    .collect::<Vec<_>>();
                let mut fields = vec![MirFieldInit {
                    name: TAG_FIELD.to_string(),
                    value: Operand::Int(member_index as u128),
                }];
                let chosen = member_field(member_index);
                for (name, ty) in members {
                    let value = if name == chosen {
                        let value = owned_text(value.clone(), values, fresh, out);
                        into_field(values, fresh, out, &class_name, &name, value)
                    } else {
                        zero(&ty, values, fresh, out)
                    };
                    fields.push(MirFieldInit { name, value });
                }
                Rvalue::Construct { class_name, fields }
            }
            None => Rvalue::UnionInject {
                value,
                union_type,
                member_type,
                member_index,
            },
        },
        Rvalue::UnionTagTest {
            place,
            union_type,
            member_index,
        } => match values.class_of(&union_type) {
            Some(_) => {
                let tag = fresh.local(Type::named(TAG_TYPE));
                out.push(Instruction::Assign {
                    target: tag.clone(),
                    value: Rvalue::Member {
                        object: Operand::Place(place),
                        field: TAG_FIELD.to_string(),
                    },
                });
                Rvalue::Binary {
                    op: BinaryOp::Eq,
                    left: Operand::Place(tag),
                    right: Operand::Int(member_index as u128),
                    span: crate::diag::Span::new(0, 0),
                    trap: Default::default(),
                }
            }
            None => Rvalue::UnionTagTest {
                place,
                union_type,
                member_index,
            },
        },
        Rvalue::UnionTakePayload {
            place,
            union_type,
            member_type,
            member_index,
        } => match values.class_of(&union_type) {
            Some(class) => out_of_field(
                values,
                fresh,
                out,
                &class,
                member_field(member_index),
                Operand::Place(place),
            ),
            None => Rvalue::UnionTakePayload {
                place,
                union_type,
                member_type,
                member_index,
            },
        },
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
                        } else if class == TEXT_CLASS {
                            TEXT_RELEASE.to_string()
                        } else if values.dicts.contains_key(&class) {
                            class_function(dicts::DROP_DICT, &class)
                        } else if let Some(element) = values.lists.get(&class).cloned() {
                            match &element {
                                Type::Named(inner, _) if values.holds_a_box(inner) => {
                                    class_function(DROP_LIST, &class)
                                }
                                _ => LIST_RELEASE.to_string(),
                            }
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
    let mut lists = values
        .lists
        .iter()
        .map(|(class, element)| (class.clone(), element.clone()))
        .collect::<Vec<_>>();
    lists.sort_by(|left, right| left.0.cmp(&right.0));
    for (class, element) in lists {
        let Type::Named(inner, _) = &element else {
            continue;
        };
        if !values.holds_a_box(inner) {
            continue;
        }
        let address = || Operand::Place("address".to_string());
        let whole = || Type::named("int64");
        let block = |label: &str, instructions, terminator| BasicBlock {
            label: label.to_string(),
            instructions,
            terminator,
        };
        // The clear: each element is released, and the list is empty.
        let mut fresh = new_fresh();
        let mut entry = Vec::new();
        let length = call(
            &mut fresh,
            &mut entry,
            LIST_LENGTH.to_string(),
            vec![address()],
            whole(),
        );
        let index = fresh.local(whole());
        entry.push(Instruction::Assign {
            target: index.clone(),
            value: Rvalue::Use(Operand::Int(0)),
        });
        let more = fresh.local(Type::named("bool"));
        let test = vec![Instruction::Assign {
            target: more.clone(),
            value: Rvalue::Binary {
                op: BinaryOp::Less,
                left: Operand::Place(index.clone()),
                right: Operand::Place(length),
                span: crate::diag::Span::new(0, 0),
                trap: Default::default(),
            },
        }];
        let mut body = Vec::new();
        let slot = call(
            &mut fresh,
            &mut body,
            LIST_SLOT_UNCHECKED.to_string(),
            vec![address(), Operand::Place(index.clone())],
            word(),
        );
        let held = call(
            &mut fresh,
            &mut body,
            class_function(LOAD, inner),
            vec![Operand::Place(slot)],
            element.clone(),
        );
        call(
            &mut fresh,
            &mut body,
            class_function(RELEASE, inner),
            vec![Operand::Place(held.clone())],
            Type::Unit,
        );
        body.push(Instruction::Release { place: held });
        body.push(Instruction::Assign {
            target: index.clone(),
            value: Rvalue::Binary {
                op: BinaryOp::Add,
                left: Operand::Place(index),
                right: Operand::Int(1),
                span: crate::diag::Span::new(0, 0),
                trap: crate::mir::ArithmeticTrap::Overflow,
            },
        });
        let mut done = Vec::new();
        call(
            &mut fresh,
            &mut done,
            LIST_CLEAR.to_string(),
            vec![address()],
            Type::Unit,
        );
        functions.push(made_function(
            class_function(CLEAR_LIST, &class),
            vec![("address", MirReceiverKind::Borrow, word())],
            Type::Unit,
            fresh.locals,
            vec![
                block("entry", entry, Terminator::Goto("test".to_string())),
                block(
                    "test",
                    test,
                    Terminator::Branch {
                        condition: Operand::Place(more),
                        then_label: "body".to_string(),
                        else_label: "done".to_string(),
                    },
                ),
                block("body", body, Terminator::Goto("test".to_string())),
                block("done", done, Terminator::Return(Operand::Unit)),
            ],
        ));
        // The drop: one reference fewer, and at the last one the list is
        // cleared and freed.
        let mut fresh = new_fresh();
        let mut entry = Vec::new();
        let last = call(
            &mut fresh,
            &mut entry,
            BOX_RELEASE.to_string(),
            vec![address()],
            Type::named("bool"),
        );
        let mut free = Vec::new();
        call(
            &mut fresh,
            &mut free,
            class_function(CLEAR_LIST, &class),
            vec![address()],
            Type::Unit,
        );
        call(
            &mut fresh,
            &mut free,
            LIST_FREE.to_string(),
            vec![address()],
            Type::Unit,
        );
        functions.push(made_function(
            class_function(DROP_LIST, &class),
            vec![("address", MirReceiverKind::Borrow, word())],
            Type::Unit,
            fresh.locals,
            vec![
                block(
                    "entry",
                    entry,
                    Terminator::Branch {
                        condition: Operand::Place(last),
                        then_label: "free".to_string(),
                        else_label: "done".to_string(),
                    },
                ),
                block("free", free, Terminator::Return(Operand::Unit)),
                block("done", Vec::new(), Terminator::Return(Operand::Unit)),
            ],
        ));
    }
    functions
}

/// The scalars of a value of type `ty`, in the order of its words.
fn leaf_kinds(ty: &DirectType, kinds: &mut Vec<ScalarKind>) -> Result<(), String> {
    match ty {
        DirectType::Scalar(kind) => kinds.push(*kind),
        DirectType::PlainClass(class) => {
            for field in &class.fields {
                leaf_kinds(&field.ty, kinds)?;
            }
        }
        // A box holds one word for each scalar; an inline array is not one.
        DirectType::Fixed(_) => {
            return Err("a `fixed` array in a heap block is not supported yet".into());
        }
        DirectType::Union(_) | DirectType::Callable(_) | DirectType::Opaque(_) => {}
    }
    Ok(())
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
        if name == TEXT_LITERAL {
            let [MirArg {
                value: Operand::String(text),
                ..
            }] = args
            else {
                return Ok(None);
            };
            let (bytes, _) = self.string_constant(text.as_bytes())?;
            return Ok(Some(ValueRef {
                values: vec![bytes],
                ty: DirectType::Scalar(ScalarKind::Uint64),
            }));
        }
        let mut kinds = Vec::new();
        if kind.starts_with(STORE) {
            let address = self.load_operand(&args[0].value)?.values[0];
            let value = self.load_operand(&args[1].value)?;
            leaf_kinds(&value.ty, &mut kinds)?;
            for (index, (kind, word)) in kinds.into_iter().zip(&value.values).enumerate() {
                self.store_leaf(box_leaf(index, kind), *word, address);
            }
            return Ok(Some(unit_value(self.builder)));
        }
        if kind.starts_with(LOAD) {
            let address = self.load_operand(&args[0].value)?.values[0];
            leaf_kinds(target, &mut kinds)?;
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

#[path = "native_codegen_freestanding_dicts.rs"]
mod dicts;

#[cfg(test)]
#[path = "native_codegen_freestanding_values_tests.rs"]
mod tests;
