//! Native fixed arrays occupy one contiguous, naturally aligned LLVM value.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FixedArrayLayout {
    pub(super) ty: Type,
    pub(super) bytes: u32,
    pub(super) align: u8,
    element: Box<DirectType>,
    count: u32,
}

impl FixedArrayLayout {
    pub(super) fn storage(&self) -> ScalarType {
        ScalarType::Inline {
            bytes: self.bytes,
            align: self.align,
        }
    }
}

pub(super) fn direct_layout(
    ty: &Type,
    classes: &HashMap<String, MirClass>,
    visiting: &mut BTreeSet<String>,
) -> Option<DirectType> {
    let (element, count) = ty.as_fixed_array()?;
    let element = direct_type_inner(element, classes, visiting)?;
    let (stride, align) = storage(&element)?;
    let count = u32::try_from(count).ok()?;
    let bytes = stride.checked_mul(count)?;
    if bytes == 0 || bytes > i32::MAX as u32 {
        return None;
    }
    Some(DirectType::Fixed(FixedArrayLayout {
        ty: ty.clone(),
        bytes,
        align,
        element: Box::new(element),
        count,
    }))
}

fn storage(ty: &DirectType) -> Option<(u32, u8)> {
    match ty {
        DirectType::Scalar(k) => {
            let size = match k {
                ScalarKind::Int8 | ScalarKind::Uint8 | ScalarKind::Bool => 1,
                ScalarKind::Int16 | ScalarKind::Uint16 => 2,
                ScalarKind::Int32 | ScalarKind::Uint32 | ScalarKind::Float32 => 4,
                ScalarKind::Int64 | ScalarKind::Uint64 | ScalarKind::Float64 => 8,
                ScalarKind::Unit => return None,
            };
            Some((size, size as u8))
        }
        DirectType::Fixed(layout) => Some((layout.bytes, layout.align)),
        DirectType::PlainClass(class) => {
            let mut size: u32 = 0;
            let mut align = 1;
            for field in &class.fields {
                let (bytes, field_align) = storage(&field.ty)?;
                align = align.max(field_align);
                size = size
                    .checked_next_multiple_of(u32::from(field_align))?
                    .checked_add(bytes)?;
            }
            Some((size.checked_next_multiple_of(u32::from(align))?, align))
        }
        _ => None,
    }
}

impl FixedArrayLayout {
    fn descriptor(&self) -> Result<Vec<u8>, String> {
        use crate::c_layout::{CField, CFieldKind, CStruct};
        use crate::ffi::FfiScalar;
        fn kind(ty: &DirectType) -> CFieldKind {
            match ty {
                DirectType::Fixed(layout) => {
                    CFieldKind::Array(Box::new(kind(&layout.element)), layout.count as usize)
                }
                DirectType::PlainClass(class) => CFieldKind::Struct(CStruct {
                    name: class.class_name.clone(),
                    union: false,
                    layout_controls: Default::default(),
                    fields: class
                        .fields
                        .iter()
                        .map(|field| CField {
                            name: field.name.clone(),
                            kind: kind(&field.ty),
                        })
                        .collect(),
                }),
                DirectType::Scalar(k) => CFieldKind::Scalar(match k {
                    ScalarKind::Int8 => FfiScalar::I8,
                    ScalarKind::Int16 => FfiScalar::I16,
                    ScalarKind::Int32 => FfiScalar::I32,
                    ScalarKind::Int64 => FfiScalar::I64,
                    ScalarKind::Uint8 => FfiScalar::U8,
                    ScalarKind::Uint16 => FfiScalar::U16,
                    ScalarKind::Uint32 => FfiScalar::U32,
                    ScalarKind::Uint64 => FfiScalar::U64,
                    ScalarKind::Float32 => FfiScalar::F32,
                    ScalarKind::Float64 => FfiScalar::F64,
                    ScalarKind::Bool => FfiScalar::Bool,
                    ScalarKind::Unit => unreachable!("unit has no array storage"),
                }),
                _ => unreachable!("array elements must have inline storage"),
            }
        }
        let layout = CStruct {
            name: "fixed".to_owned(),
            union: false,
            layout_controls: Default::default(),
            fields: vec![CField {
                name: "elements".to_owned(),
                kind: CFieldKind::Array(Box::new(kind(&self.element)), self.count as usize),
            }],
        };
        serde_json::to_vec(&layout).map_err(|error| error.to_string())
    }

