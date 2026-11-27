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
    for (bits, address, top) in [
        (8, base, 255),
        (16, base + 2, 65535),
        (32, base, u64::from(u32::MAX)),
        (64, base + 8, u64::MAX),
    ] {
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

#[test]
fn a_decoded_text_over_the_string_limit_traps_instead_of_being_built() {
    use crate::text_encodings::Encoding;
    // Each byte from 0x80 up decodes to two UTF-8 bytes, so half the limit
    // and one more byte decodes past it.
    let bytes = vec![0xE9_u8; crate::runtime_value::MAX_STRING_BYTES / 2 + 1];
    let error = decode_text_result(&bytes, Encoding::Latin1).expect_err("over the limit");
    assert_eq!(error.code, "AU4005");
    assert!(
        error.message.contains("`bytes.decode` result exceeds"),
        "{}",
        error.message
    );
    // Half the limit decodes to a text exactly at it.
    let fits = decode_text_result(&bytes[1..], Encoding::Latin1).expect("a decode");
    let Value::EnumVariant(fits) = fits else {
        panic!("a `Result`");
    };
    assert_eq!(&*fits.variant_name, "Ok");
    let Value::String(text) = &fits.payloads[0] else {
        panic!("a text");
    };
    assert_eq!(text.len(), crate::runtime_value::MAX_STRING_BYTES);
}

#[test]
fn ordered_atomics_check_order_and_arguments_before_writing() {
    let mut cell = 7_u16;
    let address = Value::Int(IntegerValue::from_u64((&mut cell as *mut u16) as u64));
    let raw_store = unsafe_mem_builtin_function("mem::atomic_store_uint16").unwrap();
    // SAFETY: `address` is the live, aligned `cell`, or a rejected address.
    let store = |args, program_args| unsafe { raw_store(args, program_args) };
    let value = Value::Int(IntegerValue::from_u64(19));
    for args in [
        vec![address.clone(), value.clone(), Value::string("acquire")],
        vec![address.clone(), value.clone(), Value::Bool(true)],
        vec![
            address.clone(),
            Value::Bool(false),
            Value::string("release"),
        ],
        vec![Value::Int(IntegerValue::from_i64(-1)), value.clone()],
    ] {
        assert!(store(args, None).is_err());
        assert_eq!(cell, 7);
    }
    unit(store(
        vec![address.clone(), value, Value::string("release")],
        None,
    ));
    assert_eq!(cell, 19);
    let raw_cas = unsafe_mem_builtin_function("mem::atomic_compare_exchange_uint16").unwrap();
    // SAFETY: `address` is the live, aligned `cell`.
    let cas = |args, program_args| unsafe { raw_cas(args, program_args) };
    let mut args = vec![
        address,
        Value::Int(IntegerValue::from_u64(19)),
        Value::Int(IntegerValue::from_u64(23)),
        Value::string("acq_rel"),
        Value::string("release"),
    ];
    assert_eq!(cas(args.clone(), None).unwrap_err().code, "AU4034");
    args[4] = Value::string("acquire");
    assert_eq!(word(cas(args, None)), 19);
    assert_eq!(cell, 23);
}

#[test]
fn allocation_functions_use_the_process_allocator() {
    let block = word(mem("alloc", &[24, 8]));
    assert_ne!(block, 0);
    assert_eq!(block % 8, 0);
    unit(mem("write_uint64", &[block + 16, 77]));
    let grown = word(mem("realloc", &[block, 24, 8, 4000]));
    assert_ne!(grown, 0);
    assert_eq!(word(mem("read_uint64", &[grown + 16])), 77);
    unit(mem("free", &[grown, 4000, 8]));
    // A request for no bytes gets an address of its own, at its alignment.
    let empty = word(mem("alloc", &[0, 4096]));
    assert_ne!(empty, 0);
    assert_eq!(empty % 4096, 0);
    unit(mem("free", &[empty, 0, 4096]));
    // `realloc` of address 0 allocates.
    let fresh = word(mem("realloc", &[0, 0, 16, 64]));
    assert_ne!(fresh, 0);
    unit(mem("free", &[fresh, 64, 16]));
    // Freeing address 0 does nothing.
    unit(mem("free", &[0, 8, 8]));
}

#[test]
fn an_allocation_the_process_cannot_describe_gives_address_zero() {
    // An alignment that is not a power of two, and a size past the
    // largest a block may have.
    assert_eq!(word(mem("alloc", &[8, 3])), 0);
    assert_eq!(word(mem("alloc", &[u64::MAX, 8])), 0);
    let block = word(mem("alloc", &[8, 8]));
    assert_eq!(word(mem("realloc", &[block, 8, 3, 16])), 0);
    assert_eq!(word(mem("realloc", &[block, 8, 8, u64::MAX])), 0);
    assert_eq!(word(mem("realloc", &[block, u64::MAX, 8, 16])), 0);
    // A size and alignment that describe no block free nothing.
    unit(mem("free", &[block, 8, 3]));
    unit(mem("free", &[block, 8, 8]));
}

#[test]
fn the_region_functions_do_nothing_in_a_hosted_program() {
    unit(mem("heap_add_region", &[4096, 4096]));
    assert_eq!(word(mem("heap_free_bytes", &[])), 0);
}

#[test]
fn allocation_functions_check_their_arguments() {
    for (name, count) in [
        ("alloc", 2),
        ("free", 3),
        ("realloc", 4),
        ("heap_add_region", 2),
        ("heap_free_bytes", 0),
    ] {
        let too_many = vec![8; count + 1];
        assert!(mem(name, &too_many).is_err(), "{name}");
        if count > 0 {
            let args = (0..count).map(|_| Value::Bool(true)).collect();
            let function =
                unsafe_mem_builtin_function(&format!("mem::{name}")).expect("a mem builtin");
            // SAFETY: a bool argument is rejected before any address is used.
            let result = unsafe { function(args, None) };
            assert_eq!(result.expect_err("a bool is no address").code, "AU4001");
        }
    }
}
