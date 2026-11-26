//! The dictionaries of a freestanding program.
//!
//! A `dict[K, V]` is a class of one word, the address of a header block
//! the library `std/freestanding_dict.au` keeps: a reference count, the
//! entries in the order they were added, and an index of them by hash. A
//! key is an `int64`, a `uint64`, or a `str`. A value is whatever a list
//! may hold. The library finds, adds, and removes entries as words. What
//! depends on the types is in functions the pass makes for each
//! dictionary class: the slot of a key, the write of an entry, the lists
//! of keys, values, and entries, and the clear and the drop.

use super::*;
use crate::runtime_ops::DictOp;

pub(super) const DICT_CLASS_PREFIX: &str = "aura::values::dict::";

/// The source of the dictionary functions, compiled with a program that
/// holds a dictionary.
pub(super) const DICT_SOURCE: &str = include_str!("../std/freestanding_dict.au");
pub(super) const DICT_SOURCE_PATH: &str = "aura/freestanding_dict.au";
pub(super) const DICT_LIBRARY_PREFIX: &str = "value_dict_";

const DICT_NEW: &str = "aura::values::value_dict_new";
const DICT_FREE: &str = "aura::values::value_dict_free";
const DICT_LENGTH: &str = "aura::values::value_dict_length";
const DICT_USED: &str = "aura::values::value_dict_used";
const DICT_WORD: &str = "aura::values::value_dict_word";
const DICT_HASH_WORD: &str = "aura::values::value_dict_hash_word";
const DICT_HASH_TEXT: &str = "aura::values::value_dict_hash_text";
const DICT_FIND: &str = "aura::values::value_dict_find";
const DICT_ADD: &str = "aura::values::value_dict_add";
const DICT_VALUE: &str = "aura::values::value_dict_value";
const DICT_VALUE_OF: &str = "aura::values::value_dict_value_of";
const DICT_ENTRY: &str = "aura::values::value_dict_entry";
const DICT_RESET: &str = "aura::values::value_dict_reset";

/// The functions the pass makes for each dictionary class.
const DICT_SLOT: &str = "dict_slot";
const DICT_SET: &str = "dict_set";
const CLEAR_DICT: &str = "clear_dict";
pub(super) const DROP_DICT: &str = "drop_dict";

/// A list a dictionary gives of itself.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) enum View {
    Keys,
    Values,
    Items,
}

impl View {
    fn function(self) -> &'static str {
        match self {
            View::Keys => "dict_keys",
            View::Values => "dict_values",
            View::Items => "dict_items",
        }
    }
}

/// What the pass knows of one dictionary class.
#[derive(Clone)]
pub(super) struct Dict {
    /// The key and the value types, after the rewrite.
    pub(super) key: Type,
    pub(super) value: Type,
    /// The key and the value types of the program.
    key_type: Type,
    value_type: Type,
}

impl Values {
    /// The class that stands for a dictionary from `key` to `value`, made
    /// at its first use: one field, the address of the dictionary's
    /// header.
    pub(super) fn dict_class(&mut self, key: &Type, value: &Type) -> Option<String> {
        let (key_class, key_suffix) = self.element(key)?;
        let keyed = matches!(
            &key_class,
            Type::Named(name, _) if matches!(name.as_str(), "int64" | "uint64") || name == TEXT_CLASS
        );
        if !keyed {
            return None;
        }
        let (value_class, value_suffix) = self.element(value)?;
        let name = format!("{DICT_CLASS_PREFIX}{key_suffix}::{value_suffix}");
        if !self.dicts.contains_key(&name) {
            self.dicts.insert(
                name.clone(),
                Dict {
                    key: key_class,
                    value: value_class,
                    key_type: key.clone(),
                    value_type: value.clone(),
                },
            );
            let class = MirClass {
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
                    content: Type::Named("dict".to_string(), vec![key.clone(), value.clone()]),
                }],
            );
        }
        Some(name)
    }

    /// Whether a value of type `ty` needs drop.
    fn drops(&self, ty: &Type) -> bool {
        matches!(ty, Type::Named(class, _) if self.classes.get(class).is_some_and(|class| !class.copy))
    }

    /// The class of `ty` when a value of it owns heap memory.
    fn owning(&self, ty: &Type) -> Option<String> {
        match ty {
            Type::Named(class, _) if self.holds_a_box(class) => Some(class.clone()),
            _ => None,
        }
    }
}

