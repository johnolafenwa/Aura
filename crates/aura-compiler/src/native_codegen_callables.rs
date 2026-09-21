//! Inline callable shapes for the direct backend (checkpoint Q15 A / Q16 A).
//!
//! A callable value is four words: a descriptor word naming the value's
//! shape and three environment words. The shape is the lowered function plus
//! its leading capture parameters, so the descriptor knows how to call the
//! function with the captures, how to release and retain the captures'
//! handles, and how to box the value into the runtime's `Value::Function`
//! at a boundary. Every shape gets one descriptor data object and four
//! adapter functions in the module.

use super::*;

/// One callable shape: a lowered function constructed with a fixed number of
/// leading capture parameters.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(super) struct CallableShapeKey {
    pub(super) function: String,
    pub(super) captures: usize,
    pub(super) consuming: bool,
}

/// The module objects that implement one shape.
#[derive(Clone, Debug)]
pub(super) struct CallableShape {
    pub(super) descriptor: DataId,
    pub(super) invoke: FuncId,
    pub(super) drop: FuncId,
    pub(super) retain: FuncId,
    pub(super) box_value: FuncId,
    /// The static signature the value carries when boxed.
    pub(super) signature: Type,
    /// Direct types of the leading capture parameters.
    pub(super) capture_types: Vec<DirectType>,
    /// Total environment words; inline when at most three.
    pub(super) env_words: usize,
}

impl CallableShape {
    pub(super) fn inline(&self) -> bool {
        self.env_words <= CALLABLE_ENVIRONMENT_WORDS
    }
}

/// Descriptor layout in eight-byte words.
pub(super) const DESCRIPTOR_INVOKE: i32 = 0;
pub(super) const DESCRIPTOR_DROP: i32 = 8;
pub(super) const DESCRIPTOR_RETAIN: i32 = 16;
pub(super) const DESCRIPTOR_BOX: i32 = 24;
pub(super) const DESCRIPTOR_ENV_WORDS: i32 = 32;
pub(super) const DESCRIPTOR_INLINE: i32 = 40;
const DESCRIPTOR_BYTES: usize = 48;

/// `drop(callable_ptr)`, `retain(callable_ptr)`.
pub(super) fn callable_unary_signature(call_conv: CallConv) -> Signature {
    let mut signature = Signature::new(call_conv);
    signature.params.push(AbiParam::new(types::I64));
    signature
}

/// `box(callable_ptr) -> handle`.
pub(super) fn callable_box_signature(call_conv: CallConv) -> Signature {
    let mut signature = Signature::new(call_conv);
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    signature
}

/// `invoke(callable_ptr, supplied_mask, public args...) -> (return words,
/// public writeback words...)`: the shape's public direct signature.
pub(super) fn callable_invoke_signature(
    function: &MirFunction,
    captures: usize,
    param_types: &[DirectType],
    return_ty: &DirectType,
    call_conv: CallConv,
) -> std::result::Result<Signature, String> {
    if param_types.len() != function.params.len() || captures > function.params.len() {
        return Err(format!(
            "direct backend callable shape for `{}` disagrees with its parameters",
            function.name
        ));
    }
    let mut signature = Signature::new(call_conv);
    signature.params.push(AbiParam::new(types::I64));
    signature.params.push(AbiParam::new(types::I64));
    for ty in &param_types[captures..] {
        for abi in ty.abi_types() {
            signature.params.push(AbiParam::new(abi));
        }
    }
    for abi in return_ty.abi_types() {
        signature.returns.push(AbiParam::new(abi));
    }
    for (param, ty) in function.params.iter().zip(param_types).skip(captures) {
        if param.passing == MirReceiverKind::BorrowMut {
            for abi in ty.abi_types() {
                signature.returns.push(AbiParam::new(abi));
            }
        }
    }
    Ok(signature)
}

/// Every shape a module constructs: `Closure` rvalues and `Function`
/// operands, found by walking the functions' serialized form so no
/// instruction or operand variant is missed.
pub(super) fn collect_callable_shapes(module: &MirModule) -> Vec<(CallableShapeKey, Type)> {
    let mut shapes = Vec::new();
    let mut seen = HashSet::new();
    for function in module.functions.iter().chain(module.top_level.iter()) {
        let Ok(value) = serde_json::to_value(function) else {
            continue;
        };
        collect_shapes_in_json(&value, &mut shapes, &mut seen);
    }
    shapes
}

fn collect_shapes_in_json(
    value: &serde_json::Value,
    shapes: &mut Vec<(CallableShapeKey, Type)>,
    seen: &mut HashSet<CallableShapeKey>,
) {
    match value {
        serde_json::Value::Object(fields) => {
            if let Some(closure) = fields.get("Closure").and_then(|v| v.as_object()) {
                if let (Some(function), Some(captures), Some(signature)) = (
                    closure.get("function").and_then(|v| v.as_str()),
                    closure.get("captures").and_then(|v| v.as_array()),
                    closure.get("signature"),
                ) {
                    let key = CallableShapeKey {
                        function: function.to_string(),
                        captures: captures.len(),
                        consuming: closure
                            .get("consuming")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                    };
                    if let Ok(signature) = serde_json::from_value::<Type>(signature.clone()) {
                        if seen.insert(key.clone()) {
                            shapes.push((key, signature));
                        }
                    }
                }
            }
            if let Some(operand) = fields.get("Function").and_then(|v| v.as_object()) {
                if let (Some(name), Some(signature)) = (
                    operand.get("name").and_then(|v| v.as_str()),
                    operand.get("signature"),
                ) {
                    let key = CallableShapeKey {
                        function: name.to_string(),
                        captures: 0,
                        consuming: false,
                    };
                    if let Ok(signature) = serde_json::from_value::<Type>(signature.clone()) {
                        if seen.insert(key.clone()) {
                            shapes.push((key, signature));
                        }
                    }
                }
            }
            for field in fields.values() {
                collect_shapes_in_json(field, shapes, seen);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_shapes_in_json(item, shapes, seen);
            }
        }
        _ => {}
    }
}

