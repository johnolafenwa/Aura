use std::collections::{BTreeSet, HashMap};

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::immediates::Ieee64;
use cranelift_codegen::ir::TrapCode;
use cranelift_codegen::ir::{
    types, AbiParam, InstBuilder, MemFlags, Signature, UserFuncName, Value,
};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{default_libcall_names, DataDescription, DataId, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

use crate::ast::{BinaryOp, UnaryOp};
use crate::mir::{
    BasicBlock, CallTarget, Instruction, MirArg, MirClass, MirFunction, MirMethod, MirModule,
    MirReceiverKind, MirSelectArm, MirSelectKind, MirTraitImpl, Operand, Rvalue, Terminator,
};
use crate::sema::Type;

pub fn emit_host_object(module: &MirModule) -> std::result::Result<Vec<u8>, String> {
    let context = NativeCodegen::new(module)?;
    context.emit()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScalarKind {
    Int32,
    Float32,
    Float64,
    Bool,
    Unit,
}

impl ScalarKind {
    fn signature_type(self) -> cranelift_codegen::ir::Type {
        match self {
            ScalarKind::Int32 | ScalarKind::Bool | ScalarKind::Unit => types::I64,
            ScalarKind::Float32 | ScalarKind::Float64 => types::F64,
        }
    }

    fn zero_value(self, builder: &mut FunctionBuilder<'_>) -> Value {
        match self {
            ScalarKind::Int32 | ScalarKind::Bool | ScalarKind::Unit => {
                builder.ins().iconst(types::I64, 0)
            }
            ScalarKind::Float32 | ScalarKind::Float64 => {
                builder.ins().f64const(Ieee64::with_float(0.0))
            }
        }
    }

    fn is_float(self) -> bool {
        matches!(self, ScalarKind::Float32 | ScalarKind::Float64)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DirectType {
    Scalar(ScalarKind),
    PlainClass(PlainClassType),
    Opaque(Type),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PlainClassType {
    class_name: String,
    fields: Vec<PlainClassField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PlainClassField {
    name: String,
    ty: DirectType,
}

impl DirectType {
    fn abi_types(&self) -> Vec<cranelift_codegen::ir::Type> {
        match self {
            DirectType::Scalar(kind) => vec![kind.signature_type()],
            DirectType::PlainClass(class) => class
                .fields
                .iter()
                .flat_map(|field| field.ty.abi_types())
                .collect(),
            DirectType::Opaque(_) => vec![types::I64],
        }
    }

    fn value_count(&self) -> usize {
        self.abi_types().len()
    }

    fn scalar_kind(&self) -> Option<ScalarKind> {
        match self {
            DirectType::Scalar(kind) => Some(*kind),
            DirectType::PlainClass(_) | DirectType::Opaque(_) => None,
        }
    }

    fn zero_values(&self, builder: &mut FunctionBuilder<'_>) -> Vec<Value> {
        match self {
            DirectType::Scalar(kind) => vec![kind.zero_value(builder)],
            DirectType::PlainClass(class) => class
                .fields
                .iter()
                .flat_map(|field| field.ty.zero_values(builder))
                .collect(),
            DirectType::Opaque(_) => vec![builder.ins().iconst(types::I64, 0)],
        }
    }

    fn field_slice(&self, field_name: &str) -> Option<(usize, usize, DirectType)> {
        let DirectType::PlainClass(class) = self else {
            return None;
        };

        let mut start = 0usize;
        for field in &class.fields {
            let end = start + field.ty.value_count();
            if field.name == field_name {
                return Some((start, end, field.ty.clone()));
            }
            start = end;
        }
        None
    }
}

#[derive(Clone)]
struct ValueRef {
    values: Vec<Value>,
    ty: DirectType,
}

struct NativeCodegen<'a> {
    module: &'a MirModule,
    object: ObjectModule,
    functions: HashMap<String, FuncId>,
    function_thunks: HashMap<String, FuncId>,
    classes: HashMap<String, MirClass>,
    trait_impls: Vec<MirTraitImpl>,
    function_return_types: HashMap<String, DirectType>,
    function_param_types: HashMap<String, Vec<DirectType>>,
    function_writeback_types: HashMap<String, Vec<DirectType>>,
    call_conv: CallConv,
    runtime_init: FuncId,
    print_i64: FuncId,
    print_f64: FuncId,
    print_bool: FuncId,
    print_value: FuncId,
    sqrt_f64: FuncId,
    fail_division_by_zero: FuncId,
    fail_int32_overflow: FuncId,
    box_i64: FuncId,
    box_uint_literal: FuncId,
    box_f64: FuncId,
    box_bool: FuncId,
    box_unit: FuncId,
    string_literal: FuncId,
    duration_literal: FuncId,
    range_new: FuncId,
    range_current: FuncId,
    range_end: FuncId,
    range_advance: FuncId,
    clone_value: FuncId,
    unbox_i64: FuncId,
    unbox_f64: FuncId,
    unbox_bool: FuncId,
    value_as_condition: FuncId,
    unary_value: FuncId,
    binary_value: FuncId,
    cast_value: FuncId,
    value_type_matches: FuncId,
    enum_variant: FuncId,
    variant_matches: FuncId,
    variant_payload: FuncId,
    instance_empty: FuncId,
    instance_get_field: FuncId,
    instance_set_field: FuncId,
    arg_buffer_new: FuncId,
    arg_buffer_store: FuncId,
    channel_new: FuncId,
    channel_send: FuncId,
    channel_recv: FuncId,
    channel_try_recv: FuncId,
    channel_close: FuncId,
    task_group_new: FuncId,
    task_group_cancel: FuncId,
    task_group_close: FuncId,
    task_join: FuncId,
    cancelled: FuncId,
    deadline_new: FuncId,
    deadline_ready: FuncId,
    sleep_ms: FuncId,
    sleep_value: FuncId,
    spawn_call: FuncId,
    string_data: HashMap<Vec<u8>, DataId>,
}

impl<'a> NativeCodegen<'a> {
    fn new(module: &'a MirModule) -> std::result::Result<Self, String> {
        validate_module(module)?;
        let classes = module
            .classes
            .iter()
            .cloned()
            .map(|class| (class.name.clone(), class))
            .collect::<HashMap<_, _>>();
        let trait_impls = module.trait_impls.clone();

        let mut flag_builder = settings::builder();
        flag_builder
            .set("is_pic", "true")
            .map_err(|error| format!("failed to configure native backend: {}", error))?;
        let flags = settings::Flags::new(flag_builder);
        let isa_builder = cranelift_native::builder()
            .map_err(|error| format!("failed to detect host ISA: {}", error))?;
        let isa = isa_builder
            .finish(flags)
            .map_err(|error| format!("failed to build host ISA: {}", error))?;
        let call_conv = isa.default_call_conv();
        let builder = ObjectBuilder::new(isa, "aurora_direct".to_string(), default_libcall_names())
            .map_err(|error| format!("failed to initialize object builder: {}", error))?;
        let mut object = ObjectModule::new(builder);

        let runtime_init =
            declare_runtime_function(&mut object, "aurora_direct_runtime_init", &[], None)?;
        let print_i64 =
            declare_runtime_function(&mut object, "aurora_direct_print_i64", &[types::I64], None)?;
        let print_f64 =
            declare_runtime_function(&mut object, "aurora_direct_print_f64", &[types::F64], None)?;
        let print_bool =
            declare_runtime_function(&mut object, "aurora_direct_print_bool", &[types::I64], None)?;
        let print_value = declare_runtime_function(
            &mut object,
            "aurora_direct_print_value",
            &[types::I64],
            None,
        )?;
        let sqrt_f64 = declare_runtime_function(
            &mut object,
            "aurora_direct_sqrt_f64",
            &[types::F64],
            Some(types::F64),
        )?;
        let fail_division_by_zero = declare_runtime_function(
            &mut object,
            "aurora_direct_fail_division_by_zero",
            &[],
            None,
        )?;
        let fail_int32_overflow = declare_runtime_function(
            &mut object,
            "aurora_direct_fail_int32_overflow",
            &[types::I64],
            None,
        )?;
        let box_i64 = declare_runtime_function(
            &mut object,
            "aurora_direct_box_i64",
            &[types::I64],
            Some(types::I64),
        )?;
        let box_uint_literal = declare_runtime_function(
            &mut object,
            "aurora_direct_box_uint_literal",
            &[types::I64, types::I64],
            Some(types::I64),
        )?;
        let box_f64 = declare_runtime_function(
            &mut object,
            "aurora_direct_box_f64",
            &[types::F64],
            Some(types::I64),
        )?;
        let box_bool = declare_runtime_function(
            &mut object,
            "aurora_direct_box_bool",
            &[types::I64],
            Some(types::I64),
        )?;
        let box_unit =
            declare_runtime_function(&mut object, "aurora_direct_box_unit", &[], Some(types::I64))?;
        let string_literal = declare_runtime_function(
            &mut object,
            "aurora_direct_string_literal",
            &[types::I64, types::I64],
            Some(types::I64),
        )?;
        let duration_literal = declare_runtime_function(
            &mut object,
            "aurora_direct_duration_literal",
            &[types::I64],
            Some(types::I64),
        )?;
        let range_new = declare_runtime_function(
            &mut object,
            "aurora_direct_range_new",
            &[types::I64, types::I64],
            Some(types::I64),
        )?;
        let range_current = declare_runtime_function(
            &mut object,
            "aurora_direct_range_current",
            &[types::I64],
            Some(types::I64),
        )?;
        let range_end = declare_runtime_function(
            &mut object,
            "aurora_direct_range_end",
            &[types::I64],
            Some(types::I64),
        )?;
        let range_advance = declare_runtime_function(
            &mut object,
            "aurora_direct_range_advance",
            &[types::I64],
            Some(types::I64),
        )?;
        let clone_value = declare_runtime_function(
            &mut object,
            "aurora_direct_clone_value",
            &[types::I64],
            Some(types::I64),
        )?;
        let unbox_i64 = declare_runtime_function(
            &mut object,
            "aurora_direct_unbox_i64",
            &[types::I64],
            Some(types::I64),
        )?;
        let unbox_f64 = declare_runtime_function(
            &mut object,
            "aurora_direct_unbox_f64",
            &[types::I64],
            Some(types::F64),
        )?;
        let unbox_bool = declare_runtime_function(
            &mut object,
            "aurora_direct_unbox_bool",
            &[types::I64],
            Some(types::I64),
        )?;
        let value_as_condition = declare_runtime_function(
            &mut object,
            "aurora_direct_value_as_condition",
            &[types::I64],
            Some(types::I64),
        )?;
        let unary_value = declare_runtime_function(
            &mut object,
            "aurora_direct_unary_value",
            &[types::I64, types::I64],
            Some(types::I64),
        )?;
        let binary_value = declare_runtime_function(
            &mut object,
            "aurora_direct_binary_value",
            &[types::I64, types::I64, types::I64],
            Some(types::I64),
        )?;
        let cast_value = declare_runtime_function(
            &mut object,
            "aurora_direct_cast_value",
            &[types::I64, types::I64, types::I64],
            Some(types::I64),
        )?;
        let value_type_matches = declare_runtime_function(
            &mut object,
            "aurora_direct_value_type_matches",
            &[types::I64, types::I64, types::I64],
            Some(types::I64),
        )?;
        let enum_variant = declare_runtime_function(
            &mut object,
            "aurora_direct_enum_variant",
            &[types::I64, types::I64, types::I64, types::I64, types::I64],
            Some(types::I64),
        )?;
        let variant_matches = declare_runtime_function(
            &mut object,
            "aurora_direct_variant_matches",
            &[types::I64, types::I64, types::I64, types::I64, types::I64],
            Some(types::I64),
        )?;
        let variant_payload = declare_runtime_function(
            &mut object,
            "aurora_direct_variant_payload",
            &[types::I64],
            Some(types::I64),
        )?;
        let instance_empty = declare_runtime_function(
            &mut object,
            "aurora_direct_instance_empty",
            &[types::I64, types::I64],
            Some(types::I64),
        )?;
        let instance_get_field = declare_runtime_function(
            &mut object,
            "aurora_direct_instance_get_field",
            &[types::I64, types::I64, types::I64],
            Some(types::I64),
        )?;
        let instance_set_field = declare_runtime_function(
            &mut object,
            "aurora_direct_instance_set_field",
            &[types::I64, types::I64, types::I64, types::I64],
            Some(types::I64),
        )?;
        let arg_buffer_new = declare_runtime_function(
            &mut object,
            "aurora_direct_arg_buffer_new",
            &[types::I64],
            Some(types::I64),
        )?;
        let arg_buffer_store = declare_runtime_function(
            &mut object,
            "aurora_direct_arg_buffer_store",
            &[types::I64, types::I64, types::I64],
            None,
        )?;
        let channel_new = declare_runtime_function(
            &mut object,
            "aurora_direct_channel_new",
            &[],
            Some(types::I64),
        )?;
        let channel_send = declare_runtime_function(
            &mut object,
            "aurora_direct_channel_send",
            &[types::I64, types::I64],
            Some(types::I64),
        )?;
        let channel_recv = declare_runtime_function(
            &mut object,
            "aurora_direct_channel_recv",
            &[types::I64],
            Some(types::I64),
        )?;
        let channel_try_recv = declare_runtime_function(
            &mut object,
            "aurora_direct_channel_try_recv",
            &[types::I64],
            Some(types::I64),
        )?;
        let channel_close = declare_runtime_function(
            &mut object,
            "aurora_direct_channel_close",
            &[types::I64],
            Some(types::I64),
        )?;
        let task_group_new = declare_runtime_function(
            &mut object,
            "aurora_direct_task_group_new",
            &[],
            Some(types::I64),
        )?;
        let task_group_cancel = declare_runtime_function(
            &mut object,
            "aurora_direct_task_group_cancel",
            &[types::I64],
            Some(types::I64),
        )?;
        let task_group_close = declare_runtime_function(
            &mut object,
            "aurora_direct_task_group_close",
            &[types::I64, types::I64],
            Some(types::I64),
        )?;
        let task_join = declare_runtime_function(
            &mut object,
            "aurora_direct_task_join",
            &[types::I64],
            Some(types::I64),
        )?;
        let cancelled = declare_runtime_function(
            &mut object,
            "aurora_direct_cancelled",
            &[],
            Some(types::I64),
        )?;
        let deadline_new = declare_runtime_function(
            &mut object,
            "aurora_direct_deadline_new",
            &[types::I64],
            Some(types::I64),
        )?;
        let deadline_ready = declare_runtime_function(
            &mut object,
            "aurora_direct_deadline_ready",
            &[types::I64],
            Some(types::I64),
        )?;
        let sleep_ms =
            declare_runtime_function(&mut object, "aurora_direct_sleep_ms", &[types::I64], None)?;
        let sleep_value = declare_runtime_function(
            &mut object,
            "aurora_direct_sleep_value",
            &[types::I64],
            Some(types::I64),
        )?;
        let spawn_call = declare_runtime_function(
            &mut object,
            "aurora_direct_spawn_call",
            &[types::I64, types::I64, types::I64, types::I64, types::I64],
            Some(types::I64),
        )?;

        let mut functions = HashMap::new();
        let mut function_thunks = HashMap::new();
        let mut function_return_types = HashMap::new();
        let mut function_param_types = HashMap::new();
        let mut function_writeback_types = HashMap::new();
        for function in module.functions.iter().chain(module.top_level.iter()) {
            let signature = signature_for(function, &classes, call_conv)?;
            let func_id = object
                .declare_function(&mangle_symbol(&function.name), Linkage::Local, &signature)
                .map_err(|error| {
                    format!("failed to declare function `{}`: {}", function.name, error)
                })?;
            functions.insert(function.name.clone(), func_id);
            let thunk_signature = thunk_signature(call_conv);
            let thunk_id = object
                .declare_function(
                    &mangle_thunk_symbol(&function.name),
                    Linkage::Local,
                    &thunk_signature,
                )
                .map_err(|error| {
                    format!(
                        "failed to declare function thunk `{}`: {}",
                        function.name, error
                    )
                })?;
            function_thunks.insert(function.name.clone(), thunk_id);
            function_return_types.insert(
                function.name.clone(),
                ensure_direct_type(
                    &function.return_type,
                    &classes,
                    &format!("return type of `{}`", function.name),
                )?,
            );
            let mut params = Vec::new();
            let mut writebacks = Vec::new();
            if function.receiver == Some(MirReceiverKind::BorrowMut) {
                writebacks.push(receiver_type(function, &classes)?);
            }
            if function.receiver.is_some() {
                params.push(receiver_type(function, &classes)?);
            }
            for param in &function.params {
                if param.passing == MirReceiverKind::BorrowMut {
                    writebacks.push(ensure_direct_type(
                        &param.ty,
                        &classes,
                        &format!("parameter `{}` on `{}`", param.name, function.name),
                    )?);
                }
                params.push(ensure_direct_type(
                    &param.ty,
                    &classes,
                    &format!("parameter `{}` on `{}`", param.name, function.name),
                )?);
            }
            function_param_types.insert(function.name.clone(), params);
            function_writeback_types.insert(function.name.clone(), writebacks);
        }

        Ok(Self {
            module,
            object,
            functions,
            function_thunks,
            classes,
            trait_impls,
            function_return_types,
            function_param_types,
            function_writeback_types,
            call_conv,
            runtime_init,
            print_i64,
            print_f64,
            print_bool,
            print_value,
            sqrt_f64,
            fail_division_by_zero,
            fail_int32_overflow,
            box_i64,
            box_uint_literal,
            box_f64,
            box_bool,
            box_unit,
            string_literal,
            duration_literal,
            range_new,
            range_current,
            range_end,
            range_advance,
            clone_value,
            unbox_i64,
            unbox_f64,
            unbox_bool,
            value_as_condition,
            unary_value,
            binary_value,
            cast_value,
            value_type_matches,
            enum_variant,
            variant_matches,
            variant_payload,
            instance_empty,
            instance_get_field,
            instance_set_field,
            arg_buffer_new,
            arg_buffer_store,
            channel_new,
            channel_send,
            channel_recv,
            channel_try_recv,
            channel_close,
            task_group_new,
            task_group_cancel,
            task_group_close,
            task_join,
            cancelled,
            deadline_new,
            deadline_ready,
            sleep_ms,
            sleep_value,
            spawn_call,
            string_data: HashMap::new(),
        })
    }

    fn emit(mut self) -> std::result::Result<Vec<u8>, String> {
        let spawn_targets = collect_spawn_targets(self.module);
        for function in self
            .module
            .functions
            .iter()
            .chain(self.module.top_level.iter())
        {
            self.define_function(function)?;
            if spawn_targets.contains(&function.name) {
                self.define_function_thunk(function)?;
            }
        }
        self.define_main_wrapper()?;
        let product = self.object.finish();
        product
            .emit()
            .map_err(|error| format!("failed to emit direct backend object: {}", error))
    }

    fn define_function(&mut self, function: &MirFunction) -> std::result::Result<(), String> {
        let func_id = self.functions[&function.name];
        let mut ctx = self.object.make_context();
        ctx.func.signature = signature_for(function, &self.classes, self.call_conv)?;
        ctx.func.name = UserFuncName::user(0, func_id.as_u32());

        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);

        let mut blocks = HashMap::new();
        for block in &function.blocks {
            blocks.insert(block.label.clone(), builder.create_block());
        }

        let entry = *blocks.get(&function.entry).ok_or_else(|| {
            format!(
                "direct backend could not find entry block `{}`",
                function.entry
            )
        })?;
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);

        let mut variable_index = 0usize;
        let mut variables = HashMap::new();
        let mut variable_types = HashMap::new();
        let entry_values = builder.block_params(entry).to_vec();
        let mut entry_index = 0usize;

        if function.receiver.is_some() {
            let receiver_ty = receiver_type(function, &self.classes)?;
            let end = entry_index + receiver_ty.value_count();
            declare_root_variables(
                &mut builder,
                &mut variable_index,
                &mut variables,
                &mut variable_types,
                "self".to_string(),
                receiver_ty,
                Some(&entry_values[entry_index..end]),
            );
            entry_index = end;
        }

        for param in &function.params {
            let ty = ensure_direct_type(
                &param.ty,
                &self.classes,
                &format!("parameter `{}` on `{}`", param.name, function.name),
            )?;
            let end = entry_index + ty.value_count();
            declare_root_variables(
                &mut builder,
                &mut variable_index,
                &mut variables,
                &mut variable_types,
                param.name.clone(),
                ty,
                Some(&entry_values[entry_index..end]),
            );
            entry_index = end;
        }

        for local in &function.local_types {
            if variables.contains_key(&local.name) {
                continue;
            }
            let ty = ensure_direct_type(
                &local.ty,
                &self.classes,
                &format!("local `{}` on `{}`", local.name, function.name),
            )?;
            declare_root_variables(
                &mut builder,
                &mut variable_index,
                &mut variables,
                &mut variable_types,
                local.name.clone(),
                ty,
                None,
            );
        }

        for block in &function.blocks {
            for instruction in &block.instructions {
                if let Instruction::Assign { target, value } = instruction {
                    if variables.contains_key(target) {
                        continue;
                    }
                    let ty = infer_rvalue_type(
                        value,
                        &variable_types,
                        &self.function_return_types,
                        &self.classes,
                    )
                    .ok_or_else(|| {
                        format!(
                            "direct backend could not infer direct type for temporary `{}` in `{}`",
                            target, function.name
                        )
                    })?;
                    declare_root_variables(
                        &mut builder,
                        &mut variable_index,
                        &mut variables,
                        &mut variable_types,
                        target.clone(),
                        ty,
                        None,
                    );
                }
            }
            if let Terminator::Select { arms, .. } = &block.terminator {
                for arm in arms {
                    let Some(binding) = &arm.binding else {
                        continue;
                    };
                    if variables.contains_key(binding) {
                        continue;
                    }
                    let ty = infer_select_binding_type(
                        arm,
                        &variable_types,
                        &self.classes,
                    )
                    .ok_or_else(|| {
                        format!(
                            "direct backend could not infer direct type for select binding `{}` in `{}`",
                            binding, function.name
                        )
                    })?;
                    declare_root_variables(
                        &mut builder,
                        &mut variable_index,
                        &mut variables,
                        &mut variable_types,
                        binding.clone(),
                        ty,
                        None,
                    );
                }
            }
            if let Terminator::ForRange { binding, .. } = &block.terminator {
                if !variables.contains_key(binding) {
                    declare_root_variables(
                        &mut builder,
                        &mut variable_index,
                        &mut variables,
                        &mut variable_types,
                        binding.clone(),
                        DirectType::Scalar(ScalarKind::Int32),
                        None,
                    );
                }
            }
        }

        let cleanup_places = function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .filter_map(|instruction| match instruction {
                Instruction::PushCleanup { place } => Some(place.clone()),
                _ => None,
            })
            .fold(Vec::<String>::new(), |mut places, place| {
                if !places.contains(&place) {
                    places.push(place);
                }
                places
            });
        let mut cleanup_active_vars = HashMap::new();
        for place in &cleanup_places {
            let variable = Variable::from_u32(variable_index as u32);
            variable_index += 1;
            builder.declare_var(variable, types::I64);
            let zero = builder.ins().iconst(types::I64, 0);
            builder.def_var(variable, zero);
            cleanup_active_vars.insert(place.clone(), variable);
        }

        let mut writeback_locals = Vec::new();
        if function.receiver == Some(MirReceiverKind::BorrowMut) {
            let receiver_ty = receiver_type(function, &self.classes)?;
            writeback_locals.push(("self".to_string(), receiver_ty));
        }
        for param in &function.params {
            if param.passing == MirReceiverKind::BorrowMut {
                let ty = ensure_direct_type(
                    &param.ty,
                    &self.classes,
                    &format!("parameter `{}` on `{}`", param.name, function.name),
                )?;
                writeback_locals.push((param.name.clone(), ty));
            }
        }

        let mut function_refs = HashMap::new();
        for (name, func_id) in &self.functions {
            let func_ref = self.object.declare_func_in_func(*func_id, builder.func);
            function_refs.insert(name.clone(), func_ref);
        }
        let mut function_thunk_refs = HashMap::new();
        for (name, func_id) in &self.function_thunks {
            let func_ref = self.object.declare_func_in_func(*func_id, builder.func);
            function_thunk_refs.insert(name.clone(), func_ref);
        }

        let print_i64 = self
            .object
            .declare_func_in_func(self.print_i64, builder.func);
        let print_f64 = self
            .object
            .declare_func_in_func(self.print_f64, builder.func);
        let print_bool = self
            .object
            .declare_func_in_func(self.print_bool, builder.func);
        let print_value = self
            .object
            .declare_func_in_func(self.print_value, builder.func);
        let sqrt_f64 = self
            .object
            .declare_func_in_func(self.sqrt_f64, builder.func);
        let fail_division_by_zero = self
            .object
            .declare_func_in_func(self.fail_division_by_zero, builder.func);
        let fail_int32_overflow = self
            .object
            .declare_func_in_func(self.fail_int32_overflow, builder.func);
        let box_i64 = self.object.declare_func_in_func(self.box_i64, builder.func);
        let box_uint_literal = self
            .object
            .declare_func_in_func(self.box_uint_literal, builder.func);
        let box_f64 = self.object.declare_func_in_func(self.box_f64, builder.func);
        let box_bool = self
            .object
            .declare_func_in_func(self.box_bool, builder.func);
        let box_unit = self
            .object
            .declare_func_in_func(self.box_unit, builder.func);
        let string_literal = self
            .object
            .declare_func_in_func(self.string_literal, builder.func);
        let duration_literal = self
            .object
            .declare_func_in_func(self.duration_literal, builder.func);
        let range_new = self
            .object
            .declare_func_in_func(self.range_new, builder.func);
        let range_current = self
            .object
            .declare_func_in_func(self.range_current, builder.func);
        let range_end = self
            .object
            .declare_func_in_func(self.range_end, builder.func);
        let range_advance = self
            .object
            .declare_func_in_func(self.range_advance, builder.func);
        let clone_value = self
            .object
            .declare_func_in_func(self.clone_value, builder.func);
        let unbox_i64 = self
            .object
            .declare_func_in_func(self.unbox_i64, builder.func);
        let unbox_f64 = self
            .object
            .declare_func_in_func(self.unbox_f64, builder.func);
        let unbox_bool = self
            .object
            .declare_func_in_func(self.unbox_bool, builder.func);
        let value_as_condition = self
            .object
            .declare_func_in_func(self.value_as_condition, builder.func);
        let unary_value = self
            .object
            .declare_func_in_func(self.unary_value, builder.func);
        let binary_value = self
            .object
            .declare_func_in_func(self.binary_value, builder.func);
        let cast_value = self
            .object
            .declare_func_in_func(self.cast_value, builder.func);
        let value_type_matches = self
            .object
            .declare_func_in_func(self.value_type_matches, builder.func);
        let enum_variant = self
            .object
            .declare_func_in_func(self.enum_variant, builder.func);
        let variant_matches = self
            .object
            .declare_func_in_func(self.variant_matches, builder.func);
        let variant_payload = self
            .object
            .declare_func_in_func(self.variant_payload, builder.func);
        let instance_empty = self
            .object
            .declare_func_in_func(self.instance_empty, builder.func);
        let instance_get_field = self
            .object
            .declare_func_in_func(self.instance_get_field, builder.func);
        let instance_set_field = self
            .object
            .declare_func_in_func(self.instance_set_field, builder.func);
        let arg_buffer_new = self
            .object
            .declare_func_in_func(self.arg_buffer_new, builder.func);
        let arg_buffer_store = self
            .object
            .declare_func_in_func(self.arg_buffer_store, builder.func);
        let channel_new = self
            .object
            .declare_func_in_func(self.channel_new, builder.func);
        let channel_send = self
            .object
            .declare_func_in_func(self.channel_send, builder.func);
        let channel_recv = self
            .object
            .declare_func_in_func(self.channel_recv, builder.func);
        let channel_try_recv = self
            .object
            .declare_func_in_func(self.channel_try_recv, builder.func);
        let channel_close = self
            .object
            .declare_func_in_func(self.channel_close, builder.func);
        let task_group_new = self
            .object
            .declare_func_in_func(self.task_group_new, builder.func);
        let task_group_cancel = self
            .object
            .declare_func_in_func(self.task_group_cancel, builder.func);
        let task_group_close = self
            .object
            .declare_func_in_func(self.task_group_close, builder.func);
        let task_join = self
            .object
            .declare_func_in_func(self.task_join, builder.func);
        let cancelled = self
            .object
            .declare_func_in_func(self.cancelled, builder.func);
        let deadline_new = self
            .object
            .declare_func_in_func(self.deadline_new, builder.func);
        let deadline_ready = self
            .object
            .declare_func_in_func(self.deadline_ready, builder.func);
        let sleep_ms = self
            .object
            .declare_func_in_func(self.sleep_ms, builder.func);
        let sleep_value = self
            .object
            .declare_func_in_func(self.sleep_value, builder.func);
        let spawn_call = self
            .object
            .declare_func_in_func(self.spawn_call, builder.func);

        let mut compiler = FunctionCompiler {
            builder,
            blocks,
            variables,
            variable_types,
            function_refs,
            function_thunk_refs,
            function_return_types: self.function_return_types.clone(),
            function_param_types: self.function_param_types.clone(),
            function_writeback_types: self.function_writeback_types.clone(),
            writeback_locals,
            classes: self.classes.clone(),
            trait_impls: self.trait_impls.clone(),
            object: &mut self.object,
            string_data: &mut self.string_data,
            cleanup_places,
            cleanup_active_vars,
            print_i64,
            print_f64,
            print_bool,
            print_value,
            sqrt_f64,
            fail_division_by_zero,
            fail_int32_overflow,
            box_i64,
            box_uint_literal,
            box_f64,
            box_bool,
            box_unit,
            string_literal,
            duration_literal,
            range_new,
            range_current,
            range_end,
            range_advance,
            clone_value,
            unbox_i64,
            unbox_f64,
            unbox_bool,
            value_as_condition,
            unary_value,
            binary_value,
            cast_value,
            value_type_matches,
            enum_variant,
            variant_matches,
            variant_payload,
            instance_empty,
            instance_get_field,
            instance_set_field,
            arg_buffer_new,
            arg_buffer_store,
            channel_new,
            channel_send,
            channel_recv,
            channel_try_recv,
            channel_close,
            task_group_new,
            task_group_cancel,
            task_group_close,
            task_join,
            cancelled,
            deadline_new,
            deadline_ready,
            sleep_ms,
            sleep_value,
            spawn_call,
        };

        let return_ty = ensure_direct_type(
            &function.return_type,
            &self.classes,
            &format!("return type of `{}`", function.name),
        )?;
        for block in &function.blocks {
            compiler.compile_block(block, &return_ty)?;
        }

        compiler.builder.seal_all_blocks();
        compiler.builder.finalize();
        self.object
            .define_function(func_id, &mut ctx)
            .map_err(|error| {
                format!(
                    "failed to define direct function `{}`: {}",
                    function.name, error
                )
            })?;
        Ok(())
    }

    fn define_function_thunk(&mut self, function: &MirFunction) -> std::result::Result<(), String> {
        if function.receiver.is_some() {
            return Err(format!(
                "direct backend does not yet support spawn thunks for methods like `{}`",
                function.name
            ));
        }

        let thunk_id = self.function_thunks[&function.name];
        let target_id = self.functions[&function.name];
        let mut ctx = self.object.make_context();
        ctx.func.signature = thunk_signature(self.call_conv);
        ctx.func.name = UserFuncName::user(0, thunk_id.as_u32());

        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);

        let args_ptr = builder.block_params(entry)[0];
        let target_ref = self.object.declare_func_in_func(target_id, builder.func);
        let unbox_i64 = self
            .object
            .declare_func_in_func(self.unbox_i64, builder.func);
        let unbox_f64 = self
            .object
            .declare_func_in_func(self.unbox_f64, builder.func);
        let unbox_bool = self
            .object
            .declare_func_in_func(self.unbox_bool, builder.func);
        let box_i64 = self.object.declare_func_in_func(self.box_i64, builder.func);
        let box_f64 = self.object.declare_func_in_func(self.box_f64, builder.func);
        let box_bool = self
            .object
            .declare_func_in_func(self.box_bool, builder.func);
        let box_unit = self
            .object
            .declare_func_in_func(self.box_unit, builder.func);

        let mut lowered_args = Vec::new();
        for (index, param_ty) in self
            .function_param_types
            .get(&function.name)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let raw = builder
                .ins()
                .load(types::I64, MemFlags::new(), args_ptr, (index as i32) * 8);
            match param_ty {
                DirectType::Opaque(_) => lowered_args.push(raw),
                DirectType::Scalar(ScalarKind::Int32) => {
                    let inst = builder.ins().call(unbox_i64, &[raw]);
                    lowered_args.push(builder.inst_results(inst)[0]);
                }
                DirectType::Scalar(ScalarKind::Float32)
                | DirectType::Scalar(ScalarKind::Float64) => {
                    let inst = builder.ins().call(unbox_f64, &[raw]);
                    lowered_args.push(builder.inst_results(inst)[0]);
                }
                DirectType::Scalar(ScalarKind::Bool) => {
                    let inst = builder.ins().call(unbox_bool, &[raw]);
                    lowered_args.push(builder.inst_results(inst)[0]);
                }
                DirectType::Scalar(ScalarKind::Unit) => {
                    lowered_args.push(builder.ins().iconst(types::I64, 0));
                }
                DirectType::PlainClass(class) => {
                    return Err(format!(
                        "direct backend does not yet support spawn thunks for plain-class parameter types like `{}` on `{}`",
                        class.class_name, function.name
                    ));
                }
            }
        }

        let inst = builder.ins().call(target_ref, &lowered_args);
        let results = builder.inst_results(inst).to_vec();
        let return_ty = self
            .function_return_types
            .get(&function.name)
            .ok_or_else(|| {
                format!(
                    "direct backend does not know return type for `{}`",
                    function.name
                )
            })?;
        let boxed = match return_ty {
            DirectType::Opaque(_) => results.first().copied().ok_or_else(|| {
                format!(
                    "thunk for `{}` expected an opaque return value",
                    function.name
                )
            })?,
            DirectType::Scalar(ScalarKind::Int32) => {
                let boxed = builder.ins().call(box_i64, &[results[0]]);
                builder.inst_results(boxed)[0]
            }
            DirectType::Scalar(ScalarKind::Float32) | DirectType::Scalar(ScalarKind::Float64) => {
                let boxed = builder.ins().call(box_f64, &[results[0]]);
                builder.inst_results(boxed)[0]
            }
            DirectType::Scalar(ScalarKind::Bool) => {
                let boxed = builder.ins().call(box_bool, &[results[0]]);
                builder.inst_results(boxed)[0]
            }
            DirectType::Scalar(ScalarKind::Unit) => {
                let boxed = builder.ins().call(box_unit, &[]);
                builder.inst_results(boxed)[0]
            }
            DirectType::PlainClass(class) => {
                return Err(format!(
                    "direct backend does not yet support spawn thunks for plain-class return types like `{}` on `{}`",
                    class.class_name, function.name
                ));
            }
        };
        builder.ins().return_(&[boxed]);
        builder.finalize();

        self.object
            .define_function(thunk_id, &mut ctx)
            .map_err(|error| {
                format!(
                    "failed to define direct function thunk `{}`: {}",
                    function.name, error
                )
            })?;
        Ok(())
    }

    fn define_main_wrapper(&mut self) -> std::result::Result<(), String> {
        let entry_name = if self.functions.contains_key("main") {
            "main".to_string()
        } else if self.functions.contains_key("__script") {
            "__script".to_string()
        } else {
            return Err(
                "direct backend requires a `main` function or top-level script".to_string(),
            );
        };
        let entry_id = self.functions[&entry_name];

        let mut ctx = self.object.make_context();
        ctx.func.signature = main_signature(self.call_conv);
        let wrapper_id = self
            .object
            .declare_function("main", Linkage::Export, &ctx.func.signature)
            .map_err(|error| format!("failed to declare main wrapper: {}", error))?;
        ctx.func.name = UserFuncName::user(0, wrapper_id.as_u32());

        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
        let entry_block = builder.create_block();
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        let entry_ref = self.object.declare_func_in_func(entry_id, builder.func);
        let runtime_init = self
            .object
            .declare_func_in_func(self.runtime_init, builder.func);
        builder.ins().call(runtime_init, &[]);
        let result = builder.ins().call(entry_ref, &[]);
        let return_value = builder
            .inst_results(result)
            .first()
            .copied()
            .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
        let return_code = builder.ins().ireduce(types::I32, return_value);
        builder.ins().return_(&[return_code]);
        builder.finalize();

        self.object
            .define_function(wrapper_id, &mut ctx)
            .map_err(|error| format!("failed to define main wrapper: {}", error))?;
        Ok(())
    }
}

struct FunctionCompiler<'a> {
    builder: FunctionBuilder<'a>,
    blocks: HashMap<String, cranelift_codegen::ir::Block>,
    variables: HashMap<String, Vec<Variable>>,
    variable_types: HashMap<String, DirectType>,
    function_refs: HashMap<String, cranelift_codegen::ir::FuncRef>,
    function_thunk_refs: HashMap<String, cranelift_codegen::ir::FuncRef>,
    function_return_types: HashMap<String, DirectType>,
    function_param_types: HashMap<String, Vec<DirectType>>,
    function_writeback_types: HashMap<String, Vec<DirectType>>,
    writeback_locals: Vec<(String, DirectType)>,
    classes: HashMap<String, MirClass>,
    trait_impls: Vec<MirTraitImpl>,
    object: &'a mut ObjectModule,
    string_data: &'a mut HashMap<Vec<u8>, DataId>,
    cleanup_places: Vec<String>,
    cleanup_active_vars: HashMap<String, Variable>,
    print_i64: cranelift_codegen::ir::FuncRef,
    print_f64: cranelift_codegen::ir::FuncRef,
    print_bool: cranelift_codegen::ir::FuncRef,
    print_value: cranelift_codegen::ir::FuncRef,
    sqrt_f64: cranelift_codegen::ir::FuncRef,
    fail_division_by_zero: cranelift_codegen::ir::FuncRef,
    fail_int32_overflow: cranelift_codegen::ir::FuncRef,
    box_i64: cranelift_codegen::ir::FuncRef,
    box_uint_literal: cranelift_codegen::ir::FuncRef,
    box_f64: cranelift_codegen::ir::FuncRef,
    box_bool: cranelift_codegen::ir::FuncRef,
    box_unit: cranelift_codegen::ir::FuncRef,
    string_literal: cranelift_codegen::ir::FuncRef,
    duration_literal: cranelift_codegen::ir::FuncRef,
    range_new: cranelift_codegen::ir::FuncRef,
    range_current: cranelift_codegen::ir::FuncRef,
    range_end: cranelift_codegen::ir::FuncRef,
    range_advance: cranelift_codegen::ir::FuncRef,
    clone_value: cranelift_codegen::ir::FuncRef,
    unbox_i64: cranelift_codegen::ir::FuncRef,
    unbox_f64: cranelift_codegen::ir::FuncRef,
    unbox_bool: cranelift_codegen::ir::FuncRef,
    value_as_condition: cranelift_codegen::ir::FuncRef,
    unary_value: cranelift_codegen::ir::FuncRef,
    binary_value: cranelift_codegen::ir::FuncRef,
    cast_value: cranelift_codegen::ir::FuncRef,
    value_type_matches: cranelift_codegen::ir::FuncRef,
    enum_variant: cranelift_codegen::ir::FuncRef,
    variant_matches: cranelift_codegen::ir::FuncRef,
    variant_payload: cranelift_codegen::ir::FuncRef,
    instance_empty: cranelift_codegen::ir::FuncRef,
    instance_get_field: cranelift_codegen::ir::FuncRef,
    instance_set_field: cranelift_codegen::ir::FuncRef,
    arg_buffer_new: cranelift_codegen::ir::FuncRef,
    arg_buffer_store: cranelift_codegen::ir::FuncRef,
    channel_new: cranelift_codegen::ir::FuncRef,
    channel_send: cranelift_codegen::ir::FuncRef,
    channel_recv: cranelift_codegen::ir::FuncRef,
    channel_try_recv: cranelift_codegen::ir::FuncRef,
    channel_close: cranelift_codegen::ir::FuncRef,
    task_group_new: cranelift_codegen::ir::FuncRef,
    task_group_cancel: cranelift_codegen::ir::FuncRef,
    task_group_close: cranelift_codegen::ir::FuncRef,
    task_join: cranelift_codegen::ir::FuncRef,
    cancelled: cranelift_codegen::ir::FuncRef,
    deadline_new: cranelift_codegen::ir::FuncRef,
    deadline_ready: cranelift_codegen::ir::FuncRef,
    sleep_ms: cranelift_codegen::ir::FuncRef,
    sleep_value: cranelift_codegen::ir::FuncRef,
    spawn_call: cranelift_codegen::ir::FuncRef,
}

