//! Finds the `json.Value` literals a function builds whole, which are
//! written into one tape instead of a tree of boxes.
//!
//! `json.Value.Object({"id": json.Value.Int(i), "tags": json.Value.Array([
//! json.Value.String(tag)])})` lowers to one variant per value, each in a
//! temporary that the dict literal, the list literal, or the variant
//! around it then takes. Built that way the document is a box per value,
//! a dict, and a list. A parsed document is one tape (see
//! `aura_runtime::json_tape`), which every read of a `json.Value` already
//! takes, so a literal whose every value is spelled in place is built as a
//! tape too: the instructions that make its parts emit nothing, and the
//! one that makes its root hands the parts' payloads to the runtime's tape
//! builder in document order.
//!
//! A part is deferred this way only when its temporary is assigned once,
//! in the root's block and before it, and named once, by the literal or
//! variant that takes it. Its payload is read when the root is built, so a
//! scalar payload must be a constant or a local nothing assigns between
//! the part and the root, and a `str` payload a constant or a temporary
//! the part takes whole.

use std::collections::{HashMap, HashSet};

use crate::mir::ownership::{instruction_accesses, Access};
use crate::mir::{instruction_place_mentions, terminator_place_mentions, BasicBlock};

use super::*;

/// The deepest literal that is built as a tape.
const MAX_DEPTH: usize = 32;

/// A value of a `json.Value` literal, with the operand of its payload.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum NestValue {
    Null,
    Bool(Operand),
    Int(Operand),
    Float(Operand),
    String(Operand),
    Array(Vec<NestValue>),
    Object(Vec<(String, NestValue)>),
}

impl NestValue {
    /// The values of the literal that are no array or object.
    pub(super) fn leaves(&self) -> Vec<&NestValue> {
        match self {
            Self::Array(elements) => elements.iter().flat_map(Self::leaves).collect(),
            Self::Object(entries) => entries
                .iter()
                .flat_map(|(_, value)| value.leaves())
                .collect(),
            leaf => vec![leaf],
        }
    }
}

/// The literals of a function that are built as tapes.
#[derive(Debug, Default)]
pub(super) struct JsonNests {
    /// The literal each root assignment builds, by block label and
    /// instruction index.
    pub(super) roots: HashMap<String, HashMap<usize, NestValue>>,
    /// The assignments of the roots' parts, which emit nothing, by block
    /// label.
    pub(super) deferred: HashMap<String, HashSet<usize>>,
}

/// How often each local of a function is assigned and named.
struct Uses<'f> {
    /// The `Assign` instructions that target each local.
    plain: HashMap<&'f str, usize>,
    /// The writes of each local, by any instruction.
    assigned: HashMap<&'f str, usize>,
    /// How often each local is named, besides as the target of an
    /// assignment.
    mentions: HashMap<&'f str, usize>,
    /// The parameters, which hold a value from the start.
    params: HashSet<&'f str>,
}

/// One block, with where it assigns each local.
struct Scope<'f> {
    block: &'f BasicBlock,
    defs: HashMap<&'f str, usize>,
    uses: &'f Uses<'f>,
}

fn root(place: &str) -> &str {
    place.split('.').next().unwrap_or(place)
}

fn is_json_value(ty: &aura_core::types::Type) -> bool {
    matches!(ty, aura_core::types::Type::Named(name, args) if name == "json.Value" && args.is_empty())
}

impl<'f> Scope<'f> {
    /// Where the block assigns `local`, when that is before `before`, the
    /// one write of `local`, and one instruction names it.
    fn part(&self, local: &str, before: usize) -> Option<usize> {
        let at = *self.defs.get(local)?;
        (at < before
            && self.uses.plain.get(local) == Some(&1)
            && self.uses.assigned.get(local) == Some(&1)
            && self.uses.mentions.get(local) == Some(&1))
        .then_some(at)
    }

