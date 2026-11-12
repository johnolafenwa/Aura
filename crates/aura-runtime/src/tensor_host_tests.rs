//! Tests of the `tensor` module's host part: each operation by the Array
//! kernels, the same expressions as waiting nodes run through a plan, and,
//! on a machine with a Metal device, the same expressions on the GPU.

use super::*;

fn int(value: i64) -> Value {
    int_value(value)
}

fn ints(values: &[i64]) -> Value {
    let mut elements = try_array_buffer(values.len(), "test list").unwrap();
    elements.extend(values.iter().map(|&value| int(value)));
    Value::Vec(VecValue {
        element_type: SharedType::named("int64"),
        elements: elements.into(),
        str_view: Default::default(),
    })
}

fn float32(values: &[f32], shape: &[usize]) -> ArrayValue {
    ArrayValue::new(
        shape.into(),
        ArrayStorage::Float32(values.to_vec().into_boxed_slice().into()),
    )
    .unwrap()
}

fn int64(values: &[i64], shape: &[usize]) -> ArrayValue {
    ArrayValue::new(
        shape.into(),
        ArrayStorage::Int64(values.to_vec().into_boxed_slice().into()),
    )
    .unwrap()
}

fn int32(values: &[i32], shape: &[usize]) -> ArrayValue {
    ArrayValue::new(
        shape.into(),
        ArrayStorage::Int32(values.to_vec().into_boxed_slice().into()),
    )
    .unwrap()
}

fn tensor(elements: ArrayValue, device: TensorDevice) -> Value {
    host_tensor_from_array(vec![Value::Array(elements), int(device as i64)], None).unwrap()
}

fn array(node: &Value) -> ArrayValue {
    match host_tensor_to_array(vec![node.clone()], None).unwrap() {
        Value::Array(elements) => elements,
        _ => unreachable!(),
    }
}

fn node(value: &Value) -> &TensorValue {
    match value {
        Value::Tensor(node) => node,
        _ => unreachable!(),
    }
}

fn binary(left: &Value, right: &Value, code: i64) -> Result<Value> {
    host_tensor_binary(vec![left.clone(), right.clone(), int(code)], None)
}

fn unary(operand: &Value, code: i64) -> Result<Value> {
    host_tensor_unary(vec![operand.clone(), int(code)], None)
}

fn compare(left: &Value, right: &Value, code: i64) -> Result<Value> {
    host_tensor_compare(vec![left.clone(), right.clone(), int(code)], None)
}

fn choose(mask: &Value, when_true: &Value, when_false: &Value) -> Result<Value> {
    host_tensor_where(
        vec![mask.clone(), when_true.clone(), when_false.clone()],
        None,
    )
}

fn reduce(operand: &Value, reduction: i64, axis: Option<i64>) -> Result<Value> {
    host_tensor_reduce(
        vec![
            operand.clone(),
            int(reduction),
            int(axis.unwrap_or(0)),
            Value::Bool(axis.is_some()),
        ],
        None,
    )
}

fn matmul(left: &Value, right: &Value) -> Result<Value> {
    host_tensor_matmul(vec![left.clone(), right.clone()], None)
}

fn reshape(operand: &Value, shape: &[i64]) -> Result<Value> {
    host_tensor_reshape(vec![operand.clone(), ints(shape)], None)
}

fn permute(operand: &Value, axes: &[i64]) -> Result<Value> {
    host_tensor_permute(vec![operand.clone(), ints(axes)], None)
}

fn cast(operand: &Value, dtype: ArrayDType) -> Result<Value> {
    host_tensor_cast(vec![operand.clone(), int(dtype as i64)], None)
}

/// The leaves every expression below is made from.
struct Leaves {
    a: Value,
    b: Value,
    row: Value,
    tall: Value,
    two: Value,
    counts: Value,
    steps: Value,
    small: Value,
}