impl<'a> FunctionCompiler<'a> {
    fn compile_block(
        &mut self,
        block: &BasicBlock,
        return_ty: &DirectType,
    ) -> std::result::Result<(), String> {
        let block_id = self.blocks[&block.label];
        if self.builder.current_block() != Some(block_id) {
            self.builder.switch_to_block(block_id);
        }

        for instruction in &block.instructions {
            self.compile_instruction(instruction)?;
        }
        self.compile_terminator(&block.terminator, return_ty)?;
        Ok(())
    }

    fn compile_instruction(
        &mut self,
        instruction: &Instruction,
    ) -> std::result::Result<(), String> {
        match instruction {
            Instruction::Assign { target, value } => {
                if let Rvalue::Try { value: try_value } = value {
                    let target_ty = self.type_of_place(target)?;
                    self.compile_try_assign(target, target_ty, try_value)?;
                    return Ok(());
                }
                let target_ty = self.type_of_place(target)?;
                let compiled = self.compile_rvalue(value)?;
                let coerced = self.coerce_value(compiled, &target_ty)?;
                self.store_place(target, coerced)?;
            }
            Instruction::Eval { value } => {
                let _ = self.load_operand(value)?;
            }
            Instruction::PushCleanup { place } => {
                self.set_cleanup_active(place, true)?;
            }
            Instruction::PopCleanup {
                place,
                cancel_before_cleanup,
            } => {
                self.emit_cleanup_for_place(place, *cancel_before_cleanup)?;
                self.set_cleanup_active(place, false)?;
            }
        }
        Ok(())
    }

