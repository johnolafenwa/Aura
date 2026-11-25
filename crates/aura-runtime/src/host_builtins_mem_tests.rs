use super::*;

fn mem(name: &str, args: &[u64]) -> Result<Value> {
    let args = args
        .iter()
        .map(|value| Value::Int(IntegerValue::from_u64(*value)))
        .collect();
    let function = unsafe_mem_builtin_function(&format!("mem::{name}")).expect("a mem builtin");
    unsafe { function(args, None) }
}

fn word(value: Result<Value>) -> u64 {
    match value.expect("the access succeeds") {
        Value::Int(value) => {
            u64::try_from(value.as_i128().expect("a small integer")).expect("an unsigned result")
        }
        other => panic!("expected an integer, found {other:?}"),
    }
}

fn unit(value: Result<Value>) {
    assert!(matches!(value, Ok(Value::Unit)));
}

#[test]
fn reads_and_writes_reach_the_address_at_each_width() {
    let mut block = [0_u64; 4];
    let base = block.as_mut_ptr() as u64;
    for (bits, value) in [
        (8, 0xa5_u64),
        (16, 0xbeef),
        (32, 0xdead_beef),
        (64, 0x0123_4567_89ab_cdef),
    ] {
        for prefix in ["", "volatile_"] {
            let address = base + 8;
            unit(mem(&format!("{prefix}write_uint{bits}"), &[address, value]));
            assert_eq!(
                word(mem(&format!("{prefix}read_uint{bits}"), &[address])),
                value
            );
            unit(mem(&format!("{prefix}write_uint{bits}"), &[address, 0]));
        }
    }
    // A plain access may sit at any address.
    unit(mem("write_uint32", &[base + 1, 0x0403_0201]));
    assert_eq!(word(mem("read_uint16", &[base + 2])), 0x0302);
    assert_eq!(block[0].to_le_bytes(), [0, 1, 2, 3, 4, 0, 0, 0]);
}

#[test]
fn atomic_operations_give_the_previous_value() {
    let mut block = [0_u64; 2];
    let base = block.as_mut_ptr() as u64;
    for (bits, address, top) in [(32, base, u64::from(u32::MAX)), (64, base + 8, u64::MAX)] {
        let call = |name: &str, args: &[u64]| mem(&format!("atomic_{name}_uint{bits}"), args);
        unit(call("store", &[address, 5]));
        assert_eq!(word(call("load", &[address])), 5);
        // The sum wraps.
        assert_eq!(word(call("add", &[address, top])), 5);
        assert_eq!(word(call("load", &[address])), 4);
        assert_eq!(word(call("swap", &[address, 9])), 4);
        assert_eq!(word(call("compare_exchange", &[address, 9, 11])), 9);
        assert_eq!(word(call("compare_exchange", &[address, 9, 12])), 11);
        assert_eq!(word(call("load", &[address])), 11);
    }
    unit(mem("fence", &[]));
}

#[test]
fn a_call_with_the_wrong_arguments_is_an_error() {
    let mut cell = 0_u64;
    let address = &mut cell as *mut u64 as u64;
    for (name, args) in [
        ("read_uint8", &[][..]),
        ("write_uint8", &[address][..]),
        ("atomic_compare_exchange_uint32", &[address, 1][..]),
        ("fence", &[address][..]),
    ] {
        let error = mem(name, args).expect_err("the argument count is wrong");
        assert!(error.message.contains("expects"), "{}", error.message);
    }
    let function = unsafe_mem_builtin_function("mem::read_uint64").expect("a mem builtin");
    let error = unsafe { function(vec![Value::Bool(true)], None) }.expect_err("not an integer");
    assert_eq!(error.message, "`mem.read_uint64` expects unsigned integers");
    let negative = Value::Int(IntegerValue::from_i64(-1));
    let error = unsafe { function(vec![negative], None) }.expect_err("not an address");
    assert_eq!(error.message, "`mem.read_uint64` expects unsigned integers");
    let wide = Value::Int(IntegerValue::from_u64(300));
    let error = unsafe {
        unsafe_mem_builtin_function("mem::write_uint64").expect("a mem builtin")(
            vec![
                Value::Int(IntegerValue::from_u64(address)),
                Value::Bool(false),
            ],
            None,
        )
    }
    .expect_err("not a value");
    assert!(error.message.contains("expects unsigned integers"));
    // A value wider than the access keeps its low bits, as generated code does.
    let write = unsafe_mem_builtin_function("mem::write_uint8").expect("a mem builtin");
    unit(unsafe {
        write(
            vec![Value::Int(IntegerValue::from_u64(address)), wide],
            None,
        )
    });
    assert_eq!(cell, 44);
}

