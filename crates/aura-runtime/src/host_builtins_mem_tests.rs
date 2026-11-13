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