/// Declares the descriptor data and the four adapter functions of every
/// shape the module constructs.
pub(super) fn declare_callable_shapes(
    module: &MirModule,
    object: &mut ObjectModule,
    function_param_types: &HashMap<String, Vec<DirectType>>,
    function_return_types: &HashMap<String, DirectType>,
    call_conv: CallConv,
) -> std::result::Result<HashMap<CallableShapeKey, CallableShape>, String> {
    let mut shapes = HashMap::new();
    for (index, (key, signature)) in collect_callable_shapes(module).into_iter().enumerate() {
        let function = module
            .functions
            .iter()
            .chain(module.top_level.iter())
            .find(|function| function.name == key.function)
            .ok_or_else(|| {
                format!(
                    "direct backend cannot find lowered function `{}` for a callable shape",
                    key.function
                )
            })?;
        let param_types = function_param_types
            .get(&key.function)
            .cloned()
            .unwrap_or_default();
        let return_ty = function_return_types
            .get(&key.function)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "direct backend does not know return type for `{}`",
                    key.function
                )
            })?;
        if key.captures > param_types.len() {
            return Err(format!(
                "direct backend callable shape for `{}` captures {} values but the function takes {}",
                key.function,
                key.captures,
                param_types.len()
            ));
        }
        let capture_types = param_types[..key.captures].to_vec();
        let env_words = capture_types.iter().map(DirectType::value_count).sum();
        let stem = format!("aura_callable_{index}");
        let descriptor = try_or_string_error!(
            object.declare_data(&format!("{stem}_descriptor"), Linkage::Local, false, false),
            "failed to declare callable descriptor: {}"
        );
        let invoke_signature =
            callable_invoke_signature(function, key.captures, &param_types, &return_ty, call_conv)?;
        let invoke = try_or_string_error!(
            object.declare_function(&format!("{stem}_invoke"), Linkage::Local, &invoke_signature),
            "failed to declare callable invoke adapter: {}"
        );
        let drop = try_or_string_error!(
            object.declare_function(
                &format!("{stem}_drop"),
                Linkage::Local,
                &callable_unary_signature(call_conv)
            ),
            "failed to declare callable drop adapter: {}"
        );
        let retain = try_or_string_error!(
            object.declare_function(
                &format!("{stem}_retain"),
                Linkage::Local,
                &callable_unary_signature(call_conv)
            ),
            "failed to declare callable retain adapter: {}"
        );
        let box_value = try_or_string_error!(
            object.declare_function(
                &format!("{stem}_box"),
                Linkage::Local,
                &callable_box_signature(call_conv)
            ),
            "failed to declare callable box adapter: {}"
        );
        shapes.insert(
            key,
            CallableShape {
                descriptor,
                invoke,
                drop,
                retain,
                box_value,
                signature,
                capture_types,
                env_words,
            },
        );
    }
    Ok(shapes)
}

/// An environment word as a direct value of `abi`, and back.
fn value_from_word(builder: &mut FunctionBuilder<'_>, word: Value, abi: types::Type) -> Value {
    if abi == types::F64 {
        builder.ins().bitcast(types::F64, MemFlags::new(), word)
    } else {
        word
    }
}

fn word_from_value(builder: &mut FunctionBuilder<'_>, value: Value, abi: types::Type) -> Value {
    if abi == types::F64 {
        builder.ins().bitcast(types::I64, MemFlags::new(), value)
    } else {
        value
    }
}

/// The address of the environment words of the callable at `callable_ptr`.
fn environment_base(builder: &mut FunctionBuilder<'_>, callable_ptr: Value, inline: bool) -> Value {
    if inline {
        builder.ins().iadd_imm(callable_ptr, 8)
    } else {
        builder
            .ins()
            .load(types::I64, MemFlags::new(), callable_ptr, 8)
    }
}

/// Loads every capture as direct values from the environment.
fn load_captures(
    builder: &mut FunctionBuilder<'_>,
    env_base: Value,
    capture_types: &[DirectType],
) -> Vec<Vec<Value>> {
    let mut captures = Vec::new();
    let mut offset = 0i32;
    for ty in capture_types {
        let mut values = Vec::new();
        for abi in ty.abi_types() {
            let word = builder
                .ins()
                .load(types::I64, MemFlags::new(), env_base, offset);
            values.push(value_from_word(builder, word, abi));
            offset += 8;
        }
        captures.push(values);
    }
    captures
}

/// Stores one capture's direct values at its environment offset.
fn store_capture(
    builder: &mut FunctionBuilder<'_>,
    env_base: Value,
    offset: i32,
    values: &[Value],
    ty: &DirectType,
) {
    for (index, (value, abi)) in values.iter().zip(ty.abi_types()).enumerate() {
        let word = word_from_value(builder, *value, abi);
        builder
            .ins()
            .store(MemFlags::new(), word, env_base, offset + (index as i32) * 8);
    }
}

