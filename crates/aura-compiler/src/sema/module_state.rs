//! Literal module storage for freestanding programs (ADR-0140).
use super::*;
use serde::{Deserialize, Serialize};

/// The bytes and language-value components of a statically initialized cell.
/// A component occupies its natural memory width, including a C array field.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateImage {
    pub bytes: Vec<u8>,
    pub alignment: u32,
    pub slots: Vec<StateSlot>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateSlot {
    pub offset: i32,
    pub ty: Type,
    pub record: Option<Box<crate::c_layout::CStruct>>,
}

impl StateSlot {
    fn value(offset: i32, ty: Type) -> Self {
        Self {
            offset,
            ty,
            record: None,
        }
    }
}

fn literal_error(expr: &Expr) -> Diagnostic {
    Diagnostic::coded_at(
        "AU2080",
        expr.span,
        "mutable module storage requires an inline value with a literal initializer",
    )
    .with_help("use scalar literals, field constructors or a fixed array literal")
}

fn scalar_bytes(ty: &Type, expr: &Expr) -> Option<(Vec<u8>, u32)> {
    let name = ty.to_string();
    let (expr, negative) = match &grouped_expr(expr).kind {
        ExprKind::Unary {
            op: UnaryOp::Neg,
            expr,
        } => (grouped_expr(expr), true),
        _ => (grouped_expr(expr), false),
    };
    let signed_float = |literal: f64| if negative { -literal } else { literal };
    let (bits, width): (u128, usize) = match (&expr.kind, name.as_str()) {
        (ExprKind::Bool(value), "bool") if !negative => (u128::from(*value), 1),
        (ExprKind::Name(n), "None") if n == "None" && !negative => (0, 1),
        (literal, "float32" | "float64") => {
            let number = match literal {
                ExprKind::Float(value) => *value,
                ExprKind::Int(value) => *value as f64,
                _ => return None,
            };
            let number = signed_float(number);
            if name == "float32" {
                ((number as f32).to_bits().into(), 4)
            } else {
                (number.to_bits().into(), 8)
            }
        }
        (ExprKind::Int(bits), _) => {
            let kind = crate::integer::IntegerKind::from_runtime_type_name(&name)?;
            if matches!(
                kind,
                crate::integer::IntegerKind::IntSize | crate::integer::IntegerKind::UintSize
            ) {
                return None;
            }
            (
                if negative { bits.wrapping_neg() } else { *bits },
                (kind.bit_width() / 8) as usize,
            )
        }
        _ => return None,
    };
    Some((bits.to_le_bytes()[..width].to_vec(), width.min(8) as u32))
}

fn scalar_word(name: &str, data: &[u8]) -> Option<u64> {
    Some(match name {
        "float32" => (f32::from_le_bytes(data[..4].try_into().unwrap()) as f64).to_bits(),
        "int8" => data[0] as i8 as u64,
        "int16" => i16::from_le_bytes(data[..2].try_into().unwrap()) as u64,
        "int32" => i32::from_le_bytes(data[..4].try_into().unwrap()) as u64,
        "bool" | "uint8" => u64::from(data[0]),
        "uint16" => u16::from_le_bytes(data[..2].try_into().unwrap()).into(),
        "uint32" => u32::from_le_bytes(data[..4].try_into().unwrap()).into(),
        "uint64" | "int64" | "float64" => u64::from_le_bytes(data[..8].try_into().unwrap()),
        "None" => 0,
        _ => return None,
    })
}

fn append_inline_words(words: &mut Vec<u64>, bytes: &[u8]) {
    for chunk in bytes.chunks(8) {
        let mut word = [0; 8];
        word[..chunk.len()].copy_from_slice(chunk);
        words.push(u64::from_le_bytes(word));
    }
}

