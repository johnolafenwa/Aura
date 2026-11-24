//! Typed pointer operations keep pointee identity in MIR. The address and
//! displacement used by a memory instruction each occupy one machine word.
use super::*;

impl Lowerer<'_> {
    fn pointer_temp(&mut self, ty: Type, value: Rvalue) -> Operand {
        let target = self.new_typed_temp(ty);
        self.emit(Instruction::Assign {
            target: target.clone(),
            value,
        });
        Operand::Place(target)
    }

    fn pointer_cast(&mut self, value: Operand, ty: Type, span: crate::diag::Span) -> Operand {
        self.pointer_temp(ty.clone(), Rvalue::Cast { value, ty, span })
    }

    fn pointer_mem_call(&mut self, operation: &str, ty: Type, values: Vec<Operand>) -> Operand {
        let args = values
            .into_iter()
            .map(|value| MirArg {
                name: None,
                writeback_place: None,
                value,
            })
            .collect();
        self.pointer_temp(
            ty,
            Rvalue::Call {
                callee: CallTarget::Name(format!("mem::{operation}")),
                args,
                type_args: Default::default(),
            },
        )
    }

    fn pointer_class_layout(&self, element: &Type) -> Option<crate::c_layout::CStruct> {
        if element == &Type::named("int128") || element == &Type::named("uint128") {
            use crate::c_layout::{CField, CFieldKind, CStruct};
            let mut layout = CStruct {
                name: format!("#{element}"),
                union: false,
                layout_controls: Default::default(),
                fields: vec![CField {
                    name: "bits".into(),
                    kind: CFieldKind::Array(
                        Box::new(CFieldKind::Scalar(crate::ffi::FfiScalar::U64)),
                        2,
                    ),
                }],
            };
            layout.layout_controls.alignment = Some(16);
            return Some(layout);
        }
        if let Some((inner, length)) = element.as_fixed_array() {
            use crate::c_layout::{CField, CFieldKind, CStruct};
            let kind = if let Some(scalar) = crate::sema::ffi_scalar(inner) {
                CFieldKind::Scalar(scalar)
            } else {
                let layout = self.pointer_class_layout(inner)?;
                if inner.as_fixed_array().is_some() {
                    layout.fields.into_iter().next()?.kind
                } else {
                    CFieldKind::Struct(layout)
                }
            };
            return Some(CStruct {
                name: "#fixed".into(),
                union: false,
                layout_controls: Default::default(),
                fields: vec![CField {
                    name: "elements".into(),
                    kind: CFieldKind::Array(Box::new(kind), length),
                }],
            });
        }
        let Type::Named(name, _) = element else {
            return None;
        };
        let class = self.resolve_class_info(name)?;
        let namespace = self.module_namespace(&class.module_name);
        let classes = namespace.map_or(&self.program.classes, |namespace| &namespace.all_classes);
        crate::sema::ffi_struct_layout(element, classes)
    }

    fn pointer_element_size(&self, element: &Type) -> Option<usize> {
        if let Some(scalar) = crate::sema::ffi_scalar(element) {
            return Some(scalar.size());
        }
        self.pointer_class_layout(element)
            .map(|layout| layout.size())
    }

    pub(super) fn lower_raw_pointer_call(
        &mut self,
        expr: &Expr,
        object: &Expr,
        method: &str,
        args: &[Argument],
    ) -> Option<Operand> {
        if !matches!(
            method,
            "read" | "write" | "volatile_read" | "volatile_write" | "offset"
        ) {
            return None;
        }
        let pointer = self.infer_expr_type(object)?;
        let element = pointer.as_raw_pointer()?.clone();
        let receiver = self.lower_expr(object);
        let word = Type::named("uint64");
        let address = self.pointer_cast(receiver, word.clone(), expr.span);
        if method == "offset" {
            let stride = self.pointer_element_size(&element)?;
            let count =
                self.lower_expr_with_expected(&args.first()?.value, Some(&Type::named("int64")));
            let count = self.pointer_temp(Type::named("int64"), Rvalue::Use(count));
            let stride = self.pointer_temp(word.clone(), Rvalue::Use(Operand::Int(stride as u128)));
            let address =
                self.pointer_mem_call("__pointer_offset", word, vec![address, count, stride]);
            return Some(self.pointer_cast(address, pointer, expr.span));
        }
        let value = args
            .first()
            .map(|arg| self.lower_expr_with_expected(&arg.value, Some(&element)));
        self.lower_pointer_access(&element, address, method, value, expr.span)
    }

    fn lower_pointer_access(
        &mut self,
        element: &Type,
        address: Operand,
        method: &str,
        value: Option<Operand>,
        span: crate::diag::Span,
    ) -> Option<Operand> {
        if element.as_raw_pointer().is_some() {
            let word = Type::named("uint64");
            let mut operands = vec![address];
            if let Some(value) = value {
                operands.push(self.pointer_cast(value, word, span));
                return Some(self.pointer_mem_call(
                    &format!("{method}_uint64"),
                    Type::Unit,
                    operands,
                ));
            }
            let loaded = self.pointer_mem_call(&format!("{method}_uint64"), word, operands);
            return Some(self.pointer_cast(loaded, element.clone(), span));
        }
        if crate::sema::ffi_scalar(element).is_some() {
            let ty = if value.is_some() {
                Type::Unit
            } else {
                element.clone()
            };
            let mut operands = vec![address];
            operands.extend(value);
            return Some(self.pointer_mem_call(&format!("{method}_{element}"), ty, operands));
        }
        let layout = self.pointer_class_layout(element)?;
        let value = value.map(|v| self.pointer_temp(element.clone(), Rvalue::Use(v)));
        let result = if value.is_some() {
            Type::Unit
        } else {
            element.clone()
        };
        Some(self.pointer_temp(
            result,
            Rvalue::CPointerAccess {
                address,
                store: value,
                layout,
                volatile: method.starts_with("volatile_"),
            },
        ))
    }
}

