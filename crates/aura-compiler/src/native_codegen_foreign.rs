//! Calls of `extern "C"` functions.
//!
//! Generated code calls a foreign function directly, as C code would: the
//! linker resolves the symbol, each scalar crosses in the C type its
//! declaration names, and the result comes back in the result register. A
//! call of `abs(x: int32) -> int32` is the same `call` instruction a C
//! compiler emits.
//!
//! The values that are not scalars cross through the runtime helpers of
//! `native_foreign_calls.rs`:
//!
//! - a `str` or `list[uint8]` argument lends its bytes as a pointer and a
//!   length, two C arguments in that order
//! - an opaque handle argument passes the address it carries, and a
//!   `Handle | None` argument passes null for `None`
//! - a `cstr` argument passes a NUL-terminated copy of the text, freed when
//!   the call returns, and a `cstr` result is copied into a new `str`
//! - a `ptr[T]` argument passes the address of the first element of a
//!   `list[T]`, with no length
//! - a `mut` scalar or `mut Handle | None` argument passes the address of a
//!   stack word that holds the value, and the word is stored back to the
//!   argument's place when the call returns
//! - an `extern "C" class` argument or result crosses by value, as the C
//!   calling convention passes a struct: in registers, on the stack, or
//!   through an address. A `mut` argument of such a class passes the
//!   address of a C copy, and the copy is read back when the call returns
//! - a callback argument, a named function for a parameter of function
//!   type, passes the address of entry code with the C signature. The
//!   entry code hands the arguments to the Aura function through the
//!   runtime, which stops a failure from unwinding into the C caller
//! - a handle result boxes the returned address, and a null address ends
//!   the task with `AU4005` unless the declared result is `Handle | None`
//! - a `bool` result that is neither 0 nor 1 ends the task with `AU4001`
//!
//! Every argument is loaded before the first view is taken, so no later
//! argument's load can move bytes an earlier view lends. A `str` literal
//! argument lends the program's constant bytes and needs no helper.

use super::*;

use crate::c_layout::{CAbi, CArgument, CPassing, CPiece, CStruct};
use crate::ffi::CCallback;
use crate::mir::MirExternParam;
use crate::mir_runtime::callback_signature;
use crate::native_backend::{Extension, Indirect};

/// How one scalar crosses the C boundary.
struct ForeignScalar {
    /// The C type the value travels as.
    ty: ScalarType,
    /// How the narrow C type fills a full register.
    extension: Extension,
    /// The Aura scalar the value is on this side.
    kind: ScalarKind,
}

fn foreign_scalar(ffi_type: FfiType) -> Option<ForeignScalar> {
    let (ty, extension, kind) = match ffi_type {
        FfiType::Bool => (types::I8, Extension::Zero, ScalarKind::Bool),
        FfiType::U8 => (types::I8, Extension::Zero, ScalarKind::Uint8),
        FfiType::I8 => (types::I8, Extension::Sign, ScalarKind::Int8),
        FfiType::U16 => (types::I16, Extension::Zero, ScalarKind::Uint16),
        FfiType::I16 => (types::I16, Extension::Sign, ScalarKind::Int16),
        FfiType::U32 => (types::I32, Extension::Zero, ScalarKind::Uint32),
        FfiType::I32 => (types::I32, Extension::Sign, ScalarKind::Int32),
        FfiType::U64 => (types::I64, Extension::None, ScalarKind::Uint64),
        FfiType::I64 => (types::I64, Extension::None, ScalarKind::Int64),
        FfiType::F32 => (types::F32, Extension::None, ScalarKind::Float32),
        FfiType::F64 => (types::F64, Extension::None, ScalarKind::Float64),
        _ => return None,
    };
    Some(ForeignScalar {
        ty,
        extension,
        kind,
    })
}

/// A callback one foreign call passes: the Aura function and the C
/// signature it is called with.
pub(super) type CallbackKey = (String, CCallback);

/// Every callback the module's foreign calls pass, once each, in an order
/// that is the same in every build.
pub(super) fn module_callbacks(module: &MirModule) -> Vec<CallbackKey> {
    let mut found: Vec<CallbackKey> = Vec::new();
    let instructions = module
        .functions
        .iter()
        .chain(module.top_level.iter())
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.instructions);
    for instruction in instructions {
        let Instruction::Assign {
            value:
                Rvalue::Call {
                    callee: CallTarget::Extern(call),
                    args,
                    ..
                },
            ..
        } = instruction
        else {
            continue;
        };
        for (param, argument) in call.params.iter().zip(args) {
            if let (Some(signature), Operand::Function { name, .. }) =
                (callback_signature(&param.ty), &argument.value)
            {
                let key = (name.clone(), signature);
                if !found.contains(&key) {
                    found.push(key);
                }
            }
        }
    }
    found.sort_by_cached_key(|(name, signature)| format!("{name} {signature:?}"));
    found
}

/// The C signature of a callback's entry code, and the signature of its
/// body, which takes the address of the call's frame.
pub(super) fn callback_signatures(callback: &CCallback) -> (Signature, Signature) {
    let mut entry = Signature::new(CallConv::C);
    for param in &callback.params {
        let scalar = foreign_scalar(param.scalar.ffi_type()).expect("a callback takes scalars");
        match param.pointer {
            true => entry.params.push(types::I64),
            false => entry.params.push(scalar.ty),
        }
        entry.extensions.push(match param.pointer {
            true => Extension::None,
            false => scalar.extension,
        });
    }
    if let Some(result) = callback.result {
        entry.returns.push(callback_result_type(result));
    }
    let mut body = Signature::new(CallConv::C);
    body.params.push(types::I64);
    (entry, body)
}

/// The type a callback returns its result in. C callers differ on whether
/// they extend a result narrower than 32 bits, so it returns extended.
fn callback_result_type(result: crate::ffi::FfiScalar) -> ScalarType {
    let scalar = foreign_scalar(result.ffi_type()).expect("a callback returns a scalar");
    match scalar.ty {
        ScalarType::I8 | ScalarType::I16 => types::I32,
        other => other,
    }
}

