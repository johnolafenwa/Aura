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
        let mut path = Vec::new();
        let mut base = place;
        while let ExprKind::Member { object, field } = &base.kind {
            path.push(field.as_str());
            base = object;
        }
        let root = self
            .render_place_expr_option(base)
            .expect("checked addressable place");
        let root_ty = self.infer_expr_type(base).expect("checked storage type");
        let layout = self
            .pointer_class_layout(&root_ty)
            .expect("checked C class");
        let mut offset = 0;
        let mut current = &layout;
        for name in path.iter().rev() {
            let (field, delta) = current
                .fields
                .iter()
                .zip(current.field_offsets())
                .find(|(field, _)| field.name == **name)
                .expect("checked C field");
            offset += delta;
            if let crate::c_layout::CFieldKind::Struct(inner) = &field.kind {
                current = inner;
            }
        }
        let element = self.infer_expr_type(place).expect("checked field type");
        self.pointer_temp(
            Type::Named("ptr".into(), vec![element]),
            Rvalue::CFieldAddress {
                root,
                layout,
                offset,
            },
        )
    }
}