    fn compile_terminator(
        &mut self,
        terminator: &Terminator,
        return_ty: &DirectType,
    ) -> std::result::Result<(), String> {
        match terminator {
            Terminator::Return(operand) => {
                let value = self.load_operand(operand)?;
                let coerced = self.coerce_value(value, return_ty)?;
                self.emit_pending_cleanups(true)?;
                let return_values = self.build_return_values(coerced)?;
                self.builder.ins().return_(&return_values);
            }
            Terminator::Goto(label) => {
                let block = self.blocks[label];
                self.builder.ins().jump(block, &[]);
            }
            Terminator::Branch {
                condition,
                then_label,
                else_label,
            } => {
                let condition = self.load_operand(condition)?;
                let condition = self.as_bool_value(condition)?;
                let then_block = self.blocks[then_label];
                let else_block = self.blocks[else_label];
                self.builder
                    .ins()
                    .brif(condition, then_block, &[], else_block, &[]);
            }
            Terminator::Match {
                scrutinee,
                arms,
                otherwise,
            } => {
                let scrutinee = self.load_operand(scrutinee)?;
                let DirectType::Opaque(_) = scrutinee.ty else {
                    return Err(
                        "direct backend expected enum matches to use opaque scrutinees".to_string(),
                    );
                };
                for arm in arms {
                    if arm.wildcard {
                        self.builder.ins().jump(self.blocks[&arm.label], &[]);
                        return Ok(());
                    }
                    let next_block = self.builder.create_block();
                    let matched = self.variant_matches_value(
                        scrutinee.values[0],
                        arm.enum_name.as_deref().unwrap_or_default(),
                        arm.variant_name.as_deref().unwrap_or_default(),
                    )?;
                    let arm_block = self.blocks[&arm.label];
                    self.builder
                        .ins()
                        .brif(matched, arm_block, &[], next_block, &[]);
                    self.builder.switch_to_block(next_block);
                }
                self.builder.ins().jump(self.blocks[otherwise], &[]);
            }
            Terminator::Select { arms, .. } => {
                self.compile_select(arms)?;
            }
            Terminator::ForRange {
                binding,
                iterable,
                body_label,
                exit_label,
            } => {
                self.compile_for_range(binding, iterable, body_label, exit_label)?;
            }
            other => {
                return Err(format!(
                    "direct backend does not support MIR terminator `{:?}`",
                    other
                ))
            }
        }
        Ok(())
    }

    fn compile_rvalue(&mut self, rvalue: &Rvalue) -> std::result::Result<ValueRef, String> {
        match rvalue {
            Rvalue::Use(operand) => self.load_operand(operand),
            Rvalue::Unary { op, value, .. } => {
                let value = self.load_operand(value)?;
                self.compile_unary(*op, value)
            }
            Rvalue::Cast { value, ty, .. } => {
                let value = self.load_operand(value)?;
                self.compile_cast(value, ty)
            }
            Rvalue::Binary {
                op, left, right, ..
            } => {
                let left = self.load_operand(left)?;
                let right = self.load_operand(right)?;
                self.compile_binary(*op, left, right)
            }
            Rvalue::Call { callee, args } => self.compile_call(callee, args),
            Rvalue::Construct { class_name, fields } => self.compile_construct(class_name, fields),
            Rvalue::Member { object, field } => {
                let object = self.load_operand(object)?;
                self.extract_field(object, field)
            }
            Rvalue::EnumVariant {
                enum_name,
                variant_name,
                payload,
            } => self.compile_enum_variant(enum_name, variant_name, payload.as_ref()),
            Rvalue::VariantPayload { scrutinee } => {
                let scrutinee = self.load_operand(scrutinee)?;
                self.compile_variant_payload(scrutinee)
            }
            Rvalue::Spawn {
                detached,
                task_group,
                function,
                args,
            } => self.compile_spawn(*detached, task_group.as_ref(), function, args),
            other => Err(format!(
                "direct backend does not support MIR rvalue `{:?}`",
                other
            )),
        }
    }

    fn compile_unary(
        &mut self,
        op: UnaryOp,
        value: ValueRef,
    ) -> std::result::Result<ValueRef, String> {
        if matches!(value.ty, DirectType::Opaque(_)) {
            let opcode = match op {
                UnaryOp::Neg => 0,
                UnaryOp::Not => 1,
            };
            let opcode = self.builder.ins().iconst(types::I64, opcode);
            let inst = self
                .builder
                .ins()
                .call(self.unary_value, &[opcode, value.values[0]]);
            return Ok(ValueRef {
                values: self.builder.inst_results(inst).to_vec(),
                ty: DirectType::Opaque(Type::named("Unknown")),
            });
        }
        match (op, value.ty.scalar_kind()) {
            (UnaryOp::Neg, Some(ScalarKind::Int32)) => Ok(ValueRef {
                values: vec![self.builder.ins().ineg(value.values[0])],
                ty: DirectType::Scalar(ScalarKind::Int32),
            }),
            (UnaryOp::Neg, Some(kind)) if kind.is_float() => Ok(ValueRef {
                values: vec![self.builder.ins().fneg(value.values[0])],
                ty: DirectType::Scalar(kind),
            }),
            (UnaryOp::Not, Some(ScalarKind::Bool)) => {
                let zero = self.builder.ins().iconst(types::I64, 0);
                let cmp = self.builder.ins().icmp(IntCC::Equal, value.values[0], zero);
                Ok(ValueRef {
                    values: vec![self.builder.ins().uextend(types::I64, cmp)],
                    ty: DirectType::Scalar(ScalarKind::Bool),
                })
            }
            _ => Err(format!(
                "direct backend does not support unary operation `{:?}` for the current operand type",
                op
            )),
        }
    }

    fn compile_cast(
        &mut self,
        value: ValueRef,
        target: &Type,
    ) -> std::result::Result<ValueRef, String> {
        let target_ty = ensure_direct_type(target, &self.classes, "cast target")?;
        if matches!(value.ty, DirectType::Opaque(_)) || matches!(target_ty, DirectType::Opaque(_)) {
            let boxed = self.ensure_opaque(value)?;
            let (target_ptr, target_len) = self.string_constant(target.to_string().as_bytes())?;
            let inst = self
                .builder
                .ins()
                .call(self.cast_value, &[boxed.values[0], target_ptr, target_len]);
            let boxed = ValueRef {
                values: self.builder.inst_results(inst).to_vec(),
                ty: DirectType::Opaque(target.clone()),
            };
            return self.coerce_value(boxed, &target_ty);
        }
        let Some(target_kind) = target_ty.scalar_kind() else {
            return Err(format!(
                "direct backend only supports numeric casts, found target `{}`",
                target
            ));
        };
        let Some(source_kind) = value.ty.scalar_kind() else {
            return Err(format!(
                "direct backend only supports numeric casts from scalar values, found `{}`",
                render_direct_type(&value.ty)
            ));
        };

        let source = value.values[0];
        let result = match (source_kind, target_kind) {
            (ScalarKind::Int32, ScalarKind::Int32) => source,
            (ScalarKind::Int32, kind) if kind.is_float() => self
                .builder
                .ins()
                .fcvt_from_sint(target_kind.signature_type(), source),
            (kind, ScalarKind::Int32) if kind.is_float() => {
                let converted = self.builder.ins().fcvt_to_sint_sat(types::I64, source);
                self.emit_int32_bounds_check(converted);
                converted
            }
            (lhs, rhs) if lhs.is_float() && rhs.is_float() => source,
            _ => {
                return Err(format!(
                    "direct backend only supports numeric casts, found `{}` to `{}`",
                    render_direct_type(&value.ty),
                    target
                ))
            }
        };

        Ok(ValueRef {
            values: vec![result],
            ty: DirectType::Scalar(target_kind),
        })
    }

    fn compile_binary(
        &mut self,
        op: BinaryOp,
        left: ValueRef,
        right: ValueRef,
    ) -> std::result::Result<ValueRef, String> {
        if matches!(left.ty, DirectType::Opaque(_)) || matches!(right.ty, DirectType::Opaque(_)) {
            let left = self.ensure_opaque(left)?;
            let right = self.ensure_opaque(right)?;
            let binary_opcode = self.binary_opcode(op);
            let opcode = self.builder.ins().iconst(types::I64, binary_opcode);
            let inst = self.builder.ins().call(
                self.binary_value,
                &[opcode, left.values[0], right.values[0]],
            );
            return Ok(ValueRef {
                values: self.builder.inst_results(inst).to_vec(),
                ty: DirectType::Opaque(Type::named("Unknown")),
            });
        }
        match (left.ty.scalar_kind(), right.ty.scalar_kind()) {
            (Some(ScalarKind::Int32), Some(ScalarKind::Int32)) => {
                self.compile_int32_binary(op, left.values[0], right.values[0])
            }
            (Some(lhs), Some(rhs)) if lhs.is_float() && rhs.is_float() => {
                self.compile_float_binary(op, left.values[0], right.values[0], lhs)
            }
            (Some(ScalarKind::Bool), Some(ScalarKind::Bool)) => {
                self.compile_bool_binary(op, left.values[0], right.values[0])
            }
            _ => Err(format!(
                "direct backend does not support binary operation `{:?}` for the current operand types",
                op
            )),
        }
    }