fn leaves(device: TensorDevice) -> Leaves {
    Leaves {
        a: tensor(float32(&[1.0, -2.0, 3.0, 4.5, 0.25, 6.0], &[2, 3]), device),
        b: tensor(float32(&[2.0, 0.5, -1.0, 3.0, 8.0, 0.125], &[2, 3]), device),
        row: tensor(float32(&[10.0, 20.0, 30.0], &[3]), device),
        tall: tensor(float32(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], &[3, 2]), device),
        two: tensor(float32(&[2.0], &[1]), device),
        counts: tensor(int64(&[5, -3, i64::MAX, 7], &[4]), device),
        steps: tensor(int64(&[2, 2, 2, -1], &[4]), device),
        small: tensor(int32(&[1, -2, 3, 40], &[2, 2]), device),
    }
}

type Expression = fn(&Leaves) -> Result<Value>;

/// Expressions a GPU library runs as a plan.
const PLANNED: &[(&str, Expression)] = &[
    ("add", |l| binary(&l.a, &l.b, 0)),
    ("sub broadcast", |l| binary(&l.a, &l.row, 1)),
    ("mul scalar", |l| binary(&l.a, &l.two, 2)),
    ("div", |l| binary(&l.a, &l.b, 3)),
    ("minimum", |l| binary(&l.a, &l.b, 16)),
    ("maximum", |l| binary(&l.a, &l.b, 17)),
    ("pow", |l| binary(&unary(&l.a, 0)?, &l.b, 18)),
    ("abs", |l| unary(&l.a, 0)),
    ("sqrt", |l| unary(&unary(&l.a, 0)?, 1)),
    ("exp", |l| unary(&l.a, 2)),
    ("log", |l| unary(&unary(&l.a, 0)?, 3)),
    ("sin", |l| unary(&l.a, 6)),
    ("cos", |l| unary(&l.a, 7)),
    ("negate", |l| unary(&l.a, NEGATE_OPERATION)),
    ("less", |l| compare(&l.a, &l.b, 0)),
    ("less equal", |l| compare(&l.a, &l.b, 1)),
    ("greater", |l| compare(&l.a, &l.b, 2)),
    ("greater equal", |l| compare(&l.a, &l.b, 3)),
    ("equal", |l| compare(&l.a, &l.a, 4)),
    ("not equal", |l| compare(&l.a, &l.b, 5)),
    ("choose", |l| choose(&compare(&l.a, &l.b, 2)?, &l.a, &l.b)),
    ("sum", |l| reduce(&l.a, 0, None)),
    ("min", |l| reduce(&l.a, 1, None)),
    ("max", |l| reduce(&l.a, 2, None)),
    ("mean", |l| reduce(&l.a, 3, None)),
    ("sum axis 0", |l| reduce(&l.a, 0, Some(0))),
    ("max axis -1", |l| reduce(&l.a, 2, Some(-1))),
    ("mean axis 1", |l| reduce(&l.a, 3, Some(1))),
    ("sum of row", |l| reduce(&l.row, 0, Some(0))),
    ("matmul", |l| matmul(&l.a, &l.tall)),
    ("reshape", |l| reshape(&l.a, &[3, 2])),
    ("reshape inferred", |l| reshape(&l.a, &[-1, 2])),
    ("transpose", |l| permute(&l.a, &[])),
    ("permute", |l| permute(&l.a, &[-1, 0])),
    ("int add wraps", |l| binary(&l.counts, &l.steps, 0)),
    ("int mul wraps", |l| binary(&l.counts, &l.steps, 2)),
    ("int negate", |l| unary(&l.counts, NEGATE_OPERATION)),
    ("int compare", |l| compare(&l.counts, &l.steps, 2)),
    ("int32 add", |l| binary(&l.small, &l.small, 0)),
    ("int32 to float32", |l| cast(&l.small, ArrayDType::Float32)),
    ("int32 to int64", |l| cast(&l.small, ArrayDType::Int64)),
    ("int64 to float32", |l| cast(&l.steps, ArrayDType::Float32)),
    ("chain", |l| {
        let scaled = binary(&binary(&l.a, &l.b, 2)?, &l.a, 0)?;
        let shifted = binary(&scaled, &l.row, 1)?;
        reduce(&matmul(&shifted, &l.tall)?, 0, Some(0))
    }),
    ("shared operand", |l| {
        let sum = binary(&l.a, &l.b, 0)?;
        binary(&sum, &sum, 2)
    }),
];