/// Retains the handles among `values` of type `ty` (a borrowed value copied
/// into a new owner), the counterpart of `release_direct_values`.
pub(super) fn retain_direct_values(
    codegen: &mut NativeCodegen<'_>,
    builder: &mut FunctionBuilder<'_>,
    values: &[Value],
    ty: &DirectType,
) -> std::result::Result<(), String> {
    match ty {
        DirectType::Scalar(_) => Ok(()),
        DirectType::Opaque(_) => {
            let retain_value = codegen
                .object
                .declare_func_in_func(codegen.retain_value, builder.func);
            let value = values.first().copied().ok_or_else(|| {
                format!(
                    "direct backend retain expected an opaque `{}` value",
                    render_direct_type(ty)
                )
            })?;
            builder.ins().call(retain_value, &[value]);
            Ok(())
        }
        DirectType::PlainClass(class) => {
            let mut start = 0;
            for field in &class.fields {
                let end = start + field.ty.value_count();
                let slice = values.get(start..end).ok_or_else(|| {
                    format!(
                        "direct backend retain expected `{}` values for `{}`",
                        class
                            .fields
                            .iter()
                            .map(|f| f.ty.value_count())
                            .sum::<usize>(),
                        class.class_name
                    )
                })?;
                retain_direct_values(codegen, builder, slice, &field.ty)?;
                start = end;
            }
            Ok(())
        }
        DirectType::Union(union) => {
            if !union.owns_handles() {
                return Ok(());
            }
            let retain_value = codegen
                .object
                .declare_func_in_func(codegen.retain_value, builder.func);
            let (tag, handle) = match values {
                [tag, handle, ..] => (*tag, *handle),
                _ => {
                    return Err(format!(
                        "direct backend retain expected union words for `{}`",
                        union.union_type
                    ))
                }
            };
            for &index in &union.owning {
                let active = builder.ins().icmp_imm(IntCC::Equal, tag, index as i64);
                let retain_block = builder.create_block();
                let continue_block = builder.create_block();
                builder
                    .ins()
                    .brif(active, retain_block, &[], continue_block, &[]);
                builder.switch_to_block(retain_block);
                builder.seal_block(retain_block);
                if matches!(union.member(index), Ok(DirectType::Callable(_))) {
                    call_callable_unary_adapter(
                        codegen,
                        builder,
                        &values[1..2 + CALLABLE_ENVIRONMENT_WORDS],
                        DESCRIPTOR_RETAIN,
                    )?;
                } else {
                    builder.ins().call(retain_value, &[handle]);
                }
                builder.ins().jump(continue_block, &[]);
                builder.switch_to_block(continue_block);
                builder.seal_block(continue_block);
            }
            Ok(())
        }
        DirectType::Callable(_) => {
            call_callable_unary_adapter(codegen, builder, values, DESCRIPTOR_RETAIN)
        }
    }
}

/// Calls the `drop` or `retain` adapter named by a callable's descriptor on
/// the value's four words; a zero descriptor (a moved-from or never-assigned
/// value) names no shape and is skipped.
pub(super) fn call_callable_unary_adapter(
    codegen: &mut NativeCodegen<'_>,
    builder: &mut FunctionBuilder<'_>,
    values: &[Value],
    descriptor_offset: i32,
) -> std::result::Result<(), String> {
    if values.len() != 1 + CALLABLE_ENVIRONMENT_WORDS {
        return Err(format!(
            "direct backend expected {} callable words, found {}",
            1 + CALLABLE_ENVIRONMENT_WORDS,
            values.len()
        ));
    }
    let live = builder.ins().icmp_imm(IntCC::NotEqual, values[0], 0);
    let call_block = builder.create_block();
    let continue_block = builder.create_block();
    builder
        .ins()
        .brif(live, call_block, &[], continue_block, &[]);
    builder.switch_to_block(call_block);
    builder.seal_block(call_block);
    call_callable_unary_adapter_live(codegen, builder, values, descriptor_offset);
    builder.ins().jump(continue_block, &[]);
    builder.switch_to_block(continue_block);
    builder.seal_block(continue_block);
    Ok(())
}

fn call_callable_unary_adapter_live(
    codegen: &mut NativeCodegen<'_>,
    builder: &mut FunctionBuilder<'_>,
    values: &[Value],
    descriptor_offset: i32,
) {
    let slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        (8 * (1 + CALLABLE_ENVIRONMENT_WORDS)) as u32,
        3,
    ));
    let callable_ptr = builder.ins().stack_addr(types::I64, slot, 0);
    for (index, value) in values.iter().enumerate() {
        builder
            .ins()
            .store(MemFlags::new(), *value, callable_ptr, (index as i32) * 8);
    }
    let descriptor = values[0];
    let adapter = builder
        .ins()
        .load(types::I64, MemFlags::new(), descriptor, descriptor_offset);
    let signature = builder
        .func
        .import_signature(callable_unary_signature(codegen.call_conv));
    builder
        .ins()
        .call_indirect(signature, adapter, &[callable_ptr]);
}