    fn compile_int32_binary(
        &mut self,
        op: BinaryOp,
        left: Value,
        right: Value,
    ) -> std::result::Result<ValueRef, String> {
        let ty = DirectType::Scalar(ScalarKind::Int32);
        let value = match op {
            BinaryOp::Add => ValueRef {
                values: vec![self.builder.ins().iadd(left, right)],
                ty,
            },
            BinaryOp::Sub => ValueRef {
                values: vec![self.builder.ins().isub(left, right)],
                ty,
            },
            BinaryOp::Mul => ValueRef {
                values: vec![self.builder.ins().imul(left, right)],
                ty,
            },
            BinaryOp::Div => {
                self.emit_int_division_guard(right);
                ValueRef {
                    values: vec![self.builder.ins().sdiv(left, right)],
                    ty,
                }
            }
            BinaryOp::Mod => {
                self.emit_int_division_guard(right);
                ValueRef {
                    values: vec![self.builder.ins().srem(left, right)],
                    ty,
                }
            }
            BinaryOp::Eq => self.boolean_from_icmp(IntCC::Equal, left, right),
            BinaryOp::NotEq => self.boolean_from_icmp(IntCC::NotEqual, left, right),
            BinaryOp::Less => self.boolean_from_icmp(IntCC::SignedLessThan, left, right),
            BinaryOp::LessEq => self.boolean_from_icmp(IntCC::SignedLessThanOrEqual, left, right),
            BinaryOp::Greater => self.boolean_from_icmp(IntCC::SignedGreaterThan, left, right),
            BinaryOp::GreaterEq => {
                self.boolean_from_icmp(IntCC::SignedGreaterThanOrEqual, left, right)
            }
            other => {
                return Err(format!(
                    "direct backend does not support integer binary operation `{:?}`",
                    other
                ))
            }
        };

        if matches!(value.ty.scalar_kind(), Some(ScalarKind::Int32)) {
            self.emit_int32_bounds_check(value.values[0]);
        }
        Ok(value)
    }

    fn compile_float_binary(
        &mut self,
        op: BinaryOp,
        left: Value,
        right: Value,
        kind: ScalarKind,
    ) -> std::result::Result<ValueRef, String> {
        let ty = DirectType::Scalar(kind);
        match op {
            BinaryOp::Add => Ok(ValueRef {
                values: vec![self.builder.ins().fadd(left, right)],
                ty,
            }),
            BinaryOp::Sub => Ok(ValueRef {
                values: vec![self.builder.ins().fsub(left, right)],
                ty,
            }),
            BinaryOp::Mul => Ok(ValueRef {
                values: vec![self.builder.ins().fmul(left, right)],
                ty,
            }),
            BinaryOp::Div => {
                self.emit_float_division_guard(right);
                Ok(ValueRef {
                    values: vec![self.builder.ins().fdiv(left, right)],
                    ty,
                })
            }
            BinaryOp::Mod => {
                let opcode_value = self.binary_opcode(BinaryOp::Mod);
                let left_box = self.builder.ins().call(self.box_f64, &[left]);
                let right_box = self.builder.ins().call(self.box_f64, &[right]);
                let left_boxed = self.builder.inst_results(left_box)[0];
                let right_boxed = self.builder.inst_results(right_box)[0];
                let opcode = self.builder.ins().iconst(types::I64, opcode_value);
                let result = self
                    .builder
                    .ins()
                    .call(self.binary_value, &[opcode, left_boxed, right_boxed]);
                let result_boxed = self.builder.inst_results(result)[0];
                let unboxed = self.builder.ins().call(self.unbox_f64, &[result_boxed]);
                Ok(ValueRef {
                    values: self.builder.inst_results(unboxed).to_vec(),
                    ty,
                })
            }
            BinaryOp::Eq => Ok(self.boolean_from_fcmp(FloatCC::Equal, left, right)),
            BinaryOp::NotEq => Ok(self.boolean_from_fcmp(FloatCC::NotEqual, left, right)),
            BinaryOp::Less => Ok(self.boolean_from_fcmp(FloatCC::LessThan, left, right)),
            BinaryOp::LessEq => Ok(self.boolean_from_fcmp(FloatCC::LessThanOrEqual, left, right)),
            BinaryOp::Greater => Ok(self.boolean_from_fcmp(FloatCC::GreaterThan, left, right)),
            BinaryOp::GreaterEq => {
                Ok(self.boolean_from_fcmp(FloatCC::GreaterThanOrEqual, left, right))
            }
            other => Err(format!(
                "direct backend does not support float binary operation `{:?}`",
                other
            )),
        }
    }

    fn compile_bool_binary(
        &mut self,
        op: BinaryOp,
        left: Value,
        right: Value,
    ) -> std::result::Result<ValueRef, String> {
        match op {
            BinaryOp::Eq => Ok(self.boolean_from_icmp(IntCC::Equal, left, right)),
            BinaryOp::NotEq => Ok(self.boolean_from_icmp(IntCC::NotEqual, left, right)),
            BinaryOp::And => Ok(ValueRef {
                values: vec![self.builder.ins().band(left, right)],
                ty: DirectType::Scalar(ScalarKind::Bool),
            }),
            BinaryOp::Or => Ok(ValueRef {
                values: vec![self.builder.ins().bor(left, right)],
                ty: DirectType::Scalar(ScalarKind::Bool),
            }),
            other => Err(format!(
                "direct backend does not support boolean binary operation `{:?}`",
                other
            )),
        }
    }

    fn boolean_from_icmp(&mut self, cc: IntCC, left: Value, right: Value) -> ValueRef {
        let cmp = self.builder.ins().icmp(cc, left, right);
        ValueRef {
            values: vec![self.builder.ins().uextend(types::I64, cmp)],
            ty: DirectType::Scalar(ScalarKind::Bool),
        }
    }

    fn boolean_from_fcmp(&mut self, cc: FloatCC, left: Value, right: Value) -> ValueRef {
        let cmp = self.builder.ins().fcmp(cc, left, right);
        ValueRef {
            values: vec![self.builder.ins().uextend(types::I64, cmp)],
            ty: DirectType::Scalar(ScalarKind::Bool),
        }
    }

    fn emit_int_division_guard(&mut self, divisor: Value) {
        let zero = self.builder.ins().iconst(types::I64, 0);
        let is_zero = self.builder.ins().icmp(IntCC::Equal, divisor, zero);
        self.emit_division_failure_branch(is_zero);
    }

    fn emit_float_division_guard(&mut self, divisor: Value) {
        let zero = self.builder.ins().f64const(Ieee64::with_float(0.0));
        let is_zero = self.builder.ins().fcmp(FloatCC::Equal, divisor, zero);
        self.emit_division_failure_branch(is_zero);
    }

    fn emit_division_failure_branch(&mut self, is_zero: Value) {
        let fail_block = self.builder.create_block();
        let continue_block = self.builder.create_block();
        self.builder
            .ins()
            .brif(is_zero, fail_block, &[], continue_block, &[]);
        self.builder.switch_to_block(fail_block);
        self.builder.ins().call(self.fail_division_by_zero, &[]);
        self.builder.ins().trap(TrapCode::INTEGER_DIVISION_BY_ZERO);
        self.builder.seal_block(fail_block);
        self.builder.switch_to_block(continue_block);
        self.builder.seal_block(continue_block);
    }

    fn compile_call(
        &mut self,
        callee: &CallTarget,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        match callee {
            CallTarget::Name(name) if name == "print" => self.compile_print(args),
            CallTarget::Name(name) => self.compile_named_call(name, args),
            CallTarget::Member {
                object,
                field,
                receiver_place,
            } => self.compile_member_call(object, field, receiver_place.as_deref(), args),
        }
    }

    fn compile_print(&mut self, args: &[MirArg]) -> std::result::Result<ValueRef, String> {
        let Some(argument) = args.first() else {
            return Err("direct backend expected `print` to receive one argument".to_string());
        };
        let argument = self.load_operand(&argument.value)?;
        match argument.ty.scalar_kind() {
            Some(ScalarKind::Int32) => {
                self.builder
                    .ins()
                    .call(self.print_i64, &[argument.values[0]]);
            }
            Some(ScalarKind::Float32) | Some(ScalarKind::Float64) => {
                self.builder
                    .ins()
                    .call(self.print_f64, &[argument.values[0]]);
            }
            Some(ScalarKind::Bool) => {
                self.builder
                    .ins()
                    .call(self.print_bool, &[argument.values[0]]);
            }
            Some(ScalarKind::Unit) => {}
            None => {
                let argument = self.ensure_opaque(argument)?;
                self.builder
                    .ins()
                    .call(self.print_value, &[argument.values[0]]);
            }
        }
        Ok(unit_value(&mut self.builder))
    }

    fn compile_named_call(
        &mut self,
        name: &str,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        if name == "range" {
            return self.compile_range(args);
        }
        if name == "channel" {
            if !args.is_empty() {
                return Err("direct backend expected `channel()` to take no arguments".to_string());
            }
            let inst = self.builder.ins().call(self.channel_new, &[]);
            return Ok(ValueRef {
                values: self.builder.inst_results(inst).to_vec(),
                ty: DirectType::Opaque(Type::Named(
                    "Channel".to_string(),
                    vec![Type::named("Unknown")],
                )),
            });
        }
        if name == "task_group" {
            if !args.is_empty() {
                return Err(
                    "direct backend expected `task_group()` to take no arguments".to_string(),
                );
            }
            let inst = self.builder.ins().call(self.task_group_new, &[]);
            return Ok(ValueRef {
                values: self.builder.inst_results(inst).to_vec(),
                ty: DirectType::Opaque(Type::named("TaskGroup")),
            });
        }
        if name == "cancelled" {
            if !args.is_empty() {
                return Err(
                    "direct backend expected `cancelled()` to take no arguments".to_string()
                );
            }
            let inst = self.builder.ins().call(self.cancelled, &[]);
            return Ok(ValueRef {
                values: self.builder.inst_results(inst).to_vec(),
                ty: DirectType::Scalar(ScalarKind::Bool),
            });
        }
        if name == "sleep" {
            let [argument] = args else {
                return Err(
                    "direct backend expected `sleep()` to receive one duration argument"
                        .to_string(),
                );
            };
            let duration = self.load_operand(&argument.value)?;
            let duration = self.ensure_opaque(duration)?;
            let inst = self
                .builder
                .ins()
                .call(self.sleep_value, &[duration.values[0]]);
            return Ok(ValueRef {
                values: self.builder.inst_results(inst).to_vec(),
                ty: DirectType::Scalar(ScalarKind::Unit),
            });
        }
        let func_ref = *self
            .function_refs
            .get(name)
            .ok_or_else(|| format!("direct backend does not know function `{}`", name))?;
        let mut lowered_args = Vec::new();
        let expected = self
            .function_param_types
            .get(name)
            .cloned()
            .unwrap_or_default();
        let mut writeback_places = Vec::new();
        for (index, argument) in args.iter().enumerate() {
            let loaded = self.load_operand(&argument.value)?;
            let coerced = if let Some(expected_ty) = expected.get(index) {
                self.coerce_value(loaded, expected_ty)?
            } else {
                loaded
            };
            if let Some(place) = &argument.writeback_place {
                writeback_places.push(place.clone());
            }
            lowered_args.extend(coerced.values);
        }
        let inst = self.builder.ins().call(func_ref, &lowered_args);
        let results = self.builder.inst_results(inst).to_vec();
        let (result, writebacks) = self.split_call_results(name, results)?;
        self.apply_writeback_places(&writeback_places, writebacks)?;
        Ok(result)
    }

    fn compile_range(&mut self, args: &[MirArg]) -> std::result::Result<ValueRef, String> {
        let int_ty = DirectType::Scalar(ScalarKind::Int32);
        let (start_arg, stop_arg) = if args.iter().all(|arg| arg.name.is_none()) {
            match args {
                [stop] => (None, Some(stop)),
                [start, stop] => (Some(start), Some(stop)),
                _ => {
                    return Err(
                        "direct backend expected `range()` to receive one or two arguments"
                            .to_string(),
                    )
                }
            }
        } else {
            let mut start = None;
            let mut stop = None;
            let mut next_positional = 0usize;
            for arg in args {
                match arg.name.as_deref() {
                    Some("start") => start = Some(arg),
                    Some("stop") => stop = Some(arg),
                    Some(other) => {
                        return Err(format!(
                            "direct backend does not recognize `range()` argument `{}`",
                            other
                        ))
                    }
                    None => {
                        if next_positional == 0 {
                            start = Some(arg);
                        } else if next_positional == 1 {
                            stop = Some(arg);
                        } else {
                            return Err(
                                "direct backend expected `range()` to receive one or two arguments"
                                    .to_string(),
                            );
                        }
                        next_positional += 1;
                    }
                }
            }
            (start, stop)
        };

        let start = if let Some(argument) = start_arg {
            let loaded = self.load_operand(&argument.value)?;
            self.coerce_value(loaded, &int_ty)?
        } else {
            ValueRef {
                values: vec![self.builder.ins().iconst(types::I64, 0)],
                ty: int_ty.clone(),
            }
        };
        let stop_arg = stop_arg.ok_or_else(|| {
            "direct backend expected `range()` to receive a `stop` argument".to_string()
        })?;
        let stop = self.load_operand(&stop_arg.value)?;
        let stop = self.coerce_value(stop, &int_ty)?;
        let inst = self
            .builder
            .ins()
            .call(self.range_new, &[start.values[0], stop.values[0]]);
        Ok(ValueRef {
            values: self.builder.inst_results(inst).to_vec(),
            ty: DirectType::Opaque(Type::named("Range")),
        })
    }

    fn compile_for_range(
        &mut self,
        binding: &str,
        iterable: &Operand,
        body_label: &str,
        exit_label: &str,
    ) -> std::result::Result<(), String> {
        let Operand::Place(iterable_place) = iterable else {
            return Err(
                "direct backend requires `for range` iterables to live in a place".to_string(),
            );
        };
        let range = self.load_place(iterable_place)?;
        let range = self.ensure_opaque(range)?;
        let current_inst = self
            .builder
            .ins()
            .call(self.range_current, &[range.values[0]]);
        let current = self.builder.inst_results(current_inst)[0];
        let end_inst = self.builder.ins().call(self.range_end, &[range.values[0]]);
        let end = self.builder.inst_results(end_inst)[0];
        let has_next = self.builder.ins().icmp(IntCC::SignedLessThan, current, end);

        let next_block = self.builder.create_block();
        let body_block = self.blocks[body_label];
        let exit_block = self.blocks[exit_label];
        self.builder
            .ins()
            .brif(has_next, next_block, &[], exit_block, &[]);

        self.builder.switch_to_block(next_block);
        let binding_ty = self.type_of_place(binding)?;
        self.store_place(
            binding,
            ValueRef {
                values: vec![current],
                ty: DirectType::Scalar(ScalarKind::Int32),
            },
        )?;
        let advanced_inst = self
            .builder
            .ins()
            .call(self.range_advance, &[range.values[0]]);
        self.store_place(
            iterable_place,
            ValueRef {
                values: self.builder.inst_results(advanced_inst).to_vec(),
                ty: DirectType::Opaque(Type::named("Range")),
            },
        )?;
        self.builder.ins().jump(body_block, &[]);
        self.builder.seal_block(next_block);
        let _ = binding_ty;
        Ok(())
    }

    fn compile_member_call(
        &mut self,
        object: &Operand,
        field: &str,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let object = self.load_operand(object)?;

        if matches!(object.ty.scalar_kind(), Some(kind) if kind.is_float()) && field == "sqrt" {
            if !args.is_empty() {
                return Err("direct backend expected `sqrt()` to take no arguments".to_string());
            }
            let inst = self.builder.ins().call(self.sqrt_f64, &[object.values[0]]);
            return Ok(ValueRef {
                values: self.builder.inst_results(inst).to_vec(),
                ty: object.ty,
            });
        }

        match object.ty.clone() {
            DirectType::PlainClass(class_ty) => self.compile_class_member_call(
                class_ty.class_name.as_str(),
                Some(Type::named(&class_ty.class_name)),
                object,
                field,
                receiver_place,
                args,
            ),
            DirectType::Opaque(ty) => {
                if let Type::Named(name, _type_args) = &ty {
                    if name == "String" && field == "clone" {
                        if !args.is_empty() || !args.is_empty() {
                            return Err("direct backend expected `clone()` to take no arguments"
                                .to_string());
                        }
                        return Ok(self.ensure_opaque(object)?);
                    }
                    return self.compile_opaque_member_call(
                        &ty,
                        object,
                        field,
                        receiver_place,
                        args,
                    );
                }
                self.compile_opaque_member_call(&ty, object, field, receiver_place, args)
            }
            DirectType::Scalar(_) => {
                let receiver_ty = direct_type_to_type(&object.ty);
                if self.find_trait_method(&receiver_ty, field).is_some() {
                    return self.compile_class_member_call(
                        &receiver_ty.to_string(),
                        Some(receiver_ty),
                        object,
                        field,
                        receiver_place,
                        args,
                    );
                }
                Err(format!(
                    "direct backend does not support member call `.{}` on `{}`",
                    field,
                    render_direct_type(&object.ty)
                ))
            }
        }
    }

    fn compile_construct(
        &mut self,
        class_name: &str,
        fields: &[crate::mir::MirFieldInit],
    ) -> std::result::Result<ValueRef, String> {
        let ty = ensure_direct_type(
            &Type::named(class_name),
            &self.classes,
            &format!("class `{}`", class_name),
        )?;
        match &ty {
            DirectType::PlainClass(class_ty) => {
                let mut by_name = HashMap::new();
                for field in fields {
                    by_name.insert(field.name.clone(), field.value.clone());
                }

                let mut values = Vec::new();
                for field in &class_ty.fields {
                    let operand = by_name.get(&field.name).ok_or_else(|| {
                        format!(
                            "direct backend construction for `{}` is missing field `{}`",
                            class_name, field.name
                        )
                    })?;
                    let value = self.load_operand(operand)?;
                    let coerced = self.coerce_value(value, &field.ty)?;
                    values.extend(coerced.values);
                }

                Ok(ValueRef {
                    values,
                    ty: ty.clone(),
                })
            }
            DirectType::Opaque(_) => self.compile_opaque_construct(class_name, fields),
            DirectType::Scalar(_) => Err(format!(
                "direct backend could not construct non-class type `{}`",
                class_name
            )),
        }
    }

