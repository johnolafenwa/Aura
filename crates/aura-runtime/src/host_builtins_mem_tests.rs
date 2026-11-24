use super::*;

fn mem(name: &str, args: &[u64]) -> Result<Value> {
    let args = args
        .iter()
        .map(|value| Value::Int(IntegerValue::from_u64(*value)))
        .collect();
    host_builtin_function(&format!("mem::{name}")).expect("a mem builtin")(args, None)
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
    let function = host_builtin_function("mem::read_uint64").expect("a mem builtin");
    let error = function(vec![Value::Bool(true)], None).expect_err("not an integer");
    assert_eq!(error.message, "`mem.read_uint64` expects unsigned integers");
    let negative = Value::Int(IntegerValue::from_i64(-1));
    let error = function(vec![negative], None).expect_err("not an address");
    assert_eq!(error.message, "`mem.read_uint64` expects unsigned integers");
    let wide = Value::Int(IntegerValue::from_u64(300));
    let error = host_builtin_function("mem::write_uint64").expect("a mem builtin")(
        vec![
            Value::Int(IntegerValue::from_u64(address)),
            Value::Bool(false),
        ],
        None,
    )
    .expect_err("not a value");
    assert!(error.message.contains("expects unsigned integers"));
    // A value wider than the access keeps its low bits, as generated code does.
    let write = host_builtin_function("mem::write_uint8").expect("a mem builtin");
    unit(write(
        vec![Value::Int(IntegerValue::from_u64(address)), wide],
        None,
    ));
    assert_eq!(cell, 44);
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
            let result =
                host_builtin_function(&format!("mem::{name}")).expect("a mem builtin")(args, None);
            assert_eq!(result.expect_err("a bool is no address").code, "AU4001");
        }
    }
}
