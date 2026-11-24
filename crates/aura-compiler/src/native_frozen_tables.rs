//! Read-only values in freestanding objects. Every aggregate starts with its
//! length followed by word slots. A nested aggregate slot holds the offset
//! from that slot to the child. This keeps a whole table in read-only data
//! without dynamic relocations or host pointers.

use super::*;

#[path = "native_frozen_places.rs"]
mod places;
use crate::const_eval::FrozenValue;
use crate::integer::IntegerRepresentation;

/// A frozen sequence index is out of range or a dictionary key is absent.
const TRAP_LOOKUP: i64 = 9;

const INDEX: &str = "$frozen.index";
const INDEX_REF: &str = "$frozen.index.ref";
const LEN: &str = "$frozen.len";
const EMPTY: &str = "$frozen.empty";
const IS_TAG: &str = "$frozen.tag_is";
const BYTE_LEN: &str = "$frozen.byte_len";
const MAP_INDEX: &str = "$frozen.map.index";
const MAP_REF: &str = "$frozen.map.ref";
const MAP_HAS: &str = "$frozen.map.has";
const SET_HAS: &str = "$frozen.set.has";

pub(in crate::native_codegen) fn is_projection(name: &str) -> bool {
    let name = name.strip_suffix(".text").unwrap_or(name);
    matches!(
        name,
        INDEX
            | INDEX_REF
            | LEN
            | EMPTY
            | BYTE_LEN
            | IS_TAG
            | MAP_INDEX
            | MAP_REF
            | MAP_HAS
            | SET_HAS
    )
}

/// A read of static data never reaches the original initializer.
pub(super) fn detach_initializers(module: &mut MirModule) {
    let frozen: HashSet<_> = module
        .constants
        .iter()
        .filter(|c| c.frozen.is_some())
        .map(|c| c.key.as_str())
        .collect();
    if frozen.is_empty() {
        return;
    }
    for instruction in module
        .functions
        .iter_mut()
        .flat_map(|f| &mut f.blocks)
        .flat_map(|b| &mut b.instructions)
    {
        if let Instruction::Assign {
            value: Rvalue::ModuleConstant { key, initializer },
            ..
        } = instruction
        {
            if frozen.contains(key.as_str()) {
                initializer.clear();
            }
        }
    }
}