    fn call_result_type(&self, name: &str) -> std::result::Result<DirectType, String> {
        self.function_return_types
            .get(name)
            .cloned()
            .ok_or_else(|| format!("direct backend does not know return type for `{}`", name))
    }

    fn type_of_place(&self, place: &str) -> std::result::Result<DirectType, String> {
        let mut segments = place.split('.');
        let root = segments
            .next()
            .ok_or_else(|| "direct backend encountered an empty place".to_string())?;
        let mut ty = self
            .variable_types
            .get(root)
            .cloned()
            .ok_or_else(|| format!("direct backend does not know local `{}`", root))?;
        for field in segments {
            let (_, _, field_ty) = ty.field_slice(field).ok_or_else(|| {
                format!(
                    "direct backend does not know field `{}` on `{}`",
                    field,
                    render_direct_type(&ty)
                )
            })?;
            ty = field_ty;
        }
        Ok(ty)
    }

    fn load_operand(&mut self, operand: &Operand) -> std::result::Result<ValueRef, String> {
        match operand {
            Operand::Place(place) => self.load_place(place),
            Operand::Int(value) => {
                if let Ok(narrowed) = i64::try_from(*value) {
                    return Ok(ValueRef {
                        values: vec![self.builder.ins().iconst(types::I64, narrowed)],
                        ty: DirectType::Scalar(ScalarKind::Int32),
                    });
                }
                let (ptr, len) = self.string_constant(value.to_string().as_bytes())?;
                let inst = self.builder.ins().call(self.box_uint_literal, &[ptr, len]);
                Ok(ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::named("Unknown")),
                })
            }
            Operand::Float(value) => Ok(ValueRef {
                values: vec![self.builder.ins().f64const(Ieee64::with_float(*value))],
                ty: DirectType::Scalar(ScalarKind::Float64),
            }),
            Operand::String(value) => {
                let (ptr, len) = self.string_constant(value.as_bytes())?;
                let inst = self.builder.ins().call(self.string_literal, &[ptr, len]);
                Ok(ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::named("String")),
                })
            }
            Operand::Duration(value) => {
                let narrowed = i64::try_from(*value).map_err(|_| {
                    format!(
                        "direct backend only supports duration constants that fit in host i64, found `{}`",
                        value
                    )
                })?;
                let narrowed = self.builder.ins().iconst(types::I64, narrowed);
                let inst = self.builder.ins().call(self.duration_literal, &[narrowed]);
                Ok(ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::named("Duration")),
                })
            }
            Operand::Bool(value) => Ok(ValueRef {
                values: vec![self
                    .builder
                    .ins()
                    .iconst(types::I64, if *value { 1 } else { 0 })],
                ty: DirectType::Scalar(ScalarKind::Bool),
            }),
            Operand::Unit => Ok(unit_value(&mut self.builder)),
        }
    }

    fn load_place(&mut self, place: &str) -> std::result::Result<ValueRef, String> {
        let mut segments = place.split('.');
        let root = segments
            .next()
            .ok_or_else(|| "direct backend encountered an empty place".to_string())?;
        let mut value = self.load_root(root)?;
        for field in segments {
            value = self.extract_field(value, field)?;
        }
        Ok(value)
    }

    fn load_root(&mut self, name: &str) -> std::result::Result<ValueRef, String> {
        let vars = self
            .variables
            .get(name)
            .ok_or_else(|| format!("direct backend does not know local `{}`", name))?
            .clone();
        let ty = self
            .variable_types
            .get(name)
            .cloned()
            .ok_or_else(|| format!("direct backend does not know local type for `{}`", name))?;
        let values = vars
            .into_iter()
            .map(|var| self.builder.use_var(var))
            .collect::<Vec<_>>();
        let value = ValueRef { values, ty };
        if matches!(value.ty, DirectType::Opaque(_)) {
            self.ensure_opaque(value)
        } else {
            Ok(value)
        }
    }

    fn extract_field(
        &mut self,
        object: ValueRef,
        field: &str,
    ) -> std::result::Result<ValueRef, String> {
        match &object.ty {
            DirectType::PlainClass(_) => {
                let (start, end, field_ty) = object.ty.field_slice(field).ok_or_else(|| {
                    format!(
                        "direct backend does not know field `{}` on `{}`",
                        field,
                        render_direct_type(&object.ty)
                    )
                })?;
                Ok(ValueRef {
                    values: object.values[start..end].to_vec(),
                    ty: field_ty,
                })
            }
            DirectType::Opaque(_) => {
                let (field_ptr, field_len) = self.string_constant(field.as_bytes())?;
                let inst = self.builder.ins().call(
                    self.instance_get_field,
                    &[object.values[0], field_ptr, field_len],
                );
                Ok(ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::named("Unknown")),
                })
            }
            DirectType::Scalar(_) => Err(format!(
                "direct backend does not know field `{}` on `{}`",
                field,
                render_direct_type(&object.ty)
            )),
        }
    }

    fn coerce_value(
        &mut self,
        value: ValueRef,
        target: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        if &value.ty == target {
            if matches!(target.scalar_kind(), Some(ScalarKind::Int32)) {
                self.emit_int32_bounds_check(value.values[0]);
            }
            return Ok(value);
        }

        if let DirectType::Opaque(target_ty) = target {
            if is_numeric_type_name(target_ty) {
                let boxed = self.ensure_opaque(value)?;
                let (target_ptr, target_len) =
                    self.string_constant(target_ty.to_string().as_bytes())?;
                let inst = self
                    .builder
                    .ins()
                    .call(self.cast_value, &[boxed.values[0], target_ptr, target_len]);
                return Ok(ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: target.clone(),
                });
            }
            return self.ensure_opaque(value);
        }

        if matches!(value.ty, DirectType::Opaque(_)) {
            let result = match target {
                DirectType::Scalar(ScalarKind::Int32) => {
                    let inst = self.builder.ins().call(self.unbox_i64, &[value.values[0]]);
                    ValueRef {
                        values: self.builder.inst_results(inst).to_vec(),
                        ty: target.clone(),
                    }
                }
                DirectType::Scalar(ScalarKind::Float32)
                | DirectType::Scalar(ScalarKind::Float64) => {
                    let inst = self.builder.ins().call(self.unbox_f64, &[value.values[0]]);
                    ValueRef {
                        values: self.builder.inst_results(inst).to_vec(),
                        ty: target.clone(),
                    }
                }
                DirectType::Scalar(ScalarKind::Bool) => {
                    let inst = self.builder.ins().call(self.unbox_bool, &[value.values[0]]);
                    ValueRef {
                        values: self.builder.inst_results(inst).to_vec(),
                        ty: target.clone(),
                    }
                }
                DirectType::Scalar(ScalarKind::Unit) => unit_value(&mut self.builder),
                DirectType::PlainClass(class) => {
                    let mut values = Vec::new();
                    for field in &class.fields {
                        let (field_ptr, field_len) = self.string_constant(field.name.as_bytes())?;
                        let inst = self.builder.ins().call(
                            self.instance_get_field,
                            &[value.values[0], field_ptr, field_len],
                        );
                        let field_value = ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Opaque(Type::named("Unknown")),
                        };
                        let coerced = self.coerce_value(field_value, &field.ty)?;
                        values.extend(coerced.values);
                    }
                    ValueRef {
                        values,
                        ty: target.clone(),
                    }
                }
                DirectType::Opaque(_) => unreachable!("opaque target handled earlier"),
            };
            if matches!(target.scalar_kind(), Some(ScalarKind::Int32)) {
                self.emit_int32_bounds_check(result.values[0]);
            }
            return Ok(result);
        }

        match (value.ty.scalar_kind(), target.scalar_kind()) {
            (Some(ScalarKind::Bool), Some(ScalarKind::Int32)) => Ok(ValueRef {
                values: value.values,
                ty: target.clone(),
            }),
            (Some(lhs), Some(rhs)) if lhs.is_float() && rhs.is_float() => Ok(ValueRef {
                values: value.values,
                ty: target.clone(),
            }),
            (Some(ScalarKind::Int32), Some(ScalarKind::Bool)) => Ok(ValueRef {
                values: value.values,
                ty: target.clone(),
            }),
            (Some(ScalarKind::Unit), Some(ScalarKind::Int32)) => Ok(ValueRef {
                values: vec![self.builder.ins().iconst(types::I64, 0)],
                ty: target.clone(),
            }),
            _ => Err(format!(
                "direct backend encountered an unsupported value coercion from `{}` to `{}`",
                render_direct_type(&value.ty),
                render_direct_type(target)
            )),
        }
    }

    fn emit_int32_bounds_check(&mut self, value: Value) {
        let min = self.builder.ins().iconst(types::I64, i32::MIN as i64);
        let max = self.builder.ins().iconst(types::I64, i32::MAX as i64);
        let below = self.builder.ins().icmp(IntCC::SignedLessThan, value, min);
        let above = self
            .builder
            .ins()
            .icmp(IntCC::SignedGreaterThan, value, max);
        let overflow = self.builder.ins().bor(below, above);
        let fail_block = self.builder.create_block();
        let continue_block = self.builder.create_block();
        self.builder
            .ins()
            .brif(overflow, fail_block, &[], continue_block, &[]);
        self.builder.switch_to_block(fail_block);
        self.builder.ins().call(self.fail_int32_overflow, &[value]);
        self.builder.ins().trap(TrapCode::INTEGER_OVERFLOW);
        self.builder.seal_block(fail_block);
        self.builder.switch_to_block(continue_block);
        self.builder.seal_block(continue_block);
    }

    fn as_bool_value(&mut self, value: ValueRef) -> std::result::Result<Value, String> {
        match value.ty.scalar_kind() {
            Some(ScalarKind::Bool) | Some(ScalarKind::Int32) | Some(ScalarKind::Unit) => {
                let zero = self.builder.ins().iconst(types::I64, 0);
                Ok(self
                    .builder
                    .ins()
                    .icmp(IntCC::NotEqual, value.values[0], zero))
            }
            None if matches!(value.ty, DirectType::Opaque(_)) => {
                let inst = self
                    .builder
                    .ins()
                    .call(self.value_as_condition, &[value.values[0]]);
                Ok(self.builder.inst_results(inst)[0])
            }
            other => Err(format!(
                "direct backend cannot use `{}` as a branch condition",
                match other {
                    Some(kind) => render_direct_type(&DirectType::Scalar(kind)),
                    None => render_direct_type(&value.ty),
                }
            )),
        }
    }

    fn store_place(&mut self, place: &str, value: ValueRef) -> std::result::Result<(), String> {
        let mut segments = place.split('.').collect::<Vec<_>>();
        let root = segments.remove(0);
        if segments.is_empty() {
            return self.store_root(root, value);
        }

        if matches!(self.variable_types.get(root), Some(DirectType::Opaque(_)))
            && segments.len() == 1
        {
            let current = self.load_root(root)?;
            let current = self.ensure_opaque(current)?;
            let updated_value = self.ensure_opaque(value)?;
            let (field_ptr, field_len) = self.string_constant(segments[0].as_bytes())?;
            let inst = self.builder.ins().call(
                self.instance_set_field,
                &[
                    current.values[0],
                    field_ptr,
                    field_len,
                    updated_value.values[0],
                ],
            );
            return self.store_root(
                root,
                ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: self.variable_types.get(root).cloned().ok_or_else(|| {
                        format!("direct backend does not know local type for `{}`", root)
                    })?,
                },
            );
        }

        let root_value = self.load_root(root)?;
        let updated = self.replace_nested_field(root_value, &segments, value)?;
        self.store_root(root, updated)
    }

    fn replace_nested_field(
        &mut self,
        current: ValueRef,
        segments: &[&str],
        new_value: ValueRef,
    ) -> std::result::Result<ValueRef, String> {
        let (start, end, field_ty) = current.ty.field_slice(segments[0]).ok_or_else(|| {
            format!(
                "direct backend does not know field `{}` on `{}`",
                segments[0],
                render_direct_type(&current.ty)
            )
        })?;

        let replacement = if segments.len() == 1 {
            self.coerce_value(new_value, &field_ty)?
        } else {
            let nested = ValueRef {
                values: current.values[start..end].to_vec(),
                ty: field_ty.clone(),
            };
            self.replace_nested_field(nested, &segments[1..], new_value)?
        };

        let mut values = Vec::with_capacity(current.values.len());
        values.extend_from_slice(&current.values[..start]);
        values.extend(replacement.values);
        values.extend_from_slice(&current.values[end..]);
        Ok(ValueRef {
            values,
            ty: current.ty,
        })
    }

    fn store_root(&mut self, name: &str, value: ValueRef) -> std::result::Result<(), String> {
        let expected = self
            .variable_types
            .get(name)
            .cloned()
            .ok_or_else(|| format!("direct backend does not know local type for `{}`", name))?;
        let value = self.coerce_value(value, &expected)?;
        let vars = self
            .variables
            .get(name)
            .cloned()
            .ok_or_else(|| format!("direct backend does not know local `{}`", name))?;
        for (var, compiled) in vars.into_iter().zip(value.values.into_iter()) {
            self.builder.def_var(var, compiled);
        }
        Ok(())
    }

    fn ensure_opaque(&mut self, value: ValueRef) -> std::result::Result<ValueRef, String> {
        match value.ty {
            DirectType::Opaque(_) => Ok(value),
            DirectType::Scalar(ScalarKind::Int32) => {
                let inst = self.builder.ins().call(self.box_i64, &[value.values[0]]);
                Ok(ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::named("int32")),
                })
            }
            DirectType::Scalar(ScalarKind::Float32) | DirectType::Scalar(ScalarKind::Float64) => {
                let inst = self.builder.ins().call(self.box_f64, &[value.values[0]]);
                Ok(ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::named("float64")),
                })
            }
            DirectType::Scalar(ScalarKind::Bool) => {
                let inst = self.builder.ins().call(self.box_bool, &[value.values[0]]);
                Ok(ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::named("bool")),
                })
            }
            DirectType::Scalar(ScalarKind::Unit) => {
                let inst = self.builder.ins().call(self.box_unit, &[]);
                Ok(ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::Unit),
                })
            }
            DirectType::PlainClass(class) => {
                let (class_ptr, class_len) = self.string_constant(class.class_name.as_bytes())?;
                let init = self
                    .builder
                    .ins()
                    .call(self.instance_empty, &[class_ptr, class_len]);
                let mut current = self.builder.inst_results(init)[0];
                let mut start = 0usize;
                for field in &class.fields {
                    let end = start + field.ty.value_count();
                    let field_value = ValueRef {
                        values: value.values[start..end].to_vec(),
                        ty: field.ty.clone(),
                    };
                    let field_value = self.ensure_opaque(field_value)?;
                    let (field_ptr, field_len) = self.string_constant(field.name.as_bytes())?;
                    let inst = self.builder.ins().call(
                        self.instance_set_field,
                        &[current, field_ptr, field_len, field_value.values[0]],
                    );
                    current = self.builder.inst_results(inst)[0];
                    start = end;
                }
                Ok(ValueRef {
                    values: vec![current],
                    ty: DirectType::Opaque(Type::named(&class.class_name)),
                })
            }
        }
    }

    fn binary_opcode(&self, op: BinaryOp) -> i64 {
        match op {
            BinaryOp::Add => 0,
            BinaryOp::Sub => 1,
            BinaryOp::Mul => 2,
            BinaryOp::Div => 3,
            BinaryOp::Mod => 4,
            BinaryOp::Eq => 5,
            BinaryOp::NotEq => 6,
            BinaryOp::Less => 7,
            BinaryOp::LessEq => 8,
            BinaryOp::Greater => 9,
            BinaryOp::GreaterEq => 10,
            BinaryOp::And => 11,
            BinaryOp::Or => 12,
        }
    }

    fn string_constant(&mut self, bytes: &[u8]) -> std::result::Result<(Value, Value), String> {
        let id = if let Some(id) = self.string_data.get(bytes) {
            *id
        } else {
            let name = format!("aurora_data_{}", self.string_data.len());
            let id = self
                .object
                .declare_data(&name, Linkage::Local, false, false)
                .map_err(|error| format!("failed to declare string data: {}", error))?;
            let mut data = DataDescription::new();
            data.define(bytes.to_vec().into_boxed_slice());
            self.object
                .define_data(id, &data)
                .map_err(|error| format!("failed to define string data: {}", error))?;
            self.string_data.insert(bytes.to_vec(), id);
            id
        };
        let global = self.object.declare_data_in_func(id, self.builder.func);
        let ptr = self.builder.ins().symbol_value(types::I64, global);
        let len = self.builder.ins().iconst(types::I64, bytes.len() as i64);
        Ok((ptr, len))
    }

    fn compile_enum_variant(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        payload: Option<&Operand>,
    ) -> std::result::Result<ValueRef, String> {
        let (enum_ptr, enum_len) = self.string_constant(enum_name.as_bytes())?;
        let (variant_ptr, variant_len) = self.string_constant(variant_name.as_bytes())?;
        let payload = if let Some(payload) = payload {
            let loaded = self.load_operand(payload)?;
            self.ensure_opaque(loaded)?.values[0]
        } else {
            self.builder.ins().iconst(types::I64, 0)
        };
        let inst = self.builder.ins().call(
            self.enum_variant,
            &[enum_ptr, enum_len, variant_ptr, variant_len, payload],
        );
        Ok(ValueRef {
            values: self.builder.inst_results(inst).to_vec(),
            ty: DirectType::Opaque(Type::named(enum_name)),
        })
    }

    fn variant_matches_value(
        &mut self,
        value: Value,
        enum_name: &str,
        variant_name: &str,
    ) -> std::result::Result<Value, String> {
        let (enum_ptr, enum_len) = self.string_constant(enum_name.as_bytes())?;
        let (variant_ptr, variant_len) = self.string_constant(variant_name.as_bytes())?;
        let inst = self.builder.ins().call(
            self.variant_matches,
            &[value, enum_ptr, enum_len, variant_ptr, variant_len],
        );
        Ok(self.builder.inst_results(inst)[0])
    }

    fn compile_variant_payload(
        &mut self,
        scrutinee: ValueRef,
    ) -> std::result::Result<ValueRef, String> {
        let scrutinee = self.ensure_opaque(scrutinee)?;
        let inst = self
            .builder
            .ins()
            .call(self.variant_payload, &[scrutinee.values[0]]);
        Ok(ValueRef {
            values: self.builder.inst_results(inst).to_vec(),
            ty: DirectType::Opaque(Type::named("Unknown")),
        })
    }

    fn compile_try_assign(
        &mut self,
        target: &str,
        target_ty: DirectType,
        try_value: &Operand,
    ) -> std::result::Result<(), String> {
        let loaded = self.load_operand(try_value)?;
        let value = self.ensure_opaque(loaded)?;
        let ok = self.variant_matches_value(value.values[0], "Result", "Ok")?;
        let ok_block = self.builder.create_block();
        let err_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder.ins().brif(ok, ok_block, &[], err_block, &[]);

        self.builder.switch_to_block(ok_block);
        let payload = self.compile_variant_payload(value.clone())?;
        let coerced = self.coerce_value(payload, &target_ty)?;
        self.store_place(target, coerced)?;
        self.builder.ins().jump(join_block, &[]);
        self.builder.seal_block(ok_block);

        self.builder.switch_to_block(err_block);
        self.emit_pending_cleanups(true)?;
        let return_values = self.build_return_values(value.clone())?;
        self.builder.ins().return_(&return_values);
        self.builder.seal_block(err_block);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        Ok(())
    }

    fn set_cleanup_active(&mut self, place: &str, active: bool) -> std::result::Result<(), String> {
        let Some(variable) = self.cleanup_active_vars.get(place).copied() else {
            return Err(format!(
                "direct backend does not know cleanup place `{}`",
                place
            ));
        };
        let value = self
            .builder
            .ins()
            .iconst(types::I64, if active { 1 } else { 0 });
        self.builder.def_var(variable, value);
        Ok(())
    }

    fn emit_pending_cleanups(
        &mut self,
        cancel_before_cleanup: bool,
    ) -> std::result::Result<(), String> {
        for place in self.cleanup_places.clone().into_iter().rev() {
            let Some(variable) = self.cleanup_active_vars.get(&place).copied() else {
                continue;
            };
            let active = self.builder.use_var(variable);
            let zero = self.builder.ins().iconst(types::I64, 0);
            let should_run = self.builder.ins().icmp(IntCC::NotEqual, active, zero);
            let run_block = self.builder.create_block();
            let next_block = self.builder.create_block();
            self.builder
                .ins()
                .brif(should_run, run_block, &[], next_block, &[]);
            self.builder.switch_to_block(run_block);
            self.emit_cleanup_for_place(&place, cancel_before_cleanup)?;
            self.builder.def_var(variable, zero);
            self.builder.ins().jump(next_block, &[]);
            self.builder.seal_block(run_block);
            self.builder.switch_to_block(next_block);
            self.builder.seal_block(next_block);
        }
        Ok(())
    }

    fn build_return_values(
        &mut self,
        primary: ValueRef,
    ) -> std::result::Result<Vec<Value>, String> {
        let mut values = primary.values;
        for (name, ty) in self.writeback_locals.clone() {
            let current = self.load_root(&name)?;
            let coerced = self.coerce_value(current, &ty)?;
            values.extend(coerced.values);
        }
        Ok(values)
    }

    fn split_call_results(
        &self,
        function_name: &str,
        results: Vec<Value>,
    ) -> std::result::Result<(ValueRef, Vec<ValueRef>), String> {
        let result_ty = self.call_result_type(function_name)?;
        let result_count = result_ty.value_count();
        if results.len() < result_count {
            return Err(format!(
                "direct backend received too few call results for `{}`",
                function_name
            ));
        }
        let mut cursor = result_count;
        let mut writebacks = Vec::new();
        for ty in self
            .function_writeback_types
            .get(function_name)
            .cloned()
            .unwrap_or_default()
        {
            let count = ty.value_count();
            if results.len() < cursor + count {
                return Err(format!(
                    "direct backend received incomplete writeback results for `{}`",
                    function_name
                ));
            }
            writebacks.push(ValueRef {
                values: results[cursor..cursor + count].to_vec(),
                ty,
            });
            cursor += count;
        }
        Ok((
            ValueRef {
                values: results[..result_count].to_vec(),
                ty: result_ty,
            },
            writebacks,
        ))
    }

    fn apply_writeback_places(
        &mut self,
        places: &[String],
        values: Vec<ValueRef>,
    ) -> std::result::Result<(), String> {
        if places.len() != values.len() {
            return Err(format!(
                "direct backend expected {} writeback values but received {}",
                places.len(),
                values.len()
            ));
        }
        for (place, value) in places.iter().zip(values.into_iter()) {
            self.store_place(place, value)?;
        }
        Ok(())
    }

    fn emit_cleanup_for_place(
        &mut self,
        place: &str,
        cancel_before_cleanup: bool,
    ) -> std::result::Result<(), String> {
        let receiver_ty = self.type_of_place(place)?;
        match &receiver_ty {
            DirectType::PlainClass(class_ty) => {
                let has_close = self
                    .classes
                    .get(&class_ty.class_name)
                    .and_then(|class| class.methods.iter().find(|method| method.name == "close"))
                    .is_some()
                    || self
                        .find_trait_method(&Type::named(&class_ty.class_name), "close")
                        .is_some();
                if has_close {
                    let operand = Operand::Place(place.to_string());
                    let _ = self.compile_member_call(&operand, "close", Some(place), &[])?;
                }
            }
            DirectType::Opaque(ty) => {
                let operand = Operand::Place(place.to_string());
                let loaded = self.load_operand(&operand)?;
                if matches!(ty, Type::Named(name, _) if name == "TaskGroup") {
                    let loaded = self.ensure_opaque(loaded)?;
                    let cancel_before = self
                        .builder
                        .ins()
                        .iconst(types::I64, if cancel_before_cleanup { 1 } else { 0 });
                    let _ = self
                        .builder
                        .ins()
                        .call(self.task_group_close, &[loaded.values[0], cancel_before]);
                    return Ok(());
                }
                if self
                    .compile_opaque_member_call(ty, loaded, "close", Some(place), &[])
                    .is_ok()
                {
                    return Ok(());
                }
            }
            DirectType::Scalar(_) => {}
        }
        Ok(())
    }

    fn compile_class_member_call(
        &mut self,
        class_name: &str,
        receiver_type_hint: Option<Type>,
        object: ValueRef,
        field: &str,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let method = find_method(self.classes.get(class_name), field)
            .cloned()
            .or_else(|| {
                receiver_type_hint
                    .as_ref()
                    .and_then(|ty| self.find_trait_method(ty, field).cloned())
            })
            .or_else(|| {
                self.find_trait_method(&Type::named(class_name), field)
                    .cloned()
            })
            .ok_or_else(|| {
                format!(
                    "direct backend does not know method `{}.{}`",
                    class_name, field
                )
            })?;
        let method_function_name = method.function_name.clone();
        if method.receiver == Some(MirReceiverKind::BorrowMut) && receiver_place.is_none() {
            return Err(format!(
                "direct backend does not yet support temporary mutable receiver method `{}.{}`",
                class_name, field
            ));
        }
        let func_ref = *self
            .function_refs
            .get(&method_function_name)
            .ok_or_else(|| {
                format!(
                    "direct backend does not know function `{}`",
                    method_function_name
                )
            })?;
        let expected = self
            .function_param_types
            .get(&method_function_name)
            .cloned()
            .unwrap_or_default();
        let mut lowered_args = Vec::new();
        let mut writeback_places = Vec::new();
        let receiver_expected = expected
            .first()
            .cloned()
            .unwrap_or_else(|| object.ty.clone());
        lowered_args.extend(
            self.coerce_value(object.clone(), &receiver_expected)?
                .values,
        );
        if method.receiver == Some(MirReceiverKind::BorrowMut) {
            let Some(place) = receiver_place else {
                return Err(format!(
                    "direct backend does not yet support temporary mutable receiver method `{}.{}`",
                    class_name, field
                ));
            };
            writeback_places.push(place.to_string());
        }
        for (index, argument) in args.iter().enumerate() {
            let loaded = self.load_operand(&argument.value)?;
            let coerced = if let Some(expected_ty) = expected.get(index + 1) {
                self.coerce_value(loaded, expected_ty)?
            } else {
                loaded
            };
            if let Some(place) = &argument.writeback_place {
                writeback_places.push(place.clone());
            }
            lowered_args.extend(coerced.values);
        }
        let inst = self.builder.ins().call(func_ref, &lowered_args);
        let results = self.builder.inst_results(inst).to_vec();
        let (result, writebacks) = self.split_call_results(&method_function_name, results)?;
        self.apply_writeback_places(&writeback_places, writebacks)?;
        Ok(result)
    }

    fn compile_opaque_member_call(
        &mut self,
        object_ty: &Type,
        object: ValueRef,
        field: &str,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        if field == "clone" {
            if !args.is_empty() {
                return Err("direct backend expected `clone()` to take no arguments".to_string());
            }
            let object = self.ensure_opaque(object)?;
            let inst = self
                .builder
                .ins()
                .call(self.clone_value, &[object.values[0]]);
            return Ok(ValueRef {
                values: self.builder.inst_results(inst).to_vec(),
                ty: DirectType::Opaque(object_ty.clone()),
            });
        }
        if let Type::Named(name, class_args) = object_ty {
            if self.classes.contains_key(name) || self.find_trait_method(object_ty, field).is_some()
            {
                return self.compile_class_member_call(
                    name,
                    Some(object_ty.clone()),
                    object,
                    field,
                    receiver_place,
                    args,
                );
            }
            if name == "Channel" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "clone" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `clone()` to take no arguments"
                                .to_string());
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.clone_value, &[object.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Opaque(Type::Named(
                                "Channel".to_string(),
                                class_args.clone(),
                            )),
                        })
                    }
                    "send" => {
                        let [argument] = args else {
                            return Err("direct backend expected `send()` to receive one argument"
                                .to_string());
                        };
                        let loaded = self.load_operand(&argument.value)?;
                        let value = self.ensure_opaque(loaded)?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.channel_send, &[object.values[0], value.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Opaque(Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Unit,
                                    Type::Named(
                                        "SendError".to_string(),
                                        vec![class_args
                                            .first()
                                            .cloned()
                                            .unwrap_or_else(|| Type::named("Unknown"))],
                                    ),
                                ],
                            )),
                        })
                    }
                    "recv" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `recv()` to take no arguments".to_string()
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.channel_recv, &[object.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Opaque(Type::Named(
                                "Option".to_string(),
                                vec![class_args
                                    .first()
                                    .cloned()
                                    .unwrap_or_else(|| Type::named("Unknown"))],
                            )),
                        })
                    }
                    "close" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `close()` to take no arguments"
                                .to_string());
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.channel_close, &[object.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Unit),
                        })
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "Task" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "clone" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `clone()` to take no arguments"
                                .to_string());
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.clone_value, &[object.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Opaque(Type::Named(
                                "Task".to_string(),
                                class_args.clone(),
                            )),
                        })
                    }
                    "join" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `join()` to take no arguments".to_string()
                            );
                        }
                        let inst = self.builder.ins().call(self.task_join, &[object.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Opaque(
                                class_args.first().cloned().unwrap_or(Type::Unit),
                            ),
                        })
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "TaskGroup" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "cancel" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `cancel()` to take no arguments"
                                .to_string());
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.task_group_cancel, &[object.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Unit),
                        })
                    }
                    "close" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `close()` to take no arguments"
                                .to_string());
                        }
                        let cancel_before = self.builder.ins().iconst(types::I64, 0);
                        let inst = self
                            .builder
                            .ins()
                            .call(self.task_group_close, &[object.values[0], cancel_before]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Unit),
                        })
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            let _ = class_args;
        }

        let candidates = self.dynamic_method_candidates(field);
        if candidates.is_empty() {
            return Err(format!(
                "direct backend does not know dynamic method `.{}` on `{}`",
                field, object_ty
            ));
        }
        if candidates.len() == 1 {
            return self.compile_class_member_call(
                &candidates[0].0,
                Some(Type::named(&candidates[0].0)),
                object,
                field,
                receiver_place,
                args,
            );
        }

        let result_ty = if candidates
            .iter()
            .map(|(_, method)| self.call_result_type(&method.function_name))
            .collect::<std::result::Result<Vec<_>, _>>()?
            .windows(2)
            .all(|window| window[0] == window[1])
        {
            self.call_result_type(&candidates[0].1.function_name)?
        } else {
            DirectType::Opaque(Type::named("Unknown"))
        };

        let join_block = self.builder.create_block();
        let mut current_fallthrough = None;
        let result_vars = self.declare_temporary_result_storage(&result_ty)?;
        for (index, (candidate_ty, _method)) in candidates.iter().enumerate() {
            let check_block = if index == 0 {
                self.builder
                    .current_block()
                    .expect("current block should exist")
            } else {
                let block = self.builder.create_block();
                self.builder.switch_to_block(block);
                block
            };
            let matched = self.value_matches_type(object.values[0], candidate_ty)?;
            let then_block = self.builder.create_block();
            let else_block = self.builder.create_block();
            self.builder
                .ins()
                .brif(matched, then_block, &[], else_block, &[]);
            self.builder.switch_to_block(then_block);
            let call_result = self.compile_class_member_call(
                candidate_ty,
                Some(Type::named(candidate_ty)),
                object.clone(),
                field,
                receiver_place,
                args,
            )?;
            self.store_result_vars(&result_vars, &call_result)?;
            self.builder.ins().jump(join_block, &[]);
            self.builder.seal_block(then_block);
            self.builder.switch_to_block(else_block);
            self.builder.seal_block(check_block);
            current_fallthrough = Some(else_block);
        }
        if let Some(else_block) = current_fallthrough {
            self.builder.switch_to_block(else_block);
            return Err(format!(
                "direct backend could not resolve dynamic method `.{}`",
                field
            ));
        }
        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        self.load_result_vars(&result_vars, result_ty)
    }

    fn compile_opaque_construct(
        &mut self,
        class_name: &str,
        fields: &[crate::mir::MirFieldInit],
    ) -> std::result::Result<ValueRef, String> {
        let (class_ptr, class_len) = self.string_constant(class_name.as_bytes())?;
        let init = self
            .builder
            .ins()
            .call(self.instance_empty, &[class_ptr, class_len]);
        let mut current = ValueRef {
            values: self.builder.inst_results(init).to_vec(),
            ty: DirectType::Opaque(Type::named(class_name)),
        };
        for field in fields {
            let loaded = self.load_operand(&field.value)?;
            let loaded = self.ensure_opaque(loaded)?;
            let (field_ptr, field_len) = self.string_constant(field.name.as_bytes())?;
            let inst = self.builder.ins().call(
                self.instance_set_field,
                &[current.values[0], field_ptr, field_len, loaded.values[0]],
            );
            current = ValueRef {
                values: self.builder.inst_results(inst).to_vec(),
                ty: DirectType::Opaque(Type::named(class_name)),
            };
        }
        Ok(current)
    }

    fn compile_spawn(
        &mut self,
        detached: bool,
        task_group: Option<&Operand>,
        function: &str,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let thunk_ref = *self.function_thunk_refs.get(function).ok_or_else(|| {
            format!(
                "direct backend does not know spawn thunk for `{}`",
                function
            )
        })?;
        let arg_count_value = self.builder.ins().iconst(types::I64, args.len() as i64);
        let buffer_call = self
            .builder
            .ins()
            .call(self.arg_buffer_new, &[arg_count_value]);
        let buffer = self.builder.inst_results(buffer_call)[0];
        for (index, arg) in args.iter().enumerate() {
            if arg.writeback_place.is_some() {
                return Err(
                    "direct backend does not yet support borrowed spawn arguments".to_string(),
                );
            }
            let value = self.load_operand(&arg.value)?;
            let value = self.ensure_opaque(value)?;
            let index_value = self.builder.ins().iconst(types::I64, index as i64);
            self.builder.ins().call(
                self.arg_buffer_store,
                &[buffer, index_value, value.values[0]],
            );
        }
        let thunk_ptr = self.builder.ins().func_addr(types::I64, thunk_ref);
        let detached_value = self
            .builder
            .ins()
            .iconst(types::I64, if detached { 1 } else { 0 });
        let task_group_value = if let Some(group) = task_group {
            let group = self.load_operand(group)?;
            let group = self.ensure_opaque(group)?;
            group.values[0]
        } else {
            self.builder.ins().iconst(types::I64, 0)
        };
        let call = self.builder.ins().call(
            self.spawn_call,
            &[
                thunk_ptr,
                buffer,
                arg_count_value,
                detached_value,
                task_group_value,
            ],
        );
        let ty = if detached {
            DirectType::Scalar(ScalarKind::Unit)
        } else {
            let return_ty = self
                .function_return_types
                .get(function)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "direct backend does not know return type for `{}`",
                        function
                    )
                })?;
            DirectType::Opaque(Type::Named(
                "Task".to_string(),
                vec![direct_type_to_type(&return_ty)],
            ))
        };
        Ok(ValueRef {
            values: self.builder.inst_results(call).to_vec(),
            ty,
        })
    }

    fn compile_select(&mut self, arms: &[MirSelectArm]) -> std::result::Result<(), String> {
        let loop_block = self.builder.create_block();
        let mut initial_deadlines = Vec::new();
        for arm in arms {
            if let MirSelectKind::After { duration } = &arm.kind {
                let duration = self.load_operand(duration)?;
                let duration = self.ensure_opaque(duration)?;
                let inst = self
                    .builder
                    .ins()
                    .call(self.deadline_new, &[duration.values[0]]);
                let deadline = self.builder.inst_results(inst)[0];
                self.builder.append_block_param(loop_block, types::I64);
                initial_deadlines.push(deadline);
            }
        }
        self.builder.ins().jump(loop_block, &initial_deadlines);
        self.builder.switch_to_block(loop_block);
        let deadline_params = self.builder.block_params(loop_block).to_vec();
        let mut deadline_index = 0usize;

        for arm in arms {
            match &arm.kind {
                MirSelectKind::Recv { channel } => {
                    let channel = self.load_operand(channel)?;
                    let channel = self.ensure_opaque(channel)?;
                    let inst = self
                        .builder
                        .ins()
                        .call(self.channel_try_recv, &[channel.values[0]]);
                    let result = self.builder.inst_results(inst)[0];
                    let zero = self.builder.ins().iconst(types::I64, 0);
                    let ready = self.builder.ins().icmp(IntCC::NotEqual, result, zero);
                    let recv_block = self.builder.create_block();
                    let next_block = self.builder.create_block();
                    self.builder
                        .ins()
                        .brif(ready, recv_block, &[], next_block, &[]);
                    self.builder.switch_to_block(recv_block);
                    if let Some(binding) = &arm.binding {
                        let binding_ty = self.type_of_place(binding)?;
                        self.store_place(
                            binding,
                            ValueRef {
                                values: vec![result],
                                ty: binding_ty,
                            },
                        )?;
                    }
                    self.builder.ins().jump(self.blocks[&arm.label], &[]);
                    self.builder.seal_block(recv_block);
                    self.builder.switch_to_block(next_block);
                }
                MirSelectKind::Send { channel, value } => {
                    let channel = self.load_operand(channel)?;
                    let channel = self.ensure_opaque(channel)?;
                    let value = self.load_operand(value)?;
                    let value = self.ensure_opaque(value)?;
                    let inst = self
                        .builder
                        .ins()
                        .call(self.channel_send, &[channel.values[0], value.values[0]]);
                    if let Some(binding) = &arm.binding {
                        let binding_ty = self.type_of_place(binding)?;
                        self.store_place(
                            binding,
                            ValueRef {
                                values: self.builder.inst_results(inst).to_vec(),
                                ty: binding_ty,
                            },
                        )?;
                    }
                    self.builder.ins().jump(self.blocks[&arm.label], &[]);
                    return Ok(());
                }
                MirSelectKind::After { .. } => {
                    let deadline = deadline_params[deadline_index];
                    deadline_index += 1;
                    let inst = self.builder.ins().call(self.deadline_ready, &[deadline]);
                    let ready = self.builder.inst_results(inst)[0];
                    let zero = self.builder.ins().iconst(types::I64, 0);
                    let ready = self.builder.ins().icmp(IntCC::NotEqual, ready, zero);
                    let next_block = self.builder.create_block();
                    self.builder
                        .ins()
                        .brif(ready, self.blocks[&arm.label], &[], next_block, &[]);
                    self.builder.switch_to_block(next_block);
                }
            }
        }

        let one_ms = self.builder.ins().iconst(types::I64, 1);
        self.builder.ins().call(self.sleep_ms, &[one_ms]);
        self.builder.ins().jump(loop_block, &deadline_params);
        Ok(())
    }

    fn value_matches_type(
        &mut self,
        value: Value,
        type_name: &str,
    ) -> std::result::Result<Value, String> {
        let (ptr, len) = self.string_constant(type_name.as_bytes())?;
        let inst = self
            .builder
            .ins()
            .call(self.value_type_matches, &[value, ptr, len]);
        Ok(self.builder.inst_results(inst)[0])
    }

    fn dynamic_method_candidates(&self, field: &str) -> Vec<(String, MirMethod)> {
        let mut candidates = Vec::new();
        for class in self.classes.values() {
            if let Some(method) = class.methods.iter().find(|method| method.name == field) {
                candidates.push((class.name.clone(), method.clone()));
            }
        }
        for trait_impl in &self.trait_impls {
            if let Type::Named(name, _) = &trait_impl.for_type {
                if let Some(method) = trait_impl
                    .methods
                    .iter()
                    .find(|method| method.name == field)
                {
                    candidates.push((name.clone(), method.clone()));
                }
            }
        }
        candidates
    }

    fn find_trait_method(&self, ty: &Type, field: &str) -> Option<&MirMethod> {
        self.trait_impls.iter().find_map(|trait_impl| {
            if &trait_impl.for_type != ty {
                return None;
            }
            trait_impl
                .methods
                .iter()
                .find(|method| method.name == field)
        })
    }

    fn declare_temporary_result_storage(
        &mut self,
        ty: &DirectType,
    ) -> std::result::Result<Vec<Variable>, String> {
        let mut vars = Vec::new();
        for abi in ty.abi_types() {
            let variable = Variable::from_u32(
                (self.variables.len() + self.variable_types.len() + vars.len() + 10000) as u32,
            );
            self.builder.declare_var(variable, abi);
            let zero = match abi {
                t if t == types::F64 => self.builder.ins().f64const(Ieee64::with_float(0.0)),
                _ => self.builder.ins().iconst(abi, 0),
            };
            self.builder.def_var(variable, zero);
            vars.push(variable);
        }
        Ok(vars)
    }

    fn store_result_vars(
        &mut self,
        vars: &[Variable],
        value: &ValueRef,
    ) -> std::result::Result<(), String> {
        for (var, compiled) in vars.iter().zip(value.values.iter()) {
            self.builder.def_var(*var, *compiled);
        }
        Ok(())
    }

    fn load_result_vars(
        &mut self,
        vars: &[Variable],
        ty: DirectType,
    ) -> std::result::Result<ValueRef, String> {
        let values = vars.iter().map(|var| self.builder.use_var(*var)).collect();
        Ok(ValueRef { values, ty })
    }
}