/// The Aura word of the C value `raw` that a callback received.
fn callback_word(builder: &mut dyn FunctionBackend, scalar: &ForeignScalar, raw: Val) -> Val {
    let bits = match (scalar.ty, scalar.extension) {
        (ScalarType::I64 | ScalarType::F64, _) => return raw,
        (ScalarType::F32, _) => return builder.fpromote(types::F64, raw),
        (ScalarType::I8, Extension::Sign) => 8,
        (ScalarType::I16, Extension::Sign) => 16,
        (ScalarType::I32, Extension::Sign) => 32,
        _ => 0,
    };
    let wide = builder.uextend(types::I64, raw);
    if scalar.kind == ScalarKind::Bool {
        // C code can pass any byte as a `bool`. Nonzero is true.
        let set = builder.icmp_imm(IntCC::NotEqual, wide, 0);
        let one = builder.iconst(types::I64, 1);
        let zero = builder.iconst(types::I64, 0);
        return builder.select(set, one, zero);
    }
    if bits == 0 {
        return wide;
    }
    let shift = builder.iconst(types::I64, 64 - bits);
    let high = builder.ishl(wide, shift);
    builder.sshr(high, shift)
}

/// The C value of type `ty` for the Aura word `word`.
fn callback_narrow(builder: &mut dyn FunctionBackend, ty: ScalarType, word: Val) -> Val {
    match ty {
        ScalarType::I64 | ScalarType::F64 => word,
        ScalarType::F32 => builder.fdemote(types::F32, word),
        narrow => builder.ireduce(narrow, word),
    }
}

impl NativeCodegen<'_> {
    /// Defines the entry code and the body of every callback the module
    /// passes to C.
    pub(super) fn define_callback_thunks(&mut self) -> std::result::Result<(), String> {
        for ((name, callback), (entry_id, body_id)) in self.callback_thunks.clone() {
            let (entry, body) = callback_signatures(&callback);
            self.define_body(
                entry_id,
                &entry,
                &format!("callback entry for `{name}`"),
                |codegen, builder| {
                    codegen.emit_callback_entry(builder, &callback, body_id);
                    Ok(())
                },
            )?;
            let target = self.functions[&name];
            self.define_body(
                body_id,
                &body,
                &format!("callback body for `{name}`"),
                |_, builder| {
                    emit_callback_body(builder, &callback, target);
                    Ok(())
                },
            )?;
        }
        Ok(())
    }

    /// The code C calls: it writes the arguments to a frame as Aura words,
    /// runs the body under the runtime's guard, and returns what the body
    /// left in the frame.
    fn emit_callback_entry(
        &mut self,
        builder: &mut dyn FunctionBackend,
        callback: &CCallback,
        body_id: FuncHandle,
    ) {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let arguments = builder.block_params(entry);
        let flags = MemFlags::trusted();
        let count = callback.params.len();
        let slot = builder.create_sized_stack_slot((count as u32 + 1) * 8, 3);
        let frame = builder.stack_addr(types::I64, slot, 0);
        let zero = builder.iconst(types::I64, 0);
        builder.store(flags, zero, frame, count as i32 * 8);
        // A null pointer argument reads and writes this word instead.
        let spare = builder.create_sized_stack_slot(8, 3);
        let spare = builder.stack_addr(types::I64, spare, 0);
        builder.store(flags, zero, spare, 0);

        let mut pointees = Vec::new();
        for (index, (param, argument)) in callback.params.iter().zip(arguments).enumerate() {
            let scalar = foreign_scalar(param.scalar.ffi_type()).expect("a callback takes scalars");
            let raw = if param.pointer {
                let present = builder.icmp_imm(IntCC::NotEqual, argument, 0);
                let address = builder.select(present, argument, spare);
                pointees.push((index, address));
                builder.load(scalar.ty, flags, address, 0)
            } else {
                argument
            };
            let word = callback_word(builder, &scalar, raw);
            builder.store(flags, word, frame, index as i32 * 8);
        }

        let body = builder.import_function(body_id);
        let body = builder.func_addr(types::I64, body);
        let guard = builder.import_function(self.runtime[RuntimeFn::FfiCallbackGuard]);
        builder.call(guard, &[body, frame]);

        for (index, address) in pointees {
            let scalar = foreign_scalar(callback.params[index].scalar.ffi_type())
                .expect("a callback takes scalars");
            let word = builder.load(scalar.kind.signature_type(), flags, frame, index as i32 * 8);
            let narrow = callback_narrow(builder, scalar.ty, word);
            builder.store(flags, narrow, address, 0);
        }
        let Some(result) = callback.result else {
            builder.return_(&[]);
            return;
        };
        let scalar = foreign_scalar(result.ffi_type()).expect("a callback returns a scalar");
        let word = builder.load(scalar.kind.signature_type(), flags, frame, count as i32 * 8);
        let narrow = callback_narrow(builder, callback_result_type(result), word);
        builder.return_(&[narrow]);
    }
}

/// The code the guard runs: it calls the Aura function `target` with the
/// words of the frame, and stores the result and what the function wrote
/// to its `mut` parameters back to the frame.
fn emit_callback_body(builder: &mut dyn FunctionBackend, callback: &CCallback, target: FuncHandle) {
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let frame = builder.block_params(entry)[0];
    let flags = MemFlags::trusted();
    let count = callback.params.len();
    let arguments = callback
        .params
        .iter()
        .enumerate()
        .map(|(index, param)| {
            let scalar = foreign_scalar(param.scalar.ffi_type()).expect("a callback takes scalars");
            builder.load(scalar.kind.signature_type(), flags, frame, index as i32 * 8)
        })
        .collect::<Vec<_>>();
    let target = builder.import_function(target);
    let results = builder.call(target, &arguments).results;
    // The function returns its result, then one word for each `mut`
    // parameter in order.
    let written = callback.params.iter().filter(|param| param.pointer).count();
    let mut cursor = results.len() - written;
    if callback.result.is_some() {
        builder.store(flags, results[0], frame, count as i32 * 8);
    }
    for (index, param) in callback.params.iter().enumerate() {
        if param.pointer {
            builder.store(flags, results[cursor], frame, index as i32 * 8);
            cursor += 1;
        }
    }
    builder.return_(&[]);
}