impl Lowerer<'_> {
    pub(super) fn lower_raw_address(&mut self, place: &Expr) -> Operand {
        let addr = self.storage_address(place);
        let element = self
            .infer_expr_type(place)
            .expect("checked pointed-to type");
        self.pointer_cast(addr, Type::Named("ptr".into(), vec![element]), place.span)
    }

    fn storage_address(&mut self, place: &Expr) -> Operand {
        let word = Type::named("uint64");
        if let Some(key) = self.state_binding(place).map(super::module_state::cell_key) {
            return self.pointer_temp(word, Rvalue::ModuleAddress { key });
        }
        let place = grouped_mir_expr(place);
        match &place.kind {
            ExprKind::Index { object, index } => {
                let array = self.infer_expr_type(object).expect("checked fixed array");
                let element = array.as_fixed_array().expect("checked inline element").0;
                let stride = self
                    .pointer_element_size(element)
                    .expect("checked element size");
                let base = self.storage_address(object);
                let index_value = self.lower_index_domain_expr(index);
                let values = vec![
                    base,
                    index_value,
                    Operand::Int(index.span.line as u128),
                    Operand::Int(index.span.column as u128),
                    Operand::Int(stride as u128),
                ];
                self.pointer_temp(
                    word,
                    super::fixed_arrays::call(super::fixed_arrays::ADDRESS, &array, values),
                )
            }
            ExprKind::Member { object, field }
                if self
                    .infer_expr_type(object)
                    .and_then(|owner| self.pointer_class_layout(&owner))
                    .is_some() =>
            {
                let owner = self.infer_expr_type(object).expect("checked C field");
                let layout = self
                    .pointer_class_layout(&owner)
                    .expect("checked containing layout");
                let offset = layout
                    .fields
                    .iter()
                    .zip(layout.field_offsets())
                    .find(|(member, _)| &member.name == field)
                    .expect("checked C field")
                    .1;
                let base = self.storage_address(object);
                if offset == 0 {
                    return base;
                }
                self.pointer_mem_call(
                    "__pointer_offset",
                    word,
                    vec![base, Operand::Int(offset as u128), Operand::Int(1)],
                )
            }
            _ => {
                let root = self
                    .render_place_expr_option(place)
                    .expect("checked addressable place");
                let ty = self.infer_expr_type(place).expect("checked storage type");
                let layout = self.pointer_class_layout(&ty).expect("checked C storage");
                self.pointer_temp(
                    word,
                    Rvalue::CFieldAddress {
                        root,
                        layout,
                        offset: 0,
                    },
                )
            }
        }
    }
}