/// Only locals that actually carry a frozen borrow become address words.
/// Ordinary collection construction still goes through the subset checker.
pub(super) fn lower_reads(module: &mut MirModule, problems: &mut Vec<Diagnostic>) {
    if !module.constants.iter().any(|c| c.frozen.is_some()) {
        return;
    }
    let classes: HashMap<_, _> = module
        .classes
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();
    let definitions: HashMap<_, _> = module
        .classes
        .iter()
        .map(|class| (class.name.clone(), class.clone()))
        .collect();
    for function in &mut module.functions {
        let mut types: HashMap<_, _> = function
            .local_types
            .iter()
            .map(|l| (l.name.clone(), l.ty.clone()))
            .collect();
        let mut inline_values = HashSet::new();
        let mut addresses = HashSet::new();
        // MIR names a computed receiver in a temporary before the projection.
        // Walk to a fixed point as a loop or branch may name an earlier block.
        loop {
            inline_values.extend(
                types
                    .iter()
                    .filter(|(_, ty)| {
                        let aggregate = matches!(ty, Type::Union(_))
                            || matches!(ty,Type::Named(n,_) if classes.contains_key(n.as_str()));
                        aggregate
                            && direct_type(ty, &definitions).is_some_and(|ty| inline_aggregate(&ty))
                    })
                    .map(|(place, _)| place.clone()),
            );
            let previous = addresses.len();
            for block in &function.blocks {
                for ins in &block.instructions {
                    let Instruction::Assign { target, value } = ins else {
                        continue;
                    };
                    let eligible = match types.get(target) {
                        Some(Type::Tuple(_) | Type::Union(_)) => true,
                        Some(Type::Named(n, _)) => {
                            matches!(n.as_str(), "list" | "str" | "set" | "dict" | "bytes")
                                || classes.contains_key(n.as_str())
                                || module.enums.iter().any(|e| e.name == *n)
                        }
                        _ => false,
                    };
                    if !eligible || inline_values.contains(target) {
                        continue;
                    }
                    let static_read = match value {
                        Rvalue::ModuleConstant { initializer, .. } => initializer.is_empty(),
                        Rvalue::Use(op) => address(op, &addresses).is_some(),
                        _ => projection(value, &addresses, &types, &classes, true).is_some(),
                    };
                    if static_read {
                        addresses.insert(target.clone());
                    }
                }
            }
            if previous == addresses.len()
                && !places::expand(function, &addresses, &mut types, &classes)
            {
                break;
            }
        }
        if addresses.is_empty() {
            continue;
        }
        let mut unsupported = false;
        let mut branches = Vec::new();
        for block in &mut function.blocks {
            // A borrow of static storage owns no reference to retain or drop.
            block.instructions.retain(|instruction| match instruction {
                Instruction::Retain { place } | Instruction::Release { place } => {
                    !addresses.contains(place)
                }
                Instruction::IndexInRange { list, .. } => !addresses.contains(list),
                _ => true,
            });
            for instruction in &mut block.instructions {
                if let Instruction::Assign { target, value } = instruction {
                    if let Some(replacement) = projection(
                        value,
                        &addresses,
                        &types,
                        &classes,
                        addresses.contains(target) || inline_values.contains(target),
                    ) {
                        *value = replacement;
                        continue;
                    }
                    if matches!(value, Rvalue::ModuleConstant {initializer, ..} if initializer.is_empty())
                        || matches!(value, Rvalue::Use(op) if addresses.contains(target) && address(op,&addresses).is_some())
                    {
                        continue;
                    }
                    if addresses.contains(target) {
                        unsupported = true;
                    }
                }
                let mut mentions = Vec::new();
                crate::mir::instruction_place_mentions(instruction, &mut mentions);
                unsupported |= mentions
                    .iter()
                    .any(|place| addresses.contains(place.split('.').next().unwrap_or(place)));
            }
            lower_match(
                block,
                &addresses,
                &types,
                &module.enums,
                &mut function.local_types,
                &mut branches,
            );
            unsupported |= crate::mir::terminator_place_mentions(&block.terminator)
                .iter()
                .any(|place| addresses.contains(place.split('.').next().unwrap_or(place)));
        }
        function.blocks.extend(branches);
        if unsupported {
            let mut error = Diagnostic::coded_at(
                UNSUPPORTED,
                function.span,
                format!(
                    "`{}` uses a frozen aggregate in an operation that needs the hosted runtime",
                    public_direct_function_name(&function.name)
                ),
            );
            error.render_path = function.source_path.clone();
            problems.push(error);
        }
        for local in &mut function.local_types {
            if addresses.contains(&local.name) {
                local.ty = Type::named("uint64");
            }
        }
    }
}

/// Branch directly on the discriminant stored after the payload slots.
fn lower_match(
    block: &mut crate::mir::BasicBlock,
    addresses: &HashSet<String>,
    types: &HashMap<String, Type>,
    enums: &[MirEnum],
    locals: &mut Vec<crate::mir::MirLocalType>,
    branches: &mut Vec<crate::mir::BasicBlock>,
) {
    let Terminator::Match {
        scrutinee,
        arms,
        otherwise,
    } = &block.terminator
    else {
        return;
    };
    let Some(Type::Named(ty, _)) = address(scrutinee, addresses).and_then(|place| types.get(place))
    else {
        return;
    };
    let Some(enumeration) = enums.iter().find(|e| e.name == *ty) else {
        return;
    };
    let root = scrutinee.clone();
    let mut otherwise = otherwise.clone();
    for arm in arms.iter().rev() {
        if arm.wildcard {
            otherwise = arm.label.clone();
            continue;
        }
        let Some(tag) = enumeration
            .variants
            .iter()
            .position(|v| Some(&v.name) == arm.variant_name.as_ref())
        else {
            return;
        };
        let id = locals
            .iter()
            .filter_map(|l| l.name.strip_prefix("%t")?.parse::<u64>().ok())
            .max()
            .map_or(0, |n| n + 1);
        let test = format!("%t{id}");
        let name = format!("$frozen.match.{id}");
        locals.push(crate::mir::MirLocalType {
            name: test.clone(),
            ty: Type::named("bool"),
        });
        let query = Rvalue::Call {
            callee: CallTarget::Name(IS_TAG.into()),
            args: [root.clone(), Operand::Int(tag as u128)]
                .into_iter()
                .map(argument)
                .collect(),
            type_args: Default::default(),
        };
        branches.push(crate::mir::BasicBlock {
            label: name.clone(),
            instructions: vec![Instruction::Assign {
                target: test.clone(),
                value: query,
            }],
            terminator: Terminator::Branch {
                condition: Operand::Place(test),
                then_label: arm.label.clone(),
                else_label: otherwise,
            },
        });
        otherwise = name;
    }
    block.terminator = Terminator::Goto(otherwise);
}

