//! SysV requires the caller to realign its stack for over-aligned by-value
//! arguments. libffi aligns the argument bytes within a 16-byte stack area,
//! which can put the bytes at a different offset than the callee expects.
//! Its public ABI calls an LLVM forwarding function with record pointers;
//! LLVM handles the actual by-value call with the target stack alignment.
use crate::c_layout::{CAbi, CArgument, CPassing, CPiece};
use crate::ffi::{self, FfiError, FfiSignature, FfiType, FfiValue, HostFunction};
use inkwell::context::Context;
use inkwell::execution_engine::ExecutionEngine;
use inkwell::memory_buffer::MemoryBuffer;
use std::cell::RefCell;
use std::rc::Rc;

struct Adapter {
    function: HostFunction,
    incoming: FfiSignature,
    // Drop the engine before the context whose allocation it borrows.
    _engine: ExecutionEngine<'static>,
    _context: Box<Context>,
}

type Cached = (usize, FfiSignature, Rc<Adapter>);
thread_local! {
    static ADAPTERS: RefCell<Vec<Cached>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn call(
    symbol: &str,
    signature: &FfiSignature,
    arguments: &mut [FfiValue],
) -> Result<FfiValue, FfiError> {
    let function = ffi::process_symbol(symbol)?;
    invoke(function, signature, arguments)
}

fn invoke(
    function: HostFunction,
    signature: &FfiSignature,
    arguments: &mut [FfiValue],
) -> Result<FfiValue, FfiError> {
    let address = function.as_ptr() as usize;
    let adapter = ADAPTERS.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some((_, _, adapter)) = cache
            .iter()
            .find(|(a, s, _)| *a == address && s == signature)
        {
            return Ok(Rc::clone(adapter));
        }
        let adapter = Rc::new(Adapter::new(address, signature).map_err(FfiError::CallAdapter)?);
        cache.push((address, signature.clone(), Rc::clone(&adapter)));
        Ok::<_, FfiError>(adapter)
    })?;
    // SAFETY: the adapter accepts precisely this pointer-based signature.
    // The interpreter has already checked the source-level foreign call.
    unsafe { ffi::call_host_function(adapter.function, &adapter.incoming, arguments) }
}

impl Adapter {
    fn new(address: usize, signature: &FfiSignature) -> Result<Self, String> {
        let (ir, incoming) = forwarding_ir(address, signature)?;
        let context = Box::new(Context::create());
        // SAFETY: the allocation is stable. It lives in the adapter until
        // after the engine is destroyed. Neither field escapes the adapter.
        let stable: &'static Context = unsafe { &*std::ptr::from_ref(&*context) };
        let buffer = MemoryBuffer::create_from_memory_range_copy(ir.as_bytes(), "ffi-stack");
        let module = stable
            .create_module_from_ir(buffer)
            .map_err(|e| e.to_string())?;
        module.verify().map_err(|e| e.to_string())?;
        let engine = module
            .create_jit_execution_engine(inkwell::OptimizationLevel::None)
            .map_err(|e| e.to_string())?;
        let entry = engine
            .get_function_address("aura_ffi_forward")
            .map_err(|e| e.to_string())?;
        let function = HostFunction::new(entry as *mut _).ok_or("null FFI adapter entry")?;
        Ok(Self {
            function,
            incoming,
            _engine: engine,
            _context: context,
        })
    }
}

fn scalar_type(ty: FfiType) -> &'static str {
    match ty {
        FfiType::Unit => "void",
        FfiType::Bool | FfiType::I8 | FfiType::U8 => "i8",
        FfiType::I16 | FfiType::U16 => "i16",
        FfiType::I32 | FfiType::U32 => "i32",
        FfiType::I64 | FfiType::U64 => "i64",
        FfiType::F32 => "float",
        FfiType::F64 => "double",
        _ => "ptr",
    }
}

fn extension(ty: FfiType) -> &'static str {
    match ty {
        FfiType::I8 | FfiType::I16 => "signext ",
        FfiType::Bool | FfiType::U8 | FfiType::U16 => "zeroext ",
        _ => "",
    }
}

fn piece_type(piece: CPiece) -> (usize, &'static str) {
    match piece {
        CPiece::Int { offset, .. } => (offset, "i64"),
        CPiece::Sse { offset, .. } => (offset, "double"),
        CPiece::Float { offset, scalar } => (offset, scalar_type(scalar.ffi_type())),
    }
}