fn unit_value(builder: &mut FunctionBuilder<'_>) -> ValueRef {
    ValueRef {
        values: vec![builder.ins().iconst(types::I64, 0)],
        ty: DirectType::Scalar(ScalarKind::Unit),
    }
}

fn find_method<'a>(class: Option<&'a MirClass>, field: &str) -> Option<&'a MirMethod> {
    class?.methods.iter().find(|method| method.name == field)
}

fn declare_runtime_function(
    module: &mut ObjectModule,
    name: &str,
    params: &[cranelift_codegen::ir::Type],
    result: Option<cranelift_codegen::ir::Type>,
) -> std::result::Result<FuncId, String> {
    let mut sig = module.make_signature();
    for param in params {
        sig.params.push(AbiParam::new(*param));
    }
    if let Some(result) = result {
        sig.returns.push(AbiParam::new(result));
    }
    module
        .declare_function(name, Linkage::Import, &sig)
        .map_err(|error| format!("failed to declare runtime function `{}`: {}", name, error))
}

fn signature_for(
    function: &MirFunction,
    classes: &HashMap<String, MirClass>,
    call_conv: CallConv,
) -> std::result::Result<Signature, String> {
    let mut signature = Signature::new(call_conv);
    let mut writeback_types = Vec::new();
    if function.receiver.is_some() {
        let receiver_ty = receiver_type(function, classes)?;
        for abi in receiver_ty.abi_types() {
            signature.params.push(AbiParam::new(abi));
        }
        if function.receiver == Some(MirReceiverKind::BorrowMut) {
            writeback_types.push(receiver_ty);
        }
    }
    for param in &function.params {
        let ty = ensure_direct_type(
            &param.ty,
            classes,
            &format!("parameter `{}` on `{}`", param.name, function.name),
        )?;
        for abi in ty.abi_types() {
            signature.params.push(AbiParam::new(abi));
        }
        if param.passing == MirReceiverKind::BorrowMut {
            writeback_types.push(ty);
        }
    }
    let return_ty = ensure_direct_type(
        &function.return_type,
        classes,
        &format!("return type of `{}`", function.name),
    )?;
    for abi in return_ty.abi_types() {
        signature.returns.push(AbiParam::new(abi));
    }
    for ty in writeback_types {
        for abi in ty.abi_types() {
            signature.returns.push(AbiParam::new(abi));
        }
    }
    Ok(signature)
}