/// Expressions that run through the Array kernels on every device.
const ON_HOST: &[(&str, Expression)] = &[
    ("int sum", |l| reduce(&l.steps, 0, None)),
    ("int max axis", |l| reduce(&l.small, 2, Some(0))),
    ("int abs", |l| unary(&l.steps, 0)),
    ("float32 to int64", |l| cast(&l.a, ArrayDType::Int64)),
    ("int matmul", |l| matmul(&l.small, &l.small)),
    ("vector matmul", |l| matmul(&l.a, &l.row)),
];

/// Runs `body` with the CPU device waiting like a GPU device.
fn deferred<T>(body: impl FnOnce() -> T) -> T {
    DEFER_ON_CPU.with(|flag| flag.set(true));
    let result = body();
    DEFER_ON_CPU.with(|flag| flag.set(false));
    result
}

#[test]
fn a_plan_gives_what_each_operation_gives_at_once() {
    for (name, expression) in PLANNED {
        let eager = expression(&leaves(TensorDevice::Cpu)).unwrap();
        assert!(
            node(&eager).is_ready(),
            "{name}: the cpu device runs at once"
        );
        let waiting = deferred(|| expression(&leaves(TensorDevice::Cpu)).unwrap());
        assert!(!node(&waiting).is_ready(), "{name}: the operation waits");
        assert_eq!(
            node(&waiting).dtype(),
            node(&eager).dtype(),
            "{name}: dtype"
        );
        assert_eq!(
            node(&waiting).shape(),
            node(&eager).shape(),
            "{name}: shape"
        );
        assert_eq!(array(&waiting), array(&eager), "{name}: elements");
        assert!(node(&waiting).is_ready(), "{name}: a read runs the work");
    }
}

#[test]
fn an_operation_a_gpu_library_lacks_runs_at_once() {
    for (name, expression) in ON_HOST {
        let eager = expression(&leaves(TensorDevice::Cpu)).unwrap();
        let fallback = deferred(|| expression(&leaves(TensorDevice::Cpu)).unwrap());
        assert!(node(&fallback).is_ready(), "{name}: runs at once");
        assert_eq!(array(&fallback), array(&eager), "{name}: elements");
    }
}

#[test]
fn a_mistake_is_reported_by_the_call_that_made_it() {
    deferred(|| {
        let l = leaves(TensorDevice::Cpu);
        for (name, result) in [
            ("shapes", binary(&l.a, &l.tall, 0)),
            ("dtypes", binary(&l.a, &l.counts, 0)),
            ("matmul", matmul(&l.a, &l.a)),
            ("reshape", reshape(&l.a, &[4, 2])),
            ("reshape two inferred", reshape(&l.a, &[-1, -1])),
            ("permute", permute(&l.a, &[0, 0])),
            ("permute rank", permute(&l.a, &[0])),
            ("axis", reduce(&l.a, 0, Some(2))),
            ("choose mask", choose(&l.a, &l.a, &l.b)),
            (
                "choose dtypes",
                choose(&compare(&l.a, &l.b, 0).unwrap(), &l.a, &l.small),
            ),
            ("unknown binary", binary(&l.a, &l.b, 9)),
            ("unknown pair", binary(&l.a, &l.b, 40)),
            ("unknown unary", unary(&l.a, 50)),
            ("unknown compare", compare(&l.a, &l.b, 9)),
            ("unknown reduction", reduce(&l.a, 9, None)),
            (
                "unknown dtype",
                host_tensor_cast(vec![l.a.clone(), int(9)], None),
            ),
        ] {
            assert_eq!(result.unwrap_err().code, "AU4007", "{name}");
        }
    });
}