impl NativeCodegen<'_> {
    /// Defines the descriptor and adapters of every declared shape.
    pub(super) fn define_callable_shapes(&mut self) -> std::result::Result<(), String> {
        let keys: Vec<CallableShapeKey> = self.callable_shapes.keys().cloned().collect();
        for key in keys {
            let shape = self.callable_shapes[&key].clone();
            self.define_callable_drop(&shape)?;
            self.define_callable_retain(&shape)?;
            self.define_callable_box(&key, &shape)?;
            self.define_callable_invoke(&key, &shape)?;
            self.define_callable_descriptor(&shape)?;
        }
        Ok(())
    }

    fn define_callable_descriptor(
        &mut self,
        shape: &CallableShape,
    ) -> std::result::Result<(), String> {
        let mut data = DataDescription::new();
        let mut bytes = vec![0u8; DESCRIPTOR_BYTES];
        bytes[DESCRIPTOR_ENV_WORDS as usize..DESCRIPTOR_ENV_WORDS as usize + 8]
            .copy_from_slice(&(shape.env_words as u64).to_le_bytes());
        bytes[DESCRIPTOR_INLINE as usize..DESCRIPTOR_INLINE as usize + 8]
            .copy_from_slice(&(u64::from(shape.inline())).to_le_bytes());
        data.define(bytes.into_boxed_slice());
        for (offset, func_id) in [
            (DESCRIPTOR_INVOKE, shape.invoke),
            (DESCRIPTOR_DROP, shape.drop),
            (DESCRIPTOR_RETAIN, shape.retain),
            (DESCRIPTOR_BOX, shape.box_value),
        ] {
            let func_ref = self.object.declare_func_in_data(func_id, &mut data);
            data.write_function_addr(offset as u32, func_ref);
        }
        try_or_string_error!(
            self.object.define_data(shape.descriptor, &data),
            "failed to define callable descriptor: {}"
        );
        Ok(())
    }

    fn define_callable_drop(&mut self, shape: &CallableShape) -> std::result::Result<(), String> {
        let mut ctx = self.object.make_context();
        ctx.func.signature = callable_unary_signature(self.call_conv);
        ctx.func.name = UserFuncName::user(0, shape.drop.as_u32());
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let callable_ptr = builder.block_params(entry)[0];
        let env_base = environment_base(&mut builder, callable_ptr, shape.inline());
        let captures = load_captures(&mut builder, env_base, &shape.capture_types);
        for (values, ty) in captures.iter().zip(&shape.capture_types) {
            release_direct_values(self, &mut builder, values, ty)?;
        }
        if !shape.inline() {
            let env_free = self
                .object
                .declare_func_in_func(self.callable_env_free, builder.func);
            builder.ins().call(env_free, &[env_base]);
        }
        builder.ins().return_(&[]);
        builder.finalize();
        try_or_string_error!(
            self.object.define_function(shape.drop, &mut ctx),
            "failed to define callable drop adapter: {}"
        );
        Ok(())
    }

    fn define_callable_retain(&mut self, shape: &CallableShape) -> std::result::Result<(), String> {
        let mut ctx = self.object.make_context();
        ctx.func.signature = callable_unary_signature(self.call_conv);
        ctx.func.name = UserFuncName::user(0, shape.retain.as_u32());
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let callable_ptr = builder.block_params(entry)[0];
        let env_base = environment_base(&mut builder, callable_ptr, shape.inline());
        let captures = load_captures(&mut builder, env_base, &shape.capture_types);
        for (values, ty) in captures.iter().zip(&shape.capture_types) {
            retain_direct_values(self, &mut builder, values, ty)?;
        }
        builder.ins().return_(&[]);
        builder.finalize();
        try_or_string_error!(
            self.object.define_function(shape.retain, &mut ctx),
            "failed to define callable retain adapter: {}"
        );
        Ok(())
    }

    /// Boxes the inline callable into today's `Value::Function` (with a
    /// `ClosureEnvironment` when it has captures); the inline value stays
    /// alive, so its handles are retained into the box.
    fn define_callable_box(
        &mut self,
        key: &CallableShapeKey,
        shape: &CallableShape,
    ) -> std::result::Result<(), String> {
        let function = self
            .module
            .functions
            .iter()
            .chain(self.module.top_level.iter())
            .find(|function| function.name == key.function)
            .cloned()
            .ok_or_else(|| format!("direct backend cannot find `{}`", key.function))?;
        let mut ctx = self.object.make_context();
        ctx.func.signature = callable_box_signature(self.call_conv);
        ctx.func.name = UserFuncName::user(0, shape.box_value.as_u32());
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let callable_ptr = builder.block_params(entry)[0];

        let thunk_id = *self.function_thunks.get(&key.function).ok_or_else(|| {
            format!(
                "direct backend does not know function thunk for `{}`",
                key.function
            )
        })?;
        let binder_id = *self
            .function_default_binders
            .get(&key.function)
            .ok_or_else(|| {
                format!(
                    "direct backend does not know function default binder for `{}`",
                    key.function
                )
            })?;
        let thunk_ref = self.object.declare_func_in_func(thunk_id, builder.func);
        let binder_ref = self.object.declare_func_in_func(binder_id, builder.func);
        let thunk_ptr = builder.ins().func_addr(types::I64, thunk_ref);
        let binder_ptr = builder.ins().func_addr(types::I64, binder_ref);
        let signature_json = serde_json::to_vec(&shape.signature).map_err(|error| {
            format!(
                "failed to serialize function signature for `{}`: {error}",
                key.function
            )
        })?;
        let path = function
            .source_path
            .clone()
            .unwrap_or_else(|| self.program_path.clone());
        let (name_ptr, name_len) = declare_string_constant(
            &mut self.object,
            &mut self.string_data,
            &mut builder,
            key.function.as_bytes(),
        )?;
        let (signature_ptr, signature_len) = declare_string_constant(
            &mut self.object,
            &mut self.string_data,
            &mut builder,
            &signature_json,
        )?;
        let (path_ptr, path_len) = declare_string_constant(
            &mut self.object,
            &mut self.string_data,
            &mut builder,
            path.as_bytes(),
        )?;
        let line = builder.ins().iconst(types::I64, function.span.line as i64);
        let column = builder
            .ins()
            .iconst(types::I64, function.span.column as i64);
        let function_value = self
            .object
            .declare_func_in_func(self.function_value, builder.func);
        let call = builder.ins().call(
            function_value,
            &[
                thunk_ptr,
                binder_ptr,
                name_ptr,
                name_len,
                signature_ptr,
                signature_len,
                path_ptr,
                path_len,
                line,
                column,
            ],
        );
        let function_handle = builder.inst_results(call)[0];
        if key.captures == 0 {
            builder.ins().return_(&[function_handle]);
            builder.finalize();
            try_or_string_error!(
                self.object.define_function(shape.box_value, &mut ctx),
                "failed to define callable box adapter: {}"
            );
            return Ok(());
        }

        let env_base = environment_base(&mut builder, callable_ptr, shape.inline());
        let captures = load_captures(&mut builder, env_base, &shape.capture_types);
        let arg_buffer_new = self
            .object
            .declare_func_in_func(self.arg_buffer_new, builder.func);
        let store_owned = self
            .object
            .declare_func_in_func(self.arg_buffer_store_owned, builder.func);
        let count = builder.ins().iconst(types::I64, key.captures as i64);
        let buffer_call = builder.ins().call(arg_buffer_new, &[count]);
        let buffer = builder.inst_results(buffer_call)[0];
        for (index, (values, ty)) in captures.iter().zip(&shape.capture_types).enumerate() {
            retain_direct_values(self, &mut builder, values, ty)?;
            let boxed = box_thunk_value(self, &mut builder, values, ty)?;
            let slot = builder.ins().iconst(types::I64, index as i64);
            builder.ins().call(store_owned, &[buffer, slot, boxed]);
        }
        let mode_slot = builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            (8 * key.captures) as u32,
            3,
        ));
        let modes = builder.ins().stack_addr(types::I64, mode_slot, 0);
        for (index, param) in function.params.iter().take(key.captures).enumerate() {
            let mutable = builder.ins().iconst(
                types::I64,
                i64::from(param.passing == MirReceiverKind::BorrowMut),
            );
            builder
                .ins()
                .store(MemFlags::new(), mutable, modes, (index as i32) * 8);
        }
        let consuming = builder.ins().iconst(types::I64, i64::from(key.consuming));
        let closure_value = self
            .object
            .declare_func_in_func(self.closure_value, builder.func);
        let call = builder.ins().call(
            closure_value,
            &[function_handle, buffer, count, modes, consuming],
        );
        let boxed = builder.inst_results(call)[0];
        builder.ins().return_(&[boxed]);
        builder.finalize();
        try_or_string_error!(
            self.object.define_function(shape.box_value, &mut ctx),
            "failed to define callable box adapter: {}"
        );
        Ok(())
    }

    /// Calls the lowered function with the captures as leading arguments
    /// and the public arguments as passed, filling a missing public
    /// argument from its default function; mutated captures are stored
    /// back into the environment and public writebacks are returned.
    fn define_callable_invoke(
        &mut self,
        key: &CallableShapeKey,
        shape: &CallableShape,
    ) -> std::result::Result<(), String> {
        let function = self
            .module
            .functions
            .iter()
            .chain(self.module.top_level.iter())
            .find(|function| function.name == key.function)
            .cloned()
            .ok_or_else(|| format!("direct backend cannot find `{}`", key.function))?;
        let param_types = self
            .function_param_types
            .get(&key.function)
            .cloned()
            .unwrap_or_default();
        let return_ty = self
            .function_return_types
            .get(&key.function)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "direct backend does not know return type for `{}`",
                    key.function
                )
            })?;
        let target_id = *self
            .functions
            .get(&key.function)
            .ok_or_else(|| format!("direct backend does not know function `{}`", key.function))?;
        let mut ctx = self.object.make_context();
        ctx.func.signature = callable_invoke_signature(
            &function,
            key.captures,
            &param_types,
            &return_ty,
            self.call_conv,
        )?;
        ctx.func.name = UserFuncName::user(0, shape.invoke.as_u32());
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let params = builder.block_params(entry).to_vec();
        let callable_ptr = params[0];
        let supplied_mask = params[1];
        let env_base = environment_base(&mut builder, callable_ptr, shape.inline());
        let captures = load_captures(&mut builder, env_base, &shape.capture_types);

        // The environment keeps its captures across calls, so a capture the
        // function takes by value (and releases at its exit) is retained
        // first; a borrowed or mutable capture is passed as it is.
        let mut lowered_args = Vec::new();
        for (index, (values, ty)) in captures.iter().zip(&shape.capture_types).enumerate() {
            if function.params[index].passing == MirReceiverKind::Value {
                retain_direct_values(self, &mut builder, values, ty)?;
            }
            lowered_args.extend(values.iter().copied());
        }
        let mut cursor = 2;
        for (index, param) in function.params.iter().enumerate().skip(key.captures) {
            let ty = &param_types[index];
            let count = ty.value_count();
            let supplied = params[cursor..cursor + count].to_vec();
            cursor += count;
            let Some(default_name) = param.default_function.as_ref() else {
                lowered_args.extend(supplied);
                continue;
            };
            // A cleared mask bit means the caller passed nothing for this
            // slot: evaluate the declared default instead.
            let bit = builder
                .ins()
                .band_imm(supplied_mask, 1i64 << (index - key.captures));
            let missing = builder.ins().icmp_imm(IntCC::Equal, bit, 0);
            let default_block = builder.create_block();
            let merge_block = builder.create_block();
            for abi in ty.abi_types() {
                builder.append_block_param(merge_block, abi);
            }
            builder
                .ins()
                .brif(missing, default_block, &[], merge_block, &supplied);
            builder.switch_to_block(default_block);
            builder.seal_block(default_block);
            let default_id = *self.functions.get(default_name).ok_or_else(|| {
                format!(
                    "direct backend is missing default function `{default_name}` for `{}`",
                    key.function
                )
            })?;
            let default_ref = self.object.declare_func_in_func(default_id, builder.func);
            let call = builder.ins().call(default_ref, &[]);
            let defaults = builder.inst_results(call).to_vec();
            builder.ins().jump(merge_block, &defaults);
            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            lowered_args.extend(builder.block_params(merge_block).to_vec());
        }

        let target_ref = self.object.declare_func_in_func(target_id, builder.func);
        let inst = builder.ins().call(target_ref, &lowered_args);
        let results = builder.inst_results(inst).to_vec();
        let return_count = return_ty.value_count();
        let mut returned = results[..return_count].to_vec();
        let mut cursor = return_count;
        let mut env_offset = 0i32;
        for (index, param) in function.params.iter().enumerate() {
            let ty = &param_types[index];
            let count = ty.value_count();
            if index < key.captures {
                if param.passing == MirReceiverKind::BorrowMut {
                    let updated = &results[cursor..cursor + count];
                    store_capture(&mut builder, env_base, env_offset, updated, ty);
                    cursor += count;
                }
                env_offset += (count as i32) * 8;
                continue;
            }
            if param.passing == MirReceiverKind::BorrowMut {
                returned.extend_from_slice(&results[cursor..cursor + count]);
                cursor += count;
            }
        }
        builder.ins().return_(&returned);
        builder.finalize();
        try_or_string_error!(
            self.object.define_function(shape.invoke, &mut ctx),
            "failed to define callable invoke adapter: {}"
        );
        Ok(())
    }
}