fn receiver_type(
    function: &MirFunction,
    classes: &HashMap<String, MirClass>,
) -> std::result::Result<DirectType, String> {
    let receiver_ty = function
        .local_types
        .iter()
        .find(|local| local.name == "self")
        .map(|local| &local.ty)
        .ok_or_else(|| {
            format!(
                "direct backend could not find receiver local type for `{}`",
                function.name
            )
        })?;
    ensure_direct_type(
        receiver_ty,
        classes,
        &format!("receiver of `{}`", function.name),
    )
}

fn declare_root_variables(
    builder: &mut FunctionBuilder<'_>,
    variable_index: &mut usize,
    variables: &mut HashMap<String, Vec<Variable>>,
    variable_types: &mut HashMap<String, DirectType>,
    name: String,
    ty: DirectType,
    initial: Option<&[Value]>,
) {
    let initial_values = initial
        .map(|values| values.to_vec())
        .unwrap_or_else(|| ty.zero_values(builder));
    let abi_types = ty.abi_types();
    let mut declared = Vec::new();
    for (offset, abi_ty) in abi_types.into_iter().enumerate() {
        let variable = Variable::from_u32(*variable_index as u32);
        *variable_index += 1;
        builder.declare_var(variable, abi_ty);
        builder.def_var(variable, initial_values[offset]);
        declared.push(variable);
    }
    variables.insert(name.clone(), declared);
    variable_types.insert(name, ty);
}

fn validate_module(module: &MirModule) -> std::result::Result<(), String> {
    let classes = module
        .classes
        .iter()
        .cloned()
        .map(|class| (class.name.clone(), class))
        .collect::<HashMap<_, _>>();
    for class in &module.classes {
        for field in &class.fields {
            ensure_direct_type(
                &field.ty,
                &classes,
                &format!("field `{}.{}`", class.name, field.name),
            )?;
        }
    }
    for function in module.functions.iter().chain(module.top_level.iter()) {
        validate_function(function, &classes)?;
    }
    Ok(())
}

fn validate_function(
    function: &MirFunction,
    classes: &HashMap<String, MirClass>,
) -> std::result::Result<(), String> {
    if function.receiver.is_some() {
        receiver_type(function, classes)?;
    }
    for param in &function.params {
        ensure_direct_type(
            &param.ty,
            classes,
            &format!("parameter `{}` on `{}`", param.name, function.name),
        )?;
    }
    ensure_direct_type(
        &function.return_type,
        classes,
        &format!("return type of `{}`", function.name),
    )?;
    for local in &function.local_types {
        ensure_direct_type(
            &local.ty,
            classes,
            &format!("local `{}` on `{}`", local.name, function.name),
        )?;
    }
    for block in &function.blocks {
        for instruction in &block.instructions {
            match instruction {
                Instruction::Assign { value, .. } => validate_rvalue(value, classes)?,
                Instruction::Eval { value } => validate_operand(value)?,
                Instruction::PushCleanup { .. } | Instruction::PopCleanup { .. } => {}
            }
        }
        match &block.terminator {
            Terminator::Return(operand) => validate_operand(operand)?,
            Terminator::Goto(_) => {}
            Terminator::Branch { condition, .. } => validate_operand(condition)?,
            Terminator::ForRange { iterable, .. } => validate_operand(iterable)?,
            Terminator::Match { scrutinee, .. } => validate_operand(scrutinee)?,
            Terminator::Select { arms, .. } => {
                for arm in arms {
                    match &arm.kind {
                        MirSelectKind::Recv { channel } => validate_operand(channel)?,
                        MirSelectKind::Send { channel, value } => {
                            validate_operand(channel)?;
                            validate_operand(value)?;
                        }
                        MirSelectKind::After { duration } => validate_operand(duration)?,
                    }
                }
            }
            other => {
                return Err(format!(
                    "direct backend does not yet support MIR terminator `{:?}`",
                    other
                ))
            }
        }
    }
    Ok(())
}

fn validate_rvalue(
    rvalue: &Rvalue,
    classes: &HashMap<String, MirClass>,
) -> std::result::Result<(), String> {
    match rvalue {
        Rvalue::Use(operand) => validate_operand(operand),
        Rvalue::Unary { value, .. } => validate_operand(value),
        Rvalue::Cast { value, ty, .. } => {
            validate_operand(value)?;
            ensure_direct_type(ty, classes, "cast target")?;
            Ok(())
        }
        Rvalue::Binary { left, right, .. } => {
            validate_operand(left)?;
            validate_operand(right)
        }
        Rvalue::Call { callee, args } => {
            match callee {
                CallTarget::Name(_) | CallTarget::Member { .. } => {}
            }
            for argument in args {
                validate_operand(&argument.value)?;
            }
            Ok(())
        }
        Rvalue::Construct { class_name, .. } => ensure_direct_type(
            &Type::named(class_name),
            classes,
            &format!("class `{}`", class_name),
        )
        .map(|_| ()),
        Rvalue::Member { object, .. } => validate_operand(object),
        Rvalue::EnumVariant { payload, .. } => payload
            .as_ref()
            .map(validate_operand)
            .transpose()
            .map(|_| ()),
        Rvalue::VariantPayload { scrutinee } => validate_operand(scrutinee),
        Rvalue::Try { value } => validate_operand(value),
        Rvalue::Spawn {
            task_group, args, ..
        } => {
            if let Some(group) = task_group {
                validate_operand(group)?;
            }
            for argument in args {
                validate_operand(&argument.value)?;
            }
            Ok(())
        }
    }
}

fn validate_operand(operand: &Operand) -> std::result::Result<(), String> {
    match operand {
        Operand::Place(_)
        | Operand::Int(_)
        | Operand::Bool(_)
        | Operand::Unit
        | Operand::Float(_)
        | Operand::String(_)
        | Operand::Duration(_) => Ok(()),
    }
}

fn ensure_direct_type(
    ty: &Type,
    classes: &HashMap<String, MirClass>,
    context: &str,
) -> std::result::Result<DirectType, String> {
    direct_type(ty, classes).ok_or_else(|| {
        format!(
            "direct backend does not yet support {} with type `{}`",
            context, ty
        )
    })
}

fn direct_type(ty: &Type, classes: &HashMap<String, MirClass>) -> Option<DirectType> {
    let mut visiting = BTreeSet::new();
    direct_type_inner(ty, classes, &mut visiting)
}

fn direct_type_inner(
    ty: &Type,
    classes: &HashMap<String, MirClass>,
    visiting: &mut BTreeSet<String>,
) -> Option<DirectType> {
    match ty {
        Type::Unit => Some(DirectType::Scalar(ScalarKind::Unit)),
        Type::TypeParam(name) => Some(DirectType::Opaque(Type::TypeParam(name.clone()))),
        Type::Module(path) => Some(DirectType::Opaque(Type::Module(path.clone()))),
        Type::Named(name, args) if args.is_empty() && name == "int32" => {
            Some(DirectType::Scalar(ScalarKind::Int32))
        }
        Type::Named(name, args) if args.is_empty() && name == "bool" => {
            Some(DirectType::Scalar(ScalarKind::Bool))
        }
        Type::Named(name, args) if args.is_empty() && name == "float32" => {
            Some(DirectType::Scalar(ScalarKind::Float32))
        }
        Type::Named(name, args) if args.is_empty() && name == "float64" => {
            Some(DirectType::Scalar(ScalarKind::Float64))
        }
        Type::Named(name, args) if args.is_empty() => {
            if let Some(class) = classes.get(name) {
                if !visiting.insert(name.clone()) {
                    return Some(DirectType::Opaque(Type::Named(name.clone(), vec![])));
                }
                let mut fields = Vec::new();
                for field in &class.fields {
                    let Some(field_ty) = direct_type_inner(&field.ty, classes, visiting) else {
                        visiting.remove(name);
                        return Some(DirectType::Opaque(Type::Named(name.clone(), vec![])));
                    };
                    if matches!(field_ty, DirectType::Opaque(_)) {
                        visiting.remove(name);
                        return Some(DirectType::Opaque(Type::Named(name.clone(), vec![])));
                    }
                    fields.push(PlainClassField {
                        name: field.name.clone(),
                        ty: field_ty,
                    });
                }
                visiting.remove(name);
                return Some(DirectType::PlainClass(PlainClassType {
                    class_name: name.clone(),
                    fields,
                }));
            }
            Some(DirectType::Opaque(Type::Named(name.clone(), vec![])))
        }
        Type::Named(name, args) => {
            Some(DirectType::Opaque(Type::Named(name.clone(), args.clone())))
        }
    }
}