/// The boundary type of one parameter of a foreign call.
pub(super) fn foreign_param_type(
    param: &MirExternParam,
) -> std::result::Result<DirectFfiType, String> {
    if matches!(param.ty, Type::Function { .. }) {
        return Ok(DirectFfiType::scalar(FfiType::Callback));
    }
    if param.c_string {
        return Ok(DirectFfiType::scalar(c_string_type(&param.ty)));
    }
    if param.pointer {
        // `ptr[T]` lowers to `list[T]`, and `T` is a scalar.
        let element = match &param.ty {
            Type::Named(_, args) => args.first(),
            _ => None,
        };
        let scalar = element
            .and_then(|element| {
                direct_ffi_type_for_source(element, Some(MirReceiverKind::Borrow)).ok()
            })
            .and_then(|element| crate::ffi::FfiScalar::of(element.ffi_type))
            .ok_or_else(|| {
                format!(
                    "direct backend cannot pass `{}` as an FFI pointer",
                    param.ty
                )
            })?;
        return Ok(DirectFfiType::scalar(match param.passing {
            MirReceiverKind::BorrowMut => FfiType::PointerMut(scalar),
            _ => FfiType::Pointer(scalar),
        }));
    }
    if let Some(layout) = &param.c_struct {
        let layout = layout.interned();
        return Ok(DirectFfiType::scalar(match param.passing {
            MirReceiverKind::BorrowMut => FfiType::StructPointer(layout),
            _ => FfiType::Struct(layout),
        }));
    }
    direct_ffi_type_for_source(&param.ty, Some(param.passing))
}

/// The list access code of a list that keeps `scalar` elements unboxed.
fn scalar_list_access(scalar: crate::ffi::FfiScalar) -> i64 {
    use crate::ffi::FfiScalar as Scalar;
    use aura_runtime::native_runtime::DirectListAccess as Access;
    let access = match scalar {
        Scalar::Bool => Access::Bool,
        Scalar::I8 => Access::Int8,
        Scalar::I16 => Access::Int16,
        Scalar::I32 => Access::Int32,
        Scalar::I64 => Access::Int64,
        Scalar::U8 => Access::Uint8,
        Scalar::U16 => Access::Uint16,
        Scalar::U32 => Access::Uint32,
        Scalar::U64 => Access::Uint64,
        Scalar::F32 => Access::Float32,
        Scalar::F64 => Access::Float64,
    };
    access as i64
}

/// The boundary type of the result of a foreign call.
pub(super) fn foreign_result_type(
    call: &MirExternCall,
) -> std::result::Result<DirectFfiType, String> {
    if call.c_string_result {
        return Ok(DirectFfiType::scalar(c_string_type(&call.return_type)));
    }
    if let Some(layout) = &call.c_struct_result {
        return Ok(DirectFfiType::scalar(FfiType::Struct(layout.interned())));
    }
    direct_ffi_type_for_source(&call.return_type, None)
}

/// `cstr` lowers to `str` and `cstr | None` to a union.
fn c_string_type(ty: &Type) -> FfiType {
    match ty {
        Type::Union(_) => FfiType::NullableCString,
        _ => FfiType::CString,
    }
}

/// One argument of a foreign call, loaded and not yet lowered to C values.
// One per argument, for the length of one call's lowering.
#[allow(clippy::large_enum_variant)]
enum ForeignOperand {
    /// A value of the given boundary type.
    Value(DirectFfiType, ValueRef),
    /// The pointer and length of a `str` literal, which lends the
    /// program's own constant bytes without a box.
    Text((Val, Val)),
    /// The address of a `cstr` literal: the program's own constant bytes,
    /// which end in a NUL.
    CText(Val),
    /// The entry code of a callback.
    Callback(FuncRef),
}

/// What a `mut` argument's place takes when the call returns.
enum ForeignWriteback {
    /// The value that was passed: a byte list the function wrote in place.
    Same(ValueRef),
    /// The scalar in the stack word at this address.
    Scalar(ForeignScalar, Val),
    /// The `Handle | None` of this union type for the address in the stack
    /// word at this address. The name is the handle's opaque type.
    Handle(String, Type, Val),
    /// The struct in the C copy at this address.
    Struct(&'static CStruct, Val),
}

/// The convention foreign calls follow on the machine being built for.
fn foreign_abi() -> std::result::Result<CAbi, String> {
    CAbi::host().ok_or(format!(
        "direct backend cannot pass a C struct by value on `{}`",
        std::env::consts::ARCH
    ))
}

/// The size of the stack slot that holds one `layout` struct.
fn struct_slot_size(layout: &CStruct) -> u32 {
    layout.size().next_multiple_of(8) as u32
}

/// The type a register's piece of a struct travels as.
fn piece_type(piece: CPiece) -> (i32, ScalarType) {
    match piece {
        CPiece::Int { offset, .. } => (offset as i32, types::I64),
        CPiece::Sse { offset, .. } => (offset as i32, types::F64),
        CPiece::Float { offset, scalar } => {
            let scalar = foreign_scalar(scalar.ffi_type()).expect("a float is a scalar");
            (offset as i32, scalar.ty)
        }
    }
}

/// The foreign call's C signature and argument values, in order.
struct ForeignArguments {
    signature: Signature,
    values: Vec<Val>,
}

impl ForeignArguments {
    fn push(&mut self, ty: ScalarType, extension: Extension, value: Val) {
        self.signature.params.push(ty);
        self.signature.extensions.push(extension);
        self.values.push(value);
    }

    fn push_word(&mut self, value: Val) {
        self.push(types::I64, Extension::None, value);
    }

    /// Marks the last argument, an address, as a struct in memory.
    fn mark_last(&mut self, indirect: Indirect) {
        let index = self.signature.params.len() - 1;
        self.signature.indirects.push((index, indirect));
    }