#[test]
fn a_long_chain_runs_before_it_is_read() {
    deferred(|| {
        let l = leaves(TensorDevice::Cpu);
        let mut total = l.a.clone();
        let mut ran = 0;
        for _ in 0..(2 * MAX_PENDING_DEPTH + 5) {
            total = binary(&total, &l.two, 0).unwrap();
            ran += usize::from(node(&total).is_ready());
        }
        assert_eq!(ran, 2, "the chain runs each time it passes the limit");
        let expected = 1.0 + 2.0 * (2 * MAX_PENDING_DEPTH + 5) as f32;
        assert_eq!(
            array(&total).value_at_flat(0),
            Value::Float(f64::from(expected))
        );
    });
}

#[test]
fn equal_expressions_have_equal_plans() {
    deferred(|| {
        let plan = |l: &Leaves| {
            let sum = binary(&l.a, &l.b, 0).unwrap();
            let product = binary(&sum, &sum, 2).unwrap();
            Plan::collect(node(&product))
        };
        let (first, first_leaves) = plan(&leaves(TensorDevice::Cpu));
        let (second, _) = plan(&leaves(TensorDevice::Cpu));
        assert_eq!(first, second);
        assert_eq!(first.slots.len(), 4, "a shared operand is one slot");
        assert_eq!(first_leaves.len(), 2);
        let other = Plan::collect(node(
            &binary(
                &leaves(TensorDevice::Cpu).a,
                &leaves(TensorDevice::Cpu).row,
                0,
            )
            .unwrap(),
        ))
        .0;
        assert_ne!(first, other);
    });
}

#[test]
fn creation_and_inspection() {
    let l = leaves(TensorDevice::Cpu);
    assert_eq!(
        host_tensor_shape(vec![l.a.clone()], None).unwrap(),
        ints(&[2, 3])
    );
    assert_eq!(host_tensor_len(vec![l.a.clone()], None).unwrap(), int(6));
    assert_eq!(host_tensor_device(vec![l.a.clone()], None).unwrap(), int(0));
    assert_eq!(
        host_tensor_device_available(vec![int(0)], None).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        host_tensor_device_available(vec![int(7)], None).unwrap(),
        Value::Bool(false)
    );
    let filled =
        host_tensor_full(vec![ints(&[2, 2]), Value::Float(1.5), int(2), int(0)], None).unwrap();
    assert_eq!(array(&filled), float32(&[1.5; 4], &[2, 2]));
    assert_eq!(
        host_tensor_item(vec![l.two.clone()], None).unwrap(),
        Value::Float(2.0)
    );
    assert!(host_tensor_item(vec![l.a.clone()], None).is_err());
    assert_eq!(
        host_tensor_eval(vec![l.a.clone()], None).unwrap(),
        Value::Unit
    );
    let shared = host_tensor_share(vec![l.a.clone()], None).unwrap();
    assert_eq!(shared, l.a);
    assert_ne!(l.a, l.b);
    assert_eq!(format!("{:?}", node(&l.a)), "<tensor-node>");
    assert!(!node(&l.a).is_empty());
    let stays = host_tensor_to_device(vec![l.a.clone(), int(0)], None).unwrap();
    assert_eq!(stays, l.a, "a tensor already on the device is the same tensor");
    let same = cast(&l.a, ArrayDType::Float32).unwrap();
    assert_eq!(same, l.a, "a cast to the same dtype is the same tensor");
    let empty = tensor(float32(&[], &[0, 3]), TensorDevice::Cpu);
    deferred(|| {
        let sum = binary(&empty, &empty, 0).unwrap();
        assert!(node(&sum).is_ready(), "an empty tensor runs at once");
        assert_eq!(node(&sum).shape(), &[0, 3]);
    });
}

