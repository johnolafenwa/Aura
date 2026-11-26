use crate::frozen_value::FrozenValue;
use crate::native_runtime;
use crate::runtime_value::Value;
use aura_core::types::Type;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn compiled_data_publishes_one_typed_tuple_per_program_cell() {
    // Teardown releases all direct constants in the process. Isolate the program
    // lifecycle from other runtime tests, including tests running in parallel.
    const HELPER: &str = "frozen-data-typed-tuple";
    if std::env::var("AURA_DIRECT_RUNTIME_HELPER").as_deref() != Ok(HELPER) {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "native_runtime::tests::compile_time_transport::compiled_data_publishes_one_typed_tuple_per_program_cell"])
            .env("AURA_DIRECT_RUNTIME_HELPER", HELPER)
            .output()
            .unwrap();
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    static CELL: AtomicUsize = AtomicUsize::new(0);
    let types = vec![Type::named("bool"), Type::named("str")];
    let descriptor = native_runtime::canonical_runtime_type_name(&Type::Tuple(types.clone()));
    let tuple = FrozenValue::Tuple(
        types,
        vec![FrozenValue::Bool(true), FrozenValue::Str("board".into())],
    );
    let data = serde_json::to_vec(&tuple).unwrap();
    // SAFETY: the static cell lasts for the program. Both buffers are valid for
    // each synchronous call and encode the type and value emitted by the compiler.
    let read = || unsafe {
        native_runtime::frozen::aura_direct_frozen_constant(
            &CELL,
            data.as_ptr(),
            data.len(),
            descriptor.as_ptr(),
            descriptor.len(),
        )
    };
    let first = read();
    assert_eq!(first, read());
    assert_eq!(CELL.load(Ordering::Acquire), first as usize);
    assert!(native_runtime::direct_value_is_frozen(first));
    // SAFETY: the cell owns the published box until program teardown below.
    unsafe {
        assert_eq!(
            native_runtime::explicit_runtime_type_name(first).as_deref(),
            Some(descriptor.as_str())
        );
        native_runtime::with_value(first, |value| {
            let Value::Tuple(decoded) = value else {
                panic!("expected a frozen tuple")
            };
            assert_eq!(
                decoded.elements,
                vec![Value::Bool(true), Value::string("board")]
            );
        });
    }
    native_runtime::clear_direct_module_constants();
    assert_eq!(CELL.load(Ordering::Acquire), 0);
}

#[test]
fn invalid_compiled_data_reports_the_decode_error_and_leaves_the_cell_empty() {
    static CELL: AtomicUsize = AtomicUsize::new(0);
    let error = super::capture_direct_boundary_error_message(|| {
        let data = b"not-json";
        let descriptor = b"bool";
        // SAFETY: the cell is static and both byte slices are valid for the call.
        unsafe {
            native_runtime::frozen::aura_direct_frozen_constant(
                &CELL,
                data.as_ptr(),
                data.len(),
                descriptor.as_ptr(),
                descriptor.len(),
            );
        }
    });
    assert!(error.starts_with("invalid compiled constant:"), "{error}");
    assert_eq!(CELL.load(Ordering::Acquire), 0);
}