fn argument(value: Operand) -> MirArg {
    MirArg {
        name: None,
        value,
        writeback_place: None,
    }
}

/// A class or union that freestanding Aura already carries in native registers.
fn inline_aggregate(ty: &DirectType) -> bool {
    let inline =
        |field: &DirectType| matches!(field, DirectType::Scalar(_)) || inline_aggregate(field);
    match ty {
        DirectType::PlainClass(record) => record.fields.iter().all(|field| inline(&field.ty)),
        DirectType::Union(union) => union.members.iter().all(|member| inline(&member.ty)),
        _ => false,
    }
}

fn is_word(ty: &Type) -> bool {
    matches!(ty, Type::Named(n, args) if args.is_empty() && ScalarKind::of_name(n).is_some())
}

fn address<'a>(op: &'a Operand, addresses: &HashSet<String>) -> Option<&'a str> {
    match op {
        Operand::Place(p) | Operand::MovePlace(p) if addresses.contains(p) => Some(p),
        _ => None,
    }
}

fn projection(
    value: &Rvalue,
    addresses: &HashSet<String>,
    types: &HashMap<String, Type>,
    classes: &HashMap<&str, &MirClass>,
    returns_address: bool,
) -> Option<Rvalue> {
    let mut text_key = false;
    let index_op = if returns_address { INDEX_REF } else { INDEX };
    // Unions use the same trailing discriminant as enums.
    if let Rvalue::UnionTagTest {
        place,
        member_index,
        ..
    }
    | Rvalue::UnionTakePayload {
        place,
        member_index,
        ..
    } = value
    {
        if !addresses.contains(place) {
            return None;
        }
        let (op, index) = if matches!(value, Rvalue::UnionTagTest { .. }) {
            (IS_TAG, *member_index)
        } else {
            (index_op, 0)
        };
        return Some(Rvalue::Call {
            callee: CallTarget::Name(op.into()),
            args: vec![
                argument(Operand::Place(place.clone())),
                argument(Operand::Int(index as u128)),
            ],
            type_args: Default::default(),
        });
    }
    let (receiver, op, operands) = match value {
        Rvalue::TupleElement { tuple, index, .. } => {
            (tuple, index_op, vec![Operand::Int(*index as u128)])
        }
        Rvalue::Member { object, field } => {
            let Type::Named(class, _) = types.get(address(object, addresses)?)? else {
                return None;
            };
            let index = classes
                .get(class.as_str())?
                .fields
                .iter()
                .position(|f| f.name == *field)?;
            (object, index_op, vec![Operand::Int(index as u128)])
        }
        Rvalue::VariantPayload {
            scrutinee, index, ..
        } => (scrutinee, index_op, vec![Operand::Int(*index as u128)]),
        Rvalue::Call {
            callee:
                CallTarget::Runtime {
                    op,
                    receiver: Some(receiver),
                    ..
                },
            args,
            ..
        } => {
            let name = match op {
                RuntimeOp::List(ListOp::Len)
                | RuntimeOp::Dict(DictOp::Len)
                | RuntimeOp::Set(SetOp::Len)
                | RuntimeOp::Str(StrOp::Len) => LEN,
                RuntimeOp::Str(StrOp::ByteLen) => BYTE_LEN,
                RuntimeOp::List(ListOp::IsEmpty)
                | RuntimeOp::Dict(DictOp::IsEmpty)
                | RuntimeOp::Set(SetOp::IsEmpty) => EMPTY,
                RuntimeOp::List(
                    ListOp::Index
                    | ListOp::IndexScalar(_)
                    | ListOp::IndexUnchecked
                    | ListOp::IndexScalarUnchecked(_),
                ) => index_op,
                RuntimeOp::Dict(DictOp::Index | DictOp::ContainsKey | DictOp::ContainsKeyInt64)
                | RuntimeOp::Set(SetOp::Contains | SetOp::ContainsInt64) => {
                    let Type::Named(_, parameters) = types.get(address(receiver, addresses)?)?
                    else {
                        return None;
                    };
                    let key = parameters.first()?;
                    if key == &Type::named("str") {
                        let argument = &args.first()?.value;
                        if !matches!(argument, Operand::String(_))
                            && address(argument, addresses).is_none()
                        {
                            return None;
                        }
                        text_key = true;
                    } else if !is_word(key) {
                        return None;
                    }
                    match op {
                        RuntimeOp::Dict(DictOp::Index) if returns_address => MAP_REF,
                        RuntimeOp::Dict(DictOp::Index) => MAP_INDEX,
                        RuntimeOp::Dict(_) => MAP_HAS,
                        _ => SET_HAS,
                    }
                }
                _ => return None,
            };
            (
                receiver,
                name,
                args.iter().map(|a| a.value.clone()).collect(),
            )
        }
        _ => return None,
    };
    address(receiver, addresses)?;
    let args = std::iter::once(receiver.clone())
        .chain(operands)
        .map(argument)
        .collect();
    Some(Rvalue::Call {
        callee: CallTarget::Name(if text_key {
            format!("{op}.text")
        } else {
            op.into()
        }),
        args,
        type_args: Default::default(),
    })
}