/// A callable whose value entered from the runtime as a boxed
/// `Value::Function`: its words are `[contract descriptor, handle, 0, 0]`
/// and the descriptor's adapters call, release, retain, and re-box the
/// handle for the contract's public direct signature.
#[derive(Clone, Debug)]
pub(super) struct ContractDescriptor {
    pub(super) descriptor: DataId,
    pub(super) invoke: FuncId,
    pub(super) drop: FuncId,
    pub(super) retain: FuncId,
    pub(super) box_value: FuncId,
    pub(super) param_types: Vec<DirectType>,
    pub(super) mutable_params: Vec<bool>,
    pub(super) return_ty: DirectType,
}

/// The public parameters and return type of a callable contract.
pub(super) fn contract_parts(
    signature: &Type,
) -> std::result::Result<(Vec<FunctionParamContract>, Type), String> {
    match signature {
        Type::Function {
            params,
            return_type,
        } => Ok((params.clone(), return_type.as_ref().clone())),
        Type::Closure {
            params,
            return_type,
            ..
        } => Ok((params.as_ref().clone(), return_type.as_ref().clone())),
        Type::Callable(callable) => Ok((callable.params.clone(), callable.return_type.clone())),
        other => Err(format!(
            "direct backend expected a callable contract, found `{other}`"
        )),
    }
}