    /// Whether `local` holds at the root what it holds at `at`: it is a
    /// parameter nothing writes, or its one write is an assignment that is
    /// not in this block after `at`.
    fn stable(&self, local: &str, at: usize) -> bool {
        match self.uses.assigned.get(local) {
            None => self.uses.params.contains(local),
            Some(1) => {
                self.uses.plain.get(local) == Some(&1)
                    && self.defs.get(local).is_none_or(|def| *def < at)
            }
            Some(_) => false,
        }
    }

    /// Whether the scalar `payload` of the part at `at` reads the same
    /// when the root is built.
    fn scalar(&self, payload: &Operand, at: usize) -> bool {
        match payload {
            Operand::Int(_) | Operand::Float(_) | Operand::Bool(_) => true,
            Operand::Place(local) => self.stable(local, at),
            _ => false,
        }
    }

    /// The value the operand `taken` hands to the literal or variant at
    /// `before`, with the assignments of its parts added to `deferred`.
    fn value(
        &self,
        taken: &Operand,
        before: usize,
        depth: usize,
        deferred: &mut Vec<usize>,
    ) -> Option<NestValue> {
        let Operand::MovePlace(temp) = taken else {
            return None;
        };
        let at = self.part(temp, before)?;
        let Instruction::Assign {
            value:
                Rvalue::EnumVariant {
                    enum_name,
                    variant_name,
                    payloads,
                },
            ..
        } = &self.block.instructions[at]
        else {
            return None;
        };
        if enum_name != "json.Value" {
            return None;
        }
        let value = self.variant(variant_name, payloads, at, depth, deferred)?;
        deferred.push(at);
        Some(value)
    }

    /// The value variant `variant_name` of `payloads` at `at` makes.
    fn variant(
        &self,
        variant_name: &str,
        payloads: &[Operand],
        at: usize,
        depth: usize,
        deferred: &mut Vec<usize>,
    ) -> Option<NestValue> {
        match (variant_name, payloads) {
            ("Null", []) => Some(NestValue::Null),
            ("Bool", [payload]) if self.scalar(payload, at) => {
                Some(NestValue::Bool(payload.clone()))
            }
            ("Int", [payload]) if self.scalar(payload, at) => Some(NestValue::Int(payload.clone())),
            ("Float", [payload]) if self.scalar(payload, at) => {
                Some(NestValue::Float(payload.clone()))
            }
            ("String", [payload @ Operand::String(_)]) => Some(NestValue::String(payload.clone())),
            ("String", [payload @ Operand::MovePlace(text)])
                if self.stable(text, at) && self.uses.mentions.get(text.as_str()) == Some(&1) =>
            {
                Some(NestValue::String(payload.clone()))
            }
            ("Array", [Operand::MovePlace(list)]) if depth < MAX_DEPTH => {
                let list_at = self.part(list, at)?;
                let Instruction::Assign {
                    value:
                        Rvalue::VecLiteral {
                            elements,
                            element_type,
                        },
                    ..
                } = &self.block.instructions[list_at]
                else {
                    return None;
                };
                if !is_json_value(element_type) {
                    return None;
                }
                let elements = elements
                    .iter()
                    .map(|element| self.value(element, list_at, depth + 1, deferred))
                    .collect::<Option<Vec<_>>>()?;
                deferred.push(list_at);
                Some(NestValue::Array(elements))
            }
            ("Object", [Operand::MovePlace(map)]) if depth < MAX_DEPTH => {
                let map_at = self.part(map, at)?;
                let Instruction::Assign {
                    value:
                        Rvalue::MapLiteral {
                            entries,
                            value_type,
                            ..
                        },
                    ..
                } = &self.block.instructions[map_at]
                else {
                    return None;
                };
                if !is_json_value(value_type) {
                    return None;
                }
                let mut keys = HashSet::new();
                let mut values = Vec::with_capacity(entries.len());
                for entry in entries {
                    // A key spelled twice keeps its last value, which only
                    // a dict works out.
                    let Operand::String(key) = &entry.key else {
                        return None;
                    };
                    if !keys.insert(key.as_str()) {
                        return None;
                    }
                    let value = self.value(&entry.value, map_at, depth + 1, deferred)?;
                    values.push((key.clone(), value));
                }
                deferred.push(map_at);
                Some(NestValue::Object(values))
            }
            _ => None,
        }
    }
}