    /// How many general and floating-point registers the arguments so far
    /// take, when there are enough of each.
    fn registers_used(&self, abi: CAbi) -> (usize, usize) {
        let mut general = 0;
        let mut float = 0;
        for (index, ty) in self.signature.params.iter().enumerate() {
            let indirect = self.signature.indirects.iter().find(|(at, _)| *at == index);
            match (indirect, ty) {
                (Some((_, Indirect::ByValue { .. })), _) => {}
                // AArch64 has a register of its own for a result's address.
                (Some((_, Indirect::Result { .. })), _) => {
                    general += usize::from(abi == CAbi::SysV64);
                }
                (None, ScalarType::F32 | ScalarType::F64) => float += 1,
                (None, _) => general += 1,
            }
        }
        (general, float)
    }
}

impl FunctionCompiler<'_> {
    pub(super) fn compile_extern_call(
        &mut self,
        call: &MirExternCall,
        args: &[MirArg],
        target: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        if call.abi != "C" {
            return Err(format!(
                "direct backend received unsupported FFI ABI `{}`",
                call.abi
            ));
        }
        if args.len() != call.params.len() {
            return Err(format!(
                "direct backend expected {} argument(s) for extern `{}`, found {}",
                call.params.len(),
                call.symbol,
                args.len()
            ));
        }

        let mut loaded = Vec::with_capacity(args.len());
        for (index, (argument, param)) in args.iter().zip(&call.params).enumerate() {
            let ffi = foreign_param_type(param)?;
            match (ffi.ffi_type, &argument.value) {
                (FfiType::Callback, operand) => {
                    let entry = match (callback_signature(&param.ty), operand) {
                        (Some(signature), Operand::Function { name, .. }) => {
                            self.callback_refs.get(&(name.clone(), signature)).copied()
                        }
                        _ => None,
                    };
                    let entry = entry.ok_or_else(|| {
                        format!(
                            "direct backend needs a named function for callback `{}` of `{}`",
                            param.name, call.symbol
                        )
                    })?;
                    loaded.push((ForeignOperand::Callback(entry), None));
                    continue;
                }
                (FfiType::StringView, Operand::String(text)) => {
                    let text = self.foreign_text_literal(text)?;
                    loaded.push((ForeignOperand::Text(text), None));
                    continue;
                }
                // Text with a NUL inside takes the helper, which reports it.
                (FfiType::CString | FfiType::NullableCString, Operand::String(text))
                    if !text.contains('\0') =>
                {
                    let mut bytes = text.as_bytes().to_vec();
                    bytes.push(0);
                    let (pointer, _) = self.string_constant(&bytes)?;
                    loaded.push((ForeignOperand::CText(pointer), None));
                    continue;
                }
                _ => {}
            }
            let expected = ensure_direct_type(
                &param.ty,
                &self.classes,
                &format!("extern parameter `{}`", param.name),
            )?;
            let value = self.load_operand_for_target(&argument.value, &expected)?;
            let value = self.coerce_value(value, &expected)?;
            let by_value = match ffi.ffi_type {
                FfiType::Out(scalar) => scalar.ffi_type(),
                other => other,
            };
            let is_struct = matches!(by_value, FfiType::Struct(_) | FfiType::StructPointer(_));
            let value = if foreign_scalar(by_value).is_some() || is_struct {
                value
            } else {
                self.ensure_opaque(value)?
            };
            let place = match (param.passing, argument.writeback_place.as_ref()) {
                (MirReceiverKind::BorrowMut, Some(place)) => Some((place.clone(), expected)),
                (MirReceiverKind::BorrowMut, None) => {
                    return Err(format!(
                        "direct backend extern mutable argument {} has no writeback place",
                        index + 1
                    ));
                }
                (_, Some(_)) => {
                    return Err(format!(
                        "direct backend extern argument {} unexpectedly requests writeback",
                        index + 1
                    ));
                }
                (_, None) => None,
            };
            loaded.push((ForeignOperand::Value(ffi, value), place));
        }

        let mut arguments = ForeignArguments {
            signature: Signature::new(CallConv::C),
            values: Vec::new(),
        };
        // A struct result in memory takes its address as the first argument.
        let struct_result = match &call.c_struct_result {
            Some(layout) => {
                let layout = layout.interned();
                let slot = self.foreign_struct_slot(layout);
                let passing = layout.passing(foreign_abi()?);
                match &passing {
                    CPassing::Memory => {
                        arguments.push_word(slot);
                        arguments.mark_last(Indirect::Result {
                            size: struct_slot_size(layout),
                        });
                    }
                    CPassing::Registers(pieces) => {
                        for piece in pieces {
                            arguments.signature.returns.push(piece_type(*piece).1);
                        }
                    }
                }
                Some((layout, slot, passing))
            }
            None => None,
        };
        let mut copies = Vec::new();
        let mut text_copies = Vec::new();
        let mut list_copies = Vec::new();
        let mut flag_lists = Vec::new();
        let mut writebacks = Vec::new();
        for (index, (operand, place)) in loaded.into_iter().enumerate() {
            let (ffi, value) = match operand {
                ForeignOperand::Value(ffi, value) => (ffi, value),
                ForeignOperand::Text((pointer, length)) => {
                    arguments.push_word(pointer);
                    arguments.push_word(length);
                    continue;
                }
                ForeignOperand::CText(pointer) => {
                    arguments.push_word(pointer);
                    continue;
                }
                ForeignOperand::Callback(entry) => {
                    let address = self.builder.func_addr(types::I64, entry);
                    arguments.push_word(address);
                    continue;
                }
            };
            let word = value.values[0];
            let writeback = match ffi.ffi_type {
                FfiType::Out(scalar) => {
                    let scalar = foreign_scalar(scalar.ffi_type())
                        .expect("an out-parameter points to a scalar");
                    let narrow = self.foreign_scalar_argument(&scalar, word);
                    let slot = self.foreign_stack_word(narrow);
                    arguments.push_word(slot);
                    ForeignWriteback::Scalar(scalar, slot)
                }
                FfiType::OpaqueHandleOut => {
                    let address = self.foreign_nullable_address(word);
                    let slot = self.foreign_stack_word(address);
                    arguments.push_word(slot);
                    ForeignWriteback::Handle(
                        ffi.opaque_name.unwrap_or_default(),
                        ffi.union_type.unwrap_or(Type::Unit),
                        slot,
                    )
                }
                FfiType::NullableOpaqueHandle => {
                    let address = self.foreign_nullable_address(word);
                    arguments.push_word(address);
                    continue;
                }
                FfiType::CString | FfiType::NullableCString => {
                    let (symbol_ptr, symbol_len) = self.string_constant(call.symbol.as_bytes())?;
                    let index = self.builder.iconst(types::I64, index as i64);
                    let copy = self.builder.call(
                        self.runtime[RuntimeFn::FfiCString],
                        &[word, index, symbol_ptr, symbol_len],
                    );
                    arguments.push_word(copy.results[0]);
                    text_copies.push(copy.results[0]);
                    continue;
                }
                FfiType::Pointer(scalar) => {
                    let (pointer, copy) = self.foreign_elements(word, scalar, false);
                    arguments.push_word(pointer);
                    list_copies.push(copy);
                    continue;
                }
                FfiType::PointerMut(scalar) => {
                    let (pointer, _) = self.foreign_elements(word, scalar, true);
                    arguments.push_word(pointer);
                    if scalar == crate::ffi::FfiScalar::Bool {
                        flag_lists.push(word);
                    }
                    ForeignWriteback::Same(value)
                }
                FfiType::Struct(layout) => {
                    let slot = self.foreign_struct_slot(layout);
                    self.store_foreign_struct(layout, &value, slot);
                    self.push_foreign_struct(layout, slot, &mut arguments)?;
                    continue;
                }
                FfiType::StructPointer(layout) => {
                    let slot = self.foreign_struct_slot(layout);
                    self.store_foreign_struct(layout, &value, slot);
                    arguments.push_word(slot);
                    ForeignWriteback::Struct(layout, slot)
                }
                ffi_type => {
                    self.push_foreign_argument(ffi_type, &value, &mut arguments, &mut copies);
                    ForeignWriteback::Same(value)
                }
            };
            if let Some((place, expected)) = place {
                writebacks.push((place, expected, writeback));
            }
        }

        let result_ffi = foreign_result_type(call)?;
        // The validated result is a scalar, a handle, a nullable handle or
        // C text, which come back as one C value, or nothing.
        let result_scalar = foreign_scalar(result_ffi.ffi_type);
        let result_handle = result_ffi.opaque_name.as_deref();
        let result_text = matches!(
            result_ffi.ffi_type,
            FfiType::CString | FfiType::NullableCString
        );
        if let Some(scalar) = &result_scalar {
            arguments.signature.returns.push(scalar.ty);
        } else if result_handle.is_some() || result_text {
            arguments.signature.returns.push(types::I64);
        }

        let symbol = self.builder.module().declare_foreign_symbol(&call.symbol)?;
        let symbol = self.builder.import_function(symbol);
        let address = self.builder.func_addr(types::I64, symbol);
        let signature = self.builder.import_signature(arguments.signature);
        let invocation = self
            .builder
            .call_indirect(signature, address, &arguments.values);

        for (copy, length) in copies {
            self.release_foreign_copy(copy, length);
        }
        for copy in text_copies {
            self.builder
                .call(self.runtime[RuntimeFn::FfiReleaseCString], &[copy]);
        }
        for copy in list_copies {
            self.release_foreign_list_copy(copy);
        }
        // A C function can leave any byte in a `bool` it writes through.
        for list in flag_lists {
            let (symbol_ptr, symbol_len) = self.string_constant(call.symbol.as_bytes())?;
            self.builder.call(
                self.runtime[RuntimeFn::FfiCheckFlags],
                &[list, symbol_ptr, symbol_len],
            );
        }
        for (place, expected, writeback) in writebacks {
            let flags = MemFlags::trusted();
            let value = match writeback {
                ForeignWriteback::Same(value) => value,
                ForeignWriteback::Scalar(scalar, slot) => {
                    let raw = self.builder.load(scalar.ty, flags, slot, 0);
                    self.foreign_scalar_result(&scalar, raw, &call.symbol)?
                }
                ForeignWriteback::Handle(name, union_type, slot) => {
                    let address = self.builder.load(types::I64, flags, slot, 0);
                    self.foreign_nullable_handle(address, &name, &union_type)?
                }
                ForeignWriteback::Struct(layout, slot) => {
                    self.load_foreign_struct(layout, slot, expected.clone(), &call.symbol)?
                }
            };
            let writeback = self.coerce_value(value, &expected)?;
            self.store_place(&place, writeback)?;
        }

        // A callback that failed under this call, or under an earlier one
        // that kept the callback, fails the call now that C has returned.
        if !self.callback_refs.is_empty() {
            self.builder
                .call(self.runtime[RuntimeFn::FfiCallbackCheck], &[]);
        }
        let return_direct = ensure_direct_type(
            &call.return_type,
            &self.classes,
            &format!("extern return from `{}`", call.symbol),
        )?;
        let result = match (result_scalar, result_handle) {
            _ if struct_result.is_some() => {
                let (layout, slot, passing) = struct_result.expect("checked by the guard");
                if let CPassing::Registers(pieces) = passing {
                    for (piece, value) in pieces.iter().zip(&invocation.results) {
                        let (offset, _) = piece_type(*piece);
                        self.builder
                            .store(MemFlags::trusted(), *value, slot, offset);
                    }
                }
                self.load_foreign_struct(layout, slot, return_direct.clone(), &call.symbol)?
            }
            (Some(scalar), _) => {
                self.foreign_scalar_result(&scalar, invocation.results[0], &call.symbol)?
            }
            (None, None) if result_text => self.foreign_text_result(invocation.results[0], call)?,
            (None, Some(name)) if result_ffi.ffi_type == FfiType::OpaqueHandle => {
                let handle = self.foreign_handle(invocation.results[0], name)?;
                self.trap_on_null_handle(handle, &call.symbol)?;
                self.owned_opaque_result(vec![handle], call.return_type.clone())
            }
            (None, Some(name)) => {
                self.foreign_nullable_handle(invocation.results[0], name, &call.return_type)?
            }
            (None, None) => unit_value(&mut *self.builder),
        };
        let result = self.coerce_value(result, &return_direct)?;
        self.coerce_value(result, target)
    }