impl FunctionChecker<'_> {
    pub(super) fn state_image(&self, ty: &Type, expr: &Expr) -> Result<StateImage> {
        if let Type::Union(union) = ty {
            return self.union_image(ty, union, expr);
        }
        let expr = grouped_expr(expr);
        if ty.as_raw_pointer().is_some() {
            if let ExprKind::Cast { expr: value, .. } = &expr.kind {
                if let ExprKind::Int(address) = grouped_expr(value).kind {
                    let address = u64::try_from(address).map_err(|_| literal_error(expr))?;
                    return Ok(StateImage {
                        bytes: address.to_le_bytes().to_vec(),
                        alignment: 8,
                        slots: vec![StateSlot::value(0, Type::named("uint64"))],
                    });
                }
            }
        }
        if let Some((bytes, alignment)) = scalar_bytes(ty, expr) {
            let slots = if bytes.len() == 16 {
                vec![
                    StateSlot::value(0, Type::named("uint64")),
                    StateSlot::value(8, Type::named("uint64")),
                ]
            } else {
                vec![StateSlot::value(0, ty.clone())]
            };
            return Ok(StateImage {
                bytes,
                alignment,
                slots,
            });
        }
        let Type::Named(name, args) = ty else {
            return Err(literal_error(expr));
        };
        if let Some((element, length)) = ty.as_fixed_array() {
            return self.array_image(ty, element, length, expr);
        }
        let class = self
            .resolve_class_info(name)
            .ok_or_else(|| literal_error(expr))?;
        let ExprKind::Call {
            callee,
            args: actuals,
        } = &expr.kind
        else {
            return Err(literal_error(expr));
        };
        // Constructors with a body or an invariant require run-time execution.
        let callee = match &grouped_expr(callee).kind {
            ExprKind::Specialize { expr, .. } => grouped_expr(expr),
            _ => grouped_expr(callee),
        };
        let constructor = match &callee.kind {
            ExprKind::Name(name) => self.resolve_class_info(name),
            ExprKind::Member { object, field } => self
                .infer_module_path(object)
                .and_then(|path| self.module_namespace(&path))
                .and_then(|module| module.classes.get(field)),
            _ => None,
        };
        if !constructor.is_some_and(|called| {
            called.module_name == class.module_name && called.decl.name == class.decl.name
        }) || class.initializer().is_some()
            || class
                .methods
                .contains_key(crate::ast::CLASS_INVARIANT_METHOD)
        {
            return Err(literal_error(expr));
        }
        let substitutions = class
            .decl
            .type_params
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();
        let c_layout = if class.decl.c_layout {
            Some(
                self.module_namespace(&class.module_name)
                    .and_then(|module| {
                        ffi_struct_layout(&Type::named(&class.decl.name), &module.all_classes)
                    })
                    .or_else(|| ffi_struct_layout(ty, self.classes))
                    .ok_or_else(|| literal_error(expr))?,
            )
        } else {
            None
        };
        let mut locations = c_layout.as_ref().map(|layout| layout.field_locations());
        let mut image = StateImage {
            bytes: vec![0; c_layout.as_ref().map_or(0, |layout| layout.size())],
            alignment: c_layout.as_ref().map_or(1, |layout| layout.align() as u32),
            slots: Vec::new(),
        };
        for (index, field) in class.decl.fields.iter().enumerate() {
            if field.ty.indirect {
                return Err(literal_error(expr));
            }
            let value = actuals
                .iter()
                .find(|a| a.name.as_ref() == Some(&field.name))
                .or_else(|| actuals.get(index).filter(|a| a.name.is_none()))
                .map(|a| &a.value)
                .or(field.default.as_ref())
                .ok_or_else(|| literal_error(expr))?;
            let ty = aura_core::types::substitute_type_in_context(
                &class.fields[&field.name].ty,
                &substitutions,
                None,
            );
            let field = self.state_image(&ty, value)?;
            if let Some(fields) = &mut locations {
                // The generated union class has a single inline backing field.
                let (offset, bits) = if class.decl.c_union.is_some() {
                    (0, None)
                } else {
                    fields.next().ok_or_else(|| literal_error(expr))?
                };
                if let Some((start, width)) = bits {
                    let mut word = [0; 8];
                    word[..field.bytes.len()].copy_from_slice(&field.bytes);
                    let word = u64::from_le_bytes(word);
                    for (index, (source, destination, mask)) in
                        crate::c_layout::bit_chunks(start, width).enumerate()
                    {
                        image.bytes[offset + index] |=
                            (((word >> source) as u8) << destination) & mask;
                    }
                } else {
                    let dest = image
                        .bytes
                        .get_mut(offset..offset + field.bytes.len())
                        .ok_or_else(|| literal_error(expr))?;
                    dest.copy_from_slice(&field.bytes);
                }
                continue;
            }
            let offset = image.bytes.len().next_multiple_of(field.alignment as usize);
            image.bytes.resize(offset, 0);
            image.bytes.extend(field.bytes);
            let offset = i32::try_from(offset).map_err(|_| literal_error(expr))?;
            for mut slot in field.slots {
                slot.offset = offset
                    .checked_add(slot.offset)
                    .ok_or_else(|| literal_error(expr))?;
                image.slots.push(slot);
            }
            image.alignment = image.alignment.max(field.alignment);
        }

        if let Some(layout) = &c_layout {
            image.slots.push(StateSlot {
                offset: 0,
                ty: ty.clone(),
                record: Some(Box::new(layout.clone())),
            });
            return Ok(image);
        }
        image.bytes.resize(
            image
                .bytes
                .len()
                .max(1)
                .next_multiple_of(image.alignment as usize),
            0,
        );
        Ok(image)
    }

    /// An inline Aura union holds its tag and the register words of its active
    /// member. Inactive words have a defined, zero initial value.
    fn union_image(&self, ty: &Type, union: &UnionType, expr: &Expr) -> Result<StateImage> {
        let id = UnionInjectionId {
            module_name: self.module_name.to_owned(),
            line: expr.span.line,
            column: expr.span.column,
            expected_key: ty.canonical_key(self.module_name, self.canonical_type_names),
        };
        let injected = self
            .union_injections
            .borrow()
            .get(&id)
            .cloned()
            .ok_or_else(|| literal_error(expr))?;
        let active = self.state_image(&injected.member_type, expr)?;
        let mut words = vec![injected.member_index as u64];
        for slot in &active.slots {
            let data = &active.bytes[slot.offset as usize..];
            if let Some(layout) = &slot.record {
                use crate::c_layout::StoragePart;
                for part in layout.storage_parts() {
                    let leaf = match part {
                        StoragePart::Scalar(leaf) => leaf,
                        StoragePart::Inline { offset, bytes } => {
                            append_inline_words(&mut words, &data[offset..offset + bytes]);
                            continue;
                        }
                    };
                    let bytes = &data[leaf.offset..];
                    if let Some((start, width)) = leaf.bits {
                        let mut word = 0;
                        for (index, (shift, bit, mask)) in
                            crate::c_layout::bit_chunks(start, width).enumerate()
                        {
                            word |= u64::from((bytes[index] & mask) >> bit) << shift;
                        }
                        words.push(word);
                    } else {
                        let name = leaf.scalar.ffi_type().to_string();
                        words.push(scalar_word(&name, bytes).ok_or_else(|| literal_error(expr))?);
                    }
                }
                continue;
            }
            if slot.ty.as_fixed_array().is_some() {
                let (bytes, _) = self
                    .fixed_storage_shape(&slot.ty)
                    .ok_or_else(|| literal_error(expr))?;
                append_inline_words(&mut words, &data[..bytes as usize]);
                continue;
            }
            words.push(scalar_word(&slot.ty.to_string(), data).ok_or_else(|| literal_error(expr))?);
        }
        let mut count = 0;
        for member in &union.members {
            count = count.max(
                self.state_words(member, 0)
                    .ok_or_else(|| literal_error(expr))?,
            );
        }
        words.resize(1 + count, 0);
        Ok(StateImage {
            slots: (0..words.len())
                .map(|i| StateSlot::value((i * 8) as i32, Type::named("uint64")))
                .collect(),
            bytes: words.into_iter().flat_map(u64::to_le_bytes).collect(),
            alignment: 8,
        })
    }

    fn state_words(&self, ty: &Type, depth: usize) -> Option<usize> {
        if depth > crate::limits::RECURSION_LIMIT {
            return None;
        }
        if let Type::Union(union) = ty {
            let mut words = 0;
            for member in &union.members {
                words = words.max(self.state_words(member, depth + 1)?);
            }
            return words.checked_add(1);
        }
        if ty.as_fixed_array().is_some() {
            let (bytes, _) = self.fixed_storage_shape(ty)?;
            return Some(bytes.div_ceil(8) as usize);
        }
        if ty == &Type::Unit {
            return Some(1);
        }
        let Type::Named(name, args) = ty else {
            return None;
        };
        if matches!(name.as_str(), "int128" | "uint128") {
            return Some(2);
        }
        if matches!(name.as_str(), "bool" | "float32" | "float64" | "ptr")
            || crate::integer::IntegerKind::from_runtime_type_name(name).is_some()
        {
            return Some(1);
        }
        let class = self.resolve_class_info(name)?;
        let substitutions = class
            .decl
            .type_params
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();
        let mut count = 0usize;
        for field in &class.decl.fields {
            if field.ty.indirect {
                return None;
            }
            let ty = substitute_type(&class.fields[&field.name].ty, &substitutions);
            count = count.checked_add(self.state_words(&ty, depth + 1)?)?;
        }
        Some(count)
    }

    fn array_image(
        &self,
        ty: &Type,
        element: &Type,
        len: usize,
        expr: &Expr,
    ) -> Result<StateImage> {
        use fixed_arrays::Initializer;
        let (bytes, alignment) = match fixed_arrays::initializer(expr) {
            Some(Initializer::Elements(items)) if items.len() == len && len > 0 => {
                let mut items = items.iter();
                let first = self.state_image(element, items.next().unwrap())?;
                let mut bytes = first.bytes;
                for item in items {
                    bytes.extend(self.state_image(element, item)?.bytes);
                }
                (bytes, first.alignment)
            }
            Some(Initializer::Repeat(item, count)) if count == len && len > 0 => {
                let item = self.state_image(element, item)?;
                if item
                    .bytes
                    .len()
                    .checked_mul(len)
                    .is_none_or(|n| n > i32::MAX as usize)
                {
                    return Err(literal_error(expr));
                }
                (item.bytes.repeat(len), item.alignment)
            }
            _ => return Err(literal_error(expr)),
        };
        Ok(StateImage {
            bytes,
            alignment,
            slots: vec![StateSlot::value(0, ty.clone())],
        })
    }

    pub(super) fn imported_state(&self, object: &Expr, field: &str) -> Option<&ConstantInfo> {
        let module = self.module_namespace(&self.infer_module_path(object)?)?;
        module
            .constants
            .get(field)
            .filter(|binding| binding.decl.mutable)
    }

    pub(super) fn is_state_place(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
    ) -> bool {
        match &grouped_expr(expr).kind {
            ExprKind::Name(name) => locals
                .get(name)
                .and_then(|b| b.borrow_origin.as_deref())
                .is_some_and(|origin| origin.starts_with("module state `")),
            ExprKind::Member { object, field } => {
                self.infer_module_path(object)
                    .and_then(|path| self.module_namespace(&path))
                    .and_then(|module| module.constants.get(field))
                    .is_some_and(|c| c.decl.mutable)
                    || self.is_state_place(object, locals)
            }
            ExprKind::Index { object, .. } | ExprKind::Slice { object, .. } => {
                self.is_state_place(object, locals)
            }
            _ => false,
        }
    }

    /// Check the generated setter on a local value. Like any other projection
    /// assignment, a C union write updates an owned snapshot of the module cell.
    pub(super) fn check_state_union_assignment(
        &self,
        assign: &AssignStmt,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<bool> {
        let AssignTarget::Member { object, field } = &assign.target else {
            return Ok(false);
        };
        if assign.op.is_some()
            || assign.mutable
            || assign.annotation.is_some()
            || !self.is_state_place(object, locals)
        {
            return Ok(false);
        }
        let ty = self.type_of_member_object_expr(object, locals)?;
        let Type::Named(name, _) = &ty else {
            return Ok(false);
        };
        if !self.resolve_class_info(name).is_some_and(|class| {
            class.decl.c_union.is_some() && class.methods.contains_key(&format!("__write_{field}"))
        }) {
            return Ok(false);
        }
        let snapshot = format!("\0module union {}:{}", assign.span.line, assign.span.column);
        let mut local = assign.clone();
        local.mutable = true;
        local.target = AssignTarget::Name(snapshot.clone());
        local.value = *object.clone();
        self.check_assign(&local, locals)?;
        let mut write = assign.clone();
        write.target = AssignTarget::Member {
            object: Box::new(Expr {
                kind: ExprKind::Name(snapshot.clone()),
                span: object.span,
            }),
            field: field.clone(),
        };
        let Some(Stmt::Expr(call)) = super::c_union_writes::setter_statement(&write) else {
            unreachable!()
        };
        let result = self.type_of_expr(&call.expr, locals);
        locals.remove(&snapshot);
        result.map(|_| true)
    }

    pub(super) fn reject_state_loan(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        if self.is_state_place(expr, locals) {
            return Err(Diagnostic::coded_at(
                "AU3030",
                expr.span,
                "module state cannot be the source of a safe view",
            )
            .with_help("copy the value into a local before borrowing it"));
        }
        Ok(())
    }
}