#[test]
fn arguments_are_checked() {
    let l = leaves(TensorDevice::Cpu);
    let not_a_tensor = int(1);
    for (name, result) in [
        ("tensor", unary(&not_a_tensor, 0)),
        ("array", host_tensor_from_array(vec![int(1), int(0)], None)),
        (
            "int",
            host_tensor_unary(vec![l.a.clone(), Value::Bool(true)], None),
        ),
        (
            "bool",
            host_tensor_reduce(vec![l.a.clone(), int(0), int(0), int(0)], None),
        ),
        ("list", host_tensor_reshape(vec![l.a.clone(), int(0)], None)),
        (
            "list element",
            host_tensor_reshape(
                vec![
                    l.a.clone(),
                    Value::Vec(VecValue {
                        element_type: SharedType::named("bool"),
                        elements: {
                            let mut elements = try_array_buffer(1, "test list").unwrap();
                            elements.extend([Value::Bool(true)]);
                            elements.into()
                        },
                        str_view: Default::default(),
                    }),
                ],
                None,
            ),
        ),
        (
            "device",
            host_tensor_from_array(vec![Value::Array(float32(&[1.0], &[1])), int(9)], None),
        ),
        (
            "shape",
            host_tensor_full(vec![ints(&[-1]), Value::Float(0.0), int(2), int(0)], None),
        ),
        (
            "dtype",
            host_tensor_full(vec![ints(&[1]), Value::Float(0.0), int(9), int(0)], None),
        ),
    ] {
        assert_eq!(result.unwrap_err().code, "AU4007", "{name}");
    }
    assert_eq!(TensorDevice::Metal.name(), "metal");
    assert_eq!(TensorDevice::from_code(1), Some(TensorDevice::Metal));
}