fn infer_rvalue_type(
    rvalue: &Rvalue,
    variable_types: &HashMap<String, DirectType>,
    function_return_types: &HashMap<String, DirectType>,
    classes: &HashMap<String, MirClass>,
) -> Option<DirectType> {
    match rvalue {
        Rvalue::Use(operand) => infer_operand_type(operand, variable_types),
        Rvalue::Unary { op, value, .. } => match (op, infer_operand_type(value, variable_types)?) {
            (UnaryOp::Neg, DirectType::Scalar(ScalarKind::Int32)) => {
                Some(DirectType::Scalar(ScalarKind::Int32))
            }
            (UnaryOp::Neg, DirectType::Scalar(kind)) if kind.is_float() => {
                Some(DirectType::Scalar(kind))
            }
            (UnaryOp::Not, _) => Some(DirectType::Scalar(ScalarKind::Bool)),
            _ => None,
        },
        Rvalue::Cast { ty, .. } => direct_type(ty, classes),
        Rvalue::Binary { op, left, .. } => match op {
            BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Less
            | BinaryOp::LessEq
            | BinaryOp::Greater
            | BinaryOp::GreaterEq
            | BinaryOp::And
            | BinaryOp::Or => Some(DirectType::Scalar(ScalarKind::Bool)),
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                infer_operand_type(left, variable_types)
            }
        },
        Rvalue::Call { callee, .. } => match callee {
            CallTarget::Name(name) if name == "print" => Some(DirectType::Scalar(ScalarKind::Unit)),
            CallTarget::Name(name) if name == "range" => {
                Some(DirectType::Opaque(Type::named("Range")))
            }
            CallTarget::Name(name) if name == "channel" => Some(DirectType::Opaque(Type::Named(
                "Channel".to_string(),
                vec![Type::named("Unknown")],
            ))),
            CallTarget::Name(name) if name == "task_group" => {
                Some(DirectType::Opaque(Type::named("TaskGroup")))
            }
            CallTarget::Name(name) if name == "cancelled" => {
                Some(DirectType::Scalar(ScalarKind::Bool))
            }
            CallTarget::Name(name) if name == "sleep" => Some(DirectType::Scalar(ScalarKind::Unit)),
            CallTarget::Name(name) => function_return_types.get(name).cloned(),
            CallTarget::Member { object, field, .. } => {
                let object_ty = infer_operand_type(object, variable_types)?;
                if matches!(object_ty.scalar_kind(), Some(kind) if kind.is_float())
                    && field == "sqrt"
                {
                    return Some(object_ty);
                }
                match object_ty {
                    DirectType::PlainClass(class_ty) => {
                        let method = find_method(classes.get(&class_ty.class_name), field)?;
                        function_return_types.get(&method.function_name).cloned()
                    }
                    DirectType::Opaque(_) => Some(DirectType::Opaque(Type::named("Unknown"))),
                    DirectType::Scalar(_) => None,
                }
            }
        },
        Rvalue::Construct { class_name, .. } => direct_type(&Type::named(class_name), classes),
        Rvalue::Member { object, field } => match infer_operand_type(object, variable_types)? {
            ty @ DirectType::Opaque(_) => Some(ty),
            ty => ty.field_slice(field).map(|(_, _, ty)| ty),
        },
        Rvalue::EnumVariant { enum_name, .. } => Some(DirectType::Opaque(Type::named(enum_name))),
        Rvalue::VariantPayload { .. } => Some(DirectType::Opaque(Type::named("Unknown"))),
        Rvalue::Try { .. } => Some(DirectType::Opaque(Type::named("Unknown"))),
        Rvalue::Spawn {
            detached, function, ..
        } => {
            if *detached {
                Some(DirectType::Scalar(ScalarKind::Unit))
            } else {
                function_return_types.get(function).map(|ty| {
                    DirectType::Opaque(Type::Named(
                        "Task".to_string(),
                        vec![direct_type_to_type(ty)],
                    ))
                })
            }
        }
    }
}

fn infer_select_binding_type(
    arm: &MirSelectArm,
    variable_types: &HashMap<String, DirectType>,
    classes: &HashMap<String, MirClass>,
) -> Option<DirectType> {
    match &arm.kind {
        MirSelectKind::Recv { channel } => {
            let channel_ty = infer_operand_type(channel, variable_types)?;
            match channel_ty {
                DirectType::Opaque(Type::Named(name, args)) if name == "Channel" => {
                    Some(DirectType::Opaque(Type::Named(
                        "Option".to_string(),
                        vec![args
                            .first()
                            .cloned()
                            .unwrap_or_else(|| Type::named("Unknown"))],
                    )))
                }
                _ => Some(DirectType::Opaque(Type::Named(
                    "Option".to_string(),
                    vec![Type::named("Unknown")],
                ))),
            }
        }
        MirSelectKind::Send { channel, .. } => {
            let channel_ty = infer_operand_type(channel, variable_types)?;
            match channel_ty {
                DirectType::Opaque(Type::Named(name, args)) if name == "Channel" => {
                    Some(DirectType::Opaque(Type::Named(
                        "Result".to_string(),
                        vec![
                            Type::Unit,
                            Type::Named(
                                "SendError".to_string(),
                                vec![args
                                    .first()
                                    .cloned()
                                    .unwrap_or_else(|| Type::named("Unknown"))],
                            ),
                        ],
                    )))
                }
                _ => Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Unit,
                        Type::Named("SendError".to_string(), vec![Type::named("Unknown")]),
                    ],
                ))),
            }
        }
        MirSelectKind::After { duration } => {
            let _ = classes;
            infer_operand_type(duration, variable_types)?;
            Some(DirectType::Scalar(ScalarKind::Unit))
        }
    }
}

fn infer_operand_type(
    operand: &Operand,
    variable_types: &HashMap<String, DirectType>,
) -> Option<DirectType> {
    match operand {
        Operand::Place(place) => {
            let mut segments = place.split('.');
            let root = segments.next()?;
            let mut ty = variable_types.get(root)?.clone();
            for field in segments {
                let (_, _, field_ty) = ty.field_slice(field)?;
                ty = field_ty;
            }
            Some(ty)
        }
        Operand::Int(value) => {
            if i64::try_from(*value).is_ok() {
                Some(DirectType::Scalar(ScalarKind::Int32))
            } else {
                Some(DirectType::Opaque(Type::named("Unknown")))
            }
        }
        Operand::Float(_) => Some(DirectType::Scalar(ScalarKind::Float64)),
        Operand::Bool(_) => Some(DirectType::Scalar(ScalarKind::Bool)),
        Operand::Unit => Some(DirectType::Scalar(ScalarKind::Unit)),
        Operand::String(_) => Some(DirectType::Opaque(Type::named("String"))),
        Operand::Duration(_) => Some(DirectType::Opaque(Type::named("Duration"))),
    }
}

fn render_direct_type(ty: &DirectType) -> String {
    match ty {
        DirectType::Scalar(ScalarKind::Int32) => "int32".to_string(),
        DirectType::Scalar(ScalarKind::Float32) => "float32".to_string(),
        DirectType::Scalar(ScalarKind::Float64) => "float64".to_string(),
        DirectType::Scalar(ScalarKind::Bool) => "bool".to_string(),
        DirectType::Scalar(ScalarKind::Unit) => "None".to_string(),
        DirectType::PlainClass(class) => class.class_name.clone(),
        DirectType::Opaque(ty) => ty.to_string(),
    }
}

fn main_signature(call_conv: CallConv) -> Signature {
    let mut signature = Signature::new(call_conv);
    signature.returns.push(AbiParam::new(types::I32));
    signature
}

fn thunk_signature(call_conv: CallConv) -> Signature {
    let mut signature = Signature::new(call_conv);
    signature.params.push(AbiParam::new(types::I64));
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    signature
}

fn mangle_symbol(name: &str) -> String {
    let mut mangled = String::from("aurora_fn_");
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            mangled.push(ch);
        } else {
            mangled.push('_');
        }
    }
    mangled
}

fn mangle_thunk_symbol(name: &str) -> String {
    let mut mangled = String::from("aurora_thunk_");
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            mangled.push(ch);
        } else {
            mangled.push('_');
        }
    }
    mangled
}

fn direct_type_to_type(ty: &DirectType) -> Type {
    match ty {
        DirectType::Scalar(ScalarKind::Int32) => Type::named("int32"),
        DirectType::Scalar(ScalarKind::Float32) => Type::named("float32"),
        DirectType::Scalar(ScalarKind::Float64) => Type::named("float64"),
        DirectType::Scalar(ScalarKind::Bool) => Type::named("bool"),
        DirectType::Scalar(ScalarKind::Unit) => Type::Unit,
        DirectType::PlainClass(class) => Type::named(&class.class_name),
        DirectType::Opaque(ty) => ty.clone(),
    }
}

fn is_numeric_type_name(ty: &Type) -> bool {
    match ty {
        Type::Named(name, args) if args.is_empty() => {
            name == "float32"
                || name == "float64"
                || name.starts_with("int")
                || name.starts_with("uint")
        }
        _ => false,
    }
}

fn collect_spawn_targets(module: &MirModule) -> BTreeSet<String> {
    let mut targets = BTreeSet::new();
    for function in module.functions.iter().chain(module.top_level.iter()) {
        for block in &function.blocks {
            for instruction in &block.instructions {
                if let Instruction::Assign {
                    value: Rvalue::Spawn { function, .. },
                    ..
                } = instruction
                {
                    targets.insert(function.clone());
                }
            }
        }
    }
    targets
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        direct_type, emit_host_object, ensure_direct_type, infer_operand_type, infer_rvalue_type,
        main_signature, mangle_symbol, render_direct_type, scalar_kind_for_tests, signature_for,
        validate_operand, DirectType, ScalarKind,
    };
    use crate::ast::{BinaryOp, UnaryOp};
    use crate::diag::Span;
    use crate::lower_source_to_mir;
    use crate::mir::MirReceiverKind;
    use crate::mir::{CallTarget, MirArg, MirFunction, Operand, Rvalue};
    use crate::sema::Type;

    #[test]
    fn direct_backend_emits_object_for_supported_scalar_program() {
        let source = "def helper(value: int32) -> int32:\n    return value + 2\n\ndef main() -> int32:\n    mut current: int32 = 1\n    if current < 5:\n        current = helper(value=current)\n    print(current)\n    return 0\n";

        let mir = lower_source_to_mir(source).expect("source should lower to MIR");
        let object = emit_host_object(&mir).expect("direct backend should emit an object");

        assert!(
            !object.is_empty(),
            "direct backend object should not be empty"
        );
    }

    #[test]
    fn direct_backend_emits_object_for_plain_class_programs() {
        let source = include_str!("../../../examples/point.au");
        let mir = lower_source_to_mir(source).expect("point example should lower to MIR");
        let object =
            emit_host_object(&mir).expect("plain classes should now be supported directly");

        assert!(!object.is_empty(), "point object should not be empty");
    }

    #[test]
    fn direct_backend_emits_object_for_trait_impl_dispatch() {
        let source = include_str!("../../../examples/traits/greeter.au");
        let mir = lower_source_to_mir(source).expect("trait example should lower to MIR");
        let object =
            emit_host_object(&mir).expect("trait impl dispatch should now compile directly");

        assert!(
            !object.is_empty(),
            "trait dispatch object should not be empty"
        );
    }

    #[test]
    fn mangle_symbol_rewrites_non_alphanumeric_characters() {
        assert_eq!(mangle_symbol("main"), "aurora_fn_main");
        assert_eq!(
            mangle_symbol("helpers.math.double"),
            "aurora_fn_helpers_math_double"
        );
    }

    #[test]
    fn direct_type_supports_plain_classes_and_scalars() {
        let source = include_str!("../../../examples/classes/methods.au");
        let mir = lower_source_to_mir(source).expect("methods example should lower");
        let classes = mir
            .classes
            .iter()
            .cloned()
            .map(|class| (class.name.clone(), class))
            .collect::<HashMap<_, _>>();

        assert_eq!(
            scalar_kind_for_tests(&Type::named("int32")),
            Some(ScalarKind::Int32)
        );
        assert_eq!(
            scalar_kind_for_tests(&Type::named("float64")),
            Some(ScalarKind::Float64)
        );
        assert_eq!(
            scalar_kind_for_tests(&Type::named("bool")),
            Some(ScalarKind::Bool)
        );
        assert_eq!(scalar_kind_for_tests(&Type::Unit), Some(ScalarKind::Unit));

        let counter =
            direct_type(&Type::named("Counter"), &classes).expect("Counter should be direct");
        assert_eq!(render_direct_type(&counter), "Counter");
        assert_eq!(counter.value_count(), 1);
    }

    #[test]
    fn infer_operand_and_rvalue_types_track_plain_classes() {
        let mut variable_types = HashMap::new();
        variable_types.insert("flag".to_string(), DirectType::Scalar(ScalarKind::Bool));
        variable_types.insert("number".to_string(), DirectType::Scalar(ScalarKind::Int32));
        variable_types.insert(
            "point".to_string(),
            DirectType::PlainClass(super::PlainClassType {
                class_name: "Point".to_string(),
                fields: vec![
                    super::PlainClassField {
                        name: "x".to_string(),
                        ty: DirectType::Scalar(ScalarKind::Float64),
                    },
                    super::PlainClassField {
                        name: "y".to_string(),
                        ty: DirectType::Scalar(ScalarKind::Float64),
                    },
                ],
            }),
        );
        let mut returns = HashMap::new();
        returns.insert(
            "helper".to_string(),
            DirectType::Scalar(ScalarKind::Float64),
        );
        let classes = HashMap::new();

        assert_eq!(
            infer_operand_type(&Operand::Place("flag".to_string()), &variable_types),
            Some(DirectType::Scalar(ScalarKind::Bool))
        );
        assert_eq!(
            infer_operand_type(&Operand::Place("point.x".to_string()), &variable_types),
            Some(DirectType::Scalar(ScalarKind::Float64))
        );
        assert_eq!(
            infer_rvalue_type(
                &Rvalue::Unary {
                    op: UnaryOp::Not,
                    value: Operand::Place("flag".to_string()),
                    span: Span::new(1, 1),
                },
                &variable_types,
                &returns,
                &classes,
            ),
            Some(DirectType::Scalar(ScalarKind::Bool))
        );
        assert_eq!(
            infer_rvalue_type(
                &Rvalue::Binary {
                    op: BinaryOp::Add,
                    left: Operand::Place("number".to_string()),
                    right: Operand::Int(2),
                    span: Span::new(1, 1),
                },
                &variable_types,
                &returns,
                &classes,
            ),
            Some(DirectType::Scalar(ScalarKind::Int32))
        );
        assert_eq!(
            infer_rvalue_type(
                &Rvalue::Call {
                    callee: CallTarget::Name("print".to_string()),
                    args: vec![MirArg {
                        name: None,
                        value: Operand::Bool(true),
                        writeback_place: None,
                    }],
                },
                &variable_types,
                &returns,
                &classes,
            ),
            Some(DirectType::Scalar(ScalarKind::Unit))
        );
    }

    #[test]
    fn validate_operand_accepts_nested_places() {
        validate_operand(&Operand::Place("point.x".to_string()))
            .expect("nested places should now validate directly");
    }

    #[test]
    fn ensure_direct_type_maps_runtime_backed_types_to_opaque_values() {
        let ty = ensure_direct_type(&Type::named("String"), &HashMap::new(), "test type")
            .expect("runtime-backed types should still be representable directly");
        assert_eq!(ty, DirectType::Opaque(Type::named("String")));
    }

    #[test]
    fn signature_helpers_flatten_plain_class_abi_types() {
        let mut classes = HashMap::new();
        classes.insert(
            "Point".to_string(),
            crate::mir::MirClass {
                name: "Point".to_string(),
                fields: vec![
                    crate::mir::MirClassField {
                        name: "x".to_string(),
                        ty: Type::named("float64"),
                    },
                    crate::mir::MirClassField {
                        name: "y".to_string(),
                        ty: Type::named("float64"),
                    },
                ],
                methods: Vec::new(),
            },
        );
        let function = MirFunction {
            name: "demo".to_string(),
            module_name: "<test>".to_string(),
            receiver: Some(MirReceiverKind::Borrow),
            params: vec![crate::mir::MirParam {
                name: "other".to_string(),
                passing: MirReceiverKind::Value,
                ty: Type::named("Point"),
            }],
            local_types: vec![crate::mir::MirLocalType {
                name: "self".to_string(),
                ty: Type::named("Point"),
            }],
            return_type: Type::named("float64"),
            entry: "entry".to_string(),
            blocks: Vec::new(),
        };

        let sig = signature_for(
            &function,
            &classes,
            cranelift_codegen::isa::CallConv::SystemV,
        )
        .expect("signature should flatten point receiver and param");
        let main_sig = main_signature(cranelift_codegen::isa::CallConv::SystemV);

        assert_eq!(sig.params.len(), 4);
        assert_eq!(sig.returns.len(), 1);
        assert_eq!(main_sig.returns.len(), 1);
    }
}

#[cfg(test)]
fn scalar_kind_for_tests(ty: &Type) -> Option<ScalarKind> {
    direct_type(ty, &HashMap::new()).and_then(|ty| ty.scalar_kind())
}