    /// The address of a new zeroed stack slot that holds one `layout`
    /// struct. The slot is a whole number of 8-byte words, so a register's
    /// piece of the struct loads and stores as one word.
    fn foreign_struct_slot(&mut self, layout: &CStruct) -> Val {
        let size = struct_slot_size(layout);
        let slot = self.builder.create_sized_stack_slot(size, 3);
        let address = self.builder.stack_addr(types::I64, slot, 0);
        let zero = self.builder.iconst(types::I64, 0);
        for offset in (0..size).step_by(8) {
            self.builder
                .store(MemFlags::trusted(), zero, address, offset as i32);
        }
        address
    }

    /// Stores the class value `value` at `slot` in the C layout. A class
    /// value has one word per scalar field, in the order of the layout.
    fn store_foreign_struct(&mut self, layout: &CStruct, value: &ValueRef, slot: Val) {
        for (leaf, word) in layout.leaves().iter().zip(&value.values) {
            let scalar = foreign_scalar(leaf.scalar.ffi_type()).expect("a leaf is a scalar");
            let narrow = self.foreign_scalar_argument(&scalar, *word);
            self.builder
                .store(MemFlags::trusted(), narrow, slot, leaf.offset as i32);
        }
    }

    /// The class value of type `ty` for the C struct at `slot`, which the
    /// foreign function `symbol` filled.
    fn load_foreign_struct(
        &mut self,
        layout: &CStruct,
        slot: Val,
        ty: DirectType,
        symbol: &str,
    ) -> std::result::Result<ValueRef, String> {
        let mut values = Vec::new();
        for leaf in layout.leaves() {
            let scalar = foreign_scalar(leaf.scalar.ffi_type()).expect("a leaf is a scalar");
            let raw = self
                .builder
                .load(scalar.ty, MemFlags::trusted(), slot, leaf.offset as i32);
            let word = self.foreign_scalar_result(&scalar, raw, symbol)?;
            values.push(word.values[0]);
        }
        Ok(ValueRef { values, ty })
    }