fn word() -> Type {
    Type::named("uint64")
}

fn whole() -> Type {
    Type::named("int64")
}

fn place(name: &str) -> Operand {
    Operand::Place(name.to_string())
}

/// `name` as the operand that hands the value on: moved when it needs
/// drop, and read otherwise.
fn handed(values: &Values, ty: &Type, name: &str) -> Operand {
    if values.drops(ty) {
        Operand::MovePlace(name.to_string())
    } else {
        place(name)
    }
}

fn block(label: &str, instructions: Vec<Instruction>, terminator: Terminator) -> BasicBlock {
    BasicBlock {
        label: label.to_string(),
        instructions,
        terminator,
    }
}

fn compare(op: BinaryOp, left: Operand, right: Operand) -> Rvalue {
    Rvalue::Binary {
        op,
        left,
        right,
        span: crate::diag::Span::new(0, 0),
        trap: Default::default(),
    }
}

/// The hash of the key in `key`, and the two words the library compares
/// a key by: the key's bits and 0, or the address of a text's bytes and
/// their length.
fn key_words(
    dict: &Dict,
    key: &str,
    fresh: &mut Fresh,
    out: &mut Vec<Instruction>,
) -> (Operand, Operand, Operand) {
    match &dict.key {
        Type::Named(name, _) if name == TEXT_CLASS => {
            let address = fresh.local(word());
            out.push(Instruction::Assign {
                target: address.clone(),
                value: Rvalue::Member {
                    object: place(key),
                    field: TEXT_FIELD.to_string(),
                },
            });
            let bytes = call(
                fresh,
                out,
                TEXT_BYTES.to_string(),
                vec![place(&address)],
                word(),
            );
            let length = call(
                fresh,
                out,
                TEXT_BYTE_LENGTH.to_string(),
                vec![place(&address)],
                word(),
            );
            let hash = call(
                fresh,
                out,
                DICT_HASH_TEXT.to_string(),
                vec![place(&bytes), place(&length)],
                word(),
            );
            (place(&hash), place(&bytes), place(&length))
        }
        Type::Named(name, _) if name == "int64" => {
            let bits = call(fresh, out, DICT_WORD.to_string(), vec![place(key)], word());
            let hash = call(
                fresh,
                out,
                DICT_HASH_WORD.to_string(),
                vec![place(&bits)],
                word(),
            );
            (place(&hash), place(&bits), Operand::Int(0))
        }
        _ => {
            let hash = call(
                fresh,
                out,
                DICT_HASH_WORD.to_string(),
                vec![place(key)],
                word(),
            );
            (place(&hash), place(key), Operand::Int(0))
        }
    }
}

fn new_fresh() -> Fresh {
    Fresh {
        count: 0,
        locals: Vec::new(),
    }
}