    fn slot(&self, builder: &mut dyn FunctionBackend) -> Val {
        let slot = builder.create_sized_stack_slot(self.bytes, self.align.trailing_zeros() as u8);
        builder.stack_addr(types::I64, slot, 0)
    }

    pub(super) fn convert(
        &self,
        builder: &mut dyn FunctionBackend,
        function: FuncRef,
        descriptor: (Val, Val),
        value: Val,
        boxing: bool,
    ) -> Val {
        let slot = self.slot(builder);
        let (text, len) = descriptor;
        if boxing {
            builder.store(MemFlags::trusted(), value, slot, 0);
            builder.call(function, &[text, len, slot]).results[0]
        } else {
            builder.call(function, &[value, text, len, slot]);
            builder.load(self.storage(), MemFlags::trusted(), slot, 0)
        }
    }

    pub(super) fn thunk_convert(
        &self,
        codegen: &mut NativeCodegen<'_>,
        builder: &mut dyn FunctionBackend,
        value: Val,
        boxing: bool,
    ) -> Result<Val, String> {
        let desc = thunk_string_constant(codegen, builder, &self.descriptor()?)?;
        let func = if boxing {
            RuntimeFn::FixedBox
        } else {
            RuntimeFn::FixedUnbox
        };
        let func = builder.import_function(codegen.runtime[func]);
        Ok(self.convert(builder, func, desc, value, boxing))
    }
}

