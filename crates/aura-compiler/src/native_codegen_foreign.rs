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
//! - an opaque handle argument passes the address it carries
//! - a handle result boxes the returned address, and a null address ends
//!   the task with `AU4005` unless the declared result is `Handle | None`
//! - a `bool` result that is neither 0 nor 1 ends the task with `AU4001`
//!
//! Every argument is loaded before the first view is taken, so no later
//! argument's load can move bytes an earlier view lends. A `str` literal
//! argument lends the program's constant bytes and needs no helper.

use super::*;

use crate::native_backend::Extension;

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

/// One argument of a foreign call, loaded and not yet lowered to C values.
enum ForeignOperand {
    /// A value of the given boundary type.
    Value(FfiType, ValueRef),
    /// The pointer and length of a `str` literal, which lends the
    /// program's own constant bytes without a box.
    Text((Val, Val)),
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
        let mut writebacks = Vec::new();
        for (index, (argument, param)) in args.iter().zip(&call.params).enumerate() {
            let ffi = direct_ffi_type_for_source(&param.ty, Some(param.passing))?;
            if let (FfiType::StringView, Operand::String(text)) = (ffi.ffi_type, &argument.value) {
                loaded.push(ForeignOperand::Text(self.foreign_text_literal(text)?));
                continue;
            }
            let expected = ensure_direct_type(
                &param.ty,
                &self.classes,
                &format!("extern parameter `{}`", param.name),
            )?;
            let value = self.load_operand_for_target(&argument.value, &expected)?;
            let value = self.coerce_value(value, &expected)?;
            let value = if foreign_scalar(ffi.ffi_type).is_some() {
                value
            } else {
                self.ensure_opaque(value)?
            };
            match (param.passing, argument.writeback_place.as_ref()) {
                (MirReceiverKind::BorrowMut, Some(place)) => {
                    writebacks.push((place.clone(), value.clone(), expected));
                }
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
                (_, None) => {}
            }
            loaded.push(ForeignOperand::Value(ffi.ffi_type, value));
        }

        let mut arguments = ForeignArguments {
            signature: Signature::new(CallConv::C),
            values: Vec::new(),
        };
        let mut copies = Vec::new();
        for operand in &loaded {
            match operand {
                ForeignOperand::Value(ffi_type, value) => {
                    self.push_foreign_argument(*ffi_type, value, &mut arguments, &mut copies)
                }
                ForeignOperand::Text((pointer, length)) => {
                    arguments.push_word(*pointer);
                    arguments.push_word(*length);
                }
            }
        }

        let result_ffi = direct_ffi_type_for_source(&call.return_type, None)?;
        // The validated result is a scalar, a handle or a nullable handle,
        // which come back as one C value, or nothing.
        let result_scalar = foreign_scalar(result_ffi.ffi_type);
        let result_handle = result_ffi.opaque_name.as_deref();
        if let Some(scalar) = &result_scalar {
            arguments.signature.returns.push(scalar.ty);
        } else if result_handle.is_some() {
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
        for (place, value, expected) in writebacks {
            let writeback = self.coerce_value(value, &expected)?;
            self.store_place(&place, writeback)?;
        }

        let result = match (result_scalar, result_handle) {
            (Some(scalar), _) => {
                self.foreign_scalar_result(&scalar, invocation.results[0], &call.symbol)?
            }
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
        let return_direct = ensure_direct_type(
            &call.return_type,
            &self.classes,
            &format!("extern return from `{}`", call.symbol),
        )?;
        let result = self.coerce_value(result, &return_direct)?;
        self.coerce_value(result, target)
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
            let converted = match scalar.ty {
                ScalarType::I64 | ScalarType::F64 => word,
                ScalarType::F32 => self.builder.fdemote(types::F32, word),
                narrow => self.builder.ireduce(narrow, word),
            };
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
        // The validated union is exactly the handle and `None`.
        let Type::Union(union) = union_type else {
            return Err("nullable-handle FFI metadata is missing its union".to_string());
        };
        let unit = union
            .members
            .iter()
            .position(|member| *member == Type::Unit);
        let none_index = unit.unwrap_or_default();
        let handle_index = 1 - none_index;
        self.require_union_plan(union_type)?;

        let handle = self.foreign_handle(address, name)?;
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