fn wide_payload(value: &FrozenValue) -> Option<[u8; 16]> {
    use crate::integer::IntegerKind;
    match value {
        FrozenValue::Duration(n) => Some(n.to_le_bytes()),
        FrozenValue::Int(i) => {
            let wide = matches!(
                i.runtime_kind(),
                Some(IntegerKind::Int128 | IntegerKind::Uint128)
            );
            match i.representation() {
                IntegerRepresentation::Signed(n) if wide || i64::try_from(n).is_err() => {
                    Some(n.to_le_bytes())
                }
                IntegerRepresentation::Unsigned(n) if wide || u64::try_from(n).is_err() => {
                    Some(n.to_le_bytes())
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Encode one node. Children are appended after all slots of their parent,
/// which keeps the words of a numeric table contiguous.
fn append_node(
    bytes: &mut Vec<u8>,
    value: &FrozenValue,
    classes: &HashMap<String, MirClass>,
    enums: &HashMap<String, MirEnum>,
) -> usize {
    let start = bytes.len();
    let tag = match value {
        FrozenValue::Enum(name, variant, _) => enums
            .get(name)
            .and_then(|e| e.variants.iter().position(|v| v.name == *variant)),
        FrozenValue::Union(_, ordinal, _) => Some(*ordinal),
        _ => None,
    };
    let mut elements = Vec::new();
    let length = match value {
        FrozenValue::Bytes(data) => {
            bytes.extend_from_slice(&(data.len() as u64).to_le_bytes());
            for byte in data {
                bytes.extend_from_slice(&u64::from(*byte).to_le_bytes());
            }
            return start;
        }
        FrozenValue::Str(text) => {
            bytes.extend_from_slice(&(text.chars().count() as u64).to_le_bytes());
            bytes.extend_from_slice(&(text.len() as u64).to_le_bytes());
            bytes.extend_from_slice(text.as_bytes());
            bytes.resize(bytes.len().next_multiple_of(8), 0);
            return start;
        }
        FrozenValue::List(_, items)
        | FrozenValue::Set(_, items)
        | FrozenValue::Tuple(_, items)
        | FrozenValue::Enum(_, _, items) => {
            elements.extend(items);
            items.len()
        }
        FrozenValue::Dict(_, _, pairs) => {
            for (k, v) in pairs {
                elements.extend([k, v]);
            }
            pairs.len()
        }
        FrozenValue::Class(name, fields) => {
            if let Some(class) = classes.get(name) {
                for field in &class.fields {
                    if let Some((_, value)) = fields.iter().find(|(n, _)| n == &field.name) {
                        elements.push(value);
                    }
                }
            } else {
                elements.extend(fields.iter().map(|(_, v)| v));
            }
            elements.len()
        }
        FrozenValue::Union(_, _, item) => {
            elements.push(item);
            1
        }
        _ => {
            elements.push(value);
            1
        }
    };
    bytes.extend_from_slice(&(length as u64).to_le_bytes());
    let slots = bytes.len();
    bytes.resize(slots + elements.len() * 8, 0);
    if let Some(tag) = tag {
        bytes.extend_from_slice(&(tag as u64).to_le_bytes());
    }
    for (i, item) in elements.into_iter().enumerate() {
        let slot = slots + 8 * i;
        if let Some(payload) = wide_payload(item) {
            let delta = (bytes.len() - slot) as u64;
            bytes.extend_from_slice(&payload);
            bytes[slot..slot + 8].copy_from_slice(&delta.to_le_bytes());
            continue;
        }
        let word = match item {
            FrozenValue::None => 0,
            FrozenValue::Bool(b) => u64::from(*b),
            FrozenValue::Int(i) => match i.representation() {
                IntegerRepresentation::Signed(n) => n as u64,
                IntegerRepresentation::Unsigned(n) => n as u64,
            },

            FrozenValue::Float(bits) => *bits,
            child => (append_node(bytes, child, classes, enums) - slot) as u64,
        };
        bytes[slot..slot + 8].copy_from_slice(&word.to_le_bytes());
    }
    start
}

enum Key {
    Text(Val),
    Scalar(ValueRef),
}

impl FunctionCompiler<'_> {
    pub(in crate::native_codegen) fn frozen_table_address(
        &mut self,
        key: &str,
        target: &DirectType,
    ) -> Result<ValueRef, String> {
        let name = format!("\0freestanding constant {key}");
        let data = if let Some(existing) = self.tag_cells.get(&name) {
            *existing
        } else {
            let value = self
                .module_constants
                .iter()
                .find(|c| c.key == key)
                .and_then(|c| c.frozen.as_ref())
                .ok_or_else(|| format!("missing frozen value for {key}"))?;
            let mut bytes = Vec::new();
            append_node(&mut bytes, value, &self.classes, &self.enums);
            let mut contents = DataDescription::new(bytes);
            contents.align = Some(8);
            let obj = self.builder.module();
            let global = obj.declare_data(
                &format!("aura_frozen_table_{}", self.tag_cells.len()),
                Linkage::Local,
                false,
            )?;
            obj.define_data(global, contents)?;
            self.tag_cells.insert(name, global);
            global
        };
        let global = self.builder.import_data(data);
        let base = self.builder.symbol_value(types::I64, global);
        if inline_aggregate(target) {
            return self.frozen_inline_value(base, target);
        }
        Ok(ValueRef {
            values: vec![base],
            ty: DirectType::Scalar(ScalarKind::Uint64),
        })
    }

    pub(in crate::native_codegen) fn frozen_table_projection(
        &mut self,
        name: &str,
        args: &[MirArg],
        target: &DirectType,
    ) -> Result<ValueRef, String> {
        let text = name.strip_suffix(".text");
        let name = text.unwrap_or(name);
        let base = self.load_operand(&args[0].value)?.values[0];
        let flags = MemFlags::new();
        let len = self.builder.load(types::I64, flags, base, 0);
        let value = match name {
            LEN => len,
            IS_TAG => {
                let tag = self.load_operand(&args[1].value)?.values[0];
                let kind = DirectType::Scalar(ScalarKind::Uint64);
                let current = self.frozen_slot(base, len, 1, false, &kind);
                let equal = self.builder.icmp(IntCC::Equal, current, tag);
                self.builder.uextend(types::I64, equal)
            }
            EMPTY => {
                let empty = self.builder.icmp_imm(IntCC::Equal, len, 0);
                self.builder.uextend(types::I64, empty)
            }
            BYTE_LEN => self.builder.load(types::I64, flags, base, 8),
            INDEX | INDEX_REF => {
                let index = self.load_operand(&args[1].value)?.values[0];
                let negative = self.builder.icmp_imm(IntCC::SignedLessThan, index, 0);
                let wrapped = self.builder.iadd(index, len);
                let index = self.builder.select(negative, wrapped, index);
                let invalid = self
                    .builder
                    .icmp(IntCC::UnsignedGreaterThanOrEqual, index, len);
                self.frozen_bounds_check(invalid, args)?;
                self.frozen_slot(base, index, 1, name == INDEX_REF, target)
            }
            MAP_HAS | SET_HAS | MAP_INDEX | MAP_REF => {
                let key = if let Operand::String(text) = &args[1].value {
                    let mut bytes = Vec::new();
                    append_node(
                        &mut bytes,
                        &FrozenValue::Str(text.clone()),
                        &self.classes,
                        &self.enums,
                    );
                    let (ptr, _) = self.string_constant(&bytes)?;
                    Key::Text(ptr)
                } else {
                    let loaded = self.load_operand(&args[1].value)?;
                    if text.is_some() {
                        Key::Text(loaded.values[0])
                    } else {
                        Key::Scalar(loaded)
                    }
                };
                self.frozen_find(name, base, len, key, args, target)?
            }
            _ => unreachable!("only frozen projections reach the table emitter"),
        };
        if inline_aggregate(target) {
            return self.frozen_inline_value(value, target);
        }
        Ok(ValueRef {
            values: vec![value],
            ty: target.clone(),
        })
    }

    /// A register-layout aggregate keeps its ordinary MIR type. In particular,
    /// its tag tests still prove projected reads in the narrowed branch.
    fn frozen_inline_value(&mut self, base: Val, ty: &DirectType) -> Result<ValueRef, String> {
        let mut values = Vec::new();
        match ty {
            DirectType::PlainClass(record) => {
                for (i, field) in record.fields.iter().enumerate() {
                    let loaded = self.frozen_field(base, i, &field.ty)?;
                    values.extend(loaded.values);
                }
            }
            DirectType::Union(union) => {
                let tag = self.builder.load(types::I64, MemFlags::new(), base, 16);
                let done = self.builder.create_block();
                values.extend(
                    ty.abi_types()
                        .into_iter()
                        .map(|abi| self.builder.append_block_param(done, abi)),
                );
                for (i, member) in union.members.iter().enumerate() {
                    let selected = self.builder.create_block();
                    let next = self.builder.create_block();
                    let equal = self.builder.icmp_imm(IntCC::Equal, tag, i as i64);
                    self.builder.brif(equal, selected, &[], next, &[]);
                    self.builder.switch_to_block(selected);
                    self.builder.seal_block(selected);
                    let member = self.frozen_field(base, 0, &member.ty)?;
                    let words = self.union_words_from_member(union, i, member)?;
                    self.builder.jump(done, &words);
                    self.builder.switch_to_block(next);
                    self.builder.seal_block(next);
                }
                self.builder.trap(TrapCode::User(1));
                self.builder.switch_to_block(done);
                self.builder.seal_block(done);
            }
            _ => unreachable!("only native aggregate layouts are unpacked"),
        }
        Ok(ValueRef {
            values,
            ty: ty.clone(),
        })
    }

    fn frozen_field(
        &mut self,
        base: Val,
        index: usize,
        ty: &DirectType,
    ) -> Result<ValueRef, String> {
        let at = self.builder.iconst(types::I64, index as i64);
        let nested = inline_aggregate(ty);
        let word = self.frozen_slot(base, at, 1, nested, ty);
        if nested {
            self.frozen_inline_value(word, ty)
        } else {
            Ok(ValueRef {
                values: vec![word],
                ty: ty.clone(),
            })
        }
    }

    fn frozen_bounds_check(&mut self, invalid: Val, args: &[MirArg]) -> Result<(), String> {
        let fail = self.builder.create_block();
        let resume = self.builder.create_block();
        self.builder.brif(invalid, fail, &[], resume, &[]);
        self.builder.switch_to_block(fail);
        self.builder.seal_block(fail);
        let (line, col) = if let [_, _, line, col, ..] = args {
            (
                self.load_operand(&line.value)?.values[0],
                self.load_operand(&col.value)?.values[0],
            )
        } else {
            self.span_values(None)
        };
        self.call_trap_hook(TRAP_LOOKUP, line, col);
        self.builder.trap(TrapCode::User(1));
        self.builder.switch_to_block(resume);
        self.builder.seal_block(resume);
        Ok(())
    }

    fn frozen_slot(
        &mut self,
        base: Val,
        index: Val,
        first: i64,
        reference: bool,
        target: &DirectType,
    ) -> Val {
        let word = self.builder.iadd_imm(index, first);
        let offset = self.builder.imul_imm(word, 8);
        let slot = self.builder.element_address(base, offset);
        if reference {
            let delta = self.builder.load(types::I64, MemFlags::new(), slot, 0);
            self.builder.element_address(slot, delta)
        } else {
            let ty = target
                .scalar_kind()
                .map_or(types::I64, |kind| kind.signature_type());
            self.builder.load(ty, MemFlags::new(), slot, 0)
        }
    }

    /// Compare the UTF-8 payloads after checking their byte lengths.
    fn frozen_text_equal(&mut self, left: Val, right: Val) -> Val {
        let flags = MemFlags::new();
        let bytes = self.builder.load(types::I64, flags, left, 8);
        let other = self.builder.load(types::I64, flags, right, 8);
        let different = self.builder.icmp(IntCC::NotEqual, bytes, other);
        let scan = self.builder.create_block();
        let compare = self.builder.create_block();
        let finish = self.builder.create_block();
        let zero = self.builder.iconst(types::I64, 0);
        let index = self.builder.append_block_param(scan, types::I64);
        let result = self.builder.append_block_param(finish, types::I64);
        self.builder.brif(different, finish, &[zero], scan, &[zero]);
        self.builder.switch_to_block(scan);
        let end = self.builder.icmp(IntCC::Equal, index, bytes);
        let one = self.builder.iconst(types::I64, 1);
        self.builder.brif(end, finish, &[one], compare, &[]);
        self.builder.switch_to_block(compare);
        self.builder.seal_block(compare);
        let a = self.builder.element_address(left, index);
        let b = self.builder.element_address(right, index);
        let a = self.builder.uload8(types::I64, flags, a, 16);
        let b = self.builder.uload8(types::I64, flags, b, 16);
        let equal = self.builder.icmp(IntCC::Equal, a, b);
        let next = self.builder.iadd_imm(index, 1);
        self.builder.brif(equal, scan, &[next], finish, &[zero]);
        self.builder.seal_block(scan);
        self.builder.switch_to_block(finish);
        self.builder.seal_block(finish);
        self.builder.icmp_imm(IntCC::NotEqual, result, 0)
    }

    fn frozen_find(
        &mut self,
        name: &str,
        base: Val,
        len: Val,
        key: Key,
        args: &[MirArg],
        target: &DirectType,
    ) -> Result<Val, String> {
        let scan = self.builder.create_block();
        let test = self.builder.create_block();
        let next = self.builder.create_block();
        let done = self.builder.create_block();
        let initial = self.builder.iconst(types::I64, 0);
        let at = self.builder.append_block_param(scan, types::I64);
        self.builder.jump(scan, &[initial]);
        self.builder.switch_to_block(scan);
        let missing = self.builder.icmp(IntCC::Equal, at, len);
        self.builder.brif(missing, done, &[], test, &[]);
        self.builder.switch_to_block(test);
        self.builder.seal_block(test);
        let stride = if name == SET_HAS { 1 } else { 2 };
        let i = self.builder.imul_imm(at, stride);
        let equal = match key {
            Key::Text(key) => {
                let candidate = self.frozen_slot(base, i, 1, true, target);
                self.frozen_text_equal(key, candidate)
            }
            Key::Scalar(key) => {
                let candidate = self.frozen_slot(base, i, 1, false, &key.ty);
                if key.ty.scalar_kind().is_some_and(ScalarKind::is_float) {
                    self.builder.fcmp(FloatCC::Equal, key.values[0], candidate)
                } else {
                    self.builder.icmp(IntCC::Equal, key.values[0], candidate)
                }
            }
        };
        self.builder.brif(equal, done, &[], next, &[]);
        self.builder.switch_to_block(next);
        self.builder.seal_block(next);
        let i = self.builder.iadd_imm(at, 1);
        self.builder.jump(scan, &[i]);
        self.builder.seal_block(scan);
        self.builder.switch_to_block(done);
        self.builder.seal_block(done);
        let missing = self.builder.icmp(IntCC::Equal, at, len);
        if matches!(name, SET_HAS | MAP_HAS) {
            let found = self.builder.icmp(IntCC::NotEqual, at, len);
            return Ok(self.builder.uextend(types::I64, found));
        }
        self.frozen_bounds_check(missing, args)?;
        let index = self.builder.imul_imm(at, 2);
        Ok(self.frozen_slot(base, index, 2, name == MAP_REF, target))
    }
}

#[cfg(test)]
#[path = "native_frozen_tables_tests.rs"]
mod tests;