/// The functions of each dictionary class of the program.
pub(super) fn dict_functions(values: &mut Values) -> Vec<MirFunction> {
    let mut functions = Vec::new();
    let mut dicts = values
        .dicts
        .iter()
        .map(|(class, dict)| (class.clone(), dict.clone()))
        .collect::<Vec<_>>();
    dicts.sort_by(|left, right| left.0.cmp(&right.0));
    for (class, dict) in dicts {
        let text_key = dict.key == Type::named(TEXT_CLASS);
        let key_name = dict.key.to_string();
        let value_name = dict.value.to_string();
        let value_owner = values.owning(&dict.value);
        let key_passing = if values.drops(&dict.key) {
            MirReceiverKind::Value
        } else {
            MirReceiverKind::Borrow
        };
        let value_passing = if values.drops(&dict.value) {
            MirReceiverKind::Value
        } else {
            MirReceiverKind::Borrow
        };

        // The slot of a key, or 0 when the dictionary holds no such key.
        let mut fresh = new_fresh();
        let mut entry = Vec::new();
        let (hash, first, second) = key_words(&dict, "key", &mut fresh, &mut entry);
        let found = call(
            &mut fresh,
            &mut entry,
            DICT_FIND.to_string(),
            vec![place("address"), hash, first, second],
            word(),
        );
        functions.push(made_function(
            class_function(DICT_SLOT, &class),
            vec![
                ("address", MirReceiverKind::Borrow, word()),
                ("key", MirReceiverKind::Borrow, dict.key.clone()),
            ],
            word(),
            fresh.locals,
            one_block(entry, Terminator::Return(place(&found))),
        ));

        // The write of an entry: over the value of the key's entry, or as
        // a new entry behind the others.
        let mut fresh = new_fresh();
        let mut entry = Vec::new();
        let (hash, first, second) = key_words(&dict, "key", &mut fresh, &mut entry);
        let found = call(
            &mut fresh,
            &mut entry,
            DICT_FIND.to_string(),
            vec![place("address"), hash.clone(), first.clone(), second],
            word(),
        );
        let held = fresh.local(Type::named("bool"));
        entry.push(Instruction::Assign {
            target: held.clone(),
            value: compare(BinaryOp::NotEq, place(&found), Operand::Int(0)),
        });
        let mut old = Vec::new();
        let slot = call(
            &mut fresh,
            &mut old,
            DICT_VALUE.to_string(),
            vec![place("address"), place(&found)],
            word(),
        );
        if let Some(owner) = &value_owner {
            let before = call(
                &mut fresh,
                &mut old,
                class_function(LOAD, &value_name),
                vec![place(&slot)],
                dict.value.clone(),
            );
            call(
                &mut fresh,
                &mut old,
                class_function(RELEASE, owner),
                vec![place(&before)],
                Type::Unit,
            );
            old.push(Instruction::Release { place: before });
        }
        call(
            &mut fresh,
            &mut old,
            class_function(STORE, &value_name),
            vec![place(&slot), handed(values, &dict.value, "value")],
            Type::Unit,
        );
        if text_key {
            // The entry keeps the key it has.
            call(
                &mut fresh,
                &mut old,
                class_function(RELEASE, TEXT_CLASS),
                vec![place("key")],
                Type::Unit,
            );
            old.push(Instruction::Release {
                place: "key".to_string(),
            });
        }
        let mut new = Vec::new();
        let added = call(
            &mut fresh,
            &mut new,
            DICT_ADD.to_string(),
            vec![place("address"), hash, first],
            word(),
        );
        if text_key {
            call(
                &mut fresh,
                &mut new,
                class_function(STORE, &key_name),
                vec![place(&added), Operand::MovePlace("key".to_string())],
                Type::Unit,
            );
        }
        let slot = call(
            &mut fresh,
            &mut new,
            DICT_VALUE.to_string(),
            vec![place("address"), place(&added)],
            word(),
        );
        call(
            &mut fresh,
            &mut new,
            class_function(STORE, &value_name),
            vec![place(&slot), handed(values, &dict.value, "value")],
            Type::Unit,
        );
        functions.push(made_function(
            class_function(DICT_SET, &class),
            vec![
                ("address", MirReceiverKind::Borrow, word()),
                ("key", key_passing, dict.key.clone()),
                ("value", value_passing, dict.value.clone()),
            ],
            Type::Unit,
            fresh.locals,
            vec![
                block(
                    "entry",
                    entry,
                    Terminator::Branch {
                        condition: place(&held),
                        then_label: "old".to_string(),
                        else_label: "new".to_string(),
                    },
                ),
                block("old", old, Terminator::Return(Operand::Unit)),
                block("new", new, Terminator::Return(Operand::Unit)),
            ],
        ));

        // The clear: what each entry owns is released, and the
        // dictionary is empty.
        let mut fresh = new_fresh();
        let at = fresh.local(word());
        let mut take = Vec::new();
        if text_key {
            let key = call(
                &mut fresh,
                &mut take,
                class_function(LOAD, &key_name),
                vec![place(&at)],
                dict.key.clone(),
            );
            call(
                &mut fresh,
                &mut take,
                class_function(RELEASE, TEXT_CLASS),
                vec![place(&key)],
                Type::Unit,
            );
            take.push(Instruction::Release { place: key });
        }
        if let Some(owner) = &value_owner {
            let slot = call(
                &mut fresh,
                &mut take,
                DICT_VALUE.to_string(),
                vec![place("address"), place(&at)],
                word(),
            );
            let value = call(
                &mut fresh,
                &mut take,
                class_function(LOAD, &value_name),
                vec![place(&slot)],
                dict.value.clone(),
            );
            call(
                &mut fresh,
                &mut take,
                class_function(RELEASE, owner),
                vec![place(&value)],
                Type::Unit,
            );
            take.push(Instruction::Release { place: value });
        }
        let mut done = Vec::new();
        call(
            &mut fresh,
            &mut done,
            DICT_RESET.to_string(),
            vec![place("address")],
            Type::Unit,
        );
        functions.push(over_entries(
            class_function(CLEAR_DICT, &class),
            Type::Unit,
            fresh,
            at,
            [Vec::new(), take, done],
            Operand::Unit,
        ));

        // The drop: one reference fewer, and at the last one the
        // dictionary is cleared and freed.
        let mut fresh = new_fresh();
        let mut entry = Vec::new();
        let last = call(
            &mut fresh,
            &mut entry,
            BOX_RELEASE.to_string(),
            vec![place("address")],
            Type::named("bool"),
        );
        let mut free = Vec::new();
        call(
            &mut fresh,
            &mut free,
            class_function(CLEAR_DICT, &class),
            vec![place("address")],
            Type::Unit,
        );
        call(
            &mut fresh,
            &mut free,
            DICT_FREE.to_string(),
            vec![place("address")],
            Type::Unit,
        );
        functions.push(made_function(
            class_function(DROP_DICT, &class),
            vec![("address", MirReceiverKind::Borrow, word())],
            Type::Unit,
            fresh.locals,
            vec![
                block(
                    "entry",
                    entry,
                    Terminator::Branch {
                        condition: place(&last),
                        then_label: "free".to_string(),
                        else_label: "done".to_string(),
                    },
                ),
                block("free", free, Terminator::Return(Operand::Unit)),
                block("done", Vec::new(), Terminator::Return(Operand::Unit)),
            ],
        ));

        // The lists of keys, of values, and of entries the program asks
        // for.
        let mut views = values
            .views
            .iter()
            .filter(|(of, _)| *of == class)
            .map(|(_, view)| *view)
            .collect::<Vec<_>>();
        views.sort();
        for view in views {
            let element_type = match view {
                View::Keys => dict.key_type.clone(),
                View::Values => dict.value_type.clone(),
                View::Items => Type::Tuple(vec![dict.key_type.clone(), dict.value_type.clone()]),
            };
            let Some(list_class) = values.list_class(&element_type) else {
                continue;
            };
            let element = values.lists[&list_class].clone();
            let mut fresh = new_fresh();
            let at = fresh.local(word());
            let mut start = Vec::new();
            let list = call(
                &mut fresh,
                &mut start,
                LIST_NEW.to_string(),
                vec![Operand::Int(0), Operand::Int(values.words(&element))],
                word(),
            );
            let mut take = Vec::new();
            let mut read = |of: &Type, slot: Operand, values: &mut Values, fresh: &mut Fresh| {
                let value = call(
                    fresh,
                    &mut take,
                    class_function(LOAD, &of.to_string()),
                    vec![slot],
                    of.clone(),
                );
                if let Some(owner) = values.owning(of) {
                    call(
                        fresh,
                        &mut take,
                        class_function(RETAIN, &owner),
                        vec![place(&value)],
                        Type::Unit,
                    );
                }
                value
            };
            let key =
                (view != View::Values).then(|| read(&dict.key, place(&at), values, &mut fresh));
            let value = (view != View::Keys).then(|| {
                let slot = fresh.local(word());
                (slot, ())
            });
            // The value's slot is asked for before the value is read.
            let value = match value {
                Some((slot, ())) => {
                    take.push(Instruction::Assign {
                        target: slot.clone(),
                        value: Rvalue::Call {
                            callee: CallTarget::Name(DICT_VALUE.to_string()),
                            args: vec![place("address"), place(&at)]
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
                    let value = call(
                        &mut fresh,
                        &mut take,
                        class_function(LOAD, &value_name),
                        vec![place(&slot)],
                        dict.value.clone(),
                    );
                    if let Some(owner) = &value_owner {
                        call(
                            &mut fresh,
                            &mut take,
                            class_function(RETAIN, owner),
                            vec![place(&value)],
                            Type::Unit,
                        );
                    }
                    Some(value)
                }
                None => None,
            };
            let item = match (key, value) {
                (Some(key), Some(value)) => {
                    let Type::Named(tuple, _) = &element else {
                        continue;
                    };
                    let both = fresh.local(element.clone());
                    take.push(Instruction::Assign {
                        target: both.clone(),
                        value: Rvalue::Construct {
                            class_name: tuple.clone(),
                            fields: vec![
                                MirFieldInit {
                                    name: element_field(0),
                                    value: handed(values, &dict.key, &key),
                                },
                                MirFieldInit {
                                    name: element_field(1),
                                    value: handed(values, &dict.value, &value),
                                },
                            ],
                        },
                    });
                    both
                }
                (Some(one), None) | (None, Some(one)) => one,
                (None, None) => continue,
            };
            let slot = call(
                &mut fresh,
                &mut take,
                LIST_PUSH_SLOT.to_string(),
                vec![place(&list)],
                word(),
            );
            call(
                &mut fresh,
                &mut take,
                class_function(STORE, &element.to_string()),
                vec![place(&slot), handed(values, &element, &item)],
                Type::Unit,
            );
            let mut done = Vec::new();
            let result = fresh.local(Type::named(&list_class));
            done.push(Instruction::Assign {
                target: result.clone(),
                value: Rvalue::Construct {
                    class_name: list_class.clone(),
                    fields: vec![MirFieldInit {
                        name: TEXT_FIELD.to_string(),
                        value: place(&list),
                    }],
                },
            });
            functions.push(over_entries(
                class_function(view.function(), &class),
                Type::named(&list_class),
                fresh,
                at,
                [start, take, done],
                Operand::MovePlace(result),
            ));
        }
    }
    functions
}

/// A function of a dictionary's `address` that runs the first of the
/// three lists of instructions (`start`), then the second (`take`)
/// for each entry the dictionary holds, with the slot of its key in
/// `at`, then `done`, and returns `result`.
fn over_entries(
    name: String,
    returns: Type,
    mut fresh: Fresh,
    at: String,
    [mut start, take, done]: [Vec<Instruction>; 3],
    result: Operand,
) -> MirFunction {
    let used = call(
        &mut fresh,
        &mut start,
        DICT_USED.to_string(),
        vec![place("address")],
        whole(),
    );
    let number = fresh.local(whole());
    start.push(Instruction::Assign {
        target: number.clone(),
        value: Rvalue::Use(Operand::Int(0)),
    });
    let more = fresh.local(Type::named("bool"));
    let test = vec![Instruction::Assign {
        target: more.clone(),
        value: compare(BinaryOp::Less, place(&number), place(&used)),
    }];
    let mut body = vec![Instruction::Assign {
        target: at.clone(),
        value: Rvalue::Call {
            callee: CallTarget::Name(DICT_ENTRY.to_string()),
            args: vec![place("address"), place(&number)]
                .into_iter()
                .map(|value| MirArg {
                    name: None,
                    value,
                    writeback_place: None,
                })
                .collect(),
            type_args: Default::default(),
        },
    }];
    let live = fresh.local(Type::named("bool"));
    body.push(Instruction::Assign {
        target: live.clone(),
        value: compare(BinaryOp::NotEq, place(&at), Operand::Int(0)),
    });
    let next = vec![Instruction::Assign {
        target: number.clone(),
        value: Rvalue::Binary {
            op: BinaryOp::Add,
            left: place(&number),
            right: Operand::Int(1),
            span: crate::diag::Span::new(0, 0),
            trap: crate::mir::ArithmeticTrap::Overflow,
        },
    }];
    made_function(
        name,
        vec![("address", MirReceiverKind::Borrow, word())],
        returns,
        fresh.locals,
        vec![
            block("entry", start, Terminator::Goto("test".to_string())),
            block(
                "test",
                test,
                Terminator::Branch {
                    condition: place(&more),
                    then_label: "body".to_string(),
                    else_label: "done".to_string(),
                },
            ),
            block(
                "body",
                body,
                Terminator::Branch {
                    condition: place(&live),
                    then_label: "take".to_string(),
                    else_label: "next".to_string(),
                },
            ),
            block("take", take, Terminator::Goto("next".to_string())),
            block("next", next, Terminator::Goto("test".to_string())),
            block("done", done, Terminator::Return(result)),
        ],
    )
}

/// `operand` as a key or a value a made function reads and leaves with
/// the caller: a literal becomes a text the caller releases in `after`.
fn lent(
    operand: Operand,
    values: &mut Values,
    fresh: &mut Fresh,
    out: &mut Vec<Instruction>,
    after: &mut Vec<Instruction>,
) -> Operand {
    match owned_text(operand, values, fresh, out) {
        Operand::MovePlace(local) if local.starts_with("%t") && fresh.made(&local) => {
            call(
                fresh,
                after,
                class_function(RELEASE, TEXT_CLASS),
                vec![place(&local)],
                Type::Unit,
            );
            after.push(Instruction::Release {
                place: local.clone(),
            });
            Operand::Place(local)
        }
        Operand::MovePlace(local) => Operand::Place(local),
        other => other,
    }
}

/// The slot of the value of `key` in the dictionary of class `class` at
/// `address`. A key the dictionary does not hold is a failed assertion.
pub(super) fn value_slot(
    class: &str,
    address: &str,
    key: Operand,
    values: &mut Values,
    fresh: &mut Fresh,
    out: &mut Vec<Instruction>,
    after: &mut Vec<Instruction>,
) -> String {
    let key = lent(key, values, fresh, out, after);
    let entry = call(
        fresh,
        out,
        class_function(DICT_SLOT, class),
        vec![place(address), key],
        word(),
    );
    call(
        fresh,
        out,
        DICT_VALUE_OF.to_string(),
        vec![place(address), place(&entry)],
        word(),
    )
}

/// Rewrites the dictionary literals and operations of `value` to calls of
/// the dictionary library and of the functions of the dictionary's class.
pub(super) fn lower_dict(
    value: Rvalue,
    types: &HashMap<String, Type>,
    values: &mut Values,
    fresh: &mut Fresh,
    out: &mut Vec<Instruction>,
    after: &mut Vec<Instruction>,
) -> Rvalue {
    let named = |function: String, args: Vec<Operand>| Rvalue::Call {
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
    };
    match value {
        Rvalue::MapLiteral {
            entries,
            key_type,
            value_type,
        } => {
            let Some(class_name) = values.dict_class(&key_type, &value_type) else {
                return Rvalue::MapLiteral {
                    entries,
                    key_type,
                    value_type,
                };
            };
            let dict = values.dicts[&class_name].clone();
            let kind = u128::from(dict.key == Type::named(TEXT_CLASS));
            let address = call(
                fresh,
                out,
                DICT_NEW.to_string(),
                vec![
                    Operand::Int(values.words(&dict.key)),
                    Operand::Int(values.words(&dict.value)),
                    Operand::Int(kind),
                ],
                word(),
            );
            for entry in entries {
                let key = owned_text(entry.key, values, fresh, out);
                let value = owned_text(entry.value, values, fresh, out);
                call(
                    fresh,
                    out,
                    class_function(DICT_SET, &class_name),
                    vec![place(&address), key, value],
                    Type::Unit,
                );
            }
            Rvalue::Construct {
                class_name,
                fields: vec![MirFieldInit {
                    name: TEXT_FIELD.to_string(),
                    value: place(&address),
                }],
            }
        }
        Rvalue::Call {
            callee:
                CallTarget::Runtime {
                    op: RuntimeOp::Dict(op),
                    receiver: Some(Operand::Place(receiver)),
                    receiver_place,
                },
            args,
            type_args,
        } => {
            let class = match place_type(&receiver, types, values) {
                Some(Type::Named(class, _)) if values.dicts.contains_key(&class) => Some(class),
                _ => None,
            };
            let known = matches!(
                (op, args.len()),
                (
                    DictOp::Len
                        | DictOp::IsEmpty
                        | DictOp::Clear
                        | DictOp::Keys
                        | DictOp::Values
                        | DictOp::Items,
                    0
                ) | (DictOp::ContainsKey | DictOp::ContainsKeyInt64, 1)
                    | (DictOp::Index, 3)
                    | (DictOp::SetIndex, 4)
            );
            let (Some(class), true) = (class, known) else {
                return Rvalue::Call {
                    callee: CallTarget::Runtime {
                        op: RuntimeOp::Dict(op),
                        receiver: Some(Operand::Place(receiver)),
                        receiver_place,
                    },
                    args,
                    type_args,
                };
            };
            let dict = values.dicts[&class].clone();
            let address = fresh.local(word());
            out.push(Instruction::Assign {
                target: address.clone(),
                value: Rvalue::Member {
                    object: Operand::Place(receiver),
                    field: TEXT_FIELD.to_string(),
                },
            });
            let mut args = args.into_iter().map(|arg| arg.value);
            let mut next = || args.next().unwrap_or(Operand::Unit);
            match op {
                DictOp::Len => named(DICT_LENGTH.to_string(), vec![place(&address)]),
                DictOp::IsEmpty => {
                    let length = call(
                        fresh,
                        out,
                        DICT_LENGTH.to_string(),
                        vec![place(&address)],
                        whole(),
                    );
                    compare(BinaryOp::Eq, place(&length), Operand::Int(0))
                }
                DictOp::Clear => named(class_function(CLEAR_DICT, &class), vec![place(&address)]),
                DictOp::Keys | DictOp::Values | DictOp::Items => {
                    let view = match op {
                        DictOp::Keys => View::Keys,
                        DictOp::Values => View::Values,
                        _ => View::Items,
                    };
                    values.views.insert((class.clone(), view));
                    // The list's class is made while functions are still
                    // being rewritten.
                    let element = match view {
                        View::Keys => dict.key_type.clone(),
                        View::Values => dict.value_type.clone(),
                        View::Items => {
                            Type::Tuple(vec![dict.key_type.clone(), dict.value_type.clone()])
                        }
                    };
                    values.list_class(&element);
                    named(
                        class_function(view.function(), &class),
                        vec![place(&address)],
                    )
                }
                DictOp::SetIndex => {
                    let key = owned_text(next(), values, fresh, out);
                    let value = owned_text(next(), values, fresh, out);
                    named(
                        class_function(DICT_SET, &class),
                        vec![place(&address), key, value],
                    )
                }
                DictOp::Index => {
                    let slot = value_slot(&class, &address, next(), values, fresh, out, after);
                    named(
                        class_function(LOAD, &dict.value.to_string()),
                        vec![place(&slot)],
                    )
                }
                _ => {
                    let key = lent(next(), values, fresh, out, after);
                    let entry = call(
                        fresh,
                        out,
                        class_function(DICT_SLOT, &class),
                        vec![place(&address), key],
                        word(),
                    );
                    compare(BinaryOp::NotEq, place(&entry), Operand::Int(0))
                }
            }
        }
        other => other,
    }
}
