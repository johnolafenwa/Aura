//! Wide integers in a freestanding call frame occupy two words, low word first.
//! Reuse the plain-record calling convention for returns, fields and loans.
use super::*;
use aura_core::integer_bits::BitMethod;

pub(crate) const SYMBOL: &str = "aura.systems.integer128";

pub(super) fn install_layouts(module: &mut MirModule) {
    for name in ["int128", "uint128"] {
        module.classes.push(MirClass {
            c_layout: None,
            name: name.into(),
            type_params: Vec::new(),
            fields: ["low", "high"]
                .map(|field| crate::mir::MirClassField {
                    name: field.into(),
                    ty: Type::named("uint64"),
                })
                .to_vec(),
            methods: Vec::new(),
            copy: true,
        });
    }
}

pub(super) fn signed(ty: &DirectType) -> Option<bool> {
    match ty {
        DirectType::PlainClass(class) if class.class_name == "int128" => Some(true),
        DirectType::PlainClass(class) if class.class_name == "uint128" => Some(false),
        _ => None,
    }
}

impl FunctionCompiler<'_> {
    pub(super) fn systems_call_span(&self) -> Option<Span> {
        self.builder
            .source_location()
            .map(|(line, column)| Span::new(line as usize, column as usize))
    }

    pub(super) fn wide_math(
        &mut self,
        op: FunctionOp,
        args: &[MirArg],
    ) -> std::result::Result<Option<ValueRef>, String> {
        let Some(ty) = args
            .iter()
            .filter_map(|arg| match &arg.value {
                Operand::Place(place) | Operand::MovePlace(place) => self.type_of_place(place).ok(),
                _ => None,
            })
            .find(|ty| signed(ty).is_some())
        else {
            return Ok(None);
        };
        if !matches!(op, FunctionOp::Abs | FunctionOp::Min | FunctionOp::Max) {
            return Ok(None);
        }
        let left = self.load_operand_for_target(&args[0].value, &ty)?;
        let lhs = self.wide_words(&left)?;
        let is_signed = signed(&ty).unwrap();
        let values = match op {
            FunctionOp::Abs => self.wide_operation(27, is_signed, lhs, lhs, None)?,
            _ => {
                let right = self.load_operand_for_target(&args[1].value, &ty)?;
                let rhs = self.wide_words(&right)?;
                let cmp = self.wide_operation(
                    if op == FunctionOp::Min { 19 } else { 21 },
                    is_signed,
                    lhs,
                    rhs,
                    None,
                )?[0];
                vec![
                    self.builder.select(cmp, lhs[0], rhs[0]),
                    self.builder.select(cmp, lhs[1], rhs[1]),
                ]
            }
        };
        Ok(Some(ValueRef { values, ty }))
    }

    pub(super) fn wide_literal(&mut self, bits: u128, ty: &DirectType) -> ValueRef {
        let low = self.builder.iconst(types::I64, bits as i64);
        let high = self.builder.iconst(types::I64, (bits >> 64) as i64);
        ValueRef {
            values: vec![low, high],
            ty: ty.clone(),
        }
    }

    fn wide_words(&mut self, value: &ValueRef) -> std::result::Result<[Val; 2], String> {
        match value.values.as_slice() {
            [low, high] if signed(&value.ty).is_some() => Ok([*low, *high]),
            [low] if value.ty.scalar_kind().is_some_and(|kind| kind.is_integer()) => {
                let high = if value
                    .ty
                    .scalar_kind()
                    .expect("integer")
                    .is_unsigned_integer()
                {
                    self.builder.iconst(types::I64, 0)
                } else {
                    let shift = self.builder.iconst(types::I64, 63);
                    self.builder.sshr(*low, shift)
                };
                Ok([*low, high])
            }
            _ => Err(format!("expected an inline integer, found {:?}", value.ty)),
        }
    }

    fn check_wide_trap(&mut self, code: Val, span: Option<Span>) {
        let fail = self.builder.create_block();
        let okay = self.builder.create_block();
        self.builder.brif(code, fail, &[], okay, &[]);
        self.builder.switch_to_block(fail);
        self.builder.seal_block(fail);
        self.builder.set_cold_block(fail);
        let (line, column) = self.span_values(span.or_else(|| self.systems_call_span()));
        self.call_trap_hook_with(code, line, column);
        self.builder.trap(TrapCode::IntegerOverflow);
        self.builder.switch_to_block(okay);
        self.builder.seal_block(okay);
    }

    fn wide_operation(
        &mut self,
        opcode: i64,
        is_signed: bool,
        lhs: [Val; 2],
        rhs: [Val; 2],
        span: Option<Span>,
    ) -> std::result::Result<Vec<Val>, String> {
        let mut signature = Signature::new(CallConv::Aura);
        signature.params = vec![types::I64; 6];
        signature.returns = vec![types::I64; 3];
        let handle = self
            .builder
            .module()
            .declare_function(SYMBOL, Linkage::Import, &signature)?;
        let callee = self.builder.import_function(handle);
        let op = self.builder.iconst(types::I64, opcode);
        let sign = self.builder.iconst(types::I64, i64::from(is_signed));
        let result = self
            .builder
            .call(callee, &[op, sign, lhs[0], lhs[1], rhs[0], rhs[1]])
            .results;
        self.check_wide_trap(result[2], span);
        Ok(result[..2].to_vec())
    }

    pub(super) fn wide_binary(
        &mut self,
        op: BinaryOp,
        left: ValueRef,
        right: ValueRef,
        span: Option<Span>,
    ) -> std::result::Result<ValueRef, String> {
        let ty = if signed(&left.ty).is_some() {
            left.ty.clone()
        } else {
            right.ty.clone()
        };
        let sign = signed(&ty).expect("inline wide operand");
        let opcode = match op {
            BinaryOp::Add => 0,
            BinaryOp::Sub => 1,
            BinaryOp::Mul => 2,
            BinaryOp::Div => 22,
            BinaryOp::FloorDiv => 3,
            BinaryOp::Mod => 4,
            BinaryOp::BitAnd => 5,
            BinaryOp::BitOr => 6,
            BinaryOp::BitXor => 7,
            BinaryOp::Shl => 8,
            BinaryOp::Shr => 9,
            BinaryOp::Eq => 16,
            BinaryOp::NotEq => 17,
            BinaryOp::Less => 18,
            BinaryOp::LessEq => 19,
            BinaryOp::Greater => 20,
            BinaryOp::GreaterEq => 21,
            _ => return Err(format!("unsupported wide integer operator {op:?}")),
        };
        let lhs = self.wide_words(&left)?;
        let rhs = self.wide_words(&right)?;
        let mut values = self.wide_operation(opcode, sign, lhs, rhs, span)?;
        if is_comparison_op(op) {
            values.truncate(1);
            Ok(ValueRef {
                values,
                ty: DirectType::Scalar(ScalarKind::Bool),
            })
        } else {
            Ok(ValueRef { values, ty })
        }
    }

    pub(super) fn wide_unary(
        &mut self,
        op: UnaryOp,
        value: ValueRef,
        span: Option<Span>,
    ) -> std::result::Result<ValueRef, String> {
        let (other, binary) = match op {
            UnaryOp::Neg => (0, BinaryOp::Sub),
            UnaryOp::BitNot => (u128::MAX, BinaryOp::BitXor),
            _ => return Err("invalid wide integer unary operation".into()),
        };
        let constant = self.wide_literal(other, &value.ty);
        self.wide_binary(binary, constant, value, span)
    }

    pub(super) fn wide_bit_method(
        &mut self,
        method: BitMethod,
        object: ValueRef,
        count: Val,
    ) -> std::result::Result<ValueRef, String> {
        let shift = self.builder.iconst(types::I64, 63);
        let high = self.builder.sshr(count, shift);
        let lhs = self.wide_words(&object)?;
        let mut values = self.wide_operation(
            10 + method as i64,
            signed(&object.ty).unwrap(),
            lhs,
            [count, high],
            None,
        )?;
        let ty = if method.counts() {
            values.truncate(1);
            DirectType::Scalar(ScalarKind::Int64)
        } else {
            object.ty
        };
        Ok(ValueRef { values, ty })
    }

    pub(super) fn wide_float(
        &mut self,
        value: &ValueRef,
        kind: ScalarKind,
        exact: bool,
        span: Option<Span>,
    ) -> std::result::Result<ValueRef, String> {
        let opcode = if !exact {
            26
        } else if kind == ScalarKind::Float32 {
            25
        } else {
            24
        };
        let words = self.wide_words(value)?;
        let zero = self.builder.iconst(types::I64, 0);
        let bits =
            self.wide_operation(opcode, signed(&value.ty).unwrap(), words, [zero; 2], span)?;
        let float = self.builder.bitcast(types::F64, bits[0]);
        Ok(ValueRef {
            values: vec![float],
            ty: DirectType::Scalar(kind),
        })
    }

    pub(super) fn wide_cast(
        &mut self,
        value: ValueRef,
        target: &DirectType,
        span: Option<Span>,
    ) -> std::result::Result<ValueRef, String> {
        if value.ty.scalar_kind().is_some_and(|k| k.is_float()) {
            let bits = self.builder.bitcast(types::I64, value.values[0]);
            let zero = self.builder.iconst(types::I64, 0);
            let values =
                self.wide_operation(23, signed(target).unwrap(), [bits, zero], [zero; 2], span)?;
            return Ok(ValueRef {
                values,
                ty: target.clone(),
            });
        }
        if let Some(kind) = target.scalar_kind().filter(|k| k.is_float()) {
            return self.wide_float(&value, kind, true, span);
        }
        let words = self.wide_words(&value)?;
        let sign = signed(&value.ty).unwrap_or_else(|| {
            value
                .ty
                .scalar_kind()
                .is_some_and(|k| !k.is_unsigned_integer())
        });
        let zero = self.builder.iconst(types::I64, 0);
        let bad = if let Some(target_sign) = signed(target) {
            if sign == target_sign {
                None
            } else {
                Some(self.builder.icmp_imm(IntCC::SignedLessThan, words[1], 0))
            }
        } else {
            let kind = target
                .scalar_kind()
                .ok_or("invalid wide integer cast target")?;
            let width = kind.integer_bits().ok_or("expected integer cast target")?;
            let back = if kind.is_unsigned_integer() {
                zero
            } else {
                let shift = self.builder.iconst(types::I64, 63);
                self.builder.sshr(words[0], shift)
            };
            let mut bad = self.builder.icmp(IntCC::NotEqual, words[1], back);
            if !sign && !kind.is_unsigned_integer() {
                let negative = self.builder.icmp_imm(IntCC::SignedLessThan, words[0], 0);
                bad = self.builder.bor(bad, negative);
            }
            if width < 64 {
                let shift = self.builder.iconst(types::I64, i64::from(64 - width));
                let low = self.builder.ishl(words[0], shift);
                let back = if kind.is_unsigned_integer() {
                    self.builder.ushr(low, shift)
                } else {
                    self.builder.sshr(low, shift)
                };
                let narrow = self.builder.icmp(IntCC::NotEqual, back, words[0]);
                bad = self.builder.bor(bad, narrow);
            }
            Some(bad)
        };
        if let Some(bad) = bad {
            let trap = self.builder.iconst(types::I64, freestanding::TRAP_CAST);
            let code = self.builder.select(bad, trap, zero);
            self.check_wide_trap(code, span);
        }
        let values = if signed(target).is_some() {
            words.to_vec()
        } else {
            vec![words[0]]
        };
        Ok(ValueRef {
            values,
            ty: target.clone(),
        })
    }
}