    /// Appends the C arguments that pass the struct at `slot` by value.
    fn push_foreign_struct(
        &mut self,
        layout: &CStruct,
        slot: Val,
        arguments: &mut ForeignArguments,
    ) -> std::result::Result<(), String> {
        let abi = foreign_abi()?;
        let (general, float) = arguments.registers_used(abi);
        match layout.argument(abi, general, float)? {
            CArgument::Registers(pieces) => {
                for piece in pieces {
                    let (offset, ty) = piece_type(piece);
                    let value = self.builder.load(ty, MemFlags::trusted(), slot, offset);
                    arguments.push(ty, Extension::None, value);
                }
            }
            CArgument::Stack => {
                arguments.push_word(slot);
                arguments.mark_last(Indirect::ByValue {
                    size: struct_slot_size(layout),
                });
            }
            CArgument::Address => arguments.push_word(slot),
        }
        Ok(())
    }

    /// The view of a `str` literal: the bytes in the program's data, or a
    /// null pointer when there are none, as every empty view is.
    fn foreign_text_literal(&mut self, text: &str) -> std::result::Result<(Val, Val), String> {
        if text.is_empty() {
            let null = self.builder.iconst(types::I64, 0);
            return Ok((null, null));
        }
        self.string_constant(text.as_bytes())
    }

    /// Appends the C arguments one Aura argument becomes: the scalar
    /// itself, the address a handle carries, or the pointer and length of
    /// a `str` or `list[uint8]` view.
    fn push_foreign_argument(
        &mut self,
        ffi_type: FfiType,
        value: &ValueRef,
        arguments: &mut ForeignArguments,
        copies: &mut Vec<(Val, Val)>,
    ) {
        let word = value.values[0];
        if let Some(scalar) = foreign_scalar(ffi_type) {
            let converted = self.foreign_scalar_argument(&scalar, word);
            arguments.push(scalar.ty, scalar.extension, converted);
            return;
        }
        let helper = match ffi_type {
            FfiType::OpaqueHandle => {
                let address = self
                    .builder
                    .call(self.runtime[RuntimeFn::FfiHandleAddress], &[word]);
                arguments.push_word(address.results[0]);
                return;
            }
            FfiType::StringView => RuntimeFn::FfiStrView,
            FfiType::BytesView => RuntimeFn::FfiBytesView,
            _ => RuntimeFn::FfiBytesViewMut,
        };
        if helper == RuntimeFn::FfiStrView {
            let (pointer, length, _) = self.foreign_view_call(helper, word);
            arguments.push_word(pointer);
            arguments.push_word(length);
            return;
        }

        // A list that keeps its bytes unboxed shows them in its view, so the
        // common case reads the pointer and length in place. A `mut` view
        // also needs room, which means no other list shares the storage.
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let flags = MemFlags::trusted();
        let access = self.builder.uload32(flags, word, layout.access_offset);
        let bytes_access = aura_runtime::native_runtime::DirectListAccess::Uint8 as i64;
        let mut lends = self.builder.icmp_imm(IntCC::Equal, access, bytes_access);
        if ffi_type == FfiType::BytesViewMut {
            let room = self.builder.uload32(flags, word, layout.capacity_offset);
            let unshared = self.builder.icmp_imm(IntCC::NotEqual, room, 0);
            lends = self.builder.band(lends, unshared);
        }
        let lent = self.builder.create_block();
        let asked = self.builder.create_block();
        let join = self.builder.create_block();
        let pointer = self.builder.append_block_param(join, types::I64);
        let length = self.builder.append_block_param(join, types::I64);
        let copy = self.builder.append_block_param(join, types::I64);
        self.builder.set_cold_block(asked);
        self.builder.brif(lends, lent, &[], asked, &[]);

        self.builder.switch_to_block(lent);
        self.builder.seal_block(lent);
        let lent_length = self
            .builder
            .load(types::I64, flags, word, layout.len_offset);
        let elements = self
            .builder
            .load(types::I64, flags, word, layout.elements_offset);
        let null = self.builder.iconst(types::I64, 0);
        let lent_pointer = self.builder.select(lent_length, elements, null);
        self.builder.jump(join, &[lent_pointer, lent_length, null]);

        self.builder.switch_to_block(asked);
        self.builder.seal_block(asked);
        let (asked_pointer, asked_length, taken) = self.foreign_view_call(helper, word);
        let asked_copy = match taken {
            Some(taken) if ffi_type == FfiType::BytesView => taken,
            _ => self.builder.iconst(types::I64, 0),
        };
        self.builder
            .jump(join, &[asked_pointer, asked_length, asked_copy]);

        self.builder.switch_to_block(join);
        self.builder.seal_block(join);
        if ffi_type == FfiType::BytesView {
            copies.push((copy, length));
        }
        arguments.push_word(pointer);
        arguments.push_word(length);
    }