/// The `json.Value` literals of `function` that are built as tapes: each
/// assignment of an `Array` or `Object` variant whose parts are all
/// spelled in place (see the module's description).
///
/// `accepts` says whether generated code can build the literal a root
/// assigns to its target as a tape.
pub(super) fn json_literal_nests(
    function: &MirFunction,
    accepts: impl Fn(&str, &NestValue) -> bool,
) -> JsonNests {
    let mut uses = Uses {
        plain: HashMap::new(),
        assigned: HashMap::new(),
        mentions: HashMap::new(),
        params: function
            .params
            .iter()
            .map(|param| param.name.as_str())
            .collect(),
    };
    for block in &function.blocks {
        let mut places = Vec::new();
        for instruction in &block.instructions {
            instruction_place_mentions(instruction, &mut places);
            if let Instruction::Assign { target, .. } = instruction {
                *uses.plain.entry(root(target)).or_default() += 1;
            }
            for (place, access) in instruction_accesses(instruction) {
                if matches!(access, Access::Assign | Access::Writeback) {
                    *uses.assigned.entry(root(place)).or_default() += 1;
                }
            }
            // What a local lends may be written wherever the loan reaches.
            if let Instruction::BeginLoan { source: lent, .. }
            | Instruction::BeginElementLoan { source: lent, .. }
            | Instruction::Lend { place: lent, .. } = instruction
            {
                *uses.assigned.entry(root(lent)).or_default() += 2;
            }
        }
        places.extend(terminator_place_mentions(&block.terminator));
        for place in places {
            *uses.mentions.entry(root(place)).or_default() += 1;
        }
    }
    let mut nests = JsonNests::default();
    for block in &function.blocks {
        let mut defs = HashMap::new();
        for (index, instruction) in block.instructions.iter().enumerate() {
            if let Instruction::Assign { target, .. } = instruction {
                if !target.contains('.') {
                    defs.insert(target.as_str(), index);
                }
            }
        }
        let scope = Scope {
            block,
            defs,
            uses: &uses,
        };
        let mut roots = HashMap::new();
        let mut parts = Vec::<(usize, Vec<usize>)>::new();
        for (index, instruction) in block.instructions.iter().enumerate() {
            let Instruction::Assign {
                target,
                value:
                    Rvalue::EnumVariant {
                        enum_name,
                        variant_name,
                        payloads,
                    },
            } = instruction
            else {
                continue;
            };
            if enum_name != "json.Value" || !matches!(variant_name.as_str(), "Array" | "Object") {
                continue;
            }
            let mut deferred = Vec::new();
            let nest = scope
                .variant(variant_name, payloads, index, 0, &mut deferred)
                .filter(|nest| accepts(target, nest));
            if let Some(nest) = nest {
                roots.insert(index, nest);
                parts.push((index, deferred));
            }
        }
        // A literal inside another is one of that one's parts.
        let inner = parts
            .iter()
            .flat_map(|(_, deferred)| deferred.iter().copied())
            .collect::<HashSet<_>>();
        roots.retain(|index, _| !inner.contains(index));
        if roots.is_empty() {
            continue;
        }
        let deferred = parts
            .into_iter()
            .filter(|(index, _)| roots.contains_key(index))
            .flat_map(|(_, deferred)| deferred)
            .collect();
        nests.roots.insert(block.label.clone(), roots);
        nests.deferred.insert(block.label.clone(), deferred);
    }
    nests
}