/// `invoke(callable_ptr, supplied_mask, public args...) -> (return words,
/// mutable-parameter writeback words...)` for a public contract.
pub(super) fn contract_invoke_signature(
    param_types: &[DirectType],
    mutable_params: &[bool],
    return_ty: &DirectType,
    call_conv: CallConv,
) -> Signature {
    let mut signature = Signature::new(call_conv);
    signature.params.push(AbiParam::new(types::I64));
    signature.params.push(AbiParam::new(types::I64));
    for ty in param_types {
        for abi in ty.abi_types() {
            signature.params.push(AbiParam::new(abi));
        }
    }
    for abi in return_ty.abi_types() {
        signature.returns.push(AbiParam::new(abi));
    }
    for (ty, mutable) in param_types.iter().zip(mutable_params) {
        if *mutable {
            for abi in ty.abi_types() {
                signature.returns.push(AbiParam::new(abi));
            }
        }
    }
    signature
}

/// The direct parameter types, mutability flags, and return type of a
/// contract.
pub(super) fn contract_direct_parts(
    signature: &Type,
    classes: &HashMap<String, MirClass>,
) -> std::result::Result<(Vec<DirectType>, Vec<bool>, DirectType), String> {
    let (params, return_type) = contract_parts(signature)?;
    let param_types = function_value_param_types(&params, classes, "callable parameter")?;
    let mutable_params = params
        .iter()
        .map(|param| param.passing == ReceiverKind::BorrowMut)
        .collect();
    let return_ty = ensure_direct_type(
        crate::sema::returned_view_pointee(&return_type),
        classes,
        "callable return type",
    )?;
    Ok((param_types, mutable_params, return_ty))
}

