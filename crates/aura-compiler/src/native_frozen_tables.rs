//! Read-only values in freestanding objects. Every aggregate starts with its
//! length followed by word slots. A nested aggregate slot holds the offset
//! from that slot to the child. This keeps a whole table in read-only data
//! without dynamic relocations or host pointers.

use super::*;
use crate::const_eval::FrozenValue;
use crate::integer::IntegerRepresentation;

const INDEX: &str = "$frozen.index";
const INDEX_REF: &str = "$frozen.index.ref";
const LEN: &str = "$frozen.len";
const EMPTY: &str = "$frozen.empty";
const BYTE_LEN: &str = "$frozen.byte_len";
const MAP_INDEX: &str = "$frozen.map.index";
const MAP_REF: &str = "$frozen.map.ref";
const MAP_HAS: &str = "$frozen.map.has";
const SET_HAS: &str = "$frozen.set.has";

pub(in crate::native_codegen) fn is_projection(name: &str) -> bool {
    matches!(
        name,
        INDEX | INDEX_REF | LEN | EMPTY | BYTE_LEN | MAP_INDEX | MAP_REF | MAP_HAS | SET_HAS
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
    for function in &mut module.functions {
        let types: HashMap<_, _> = function
            .local_types
            .iter()
            .map(|l| (l.name.clone(), l.ty.clone()))
            .collect();
        let mut addresses = HashSet::new();
        // MIR names a computed receiver in a temporary before the projection.
        // Walk to a fixed point as a loop or branch may name an earlier block.
        loop {
            let previous = addresses.len();
            for block in &function.blocks {
                for ins in &block.instructions {
                    let Instruction::Assign { target, value } = ins else {
                        continue;
                    };
                    if types.get(target).is_some_and(is_word) {
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
            if previous == addresses.len() {
                break;
            }
        }
        if addresses.is_empty() {
            continue;
        }
        let mut unsupported = false;
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
                        addresses.contains(target),
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
                unsupported |= mentions.iter().any(|place| addresses.contains(*place));
            }
            unsupported |= crate::mir::terminator_place_mentions(&block.terminator)
                .iter()
                .any(|place| addresses.contains(*place));
        }
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
    let mut key_type = None;
    let index_op = if returns_address { INDEX_REF } else { INDEX };
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
                        key_type = Some(key.clone());
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
        .map(|value| MirArg {
            name: None,
            value,
            writeback_place: None,
        })
        .collect();
    Some(Rvalue::Call {
        callee: CallTarget::Name(op.into()),
        args,
        type_args: key_type.map(|ty| ("key".into(), ty)).into_iter().collect(),
    })
}

/// Encode one node. Children are appended after all slots of their parent,
/// which keeps the words of a numeric table contiguous.
fn append_node(
    bytes: &mut Vec<u8>,
    value: &FrozenValue,
    classes: &HashMap<String, MirClass>,
) -> usize {
    let start = bytes.len();
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
    for (i, item) in elements.into_iter().enumerate() {
        let slot = slots + 8 * i;
        let word = match item {
            FrozenValue::None => 0,
            FrozenValue::Bool(b) => u64::from(*b),
            FrozenValue::Int(i) => match i.representation() {
                IntegerRepresentation::Signed(n) => n as u64,
                IntegerRepresentation::Unsigned(n) => n as u64,
            },
            FrozenValue::Duration(n) => *n as u64,
            FrozenValue::Float(bits) => *bits,
            child => (append_node(bytes, child, classes) - slot) as u64,
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
            append_node(&mut bytes, value, &self.classes);
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
        Ok(ValueRef {
            values: vec![base],
            ty: DirectType::Scalar(ScalarKind::Uint64),
        })
    }

    pub(in crate::native_codegen) fn frozen_table_projection(
        &mut self,
        name: &str,
        args: &[MirArg],
        type_args: &BTreeMap<String, Type>,
        target: &DirectType,
    ) -> Result<ValueRef, String> {
        let base = self.load_operand(&args[0].value)?.values[0];
        let flags = MemFlags::new();
        let len = self.builder.load(types::I64, flags, base, 0);
        let value = match name {
            LEN => len,
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
                let text = type_args.contains_key("key");
                let key = if let Operand::String(text) = &args[1].value {
                    let mut bytes = Vec::new();
                    append_node(&mut bytes, &FrozenValue::Str(text.clone()), &self.classes);
                    let (ptr, _) = self.string_constant(&bytes)?;
                    Key::Text(ptr)
                } else {
                    let loaded = self.load_operand(&args[1].value)?;
                    if text {
                        Key::Text(loaded.values[0])
                    } else {
                        Key::Scalar(loaded)
                    }
                };
                self.frozen_find(name, base, len, key, args, target)?
            }
            _ => unreachable!("only frozen projections reach the table emitter"),
        };
        Ok(ValueRef {
            values: vec![value],
            ty: target.clone(),
        })
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
        self.call_trap_hook(TRAP_ASSERT, line, col);
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
mod tests {
    use super::*;
    use crate::integer::IntegerValue;

    fn word(bytes: &[u8], offset: usize) -> u64 {
        u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
    }

    #[test]
    fn nested_frozen_nodes_encode_distances_relative_to_the_containing_slot() {
        let word_type = Type::named("int64");
        let value = FrozenValue::Tuple(
            vec![],
            vec![
                FrozenValue::Set(
                    word_type.clone(),
                    vec![FrozenValue::Int(IntegerValue::from_i64(-9))],
                ),
                FrozenValue::Dict(
                    word_type.clone(),
                    word_type.clone(),
                    vec![(
                        FrozenValue::Int(IntegerValue::from_u64(u64::MAX)),
                        FrozenValue::Bool(true),
                    )],
                ),
                FrozenValue::Class(
                    "P".into(),
                    vec![("x".into(), FrozenValue::Float(1.25f64.to_bits()))],
                ),
                FrozenValue::Enum("E".into(), "Some".into(), vec![FrozenValue::Duration(7)]),
                FrozenValue::Union(word_type, 0, Box::new(FrozenValue::None)),
                FrozenValue::Bytes(vec![0x80, 0xff]),
                FrozenValue::Str("λa".into()),
            ],
        );
        let mut bytes = Vec::new();
        let base = append_node(&mut bytes, &value, &HashMap::new());
        assert_eq!(word(&bytes, base), 7);
        let child = |i: usize| {
            let slot = base + (i + 1) * 8;
            slot + word(&bytes, slot) as usize
        };
        assert_eq!(word(&bytes, child(0) + 8), (-9i64) as u64);
        assert_eq!(word(&bytes, child(1) + 8), u64::MAX);
        assert_eq!(word(&bytes, child(1) + 16), 1);
        assert_eq!(word(&bytes, child(2) + 8), 1.25f64.to_bits());
        assert_eq!(word(&bytes, child(3) + 8), 7);
        assert_eq!(word(&bytes, child(4) + 8), 0);
        assert_eq!(word(&bytes, child(5) + 16), 255);
        assert_eq!(word(&bytes, child(6)), 2);
        assert_eq!(word(&bytes, child(6) + 8), 3);
    }
}