    /// The Aura word `word` in the C type of `scalar`.
    fn foreign_scalar_argument(&mut self, scalar: &ForeignScalar, word: Val) -> Val {
        match scalar.ty {
            ScalarType::I64 | ScalarType::F64 => word,
            ScalarType::F32 => self.builder.fdemote(types::F32, word),
            narrow => self.builder.ireduce(narrow, word),
        }
    }

    /// The address of a new stack word that holds `value`, which is at
    /// most 8 bytes. The bytes of the word above a narrower value are zero.
    fn foreign_stack_word(&mut self, value: Val) -> Val {
        let slot = self.builder.create_sized_stack_slot(8, 3);
        let address = self.builder.stack_addr(types::I64, slot, 0);
        let zero = self.builder.iconst(types::I64, 0);
        self.builder.store(MemFlags::trusted(), zero, address, 0);
        self.builder.store(MemFlags::trusted(), value, address, 0);
        address
    }

    /// The address the `Handle | None` box `value` carries, null for
    /// `None`.
    fn foreign_nullable_address(&mut self, value: Val) -> Val {
        let address = self
            .builder
            .call(self.runtime[RuntimeFn::FfiNullableHandleAddress], &[value]);
        address.results[0]
    }

    /// The `str` or `str | None` copy of the C text at `pointer`, the
    /// result of `call`.
    fn foreign_text_result(
        &mut self,
        pointer: Val,
        call: &MirExternCall,
    ) -> std::result::Result<ValueRef, String> {
        let nullable = matches!(call.return_type, Type::Union(_));
        let (symbol_ptr, symbol_len) = self.string_constant(call.symbol.as_bytes())?;
        let flag = self.builder.iconst(types::I64, i64::from(nullable));
        let text = self.builder.call(
            self.runtime[RuntimeFn::FfiCStringResult],
            &[pointer, flag, symbol_ptr, symbol_len],
        );
        if nullable {
            return self.foreign_nullable(text.results[0], &call.return_type);
        }
        Ok(self.owned_opaque_result(text.results, call.return_type.clone()))
    }

    /// The address of the first element of the list `value`, laid out as a
    /// C array of `scalar`, and the copy the runtime made of a list that
    /// keeps its elements another way, or 0. A list that keeps them unboxed
    /// shows them in its view, so the common case reads the address in
    /// place. A `mut` pointer also needs room, which means no other list
    /// shares the storage, and it never makes a copy.
    fn foreign_elements(
        &mut self,
        value: Val,
        scalar: crate::ffi::FfiScalar,
        mutable: bool,
    ) -> (Val, Val) {
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let flags = MemFlags::trusted();
        let code = scalar_list_access(scalar);
        let access = self.builder.uload32(flags, value, layout.access_offset);
        let mut lends = self.builder.icmp_imm(IntCC::Equal, access, code);
        if mutable {
            let room = self.builder.uload32(flags, value, layout.capacity_offset);
            let unshared = self.builder.icmp_imm(IntCC::NotEqual, room, 0);
            lends = self.builder.band(lends, unshared);
        }
        let lent = self.builder.create_block();
        let asked = self.builder.create_block();
        let join = self.builder.create_block();
        let pointer = self.builder.append_block_param(join, types::I64);
        let copy = self.builder.append_block_param(join, types::I64);
        self.builder.set_cold_block(asked);
        self.builder.brif(lends, lent, &[], asked, &[]);

        self.builder.switch_to_block(lent);
        self.builder.seal_block(lent);
        let length = self
            .builder
            .load(types::I64, flags, value, layout.len_offset);
        let elements = self
            .builder
            .load(types::I64, flags, value, layout.elements_offset);
        let null = self.builder.iconst(types::I64, 0);
        let lent_pointer = self.builder.select(length, elements, null);
        self.builder.jump(join, &[lent_pointer, null]);

        self.builder.switch_to_block(asked);
        self.builder.seal_block(asked);
        let slot = self.builder.create_sized_stack_slot(16, 3);
        let view = self.builder.stack_addr(types::I64, slot, 0);
        let code = self.builder.iconst(types::I64, code);
        let helper = match mutable {
            true => RuntimeFn::FfiElementsMut,
            false => RuntimeFn::FfiElements,
        };
        let taken = self
            .builder
            .call(self.runtime[helper], &[value, code, view]);
        let asked_pointer = self.builder.load(types::I64, flags, view, 0);
        let asked_copy = match taken.results.first() {
            Some(copy) => *copy,
            None => self.builder.iconst(types::I64, 0),
        };
        self.builder.jump(join, &[asked_pointer, asked_copy]);

        self.builder.switch_to_block(join);
        self.builder.seal_block(join);
        (pointer, copy)
    }

    /// Releases the list the runtime made to lay a pointer argument's
    /// elements out as a C array. A list that lent its own elements made
    /// none, which is the common case, so the release sits on a cold path.
    fn release_foreign_list_copy(&mut self, copy: Val) {
        let release = self.builder.create_block();
        let done = self.builder.create_block();
        self.builder.set_cold_block(release);
        self.builder.brif(copy, release, &[], done, &[]);
        self.builder.switch_to_block(release);
        self.builder.seal_block(release);
        self.builder
            .call(self.runtime[RuntimeFn::ReleaseValue], &[copy]);
        self.builder.jump(done, &[]);
        self.builder.switch_to_block(done);
        self.builder.seal_block(done);
    }

    /// Asks the runtime for the view of `value`: its pointer, its length,
    /// and what the helper returned, if it returns anything.
    fn foreign_view_call(&mut self, helper: RuntimeFn, value: Val) -> (Val, Val, Option<Val>) {
        let slot = self.builder.create_sized_stack_slot(16, 3);
        let view = self.builder.stack_addr(types::I64, slot, 0);
        let taken = self.builder.call(self.runtime[helper], &[value, view]);
        let pointer = self.builder.load(types::I64, MemFlags::trusted(), view, 0);
        let length = self.builder.load(types::I64, MemFlags::trusted(), view, 8);
        (pointer, length, taken.results.first().copied())
    }

    /// Frees the copy a byte view made of a list that keeps its bytes
    /// boxed. A list that lent its own bytes made none, which is the
    /// common case, so the release sits on a cold path.
    fn release_foreign_copy(&mut self, copy: Val, length: Val) {
        let release = self.builder.create_block();
        let done = self.builder.create_block();
        self.builder.set_cold_block(release);
        self.builder.brif(copy, release, &[], done, &[]);
        self.builder.switch_to_block(release);
        self.builder.seal_block(release);
        self.builder
            .call(self.runtime[RuntimeFn::FfiReleaseBytes], &[copy, length]);
        self.builder.jump(done, &[]);
        self.builder.switch_to_block(done);
        self.builder.seal_block(done);
    }