/// Declares (once per contract spelling) the boxed descriptor and its
/// adapters; they are defined at the end of the module.
pub(super) fn declare_contract_descriptor(
    object: &mut ObjectModule,
    contract_descriptors: &mut HashMap<String, ContractDescriptor>,
    classes: &HashMap<String, MirClass>,
    call_conv: CallConv,
    signature: &Type,
) -> std::result::Result<ContractDescriptor, String> {
    let key = signature.to_string();
    if let Some(existing) = contract_descriptors.get(&key) {
        return Ok(existing.clone());
    }
    let (param_types, mutable_params, return_ty) = contract_direct_parts(signature, classes)?;
    let stem = format!("aura_contract_{}", contract_descriptors.len());
    let descriptor = try_or_string_error!(
        object.declare_data(&format!("{stem}_descriptor"), Linkage::Local, false, false),
        "failed to declare contract descriptor: {}"
    );
    let invoke = try_or_string_error!(
        object.declare_function(
            &format!("{stem}_invoke"),
            Linkage::Local,
            &contract_invoke_signature(&param_types, &mutable_params, &return_ty, call_conv),
        ),
        "failed to declare contract invoke adapter: {}"
    );
    let drop = try_or_string_error!(
        object.declare_function(
            &format!("{stem}_drop"),
            Linkage::Local,
            &callable_unary_signature(call_conv)
        ),
        "failed to declare contract drop adapter: {}"
    );
    let retain = try_or_string_error!(
        object.declare_function(
            &format!("{stem}_retain"),
            Linkage::Local,
            &callable_unary_signature(call_conv)
        ),
        "failed to declare contract retain adapter: {}"
    );
    let box_value = try_or_string_error!(
        object.declare_function(
            &format!("{stem}_box"),
            Linkage::Local,
            &callable_box_signature(call_conv)
        ),
        "failed to declare contract box adapter: {}"
    );
    let contract = ContractDescriptor {
        descriptor,
        invoke,
        drop,
        retain,
        box_value,
        param_types,
        mutable_params,
        return_ty,
    };
    contract_descriptors.insert(key, contract.clone());
    Ok(contract)
}

/// Calls the `box` adapter named by a callable's descriptor on the value's
/// four words and returns the owned handle.
pub(super) fn call_callable_box_adapter(
    codegen: &mut NativeCodegen<'_>,
    builder: &mut FunctionBuilder<'_>,
    values: &[Value],
) -> std::result::Result<Value, String> {
    if values.len() != 1 + CALLABLE_ENVIRONMENT_WORDS {
        return Err(format!(
            "direct backend expected {} callable words, found {}",
            1 + CALLABLE_ENVIRONMENT_WORDS,
            values.len()
        ));
    }
    let slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        (8 * (1 + CALLABLE_ENVIRONMENT_WORDS)) as u32,
        3,
    ));
    let callable_ptr = builder.ins().stack_addr(types::I64, slot, 0);
    for (index, value) in values.iter().enumerate() {
        builder
            .ins()
            .store(MemFlags::new(), *value, callable_ptr, (index as i32) * 8);
    }
    let adapter = builder
        .ins()
        .load(types::I64, MemFlags::new(), values[0], DESCRIPTOR_BOX);
    let signature = builder
        .func
        .import_signature(callable_box_signature(codegen.call_conv));
    let call = builder
        .ins()
        .call_indirect(signature, adapter, &[callable_ptr]);
    Ok(builder.inst_results(call)[0])
}

/// The four words of a boxed function value re-entering as an inline
/// callable of `signature`: the contract descriptor, the handle, and two
/// zero words.
pub(super) fn boxed_callable_words(
    codegen: &mut NativeCodegen<'_>,
    builder: &mut FunctionBuilder<'_>,
    handle: Value,
    signature: &Type,
) -> std::result::Result<Vec<Value>, String> {
    let contract = declare_contract_descriptor(
        &mut codegen.object,
        &mut codegen.contract_descriptors,
        &codegen.classes,
        codegen.call_conv,
        signature,
    )?;
    let global = codegen
        .object
        .declare_data_in_func(contract.descriptor, builder.func);
    let descriptor = builder.ins().symbol_value(types::I64, global);
    let zero = builder.ins().iconst(types::I64, 0);
    Ok(vec![descriptor, handle, zero, zero])
}