#[test]
fn host_functions_are_found_by_name() {
    for name in [
        "tensor::__aura_device_available",
        "tensor::__aura_from_array_f32",
        "tensor::__aura_to_array_i64",
        "tensor::__aura_item_bool",
        "tensor::__aura_full_i32",
        "tensor::__aura_shape",
        "tensor::__aura_len",
        "tensor::__aura_device",
        "tensor::__aura_binary",
        "tensor::__aura_unary",
        "tensor::__aura_compare",
        "tensor::__aura_where",
        "tensor::__aura_reduce",
        "tensor::__aura_matmul",
        "tensor::__aura_reshape",
        "tensor::__aura_permute",
        "tensor::__aura_cast",
        "tensor::__aura_to_device",
        "tensor::__aura_eval",
        "tensor::__aura_share",
    ] {
        assert!(tensor_host_function(name).is_some(), "{name}");
    }
    assert!(tensor_host_function("tensor::__aura_missing").is_none());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn the_metal_device_exists_only_on_macos() {
    assert!(!TensorDevice::Metal.is_available());
    let error = host_tensor_from_array(vec![Value::Array(float32(&[1.0], &[1])), int(1)], None)
        .unwrap_err();
    assert!(error.message.contains("cannot use the metal device"));
    let l = leaves(TensorDevice::Cpu);
    let other = Value::Tensor(TensorValue::host(
        TensorDevice::Metal,
        float32(&[1.0], &[1]),
    ));
    assert!(binary(&l.two, &other, 0)
        .unwrap_err()
        .message
        .contains("expects tensors on one device"));
    // A GPU tensor never holds `float64`, wherever its memory is.
    assert!(cast(&other, ArrayDType::Float64)
        .unwrap_err()
        .message
        .contains("no float64"));
}

/// How far a GPU result may be from the Array kernels' result: exact for
/// integers, masks, and operations that round once; a few units in the
/// last place for a sum, whose order differs, and for the library's
/// `exp`, `log`, `sin`, `cos` and `pow`.
#[cfg(target_os = "macos")]
fn close(name: &str, gpu: &ArrayValue, host: &ArrayValue) {
    assert_eq!(gpu.dtype(), host.dtype(), "{name}: dtype");
    assert_eq!(gpu.shape, host.shape, "{name}: shape");
    for index in 0..host.len() {
        match (gpu.value_at_flat(index), host.value_at_flat(index)) {
            (Value::Float(gpu), Value::Float(host)) => {
                let tolerance = 8.0 * f64::from(f32::EPSILON) * host.abs().max(1.0);
                assert!(
                    (gpu - host).abs() <= tolerance || (gpu.is_nan() && host.is_nan()),
                    "{name}: element {index} is {gpu} on the gpu and {host} on the host"
                );
            }
            (gpu, host) => assert_eq!(gpu, host, "{name}: element {index}"),
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn the_metal_device_agrees_with_the_array_kernels() {
    if !TensorDevice::Metal.is_available() {
        eprintln!("SKIPPED: this machine has no usable Metal device");
        return;
    }
    for (name, expression) in PLANNED.iter().chain(ON_HOST) {
        let host = expression(&leaves(TensorDevice::Cpu)).unwrap();
        let gpu = expression(&leaves(TensorDevice::Metal)).unwrap();
        assert_eq!(node(&gpu).device(), TensorDevice::Metal);
        close(name, &array(&gpu), &array(&host));
        // A second run takes the compiled graph.
        let again = expression(&leaves(TensorDevice::Metal)).unwrap();
        close(name, &array(&again), &array(&host));
    }
    for (name, expression) in PLANNED {
        let gpu = expression(&leaves(TensorDevice::Metal)).unwrap();
        assert!(!node(&gpu).is_ready(), "{name}: waits on the gpu");
        host_tensor_eval(vec![gpu.clone()], None).unwrap();
        assert!(
            matches!(node(&gpu).storage().unwrap(), Storage::Metal(_)),
            "{name}: the result is in a device buffer"
        );
    }
    println!(
        "RAN: {} expressions on the Metal device",
        PLANNED.len() + ON_HOST.len()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn the_metal_device_keeps_special_values() {
    if !TensorDevice::Metal.is_available() {
        eprintln!("SKIPPED: this machine has no usable Metal device");
        return;
    }
    let special = |device| {
        (
            tensor(
                float32(&[f32::NAN, 1.0, -0.0, f32::INFINITY, -3.0, 0.0], &[6]),
                device,
            ),
            tensor(
                float32(&[1.0, f32::NAN, 0.0, 2.0, f32::NEG_INFINITY, 0.0], &[6]),
                device,
            ),
        )
    };
    let cases: &[(&str, fn(&Value, &Value) -> Result<Value>)] = &[
        ("minimum", |a, b| binary(a, b, 16)),
        ("maximum", |a, b| binary(a, b, 17)),
        ("div", |a, b| binary(a, b, 3)),
        ("less", |a, b| compare(a, b, 0)),
        ("equal", |a, b| compare(a, b, 4)),
        ("not equal", |a, b| compare(a, b, 5)),
        ("min", |a, _| reduce(a, 1, None)),
        ("max", |a, _| reduce(a, 2, None)),
        ("sqrt", |a, _| unary(a, 1)),
        ("abs", |a, _| unary(a, 0)),
        ("negate", |a, _| unary(a, NEGATE_OPERATION)),
    ];
    for (name, expression) in cases {
        let (a, b) = special(TensorDevice::Cpu);
        let host = expression(&a, &b).unwrap();
        let (a, b) = special(TensorDevice::Metal);
        let gpu = expression(&a, &b).unwrap();
        close(name, &array(&gpu), &array(&host));
    }
    let (a, _) = special(TensorDevice::Metal);
    let home = host_tensor_to_device(vec![a.clone(), int(0)], None).unwrap();
    assert_eq!(node(&home).device(), TensorDevice::Cpu);
    let back = host_tensor_to_device(vec![home.clone(), int(1)], None).unwrap();
    assert_eq!(node(&back).device(), TensorDevice::Metal);
    close("round trip", &array(&back), &array(&a));
    assert!(cast(&a, ArrayDType::Float64)
        .unwrap_err()
        .message
        .contains("no float64"));
    let error = host_tensor_from_array(
        vec![
            Value::Array(
                ArrayValue::new(
                    Box::new([1]),
                    ArrayStorage::Float64(vec![1.0].into_boxed_slice().into()),
                )
                .unwrap(),
            ),
            int(1),
        ],
        None,
    )
    .unwrap_err();
    assert!(error.message.contains("no float64"));
    let empty = tensor(float32(&[], &[0, 3]), TensorDevice::Metal);
    assert_eq!(
        array(&binary(&empty, &empty, 0).unwrap()).shape.as_ref(),
        &[0, 3]
    );
}