impl FunctionCompiler<'_> {
    /// Builds the `json.Value` literal `nest` as one tape.
    pub(super) fn compile_json_nest(
        &mut self,
        nest: &NestValue,
    ) -> std::result::Result<ValueRef, String> {
        self.append_json_nest_value(None, nest, true)?;
        let built = self
            .builder
            .call(self.runtime[RuntimeFn::JsonTapeFinish], &[]);
        Ok(self.owned_opaque_result(built.results, Type::named("json.Value")))
    }

    /// Opens an array (`object` 0) or an object (1) of `count` elements or
    /// entries in the tape.
    fn open_json_nest(
        &mut self,
        key_ptr: Val,
        key_len: Val,
        object: i64,
        count: usize,
        root: bool,
    ) {
        let kind = self
            .builder
            .iconst(types::I64, object | (i64::from(root) << 1));
        let count = self.builder.iconst(types::I64, count as i64);
        self.builder.call(
            self.runtime[RuntimeFn::JsonTapeOpen],
            &[key_ptr, key_len, kind, count],
        );
    }

    /// Hands `value`, under `key` when it is an object's entry, to the
    /// runtime's tape builder. `root` marks the literal's outermost value,
    /// which starts the tape.
    fn append_json_nest_value(
        &mut self,
        key: Option<&str>,
        value: &NestValue,
        root: bool,
    ) -> std::result::Result<(), String> {
        let (key_ptr, key_len) = match key {
            Some(key) => self.string_constant(key.as_bytes())?,
            None => (
                self.builder.iconst(types::I64, 0),
                self.builder.iconst(types::I64, -1),
            ),
        };
        let (kind, payload) = match value {
            NestValue::Null => {
                self.builder
                    .call(self.runtime[RuntimeFn::JsonTapeNull], &[key_ptr, key_len]);
                return Ok(());
            }
            NestValue::Bool(payload) => (ScalarKind::Bool, payload),
            NestValue::Int(payload) => (ScalarKind::Int64, payload),
            NestValue::Float(payload) => (ScalarKind::Float64, payload),
            NestValue::String(Operand::String(text)) => {
                let (text_ptr, text_len) = self.string_constant(text.as_bytes())?;
                self.builder.call(
                    self.runtime[RuntimeFn::JsonTapeText],
                    &[key_ptr, key_len, text_ptr, text_len],
                );
                return Ok(());
            }
            NestValue::String(payload) => {
                let text_ty = DirectType::Opaque(Type::named("str"));
                let text = self.load_operand_as_opaque_direct(payload, &text_ty)?;
                let text = self.transfer_owned_opaque_value(&text);
                self.builder.call(
                    self.runtime[RuntimeFn::JsonTapeString],
                    &[key_ptr, key_len, text],
                );
                return Ok(());
            }
            NestValue::Array(elements) => {
                self.open_json_nest(key_ptr, key_len, 0, elements.len(), root);
                for element in elements {
                    self.append_json_nest_value(None, element, false)?;
                }
                self.builder
                    .call(self.runtime[RuntimeFn::JsonTapeClose], &[]);
                return Ok(());
            }
            NestValue::Object(entries) => {
                self.open_json_nest(key_ptr, key_len, 1, entries.len(), root);
                for (key, value) in entries {
                    self.append_json_nest_value(Some(key), value, false)?;
                }
                self.builder
                    .call(self.runtime[RuntimeFn::JsonTapeClose], &[]);
                return Ok(());
            }
        };
        let scalar_ty = DirectType::Scalar(kind);
        let loaded = self.load_operand_for_target(payload, &scalar_ty)?;
        let loaded = self.coerce_value(loaded, &scalar_ty)?;
        let append = match kind {
            ScalarKind::Bool => RuntimeFn::JsonTapeBool,
            ScalarKind::Int64 => RuntimeFn::JsonTapeInt,
            _ => RuntimeFn::JsonTapeFloat,
        };
        self.builder
            .call(self.runtime[append], &[key_ptr, key_len, loaded.values[0]]);
        Ok(())
    }
}

#[cfg(test)]
#[path = "native_codegen_json_literals_tests.rs"]
mod tests;