impl FunctionCompiler<'_> {
    pub(super) fn fixed_convert(
        &mut self,
        layout: &FixedArrayLayout,
        value: Val,
        boxing: bool,
    ) -> Result<Val, String> {
        let desc = self.string_constant(&layout.descriptor()?)?;
        let func = if boxing {
            RuntimeFn::FixedBox
        } else {
            RuntimeFn::FixedUnbox
        };
        Ok(layout.convert(self.builder, self.runtime[func], desc, value, boxing))
    }

    /// The address of a local array or a fixed field flattened into a local's variables.
    /// The caller must resolve a view or module cell before using this local-only lookup.
    pub(super) fn fixed_local_address(&mut self, place: &str) -> Option<(Val, FixedArrayLayout)> {
        let mut path = place.split('.');
        let root = path.next()?;
        if self.view_places.contains_key(root) {
            return None;
        }
        let mut ty = self.variable_types.get(root)?.clone();
        let mut start = 0;
        for field in path {
            let (offset, _, member) = ty.field_slice(field)?;
            start += offset;
            ty = member;
        }
        let DirectType::Fixed(layout) = ty else {
            return None;
        };
        let var = *self.variables.get(root)?.get(start)?;
        Some((self.builder.var_addr(var), layout))
    }

    fn fixed_parts(
        &mut self,
        ty: &DirectType,
        address: Val,
        offset: u32,
        dest: &mut Vec<Val>,
        src: Option<&[Val]>,
    ) -> Result<(), String> {
        let flags = MemFlags::trusted();
        if let DirectType::PlainClass(class) = ty {
            let mut at = offset;
            for field in &class.fields {
                let (bytes, align) = storage(&field.ty).ok_or("field has no inline storage")?;
                at = at.next_multiple_of(u32::from(align));
                self.fixed_parts(&field.ty, address, at, dest, src)?;
                at += bytes;
            }
            return Ok(());
        }
        let width = match ty {
            DirectType::Fixed(layout) => layout.storage(),
            DirectType::Scalar(k) => match k {
                ScalarKind::Int8 | ScalarKind::Uint8 | ScalarKind::Bool => types::I8,
                ScalarKind::Int16 | ScalarKind::Uint16 => types::I16,
                ScalarKind::Int32 | ScalarKind::Uint32 => types::I32,
                ScalarKind::Int64 | ScalarKind::Uint64 => types::I64,
                ScalarKind::Float32 => types::F32,
                ScalarKind::Float64 => types::F64,
                ScalarKind::Unit => return Err("unit has no storage".to_owned()),
            },
            _ => return Err("array element has no inline storage".to_owned()),
        };
        let store = src.and_then(|values| values.get(dest.len())).copied();
        let offset = i32::try_from(offset)
            .map_err(|_| "fixed array field offset exceeds the target limit")?;
        let word = if let Some(word) = store {
            let packed = match width {
                ScalarType::Inline { .. } => word,
                _ => foreign::callback_narrow(self.builder, width, word),
            };
            self.builder.store(flags, packed, address, offset);
            word
        } else {
            let raw = self.builder.load(width, flags, address, offset);
            match (ty, width) {
                (_, ScalarType::Inline { .. } | ScalarType::I64 | ScalarType::F64) => raw,
                (_, ScalarType::F32) => self.builder.fpromote(types::F64, raw),
                (DirectType::Scalar(ScalarKind::Int8), _) => {
                    self.builder.sload8(flags, address, offset)
                }
                (DirectType::Scalar(ScalarKind::Int16), _) => {
                    self.builder.sload16(flags, address, offset)
                }
                (DirectType::Scalar(ScalarKind::Int32), _) => {
                    self.builder.sload32(flags, address, offset)
                }
                _ => self.builder.uextend(types::I64, raw),
            }
        };
        dest.push(word);
        Ok(())
    }

    fn fixed_store_element(
        &mut self,
        layout: &FixedArrayLayout,
        address: Val,
        value: &ValueRef,
    ) -> Result<(), String> {
        let typed = self.coerce_value(value.clone(), &layout.element)?;
        self.fixed_parts(
            &layout.element,
            address,
            0,
            &mut Vec::new(),
            Some(&typed.values),
        )
    }

    pub(super) fn fixed_element_address(
        &mut self,
        layout: &FixedArrayLayout,
        base: Val,
        index: Val,
        line: Val,
        column: Val,
    ) -> Val {
        let n = i64::from(layout.count);
        if !self
            .builder
            .integer_constant(index)
            .is_some_and(|i| (0..n).contains(&i))
        {
            let bad = self.builder.create_block();
            let good = self.builder.create_block();
            let valid = self.builder.icmp_imm(IntCC::UnsignedLessThan, index, n);
            self.builder.brif(valid, good, &[], bad, &[]);
            self.builder.switch_to_block(bad);
            self.builder.seal_block(bad);
            if self.freestanding.is_some() {
                self.call_trap_hook(9, line, column);
            } else {
                let n = self.builder.iconst(types::I64, n);
                self.builder.call(
                    self.runtime[RuntimeFn::FixedBounds],
                    &[index, n, line, column],
                );
            }
            self.builder.trap(TrapCode::User(9));
            self.builder.switch_to_block(good);
            self.builder.seal_block(good);
        }
        self.fixed_offset(layout, base, index)
    }

    fn fixed_offset(&mut self, layout: &FixedArrayLayout, base: Val, index: Val) -> Val {
        let stride = self
            .builder
            .iconst(types::I64, i64::from(layout.bytes / layout.count));
        let offset = self.builder.imul(index, stride);
        self.builder.iadd(base, offset)
    }

    pub(super) fn compile_fixed(
        &mut self,
        op: &str,
        args: &[MirArg],
        types: &BTreeMap<String, Type>,
    ) -> Result<ValueRef, String> {
        use crate::mir::fixed_arrays::{LITERAL, READ, REPEAT, WRITE};
        let ty = types.get("array").ok_or("missing fixed array type")?;
        let Some(DirectType::Fixed(layout)) = direct_type(ty, &self.classes) else {
            return Err(format!("cannot lay out `{ty}` inline"));
        };
        if op == crate::mir::fixed_arrays::ADDRESS && args.len() == 5 {
            let address = self.load_operand(&args[0].value)?.values[0];
            let index = self.load_operand(&args[1].value)?.values[0];
            let line = self.load_operand(&args[2].value)?.values[0];
            let column = self.load_operand(&args[3].value)?.values[0];
            let at = self.fixed_element_address(&layout, address, index, line, column);
            return Ok(ValueRef { values: vec![at], ty: DirectType::Scalar(ScalarKind::Uint64) });
        }
        let base = layout.slot(self.builder);
        let storage = layout.storage();
        match op {
            LITERAL if args.len() == layout.count as usize => {
                for (index, arg) in args.iter().enumerate() {
                    let value = self.load_operand_for_target(&arg.value, &layout.element)?;
                    let ix = self.builder.iconst(types::I64, index as i64);
                    let at = self.fixed_offset(&layout, base, ix);
                    self.fixed_store_element(&layout, at, &value)?;
                }
            }
            REPEAT if args.len() == 1 => {
                let value = self.load_operand_for_target(&args[0].value, &layout.element)?;
                let head = self.builder.create_block();
                let done = self.builder.create_block();
                self.builder.append_block_param(head, types::I64);
                let zero = self.builder.iconst(types::I64, 0);
                self.builder.jump(head, &[zero]);
                self.builder.switch_to_block(head);
                let ix = self.builder.block_params(head)[0];
                let at = self.fixed_offset(&layout, base, ix);
                self.fixed_store_element(&layout, at, &value)?;
                let next = self.builder.iadd_imm(ix, 1);
                let more =
                    self.builder
                        .icmp_imm(IntCC::UnsignedLessThan, next, i64::from(layout.count));
                self.builder.brif(more, head, &[next], done, &[]);
                self.builder.seal_block(head);
                self.builder.switch_to_block(done);
                self.builder.seal_block(done);
            }
            READ | WRITE if args.len() == if op == READ { 4 } else { 5 } => {
                let original = if op == READ {
                    match &args[0].value {
                        Operand::Place(place) => self
                            .fixed_local_address(place)
                            .filter(|(_, actual)| actual == &layout)
                            .map(|(base, _)| base),
                        _ => None,
                    }
                } else {
                    None
                };
                let source = if let Some(original) = original {
                    original
                } else {
                    let array = self.load_operand(&args[0].value)?;
                    if array.ty != DirectType::Fixed(layout.clone()) {
                        return Err(format!("array operand has the wrong type for {op}"));
                    }
                    self.builder
                        .store(MemFlags::trusted(), array.values[0], base, 0);
                    base
                };
                let ix = self.load_operand(&args[1].value)?.values[0];
                let line = self.load_operand(&args[2].value)?.values[0];
                let column = self.load_operand(&args[3].value)?.values[0];
                let at = self.fixed_element_address(&layout, source, ix, line, column);
                if op == READ {
                    let mut values = Vec::new();
                    self.fixed_parts(&layout.element, at, 0, &mut values, None)?;
                    return Ok(ValueRef {
                        values,
                        ty: *layout.element,
                    });
                }
                let value = self.load_operand_for_target(&args[4].value, &layout.element)?;
                self.fixed_store_element(&layout, at, &value)?;
            }
            _ => return Err(format!("invalid arguments to {op}")),
        }
        Ok(ValueRef {
            values: vec![self.builder.load(storage, MemFlags::trusted(), base, 0)],
            ty: DirectType::Fixed(layout),
        })
    }
}
