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
    ("reduce an expression", |l| {
        reduce(&binary(&l.a, &l.row, 2)?, 2, Some(0))
    }),
    ("mean of a transpose", |l| {
        reduce(&permute(&l.a, &[])?, 3, Some(1))
    }),
    ("batched matmul", |l| {
        let stack = reshape(
            &binary(&l.a, &l.tall, 18).or_else(|_| binary(&l.a, &l.b, 0))?,
            &[1, 2, 3],
        )?;
        let pair = binary(
            &reshape(&l.tall, &[1, 3, 2])?,
            &reshape(&l.two, &[1, 1, 1])?,
            2,
        )?;
        matmul(&stack, &pair)
    }),
    ("broadcast batched matmul", |l| {
        let left = reshape(&binary(&l.a, &l.b, 0)?, &[2, 1, 3])?;
        let right = reshape(&l.tall, &[1, 3, 2])?;
        let wide = binary(&left, &reshape(&l.row, &[1, 1, 3])?, 0)?;
        matmul(&binary(&wide, &reshape(&l.a, &[2, 1, 3])?, 1)?, &right)
    }),
    ("matching stacks matmul", |l| {
        let left = reshape(&binary(&l.a, &l.b, 0)?, &[2, 1, 3])?;
        matmul(&left, &reshape(&l.tall, &[2, 3, 1])?)
    }),
    ("permute three axes", |l| {
        let cube = binary(
            &reshape(&l.a, &[2, 3, 1])?,
            &reshape(&l.tall, &[1, 3, 2])?,
            2,
        )?;
        permute(&cube, &[2, 0, 1])
    }),
    ("choose broadcasts", |l| {
        choose(&compare(&l.a, &l.row, 0)?, &l.row, &l.two)
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
fn float_maths_follows_ieee_and_never_stops_the_program() {
    let wide = tensor(float32(&[1000.0, -1.0, 0.0, 4.0], &[4]), TensorDevice::Cpu);
    let grown = array(&unary(&wide, 2).unwrap());
    assert_eq!(grown.value_at_flat(0), Value::Float(f64::INFINITY));
    let logs = array(&unary(&wide, 3).unwrap());
    assert!(matches!(logs.value_at_flat(1), Value::Float(value) if value.is_nan()));
    assert_eq!(logs.value_at_flat(2), Value::Float(f64::NEG_INFINITY));
    let roots = array(&unary(&wide, 1).unwrap());
    assert!(matches!(roots.value_at_flat(1), Value::Float(value) if value.is_nan()));
    assert_eq!(roots.value_at_flat(3), Value::Float(2.0));
    let half = tensor(float32(&[0.5], &[1]), TensorDevice::Cpu);
    let powers = array(&binary(&wide, &half, 18).unwrap());
    assert_eq!(powers.shape.as_ref(), &[4]);
    assert_eq!(powers.value_at_flat(3), Value::Float(2.0));
    assert!(matches!(powers.value_at_flat(1), Value::Float(value) if value.is_nan()));
    let zero = tensor(float32(&[0.0], &[1]), TensorDevice::Cpu);
    let quotients = array(&binary(&wide, &zero, 3).unwrap());
    assert_eq!(quotients.value_at_flat(0), Value::Float(f64::INFINITY));
    assert_eq!(quotients.value_at_flat(1), Value::Float(f64::NEG_INFINITY));
    assert!(matches!(quotients.value_at_flat(2), Value::Float(value) if value.is_nan()));
    let double = host_tensor_full(
        vec![ints(&[2]), Value::Float(1.0e200), int(3), int(0)],
        None,
    )
    .unwrap();
    let squared = array(&binary(&double, &double, 18).unwrap());
    assert_eq!(squared.value_at_flat(0), Value::Float(f64::INFINITY));
    assert_eq!(
        array(&binary(&double, &double, 3).unwrap()).value_at_flat(0),
        Value::Float(1.0)
    );
    assert_eq!(
        array(&unary(&double, 1).unwrap()).value_at_flat(0),
        Value::Float(1.0e100)
    );
    // A function of an integer tensor is the Array member's error.
    let l = leaves(TensorDevice::Cpu);
    assert!(unary(&l.counts, 2).is_err());
    assert!(binary(&l.a, &l.tall, 18).is_err());
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
    assert_eq!(
        stays, l.a,
        "a tensor already on the device is the same tensor"
    );
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
        "tensor::__aura_scalar_like_f64",
        "tensor::__aura_scalar_like_i64",
        "tensor::__aura_node_op",
        "tensor::__aura_pair_op",
        "tensor::__aura_filled",
        "tensor::__aura_weights_names",
        "tensor::__aura_weights_load",
        "tensor::__aura_weights_put",
        "tensor::__aura_weights_write",
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
fn differences(name: &str, gpu: &ArrayValue, host: &ArrayValue) -> Vec<String> {
    if gpu.dtype() != host.dtype() || gpu.shape != host.shape {
        return vec![format!(
            "{name}: {:?} {:?} on the gpu and {:?} {:?} on the host",
            gpu.dtype(),
            gpu.shape,
            host.dtype(),
            host.shape
        )];
    }
    (0..host.len())
        .filter_map(|index| {
            let (gpu, host) = (gpu.value_at_flat(index), host.value_at_flat(index));
            let same = match (&gpu, &host) {
                (Value::Float(gpu), Value::Float(host)) => {
                    let tolerance = 8.0 * f64::from(f32::EPSILON) * host.abs().max(1.0);
                    gpu == host
                        || (gpu.is_nan() && host.is_nan())
                        || (gpu - host).abs() <= tolerance
                }
                (gpu, host) => gpu == host,
            };
            (!same).then(|| {
                format!("{name}: element {index} is {gpu:?} on the gpu and {host:?} on the host")
            })
        })
        .collect()
}

fn close(name: &str, gpu: &ArrayValue, host: &ArrayValue) {
    let found = differences(name, gpu, host);
    assert!(found.is_empty(), "{}", found.join("\n"));
}

/// The GPU devices this machine can run.
fn gpus() -> Vec<TensorDevice> {
    let found: Vec<TensorDevice> = [TensorDevice::Metal, TensorDevice::Cuda]
        .into_iter()
        .filter(|device| device.is_available())
        .collect();
    if found.is_empty() {
        eprintln!("SKIPPED: this machine has no usable GPU device");
    }
    found
}

#[test]
fn a_gpu_device_agrees_with_the_array_kernels() {
    for device in gpus() {
        agrees_with_the_array_kernels(device);
    }
}

fn agrees_with_the_array_kernels(gpu_device: TensorDevice) {
    for (name, expression) in PLANNED.iter().chain(ON_HOST) {
        let host = expression(&leaves(TensorDevice::Cpu)).unwrap();
        let gpu = expression(&leaves(gpu_device)).unwrap();
        assert_eq!(node(&gpu).device(), gpu_device);
        close(name, &array(&gpu), &array(&host));
        // A second run takes the compiled graph.
        let again = expression(&leaves(gpu_device)).unwrap();
        close(name, &array(&again), &array(&host));
    }
    for (name, expression) in PLANNED {
        let gpu = expression(&leaves(gpu_device)).unwrap();
        assert!(!node(&gpu).is_ready(), "{name}: waits on the gpu");
        host_tensor_eval(vec![gpu.clone()], None).unwrap();
        assert!(
            !matches!(node(&gpu).storage().unwrap(), Storage::Host(_)),
            "{name}: the result is in a device buffer"
        );
    }
    println!(
        "RAN: {} expressions on the {} device",
        PLANNED.len() + ON_HOST.len(),
        gpu_device.name()
    );
}

#[test]
fn a_gpu_device_keeps_special_values() {
    for device in gpus() {
        keeps_special_values(device);
    }
}

fn keeps_special_values(gpu_device: TensorDevice) {
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
    type Pair = fn(&Value, &Value) -> Result<Value>;
    let cases: &[(&str, Pair)] = &[
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
    let mut found = Vec::new();
    for (name, expression) in cases {
        let (a, b) = special(TensorDevice::Cpu);
        let host = expression(&a, &b).unwrap();
        let (a, b) = special(gpu_device);
        let gpu = expression(&a, &b).unwrap();
        found.extend(differences(name, &array(&gpu), &array(&host)));
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
    let (a, _) = special(gpu_device);
    let home = host_tensor_to_device(vec![a.clone(), int(0)], None).unwrap();
    assert_eq!(node(&home).device(), TensorDevice::Cpu);
    let back = host_tensor_to_device(vec![home.clone(), int(gpu_device as i64)], None).unwrap();
    assert_eq!(node(&back).device(), gpu_device);
    close("round trip", &array(&back), &array(&a));
    let double = ArrayValue::new(
        Box::new([2]),
        ArrayStorage::Float64(vec![1.5, -2.0].into_boxed_slice().into()),
    )
    .unwrap();
    let placed = host_tensor_from_array(
        vec![Value::Array(double.clone()), int(gpu_device as i64)],
        None,
    );
    if gpu_device.has_float64() {
        let placed = placed.unwrap();
        let widened = cast(&a, ArrayDType::Float64).unwrap();
        assert_eq!(node(&widened).dtype(), ArrayDType::Float64);
        let doubled = binary(&placed, &placed, 0).unwrap();
        assert!(
            !node(&doubled).is_ready(),
            "float64 arithmetic waits on the gpu"
        );
        let host = binary(
            &tensor(double.clone(), TensorDevice::Cpu),
            &tensor(double, TensorDevice::Cpu),
            0,
        )
        .unwrap();
        close("float64 add", &array(&doubled), &array(&host));
    } else {
        assert!(placed.unwrap_err().message.contains("no float64"));
        assert!(cast(&a, ArrayDType::Float64)
            .unwrap_err()
            .message
            .contains("no float64"));
    }
    let empty = tensor(float32(&[], &[0, 3]), gpu_device);
    assert_eq!(
        array(&binary(&empty, &empty, 0).unwrap()).shape.as_ref(),
        &[0, 3]
    );
}

/// The generated CUDA C, compiled for the CPU and run one thread index at
/// a time. This checks what the kernels compute on a machine with no GPU.
mod generated_kernels {
    use super::*;
    use tensor_codegen::{Action, BLOCK_THREADS};

    const HARNESS: &str = r#"
#include <cmath>
#include <math.h>
using std::copysign;
#define __device__
#define __forceinline__ inline
#define __global__
struct AuraIndex { unsigned x, y, z; };
static AuraIndex blockIdx, blockDim, threadIdx;
template <class T> T atomicAdd(T* address, T value) { T old = *address; *address += value; return old; }
extern "C" void aura_thread(unsigned block, unsigned thread) {
    blockIdx.x = block;
    blockDim.x = 256;
    threadIdx.x = thread;
}
"#;

    fn to_bytes(elements: &ArrayValue) -> Vec<u8> {
        let storage = elements.elements().unwrap();
        match &*storage {
            ArrayStorage::Int32(values) => values.iter().flat_map(|v| v.to_ne_bytes()).collect(),
            ArrayStorage::Int64(values) => values.iter().flat_map(|v| v.to_ne_bytes()).collect(),
            ArrayStorage::Float32(values) => values.iter().flat_map(|v| v.to_ne_bytes()).collect(),
            ArrayStorage::Float64(values) => values.iter().flat_map(|v| v.to_ne_bytes()).collect(),
            ArrayStorage::Bool(values) => values.iter().map(|&v| u8::from(v)).collect(),
        }
    }

    fn from_bytes(dtype: ArrayDType, shape: &[usize], bytes: &[u8]) -> ArrayValue {
        let storage = match dtype {
            ArrayDType::Int32 => ArrayStorage::Int32(
                bytes
                    .chunks_exact(4)
                    .map(|chunk| i32::from_ne_bytes(chunk.try_into().unwrap()))
                    .collect::<Box<[i32]>>()
                    .into(),
            ),
            ArrayDType::Int64 => ArrayStorage::Int64(
                bytes
                    .chunks_exact(8)
                    .map(|chunk| i64::from_ne_bytes(chunk.try_into().unwrap()))
                    .collect::<Box<[i64]>>()
                    .into(),
            ),
            ArrayDType::Float32 => ArrayStorage::Float32(
                bytes
                    .chunks_exact(4)
                    .map(|chunk| f32::from_ne_bytes(chunk.try_into().unwrap()))
                    .collect::<Box<[f32]>>()
                    .into(),
            ),
            ArrayDType::Float64 => ArrayStorage::Float64(
                bytes
                    .chunks_exact(8)
                    .map(|chunk| f64::from_ne_bytes(chunk.try_into().unwrap()))
                    .collect::<Box<[f64]>>()
                    .into(),
            ),
            ArrayDType::Bool => ArrayStorage::Bool(
                bytes
                    .iter()
                    .map(|&byte| byte != 0)
                    .collect::<Box<[bool]>>()
                    .into(),
            ),
        };
        ArrayValue::new(shape.into(), storage).unwrap()
    }

    /// Compiles `source` for the CPU, or `None` on a machine with no C++
    /// compiler.
    fn compile(source: &str, tag: usize) -> Option<*mut std::ffi::c_void> {
        let directory =
            std::env::temp_dir().join(format!("aura-tensor-kernels-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let (input, output) = (directory.join("plan.cpp"), directory.join("plan.so"));
        std::fs::write(&input, format!("{HARNESS}{source}")).unwrap();
        let status = std::process::Command::new("c++")
            .args([
                "-std=c++14",
                "-O1",
                "-shared",
                "-fPIC",
                "-ffp-contract=off",
                "-o",
            ])
            .arg(&output)
            .arg(&input)
            .output()
            .ok()?;
        assert!(
            status.status.success(),
            "the generated kernels do not compile:\n{}\n{source}",
            String::from_utf8_lossy(&status.stderr)
        );
        let path = std::ffi::CString::new(output.to_str().unwrap()).unwrap();
        // SAFETY: `path` names the library just built.
        let library = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW) };
        assert!(!library.is_null(), "the built kernels load");
        let _ = std::fs::remove_dir_all(&directory);
        Some(library)
    }

    fn function(library: *mut std::ffi::c_void, name: &str) -> *mut std::ffi::c_void {
        let name = std::ffi::CString::new(name).unwrap();
        // SAFETY: `library` is open and `name` is NUL-terminated.
        let address = unsafe { libc::dlsym(library, name.as_ptr()) };
        assert!(!address.is_null(), "kernel {name:?} exists");
        address
    }

    /// Runs `plan` through its generated kernels on the CPU.
    pub(super) fn run(plan: &Plan, leaves: &[Storage], tag: usize) -> Option<ArrayValue> {
        run_all(plan, leaves, tag).and_then(|mut outputs| outputs.pop())
    }

    /// Runs `plan` through its generated kernels on the CPU and gives each
    /// of its outputs.
    pub(super) fn run_all(plan: &Plan, leaves: &[Storage], tag: usize) -> Option<Vec<ArrayValue>> {
        let program = tensor_codegen::generate(plan);
        let library = compile(&program.source, tag)?;
        // SAFETY: the harness defines `aura_thread` with this signature.
        let set_thread = unsafe {
            std::mem::transmute::<*mut std::ffi::c_void, unsafe extern "C" fn(u32, u32)>(function(
                library,
                "aura_thread",
            ))
        };
        let mut buffers: Vec<Vec<u8>> = vec![Vec::new(); program.buffers];
        let mut leaves = leaves.iter();
        for (slot, buffer) in plan.slots.iter().zip(buffers.iter_mut()) {
            if let Slot::Leaf { dtype, shape } = slot {
                *buffer = to_bytes(&leaves.next().unwrap().to_host(*dtype, shape).unwrap());
            }
        }
        let finished = tensor_codegen::finished_buffers(&program, plan);
        for (index, action) in program.actions.iter().enumerate() {
            let read: &[usize] = match action {
                Action::Kernel { inputs, .. } => inputs,
                Action::Matmul { left, right, .. } => &[*left, *right],
                Action::Alias { input, .. } => std::slice::from_ref(input),
            };
            for &buffer in read {
                assert!(
                    !buffers[buffer].is_empty(),
                    "an action reads a buffer an earlier one let go of"
                );
            }
            match action {
                Action::Kernel {
                    name,
                    output,
                    bytes,
                    threads,
                    inputs,
                    clear,
                    more,
                } => {
                    let mut made = vec![if *clear { 0 } else { 0xAA_u8 }; *bytes];
                    let kernel = function(library, name);
                    // The buffers written beside the first come before
                    // the ones the kernel reads.
                    for &(other, bytes) in more {
                        buffers[other] = vec![0xAA_u8; bytes];
                    }
                    let mut pointers: Vec<*const u8> = Vec::new();
                    for &(other, _) in more {
                        pointers.push(buffers[other].as_mut_ptr().cast_const());
                    }
                    pointers.extend(inputs.iter().map(|&input| buffers[input].as_ptr()));
                    for thread in 0..threads.div_ceil(BLOCK_THREADS).max(1) * BLOCK_THREADS {
                        // SAFETY: the kernel takes the output and one
                        // pointer for each input, and checks its index.
                        unsafe {
                            set_thread(
                                (thread / BLOCK_THREADS) as u32,
                                (thread % BLOCK_THREADS) as u32,
                            );
                            let out = made.as_mut_ptr();
                            type P = *const u8;
                            match pointers[..] {
                                [a] => std::mem::transmute::<
                                    *mut std::ffi::c_void,
                                    unsafe extern "C" fn(*mut u8, P),
                                >(kernel)(out, a),
                                [a, b] => std::mem::transmute::<
                                    *mut std::ffi::c_void,
                                    unsafe extern "C" fn(*mut u8, P, P),
                                >(kernel)(out, a, b),
                                [a, b, c] => std::mem::transmute::<
                                    *mut std::ffi::c_void,
                                    unsafe extern "C" fn(*mut u8, P, P, P),
                                >(kernel)(out, a, b, c),
                                [a, b, c, d] => {
                                    std::mem::transmute::<
                                        *mut std::ffi::c_void,
                                        unsafe extern "C" fn(*mut u8, P, P, P, P),
                                    >(kernel)(out, a, b, c, d)
                                }
                                [a, b, c, d, e] => {
                                    std::mem::transmute::<
                                        *mut std::ffi::c_void,
                                        unsafe extern "C" fn(*mut u8, P, P, P, P, P),
                                    >(kernel)(out, a, b, c, d, e)
                                }
                                [a, b, c, d, e, f] => std::mem::transmute::<
                                    *mut std::ffi::c_void,
                                    unsafe extern "C" fn(*mut u8, P, P, P, P, P, P),
                                >(kernel)(
                                    out, a, b, c, d, e, f
                                ),
                                _ => {
                                    panic!("a test kernel has one to six pointers after the first")
                                }
                            }
                        }
                    }
                    buffers[*output] = made;
                }
                Action::Matmul {
                    left,
                    right,
                    output,
                    bytes,
                    double,
                    rows,
                    inner,
                    columns,
                    pairs,
                    swapped,
                } => {
                    assert!(!double, "the test products are float32");
                    let read = |buffer: &[u8], index: usize| {
                        f32::from_ne_bytes(buffer[index * 4..index * 4 + 4].try_into().unwrap())
                    };
                    let mut made = vec![0_u8; *bytes];
                    for (product, (left_at, right_at)) in pairs.iter().enumerate() {
                        for row in 0..*rows {
                            for column in 0..*columns {
                                let mut sum = 0.0_f32;
                                for k in 0..*inner {
                                    let left_place = if swapped.0 {
                                        k * rows + row
                                    } else {
                                        row * inner + k
                                    };
                                    let right_place = if swapped.1 {
                                        column * inner + k
                                    } else {
                                        k * columns + column
                                    };
                                    sum += read(&buffers[*left], left_at + left_place)
                                        * read(&buffers[*right], right_at + right_place);
                                }
                                let at = (product * rows * columns + row * columns + column) * 4;
                                made[at..at + 4].copy_from_slice(&sum.to_ne_bytes());
                            }
                        }
                    }
                    buffers[*output] = made;
                }
                Action::Alias { output, input } => {
                    buffers[*output] = buffers[*input].clone();
                }
            }
            for &buffer in &finished[index] {
                buffers[buffer] = Vec::new();
            }
        }
        Some(
            plan.outputs
                .iter()
                .map(|&output| {
                    let (dtype, shape) = match &plan.slots[output] {
                        Slot::Leaf { dtype, shape } | Slot::Step { dtype, shape, .. } => {
                            (*dtype, shape)
                        }
                    };
                    from_bytes(
                        ArrayDType::from_code(dtype).unwrap(),
                        shape,
                        &buffers[output],
                    )
                })
                .collect(),
        )
    }

    #[test]
    fn the_generated_kernels_compute_what_the_array_kernels_compute() {
        deferred(|| {
            for (tag, (name, expression)) in PLANNED.iter().enumerate() {
                let waiting = expression(&leaves(TensorDevice::Cpu)).unwrap();
                let (plan, storage) = Plan::collect(node(&waiting));
                let Some(generated) = run(&plan, &storage, tag) else {
                    eprintln!("SKIPPED: this machine has no C++ compiler");
                    return;
                };
                close(name, &generated, &array(&waiting));
            }
        });
    }

    #[test]
    fn a_large_whole_reduction_runs_in_stages() {
        deferred(|| {
            // 20,000 elements reduce to 313, then 5, then 1.
            let values: Vec<f32> = (0..20_000)
                .map(|index| ((index * 37) % 1_001) as f32 * 0.25 - 100.0)
                .collect();
            let wide = tensor(float32(&values, &[100, 200]), TensorDevice::Cpu);
            let doubled = binary(&wide, &wide, 0).unwrap();
            for (tag, reduction) in [0, 1, 2, 3].into_iter().enumerate() {
                let waiting = reduce(&doubled, reduction, None).unwrap();
                let (plan, storage) = Plan::collect(node(&waiting));
                let program = tensor_codegen::generate(&plan);
                let threads: Vec<usize> = program
                    .actions
                    .iter()
                    .map(|action| match action {
                        Action::Kernel { threads, .. } => *threads,
                        Action::Matmul { .. } | Action::Alias { .. } => {
                            panic!("a reduction is kernels")
                        }
                    })
                    .collect();
                assert_eq!(threads, [313, 5, 1]);
                assert_eq!(program.buffers, plan.slots.len() + 2, "two scratch buffers");
                let Some(generated) = run(&plan, &storage, 900 + tag) else {
                    eprintln!("SKIPPED: this machine has no C++ compiler");
                    return;
                };
                close("staged reduction", &generated, &array(&waiting));
            }
        });
    }

    #[test]
    fn elementwise_steps_fuse_into_one_kernel() {
        deferred(|| {
            let l = leaves(TensorDevice::Cpu);
            let chain = unary(
                &binary(&binary(&l.a, &l.b, 2).unwrap(), &l.row, 0).unwrap(),
                1,
            )
            .unwrap();
            let (plan, _) = Plan::collect(node(&chain));
            let program = tensor_codegen::generate(&plan);
            assert_eq!(program.actions.len(), 1, "three steps are one kernel");
            assert_eq!(program.buffers, plan.slots.len(), "no scratch buffer");
            let Action::Kernel {
                inputs,
                threads,
                bytes,
                ..
            } = &program.actions[0]
            else {
                panic!("an elementwise chain is a kernel");
            };
            assert_eq!((inputs.len(), *threads, *bytes), (3, 6, 24));

            let total = reduce(&chain, 0, None).unwrap();
            let (plan, _) = Plan::collect(node(&total));
            let program = tensor_codegen::generate(&plan);
            assert_eq!(
                program.actions.len(),
                1,
                "a small whole sum is one kernel over the fused chain"
            );
            assert_eq!(program.buffers, plan.slots.len(), "no scratch buffer");

            let product = matmul(&chain, &l.tall).unwrap();
            let (plan, _) = Plan::collect(node(&product));
            let program = tensor_codegen::generate(&plan);
            assert!(matches!(
                program.actions[..],
                [
                    Action::Kernel { .. },
                    Action::Matmul {
                        rows: 2,
                        inner: 3,
                        columns: 2,
                        ..
                    }
                ]
            ));
        });
    }
}

#[path = "tensor_ops_tests.rs"]
mod indexed_and_gradients;