#[test]
fn mem_builtins_are_excluded_from_safe_table_and_validate_addresses() {
    for name in [
        "mem::read_uint8",
        "mem::read_uint16",
        "mem::read_uint32",
        "mem::read_uint64",
        "mem::write_uint8",
        "mem::write_uint16",
        "mem::write_uint32",
        "mem::write_uint64",
        "mem::volatile_read_uint8",
        "mem::volatile_read_uint16",
        "mem::volatile_read_uint32",
        "mem::volatile_read_uint64",
        "mem::volatile_write_uint8",
        "mem::volatile_write_uint16",
        "mem::volatile_write_uint32",
        "mem::volatile_write_uint64",
        "mem::atomic_load_uint32",
        "mem::atomic_load_uint64",
        "mem::atomic_store_uint32",
        "mem::atomic_store_uint64",
        "mem::atomic_swap_uint32",
        "mem::atomic_swap_uint64",
        "mem::atomic_add_uint32",
        "mem::atomic_add_uint64",
        "mem::atomic_compare_exchange_uint32",
        "mem::atomic_compare_exchange_uint64",
        "mem::fence",
    ] {
        assert!(
            host_builtin_function(name).is_none(),
            "`{name}` must not be exposed via safe host_builtin_function"
        );
        assert!(
            unsafe_mem_builtin_function(name).is_some(),
            "`{name}` must be registered in unsafe_mem_builtin_function"
        );
    }
    assert!(unsafe_mem_builtin_function("math::floor").is_none());
    assert!(
        evaluate_host_builtin("mem::fence", Vec::new()).is_err(),
        "safe evaluate_host_builtin must not dispatch mem::* builtins"
    );
    assert!(matches!(
        unsafe { evaluate_unsafe_host_builtin_with_program_args("mem::fence", Vec::new(), &[]) },
        Ok(Value::Unit)
    ));
    assert!(unsafe {
        evaluate_unsafe_host_builtin_with_program_args("math::floor", vec![Value::Float(1.5)], &[])
    }
    .is_ok());

    // Null addresses are rejected before dereference for every width and mode.
    for (name, args) in [
        ("read_uint8", &[0][..]),
        ("read_uint64", &[0][..]),
        ("write_uint8", &[0, 1][..]),
        ("write_uint64", &[0, 1][..]),
        ("volatile_read_uint8", &[0][..]),
        ("volatile_read_uint16", &[0][..]),
        ("volatile_read_uint32", &[0][..]),
        ("volatile_read_uint64", &[0][..]),
        ("volatile_write_uint8", &[0, 1][..]),
        ("volatile_write_uint32", &[0, 1][..]),
        ("atomic_load_uint32", &[0][..]),
        ("atomic_load_uint64", &[0][..]),
        ("atomic_store_uint32", &[0, 1][..]),
        ("atomic_store_uint64", &[0, 1][..]),
        ("atomic_compare_exchange_uint32", &[0, 0, 1][..]),
        ("atomic_compare_exchange_uint64", &[0, 0, 1][..]),
    ] {
        let error = mem(name, args).expect_err(&format!("{name} must reject null address"));
        assert_eq!(error.code, "AU4001", "{name}: {error:?}");
        assert!(error.message.contains("null"), "{name}: {}", error.message);
    }

    // Misaligned addresses are rejected for volatile and atomic accesses.
    let mut block = [0_u64; 4];
    let base = block.as_mut_ptr() as u64;
    for (name, args) in [
        ("volatile_read_uint16", &[base + 1][..]),
        ("volatile_write_uint16", &[base + 1, 7][..]),
        ("volatile_read_uint32", &[base + 2][..]),
        ("volatile_write_uint32", &[base + 2, 7][..]),
        ("volatile_read_uint64", &[base + 4][..]),
        ("volatile_write_uint64", &[base + 4, 7][..]),
        ("atomic_load_uint32", &[base + 1][..]),
        ("atomic_load_uint32", &[base + 2][..]),
        ("atomic_store_uint32", &[base + 2, 1][..]),
        ("atomic_swap_uint32", &[base + 2, 1][..]),
        ("atomic_add_uint32", &[base + 2, 1][..]),
        ("atomic_compare_exchange_uint32", &[base + 2, 0, 1][..]),
        ("atomic_load_uint64", &[base + 4][..]),
        ("atomic_store_uint64", &[base + 4, 1][..]),
        ("atomic_swap_uint64", &[base + 4, 1][..]),
        ("atomic_add_uint64", &[base + 4, 1][..]),
        ("atomic_compare_exchange_uint64", &[base + 4, 0, 1][..]),
    ] {
        let error = mem(name, args).expect_err(&format!("{name} must reject misaligned address"));
        assert_eq!(error.code, "AU4001", "{name}: {error:?}");
        assert!(
            error.message.contains("aligned"),
            "{name}: {}",
            error.message
        );
    }
}