fn forwarding_ir(
    address: usize,
    signature: &FfiSignature,
) -> Result<(String, FfiSignature), String> {
    let mut entry = Vec::new();
    let mut target = Vec::new();
    let mut loads = String::new();
    let (mut general, mut floats) = (0, 0);
    let result = match signature.result() {
        FfiType::Struct(layout) => match layout.passing(CAbi::SysV64) {
            CPassing::Memory => {
                let out = format!(
                    "ptr sret([{} x i8]) align {} %out",
                    layout.size(),
                    layout.align()
                );
                entry.push(out.clone());
                target.push(out);
                general += 1;
                "void".into()
            }
            CPassing::Registers(pieces) => {
                let types: Vec<_> = pieces.into_iter().map(|p| piece_type(p).1).collect();
                if let [ty] = types.as_slice() {
                    (*ty).to_owned()
                } else {
                    format!("{{ {} }}", types.join(", "))
                }
            }
        },
        scalar => scalar_type(scalar).to_owned(),
    };
    let mut params = Vec::new();
    for (i, &ty) in signature.parameters().iter().enumerate() {
        if let FfiType::Struct(layout) = ty {
            entry.push(format!("ptr %p{i}"));
            params.push(FfiType::StructPointer(layout));
            match layout.argument(CAbi::SysV64, general, floats)? {
                CArgument::Stack => target.push(format!(
                    "ptr byval([{} x i8]) align {} %p{i}",
                    layout.size(),
                    layout.align().max(8)
                )),
                CArgument::Registers(pieces) => {
                    for (n, piece) in pieces.into_iter().enumerate() {
                        let (offset, ty) = piece_type(piece);
                        let name = format!("p{i}_{n}");
                        loads.push_str(&format!("  %{name}_addr = getelementptr i8, ptr %p{i}, i64 {offset}\n  %{name} = load {ty}, ptr %{name}_addr, align 1\n"));
                        target.push(format!("{ty} %{name}"));
                        if ty == "i64" {
                            general += 1
                        } else {
                            floats += 1
                        }
                    }
                }
                CArgument::Address => return Err("unexpected indirect System V record".into()),
            }
        } else {
            params.push(ty);
            let extend = extension(ty);
            let parts = if matches!(
                ty,
                FfiType::StringView | FfiType::BytesView | FfiType::BytesViewMut
            ) {
                vec!["ptr", "i64"]
            } else {
                vec![scalar_type(ty)]
            };
            for (n, ty) in parts.into_iter().enumerate() {
                let arg = format!("{ty} {extend}%p{i}_{n}");
                entry.push(arg.clone());
                target.push(arg);
                if matches!(ty, "float" | "double") {
                    floats += 1
                } else {
                    general += 1
                }
            }
        }
    }
    let (assign, ret) = if result == "void" {
        ("", "ret void".into())
    } else {
        ("%r = ", format!("ret {result} %r"))
    };
    let extend = extension(signature.result());
    let ir = format!("define {extend}{result} @aura_ffi_forward({}) {{\n{loads}  {assign}call {extend}{result} inttoptr (i64 {address} to ptr)({})\n  {ret}\n}}\n", entry.join(", "), target.join(", "));
    Ok((ir, FfiSignature::new(params, signature.result())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c_layout::{CField, CFieldKind, CStruct};
    use crate::ffi::FfiScalar;

    fn record(fields: &[FfiScalar], align: usize) -> &'static CStruct {
        let mut layout = CStruct {
            name: "Record".into(),
            union: false,
            layout_controls: Default::default(),
            fields: fields
                .iter()
                .enumerate()
                .map(|(n, &scalar)| CField {
                    name: n.to_string(),
                    kind: CFieldKind::Scalar(scalar),
                })
                .collect(),
        };
        layout.layout_controls.alignment = Some(align);
        Box::leak(Box::new(layout))
    }

    #[test]
    fn adapter_signatures_preserve_scalar_and_register_piece_shapes() {
        let one = record(&[FfiScalar::U32], 4);
        let mixed = record(&[FfiScalar::U64, FfiScalar::F64], 8);
        let types = vec![
            FfiType::Bool,
            FfiType::I8,
            FfiType::I16,
            FfiType::U16,
            FfiType::I32,
            FfiType::U32,
            FfiType::I64,
            FfiType::U64,
            FfiType::F32,
            FfiType::F64,
            FfiType::StringView,
            FfiType::BytesView,
            FfiType::BytesViewMut,
            FfiType::CString,
            FfiType::Struct(one),
        ];
        for result in [
            FfiType::Unit,
            FfiType::U8,
            FfiType::Struct(one),
            FfiType::Struct(mixed),
        ] {
            let mut params = vec![FfiType::Struct(mixed)];
            params.extend_from_slice(&types);
            let signature = FfiSignature::new(params, result);
            let (ir, incoming) = forwarding_ir(1, &signature).unwrap();
            assert_eq!(incoming.parameters()[0], FfiType::StructPointer(mixed));
            let context = Context::create();
            let module = context
                .create_module_from_ir(MemoryBuffer::create_from_memory_range_copy(
                    ir.as_bytes(),
                    "adapter",
                ))
                .unwrap();
            module.verify().unwrap();
        }
        assert_eq!(
            piece_type(CPiece::Float {
                offset: 4,
                scalar: FfiScalar::F32
            }),
            (4, "float")
        );
    }

    #[cfg(all(target_arch = "x86_64", unix))]
    #[test]
    fn a_sysv_stack_adapter_passes_an_aligned_value_and_preserves_the_input() {
        #[repr(C, align(32))]
        struct Line {
            count: u32,
        }
        extern "C" fn bump(before: u32, value: Line, after: u32) -> Line {
            Line {
                count: before + value.count + after,
            }
        }
        let layout = record(&[FfiScalar::U32], 32);
        let sig = FfiSignature::new(
            vec![FfiType::U32, FfiType::Struct(layout), FfiType::U32],
            FfiType::Struct(layout),
        );
        assert!(sig.needs_aligned_stack());
        let mut bytes = vec![0; 32];
        bytes[..4].copy_from_slice(&5u32.to_ne_bytes());
        let mut args = [
            FfiValue::U32(3),
            FfiValue::Struct(layout, bytes),
            FfiValue::U32(7),
        ];
        let function = HostFunction::new(bump as *const () as *mut _).unwrap();
        // SAFETY: this is bump's exact C signature and all values are live.
        let direct = unsafe { ffi::call_host_function(function, &sig, &mut args) }.unwrap_err();
        assert!(direct.to_string().contains("aligned stack adapter"));
        for _ in 0..2 {
            let FfiValue::Struct(_, got) = invoke(function, &sig, &mut args).unwrap() else {
                panic!()
            };
            assert_eq!(u32::from_ne_bytes(got[..4].try_into().unwrap()), 15);
            assert!(matches!(&args[1], FfiValue::Struct(_, v) if v[0] == 5));
        }
        assert!(matches!(
            call("__aura_missing_aligned_host_function__", &sig, &mut args),
            Err(FfiError::SymbolNotFound { .. })
        ));
    }
}