    /// The Aura word of a scalar a foreign function returned. A C function
    /// may leave the bits of a register above a narrow result undefined, so
    /// the word is extended here from the narrow value.
    fn foreign_scalar_result(
        &mut self,
        scalar: &ForeignScalar,
        raw: Val,
        symbol: &str,
    ) -> std::result::Result<ValueRef, String> {
        let word = match (scalar.ty, scalar.extension) {
            (ScalarType::I64 | ScalarType::F64, _) => raw,
            (ScalarType::F32, _) => self.builder.fpromote(types::F64, raw),
            (ScalarType::I8, Extension::Sign) => self.sign_extend(raw, 8),
            (ScalarType::I16, Extension::Sign) => self.sign_extend(raw, 16),
            (ScalarType::I32, Extension::Sign) => self.sign_extend(raw, 32),
            _ => self.builder.uextend(types::I64, raw),
        };
        if scalar.kind == ScalarKind::Bool {
            self.trap_on_bad_bool(word, symbol)?;
        }
        Ok(ValueRef {
            values: vec![word],
            ty: DirectType::Scalar(scalar.kind),
        })
    }

    /// Ends the task with `AU4001` unless `byte`, a C `bool` result, is 0
    /// or 1.
    fn trap_on_bad_bool(&mut self, byte: Val, symbol: &str) -> std::result::Result<(), String> {
        let invalid = self.builder.icmp_imm(IntCC::UnsignedGreaterThan, byte, 1);
        let fail = self.builder.create_block();
        let valid = self.builder.create_block();
        self.builder.set_cold_block(fail);
        self.builder.brif(invalid, fail, &[], valid, &[]);
        self.builder.switch_to_block(fail);
        self.builder.seal_block(fail);
        let (symbol_ptr, symbol_len) = self.string_constant(symbol.as_bytes())?;
        self.builder.call(
            self.runtime[RuntimeFn::FfiBadBool],
            &[byte, symbol_ptr, symbol_len],
        );
        self.builder.trap(TrapCode::User(2));
        self.builder.switch_to_block(valid);
        self.builder.seal_block(valid);
        Ok(())
    }

    /// `raw`, an integer of `bits` bits, as a signed 64-bit word.
    fn sign_extend(&mut self, raw: Val, bits: i64) -> Val {
        let wide = self.builder.uextend(types::I64, raw);
        let shift = self.builder.iconst(types::I64, 64 - bits);
        let high = self.builder.ishl(wide, shift);
        self.builder.sshr(high, shift)
    }

    /// A new handle box of the opaque type `name` for `address`, or 0 when
    /// the address is null.
    fn foreign_handle(&mut self, address: Val, name: &str) -> std::result::Result<Val, String> {
        let (name_ptr, name_len) = self.string_constant(name.as_bytes())?;
        let handle = self.builder.call(
            self.runtime[RuntimeFn::FfiHandle],
            &[address, name_ptr, name_len],
        );
        Ok(handle.results[0])
    }

    fn trap_on_null_handle(
        &mut self,
        handle: Val,
        symbol: &str,
    ) -> std::result::Result<(), String> {
        let fail = self.builder.create_block();
        let valid = self.builder.create_block();
        self.builder.set_cold_block(fail);
        self.builder.brif(handle, valid, &[], fail, &[]);
        self.builder.switch_to_block(fail);
        self.builder.seal_block(fail);
        let (symbol_ptr, symbol_len) = self.string_constant(symbol.as_bytes())?;
        self.builder.call(
            self.runtime[RuntimeFn::FfiNullHandle],
            &[symbol_ptr, symbol_len],
        );
        self.builder.trap(TrapCode::User(2));
        self.builder.switch_to_block(valid);
        self.builder.seal_block(valid);
        Ok(())
    }

    /// The `Handle | None` value of a returned address: the handle member
    /// for a non-null address and `None` for null (ADR-0052 A10).
    fn foreign_nullable_handle(
        &mut self,
        address: Val,
        name: &str,
        union_type: &Type,
    ) -> std::result::Result<ValueRef, String> {
        let handle = self.foreign_handle(address, name)?;
        self.foreign_nullable(handle, union_type)
    }

    /// The value of the two-member union `union_type` for `payload`, a box
    /// of the member that is not `None`, or 0 for `None`.
    fn foreign_nullable(
        &mut self,
        payload: Val,
        union_type: &Type,
    ) -> std::result::Result<ValueRef, String> {
        // The validated union is exactly one member and `None`.
        let Type::Union(union) = union_type else {
            return Err("nullable FFI metadata is missing its union".to_string());
        };
        let unit = union
            .members
            .iter()
            .position(|member| *member == Type::Unit);
        let none_index = unit.unwrap_or_default();
        let handle_index = 1 - none_index;
        self.require_union_plan(union_type)?;

        let handle = payload;
        let none_block = self.builder.create_block();
        let join = self.builder.create_block();
        let payload = self.builder.append_block_param(join, types::I64);
        let tag = self.builder.append_block_param(join, types::I64);
        let handle_tag = self.builder.iconst(types::I64, handle_index as i64);
        self.builder
            .brif(handle, join, &[handle, handle_tag], none_block, &[]);
        self.builder.switch_to_block(none_block);
        self.builder.seal_block(none_block);
        let none = self.builder.call(self.runtime[RuntimeFn::FfiNone], &[]);
        let none_tag = self.builder.iconst(types::I64, none_index as i64);
        self.builder.jump(join, &[none.results[0], none_tag]);
        self.builder.switch_to_block(join);
        self.builder.seal_block(join);

        let encoded = crate::native_runtime::canonical_runtime_type_name(union_type);
        let (type_ptr, type_len) = self.string_constant(encoded.as_bytes())?;
        let injected = self.builder.call(
            self.runtime[RuntimeFn::UnionInject],
            &[type_ptr, type_len, tag, payload],
        );
        Ok(self.owned_opaque_result(injected.results, union_type.clone()))
    }
}