impl NativeCodegen<'_> {
    /// Defines every contract descriptor declared while compiling the
    /// module's functions.
    pub(super) fn define_contract_descriptors(&mut self) -> std::result::Result<(), String> {
        let contracts: Vec<ContractDescriptor> =
            self.contract_descriptors.values().cloned().collect();
        for contract in contracts {
            self.define_contract_handle_adapter(contract.drop, self.release_value, false)?;
            self.define_contract_handle_adapter(contract.retain, self.retain_value, false)?;
            self.define_contract_handle_adapter(contract.box_value, self.retain_value, true)?;
            self.define_contract_invoke(&contract)?;
            let mut data = DataDescription::new();
            let mut bytes = vec![0u8; DESCRIPTOR_BYTES];
            bytes[DESCRIPTOR_ENV_WORDS as usize..DESCRIPTOR_ENV_WORDS as usize + 8]
                .copy_from_slice(&1u64.to_le_bytes());
            bytes[DESCRIPTOR_INLINE as usize..DESCRIPTOR_INLINE as usize + 8]
                .copy_from_slice(&1u64.to_le_bytes());
            data.define(bytes.into_boxed_slice());
            for (offset, func_id) in [
                (DESCRIPTOR_INVOKE, contract.invoke),
                (DESCRIPTOR_DROP, contract.drop),
                (DESCRIPTOR_RETAIN, contract.retain),
                (DESCRIPTOR_BOX, contract.box_value),
            ] {
                let func_ref = self.object.declare_func_in_data(func_id, &mut data);
                data.write_function_addr(offset as u32, func_ref);
            }
            try_or_string_error!(
                self.object.define_data(contract.descriptor, &data),
                "failed to define contract descriptor: {}"
            );
        }
        Ok(())
    }

    /// `drop`, `retain`, and `box` of a boxed callable act on the handle in
    /// word 1; `box` also returns it.
    fn define_contract_handle_adapter(
        &mut self,
        adapter: FuncId,
        helper: FuncId,
        returns_handle: bool,
    ) -> std::result::Result<(), String> {
        let mut ctx = self.object.make_context();
        ctx.func.signature = if returns_handle {
            callable_box_signature(self.call_conv)
        } else {
            callable_unary_signature(self.call_conv)
        };
        ctx.func.name = UserFuncName::user(0, adapter.as_u32());
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let callable_ptr = builder.block_params(entry)[0];
        let handle = builder
            .ins()
            .load(types::I64, MemFlags::new(), callable_ptr, 8);
        let helper_ref = self.object.declare_func_in_func(helper, builder.func);
        builder.ins().call(helper_ref, &[handle]);
        if returns_handle {
            builder.ins().return_(&[handle]);
        } else {
            builder.ins().return_(&[]);
        }
        builder.finalize();
        try_or_string_error!(
            self.object.define_function(adapter, &mut ctx),
            "failed to define contract handle adapter: {}"
        );
        Ok(())
    }

    /// Boxes the public arguments into a uniform buffer, binds the
    /// callee's defaults, calls the function value through the runtime,
    /// and unboxes the result and the mutable-parameter writebacks.
    fn define_contract_invoke(
        &mut self,
        contract: &ContractDescriptor,
    ) -> std::result::Result<(), String> {
        let mut ctx = self.object.make_context();
        ctx.func.signature = contract_invoke_signature(
            &contract.param_types,
            &contract.mutable_params,
            &contract.return_ty,
            self.call_conv,
        );
        ctx.func.name = UserFuncName::user(0, contract.invoke.as_u32());
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let params = builder.block_params(entry).to_vec();
        let callable_ptr = params[0];
        let supplied_mask = params[1];
        let handle = builder
            .ins()
            .load(types::I64, MemFlags::new(), callable_ptr, 8);
        let count = contract.param_types.len();
        let buffer_slot = builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            (8 * count.max(1)) as u32,
            3,
        ));
        let buffer = builder.ins().stack_addr(types::I64, buffer_slot, 0);
        let zero = builder.ins().iconst(types::I64, 0);
        for index in 0..count {
            builder
                .ins()
                .store(MemFlags::new(), zero, buffer, (index as i32) * 8);
        }
        let mut cursor = 2;
        for (index, ty) in contract.param_types.iter().enumerate() {
            let width = ty.value_count();
            let values = params[cursor..cursor + width].to_vec();
            cursor += width;
            let bit = builder.ins().band_imm(supplied_mask, 1i64 << index);
            let supplied = builder.ins().icmp_imm(IntCC::NotEqual, bit, 0);
            let box_block = builder.create_block();
            let next_block = builder.create_block();
            builder
                .ins()
                .brif(supplied, box_block, &[], next_block, &[]);
            builder.switch_to_block(box_block);
            builder.seal_block(box_block);
            let boxed = box_thunk_value(self, &mut builder, &values, ty)?;
            builder
                .ins()
                .store(MemFlags::new(), boxed, buffer, (index as i32) * 8);
            builder.ins().jump(next_block, &[]);
            builder.switch_to_block(next_block);
            builder.seal_block(next_block);
        }
        let count_value = builder.ins().iconst(types::I64, count as i64);
        let bind_defaults = self
            .object
            .declare_func_in_func(self.function_bind_defaults, builder.func);
        let keep_defaults_owned = builder.ins().iconst(types::I64, 0);
        builder.ins().call(
            bind_defaults,
            &[handle, buffer, count_value, keep_defaults_owned],
        );
        let function_call = self
            .object
            .declare_func_in_func(self.function_call, builder.func);
        let call = builder
            .ins()
            .call(function_call, &[handle, buffer, count_value]);
        let raw_result = builder.inst_results(call)[0];
        let release_value = self
            .object
            .declare_func_in_func(self.release_value, builder.func);
        let mut returned = unbox_thunk_value(self, &mut builder, raw_result, &contract.return_ty)?;
        if !matches!(contract.return_ty, DirectType::Opaque(_)) {
            builder.ins().call(release_value, &[raw_result]);
        }
        for (index, (ty, mutable)) in contract
            .param_types
            .iter()
            .zip(&contract.mutable_params)
            .enumerate()
        {
            if !*mutable {
                continue;
            }
            let raw = builder
                .ins()
                .load(types::I64, MemFlags::new(), buffer, (index as i32) * 8);
            returned.extend(unbox_thunk_value(self, &mut builder, raw, ty)?);
            if !matches!(ty, DirectType::Opaque(_)) {
                builder.ins().call(release_value, &[raw]);
            }
        }
        builder.ins().return_(&returned);
        builder.finalize();
        try_or_string_error!(
            self.object.define_function(contract.invoke, &mut ctx),
            "failed to define contract invoke adapter: {}"
        );
        Ok(())
    }
}
