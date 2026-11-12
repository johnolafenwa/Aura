//! Tests of the tensor operations that move elements by index, of the
//! functions `Array` lacks, of random tensors, and of gradients.

use super::*;

fn node_op(operand: &Value, code: i64, parameters: &[i64]) -> Result<Value> {
    host_tensor_node_op(vec![operand.clone(), int(code), ints(parameters)], None)
}

fn pair_op(left: &Value, right: &Value, code: i64, parameters: &[i64]) -> Result<Value> {
    host_tensor_pair_op(
        vec![left.clone(), right.clone(), int(code), ints(parameters)],
        None,
    )
}

fn float64(values: &[f64], shape: &[usize]) -> ArrayValue {
    ArrayValue::new(
        shape.into(),
        ArrayStorage::Float64(values.to_vec().into_boxed_slice().into()),
    )
    .unwrap()
}

fn floats(elements: &ArrayValue) -> Vec<f64> {
    (0..elements.len())
        .map(|index| match elements.value_at_flat(index) {
            Value::Float(value) => value,
            other => panic!("expected a float, found {other:?}"),
        })
        .collect()
}

fn places(elements: &ArrayValue) -> Vec<i64> {
    match &*elements.elements().unwrap() {
        ArrayStorage::Int64(values) => values.to_vec(),
        _ => panic!("expected int64 elements"),
    }
}

/// A tensor of indices on the device of `like`.
fn indices(values: &[i64], shape: &[usize], like: &Value) -> Value {
    tensor(int64(values, shape), node(like).device())
}

/// Random floats on the device of `like`.
fn random(fill: i64, seed: i64, dtype: ArrayDType, shape: &[i64], like: &Value) -> Result<Value> {
    host_tensor_filled(
        vec![
            ints(shape),
            int(fill),
            int(seed),
            int(dtype as i64),
            int(node(like).device() as i64),
        ],
        None,
    )
}

fn slice(operand: &Value, axis: i64, start: i64, stop: i64, step: i64) -> Result<Value> {
    node_op(operand, 0, &[axis, start, stop, step])
}

fn unslice(operand: &Value, axis: usize, start: usize, step: usize, size: usize) -> Result<Value> {
    operate(
        Operation::Unslice { axis, step, size },
        &[
            node(operand),
            &start_tensor(node(operand).device(), start).unwrap(),
        ],
    )
}

/// Expressions of the indexed operations, which a GPU device with
/// generated kernels runs as a plan.
const INDEXED: &[(&str, Expression)] = &[
    ("tanh", |l| unary(&l.a, TANH_OPERATION)),
    ("sigmoid", |l| unary(&l.a, SIGMOID_OPERATION)),
    ("erf", |l| unary(&l.a, ERF_OPERATION)),
    ("slice", |l| slice(&l.a, 1, 1, 3, 1)),
    ("slice with a step", |l| slice(&l.a, -1, 0, 3, 2)),
    ("slice rows", |l| slice(&l.tall, 0, 1, -1, 1)),
    ("slice of an expression", |l| {
        slice(&binary(&l.a, &l.row, 0)?, 0, 1, 2, 1)
    }),
    ("unslice", |l| unslice(&l.a, 1, 1, 2, 6)),
    ("unslice rows", |l| unslice(&l.tall, 0, 2, 1, 6)),
    ("expand", |l| node_op(&l.row, 1, &[2, 3])),
    ("expand one element", |l| node_op(&l.two, 1, &[2, 2])),
    ("concat rows", |l| pair_op(&l.a, &l.b, 0, &[0])),
    ("concat columns of an expression", |l| {
        pair_op(&binary(&l.a, &l.b, 2)?, &l.a, 0, &[-1])
    }),
    ("take rows", |l| {
        pair_op(&l.tall, &indices(&[2, 0, 1, 2], &[2, 2], &l.a), 1, &[0])
    }),
    ("take columns", |l| {
        pair_op(&l.a, &indices(&[2, 0], &[2], &l.a), 1, &[1])
    }),
    ("take with int32 indices", |l| {
        let places = tensor(int32(&[1, 2, 0, 1], &[4]), node(&l.a).device());
        pair_op(&l.row, &places, 1, &[0])
    }),
    ("index add rows", |l| {
        pair_op(&l.tall, &indices(&[1, 0, 1], &[3], &l.a), 2, &[0, 4])
    }),
    ("index add columns", |l| {
        pair_op(&l.a, &indices(&[0, 0, 1], &[3], &l.a), 2, &[1, 2])
    }),
    ("index add of an expression", |l| {
        pair_op(
            &binary(&l.a, &l.b, 2)?,
            &indices(&[1, 1], &[2], &l.a),
            2,
            &[0, 2],
        )
    }),
    ("argmax", |l| node_op(&l.a, 2, &[1])),
    ("argmin", |l| node_op(&l.a, 3, &[0])),
    ("argmax of integers", |l| node_op(&l.counts, 2, &[0])),
    ("argmax of an expression", |l| {
        node_op(&binary(&l.a, &l.b, 2)?, 2, &[-1])
    }),
    ("uniform", |l| {
        random(0, 7, ArrayDType::Float32, &[3, 5], &l.a)
    }),
    ("normal", |l| {
        random(1, 7, ArrayDType::Float32, &[4, 2], &l.a)
    }),
    ("random in an expression", |l| {
        binary(&random(0, 11, ArrayDType::Float32, &[2, 3], &l.a)?, &l.a, 2)
    }),
    ("embedding then product", |l| {
        let rows = pair_op(&l.tall, &indices(&[2, 0, 2], &[3], &l.a), 1, &[0])?;
        matmul(&l.a, &rows)
    }),
];

#[test]
fn the_indexed_operations_give_known_elements() {
    let l = leaves(TensorDevice::Cpu);
    let of = |name: &str| {
        let (_, expression) = INDEXED.iter().find(|(known, _)| *known == name).unwrap();
        array(&expression(&l).unwrap())
    };
    assert_eq!(of("slice"), float32(&[-2.0, 3.0, 0.25, 6.0], &[2, 2]));
    assert_eq!(
        of("slice with a step"),
        float32(&[1.0, 3.0, 4.5, 6.0], &[2, 2])
    );
    assert_eq!(of("slice rows"), float32(&[3.0, 4.0], &[1, 2]));
    assert_eq!(
        of("unslice"),
        float32(
            &[0.0, 1.0, 0.0, -2.0, 0.0, 3.0, 0.0, 4.5, 0.0, 0.25, 0.0, 6.0],
            &[2, 6]
        )
    );
    assert_eq!(
        of("expand"),
        float32(&[10.0, 20.0, 30.0, 10.0, 20.0, 30.0], &[2, 3])
    );
    assert_eq!(
        of("concat rows"),
        float32(
            &[1.0, -2.0, 3.0, 4.5, 0.25, 6.0, 2.0, 0.5, -1.0, 3.0, 8.0, 0.125],
            &[4, 3]
        )
    );
    assert_eq!(
        of("take rows"),
        float32(&[5.0, 6.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 2, 2])
    );
    assert_eq!(of("take columns"), float32(&[3.0, 1.0, 6.0, 4.5], &[2, 2]));
    assert_eq!(
        of("index add rows"),
        float32(&[3.0, 4.0, 6.0, 8.0, 0.0, 0.0, 0.0, 0.0], &[4, 2])
    );
    assert_eq!(
        of("index add columns"),
        float32(&[-1.0, 3.0, 4.75, 6.0], &[2, 2])
    );
    assert_eq!(places(&of("argmax")), [2, 2]);
    assert_eq!(places(&of("argmin")), [0, 0, 0]);
    assert_eq!(places(&of("argmax of integers")), [2]);
    // An empty slice is an empty tensor.
    assert_eq!(node(&slice(&l.a, 1, 2, 1, 1).unwrap()).shape(), [2, 0]);
    // A NaN wins, and the first of equal elements wins.
    let odd = tensor(
        float32(&[1.0, f32::NAN, 9.0, 4.0, 4.0, 2.0], &[2, 3]),
        TensorDevice::Cpu,
    );
    assert_eq!(places(&array(&node_op(&odd, 2, &[1]).unwrap())), [1, 0]);
    assert_eq!(places(&array(&node_op(&odd, 3, &[1]).unwrap())), [1, 2]);
}

#[test]
fn a_plan_of_indexed_operations_gives_what_each_gives_at_once() {
    for (name, expression) in INDEXED {
        let eager = expression(&leaves(TensorDevice::Cpu)).unwrap();
        let waiting = deferred(|| expression(&leaves(TensorDevice::Cpu)).unwrap());
        assert!(!node(&waiting).is_ready(), "{name}: the operation waits");
        assert_eq!(node(&waiting).dtype(), node(&eager).dtype(), "{name}");
        assert_eq!(node(&waiting).shape(), node(&eager).shape(), "{name}");
        assert_eq!(array(&waiting), array(&eager), "{name}: elements");
    }
}

#[test]
fn the_generated_kernels_of_indexed_operations_agree_with_the_cpu_kernels() {
    deferred(|| {
        for (tag, (name, expression)) in INDEXED.iter().enumerate() {
            let waiting = expression(&leaves(TensorDevice::Cpu)).unwrap();
            let (plan, storage) = Plan::collect(node(&waiting));
            let Some(generated) = generated_kernels::run(&plan, &storage, 1000 + tag) else {
                eprintln!("SKIPPED: this machine has no C++ compiler");
                return;
            };
            close(name, &generated, &array(&waiting));
        }
    });
}

#[test]
fn a_gpu_device_agrees_on_the_indexed_operations() {
    for device in gpus() {
        for (name, expression) in INDEXED {
            let host = expression(&leaves(TensorDevice::Cpu)).unwrap();
            let gpu = expression(&leaves(device)).unwrap();
            assert_eq!(node(&gpu).device(), device);
            close(name, &array(&gpu), &array(&host));
        }
        println!(
            "RAN: {} indexed expressions on the {} device",
            INDEXED.len(),
            device.name()
        );
    }
}

#[test]
fn indexed_operations_check_their_arguments() {
    let l = leaves(TensorDevice::Cpu);
    let message = |result: Result<Value>| result.unwrap_err().message;
    assert!(message(slice(&l.a, 2, 0, 1, 1)).contains("axis 2 for a tensor of rank 2"));
    assert!(message(slice(&l.a, 0, 0, 1, 0)).contains("a step of at least 1"));
    assert!(message(node_op(&l.a, 0, &[0])).contains("expects 4 parameters"));
    assert!(message(node_op(&l.a, 1, &[3, 2])).contains("cannot broadcast shape [2, 3] to [3, 2]"));
    assert!(message(node_op(&l.a, 99, &[])).contains("unknown operation"));
    let mask = compare(&l.a, &l.b, 0).unwrap();
    assert!(message(node_op(&mask, 2, &[0])).contains("a tensor of numbers"));
    assert!(message(pair_op(&l.a, &l.tall, 0, &[0])).contains("differ on axis 0 only"));
    assert!(message(pair_op(&l.a, &l.small, 0, &[0])).contains("one dtype"));
    assert!(message(pair_op(&l.a, &l.b, 1, &[0])).contains("integer indices"));
    assert!(message(pair_op(&l.a, &l.b, 2, &[0, 2])).contains("integer indices"));
    let index = indices(&[0, 5], &[2], &l.a);
    assert!(message(pair_op(&l.a, &index, 1, &[0])).contains("index 5 for an axis of length 2"));
    assert!(message(pair_op(&l.a, &index, 2, &[0, 3])).contains("index 5 for an axis of length 3"));
    assert!(message(pair_op(&l.a, &index, 2, &[0, -1])).contains("not negative"));
    assert!(message(pair_op(&l.a, &index, 2, &[1, 3])).contains("the indices' shape [2]"));
    assert!(message(pair_op(&mask, &index, 2, &[0, 3])).contains("a tensor of numbers"));
    assert!(message(pair_op(&l.a, &l.b, 99, &[])).contains("unknown operation"));
    assert!(message(unary(&l.counts, TANH_OPERATION)).contains("expects a float tensor"));
}

#[test]
fn the_error_function_matches_known_values() {
    // Values of erf to 17 digits.
    for (x, expected) in [
        (0.0, 0.0),
        (0.1, 0.112_462_916_018_284_89),
        (0.5, 0.520_499_877_813_046_5),
        (1.0, 0.842_700_792_949_714_9),
        (2.0, 0.995_322_265_018_952_7),
        (2.9, 0.999_958_902_121_900_5),
        (3.0, 0.999_977_909_503_001_4),
        (3.5, 0.999_999_256_901_627_7),
        (5.0, 0.999_999_999_998_462_5),
        (7.0, 1.0),
    ] {
        let found = tensor_kernels::erf(x);
        // The kernel is within a few units in the last place.
        assert!((found - expected).abs() < 2e-15, "erf({x}) = {found}");
        assert_eq!(tensor_kernels::erf(-x), -found);
    }
    assert!(tensor_kernels::erf(f64::NAN).is_nan());
    assert_eq!(tensor_kernels::erf(f64::INFINITY), 1.0);
    assert_eq!(tensor_kernels::sigmoid(0.0), 0.5);
}

fn filled(shape: &[i64], fill: i64, seed: i64, dtype: ArrayDType) -> Result<Value> {
    host_tensor_filled(
        vec![ints(shape), int(fill), int(seed), int(dtype as i64), int(0)],
        None,
    )
}

#[test]
fn a_random_tensor_depends_on_its_seed_only() {
    let uniform = floats(&array(
        &filled(&[100, 100], 0, 7, ArrayDType::Float32).unwrap(),
    ));
    assert!(uniform.iter().all(|&value| (0.0..1.0).contains(&value)));
    let mean = uniform.iter().sum::<f64>() / uniform.len() as f64;
    assert!((mean - 0.5).abs() < 0.01, "mean {mean}");
    assert_eq!(
        uniform,
        floats(&array(
            &filled(&[100, 100], 0, 7, ArrayDType::Float32).unwrap()
        ))
    );
    assert_ne!(
        uniform,
        floats(&array(
            &filled(&[100, 100], 0, 8, ArrayDType::Float32).unwrap()
        ))
    );
    // A longer tensor begins with the shorter one.
    let short = floats(&array(&filled(&[10], 0, 7, ArrayDType::Float32).unwrap()));
    assert_eq!(short, uniform[..10]);

    let normal = floats(&array(
        &filled(&[20_000], 1, 3, ArrayDType::Float64).unwrap(),
    ));
    let mean = normal.iter().sum::<f64>() / normal.len() as f64;
    let variance = normal
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / normal.len() as f64;
    assert!(mean.abs() < 0.03, "mean {mean}");
    assert!((variance - 1.0).abs() < 0.05, "variance {variance}");
    assert!(normal.iter().all(|value| value.is_finite()));

    assert_eq!(
        places(&array(&filled(&[2, 3], 2, 0, ArrayDType::Int64).unwrap())),
        [0, 1, 2, 3, 4, 5]
    );
    assert_eq!(
        array(&filled(&[3], 2, 0, ArrayDType::Int32).unwrap()),
        int32(&[0, 1, 2], &[3])
    );
    assert_eq!(
        floats(&array(&filled(&[3], 2, 0, ArrayDType::Float32).unwrap())),
        [0.0, 1.0, 2.0]
    );
    let message = |result: Result<Value>| result.unwrap_err().message;
    assert!(message(filled(&[3], 0, 0, ArrayDType::Int64)).contains("`float32` or `float64`"));
    assert!(message(filled(&[3], 9, 0, ArrayDType::Float32)).contains("unknown fill"));
}

fn watch(operand: &Value) -> Value {
    node_op(operand, 4, &[]).unwrap()
}

fn detach(operand: &Value) -> Value {
    node_op(operand, 5, &[]).unwrap()
}

fn gradient(result: &Value, wrt: &Value) -> Result<Value> {
    pair_op(result, wrt, 3, &[])
}

type Function = fn(&[Value]) -> Result<Value>;

/// Checks the gradient of the sum of `function` over `inputs` against
/// central differences, with the CPU device running at once and waiting.
fn check_gradients(name: &str, inputs: &[ArrayValue], function: Function) {
    let total = |values: &[Value]| -> f64 {
        let result = reduce(&function(values).unwrap(), 0, None).unwrap();
        floats(&array(&result))[0]
    };
    let plain: Vec<Value> = inputs
        .iter()
        .map(|input| tensor(input.clone(), TensorDevice::Cpu))
        .collect();
    for waits in [false, true] {
        let gradients: Vec<Vec<f64>> = {
            let body = || {
                let watched: Vec<Value> = plain.iter().map(watch).collect();
                let result = reduce(&function(&watched).unwrap(), 0, None).unwrap();
                watched
                    .iter()
                    .map(|input| {
                        let made = gradient(&result, input).unwrap();
                        assert_eq!(node(&made).shape(), node(input).shape(), "{name}");
                        assert!(
                            !node(&made).is_tracked(),
                            "{name}: a gradient is not tracked"
                        );
                        floats(&array(&made))
                    })
                    .collect()
            };
            if waits {
                deferred(body)
            } else {
                body()
            }
        };
        for (position, input) in inputs.iter().enumerate() {
            let values = floats(input);
            for element in 0..values.len() {
                let moved = |by: f64| {
                    let mut values = values.clone();
                    values[element] += by;
                    let mut plain = plain.clone();
                    plain[position] = tensor(float64(&values, &input.shape), TensorDevice::Cpu);
                    total(&plain)
                };
                let step = 1e-6;
                let expected = (moved(step) - moved(-step)) / (2.0 * step);
                let found = gradients[position][element];
                assert!(
                    (found - expected).abs() <= 1e-6 * expected.abs().max(1.0),
                    "{name}: input {position} element {element}: found {found}, expected {expected} (waits: {waits})"
                );
            }
        }
    }
}

#[test]
fn gradients_agree_with_differences() {
    let a = float64(&[1.0, -2.0, 3.0, 4.5, 0.25, 6.0], &[2, 3]);
    let b = float64(&[2.0, 0.5, -1.0, 3.0, 8.0, 0.125], &[2, 3]);
    let row = float64(&[0.7, 1.3, 2.1], &[3]);
    let tall = float64(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], &[3, 2]);
    let positive = float64(&[1.5, 0.2, 3.0, 4.5, 0.25, 6.0], &[2, 3]);
    let two = float64(&[2.0], &[1]);
    let cases: &[(&str, Vec<ArrayValue>, Function)] = &[
        ("add", vec![a.clone(), b.clone()], |x| {
            binary(&x[0], &x[1], 0)
        }),
        ("add broadcast", vec![a.clone(), row.clone()], |x| {
            binary(&x[0], &x[1], 0)
        }),
        ("sub broadcast", vec![row.clone(), a.clone()], |x| {
            binary(&x[0], &x[1], 1)
        }),
        ("mul", vec![a.clone(), b.clone()], |x| {
            binary(&x[0], &x[1], 2)
        }),
        ("mul by one element", vec![a.clone(), two.clone()], |x| {
            binary(&x[0], &x[1], 2)
        }),
        ("div", vec![a.clone(), b.clone()], |x| {
            binary(&x[0], &x[1], 3)
        }),
        ("div broadcast", vec![row.clone(), b.clone()], |x| {
            binary(&x[0], &x[1], 3)
        }),
        ("minimum", vec![a.clone(), b.clone()], |x| {
            binary(&x[0], &x[1], 16)
        }),
        ("maximum", vec![a.clone(), row.clone()], |x| {
            binary(&x[0], &x[1], 17)
        }),
        ("pow", vec![positive.clone(), b.clone()], |x| {
            binary(&x[0], &x[1], 18)
        }),
        ("negate", vec![a.clone()], |x| {
            unary(&x[0], NEGATE_OPERATION)
        }),
        ("abs", vec![a.clone()], |x| unary(&x[0], 0)),
        ("sqrt", vec![positive.clone()], |x| unary(&x[0], 1)),
        ("exp", vec![a.clone()], |x| unary(&x[0], 2)),
        ("log", vec![positive.clone()], |x| unary(&x[0], 3)),
        ("sin", vec![a.clone()], |x| unary(&x[0], 6)),
        ("cos", vec![a.clone()], |x| unary(&x[0], 7)),
        ("tanh", vec![a.clone()], |x| unary(&x[0], TANH_OPERATION)),
        ("sigmoid", vec![a.clone()], |x| {
            unary(&x[0], SIGMOID_OPERATION)
        }),
        ("erf", vec![b.clone()], |x| unary(&x[0], ERF_OPERATION)),
        ("choose", vec![a.clone(), row.clone()], |x| {
            choose(&compare(&x[0], &x[1], 0)?, &x[0], &x[1])
        }),
        ("sum axis", vec![a.clone(), row.clone()], |x| {
            binary(&reduce(&x[0], 0, Some(0))?, &x[1], 2)
        }),
        ("sum", vec![a.clone(), row.clone()], |x| {
            binary(&reduce(&x[0], 0, None)?, &x[1], 2)
        }),
        ("mean axis", vec![a.clone()], |x| {
            let mean = reduce(&x[0], 3, Some(-1))?;
            binary(&mean, &mean, 2)
        }),
        ("mean", vec![a.clone()], |x| {
            let mean = reduce(&x[0], 3, None)?;
            binary(&mean, &mean, 2)
        }),
        ("max axis", vec![a.clone(), row.clone()], |x| {
            binary(&reduce(&x[0], 2, Some(0))?, &x[1], 2)
        }),
        ("min", vec![a.clone()], |x| {
            let least = reduce(&x[0], 1, None)?;
            binary(&least, &least, 2)
        }),
        ("matmul", vec![a.clone(), tall.clone()], |x| {
            matmul(&x[0], &x[1])
        }),
        ("batched matmul", vec![a.clone(), tall.clone()], |x| {
            let left = reshape(&x[0], &[2, 1, 3])?;
            let right = reshape(&x[1], &[1, 3, 2])?;
            let stack = pair_op(&right, &binary(&right, &right, 2)?, 0, &[0])?;
            matmul(&left, &stack)
        }),
        ("reshape", vec![a.clone(), tall.clone()], |x| {
            binary(&reshape(&x[0], &[3, 2])?, &x[1], 2)
        }),
        ("transpose", vec![a.clone(), tall.clone()], |x| {
            binary(&permute(&x[0], &[])?, &x[1], 2)
        }),
        ("permute", vec![a.clone(), tall.clone()], |x| {
            let cube = binary(
                &reshape(&x[0], &[2, 3, 1])?,
                &reshape(&x[1], &[1, 3, 2])?,
                2,
            )?;
            let turned = permute(&cube, &[2, 0, 1])?;
            binary(&turned, &reshape(&x[0], &[1, 2, 3])?, 2)
        }),
        ("expand", vec![row.clone(), a.clone()], |x| {
            binary(&node_op(&x[0], 1, &[2, 3])?, &x[1], 2)
        }),
        ("slice", vec![a.clone(), two.clone()], |x| {
            binary(&slice(&x[0], 1, 0, 3, 2)?, &x[1], 18)
        }),
        ("unslice", vec![a.clone()], |x| {
            let wide = unslice(&x[0], 1, 1, 2, 7)?;
            binary(&wide, &wide, 2)
        }),
        ("concat", vec![a.clone(), b.clone()], |x| {
            let joined = pair_op(&x[0], &x[1], 0, &[1])?;
            binary(
                &slice(&joined, 1, 1, 5, 1)?,
                &slice(&joined, 1, 2, 6, 1)?,
                2,
            )
        }),
        ("take", vec![tall.clone(), a.clone()], |x| {
            let rows = pair_op(&x[0], &indices(&[2, 0, 2], &[3], &x[0]), 1, &[0])?;
            matmul(&x[1], &rows)
        }),
        ("take columns", vec![a.clone()], |x| {
            let columns = pair_op(&x[0], &indices(&[1, 1, 0, 2], &[2, 2], &x[0]), 1, &[1])?;
            binary(&columns, &columns, 2)
        }),
        ("index add", vec![tall.clone()], |x| {
            let sums = pair_op(&x[0], &indices(&[1, 0, 1], &[3], &x[0]), 2, &[0, 4])?;
            binary(&sums, &sums, 2)
        }),
        ("an operand used twice", vec![a.clone()], |x| {
            binary(&binary(&x[0], &x[0], 2)?, &x[0], 0)
        }),
        ("softmax", vec![a.clone(), b.clone()], |x| {
            let shifted = binary(&x[0], &reshape(&reduce(&x[0], 2, Some(1))?, &[2, 1])?, 1)?;
            let powers = unary(&shifted, 2)?;
            let total = reshape(&reduce(&powers, 0, Some(1))?, &[2, 1])?;
            binary(&binary(&powers, &total, 3)?, &x[1], 2)
        }),
    ];
    for (name, inputs, function) in cases {
        check_gradients(name, inputs, *function);
    }
}

#[test]
fn a_gradient_follows_the_watched_tensors_only() {
    let l = leaves(TensorDevice::Cpu);
    let (a, b) = (watch(&l.a), watch(&l.b));
    assert!(node(&a).is_tracked() && !node(&l.a).is_tracked());
    assert_eq!(array(&a), array(&l.a));

    // The gradient of a tensor of several elements is that of its sum.
    let product = binary(&a, &l.b, 2).unwrap();
    assert!(node(&product).is_tracked());
    assert_eq!(array(&gradient(&product, &a).unwrap()), array(&l.b));
    // A second request takes the kept gradients.
    assert_eq!(array(&gradient(&product, &a).unwrap()), array(&l.b));
    // A watched tensor the result was not computed from has a zero
    // gradient.
    assert_eq!(
        array(&gradient(&product, &b).unwrap()),
        float32(&[0.0; 6], &[2, 3])
    );
    // A watched tensor's gradient with respect to itself is one.
    assert_eq!(
        array(&gradient(&a, &a).unwrap()),
        float32(&[1.0; 6], &[2, 3])
    );

    // A detached tensor is a constant.
    let through = binary(&a, &detach(&binary(&a, &b, 0).unwrap()), 2).unwrap();
    assert_eq!(
        array(&gradient(&through, &a).unwrap()),
        array(&binary(&l.a, &l.b, 0).unwrap())
    );
    assert!(!node(&detach(&a)).is_tracked());
    let plain = detach(&l.a);
    assert_eq!(node(&plain), node(&l.a));
    // A detached waiting tensor still waits, and reads the same elements.
    deferred(|| {
        let sum = binary(&a, &b, 0).unwrap();
        let constant = detach(&sum);
        assert!(!node(&constant).is_ready() && !node(&constant).is_tracked());
        assert_eq!(array(&constant), array(&sum));
    });

    // Masks and integers are not tracked.
    assert!(!node(&compare(&a, &b, 0).unwrap()).is_tracked());
    assert!(!node(&node_op(&a, 2, &[0]).unwrap()).is_tracked());

    let message = |result: Result<Value>| result.unwrap_err().message;
    assert!(message(node_op(&l.counts, 4, &[])).contains("expects a float tensor"));
    assert!(message(gradient(&product, &l.a)).contains("made with `requires_grad()`"));
    assert!(message(gradient(&product, &product)).contains("made with `requires_grad()`"));
    let vector = watch(&l.row);
    let through_vector = matmul(&l.a, &vector).unwrap();
    assert!(message(gradient(&through_vector, &vector)).contains("rank-one operand"));
    let mask = compare(&l.a, &l.b, 0).unwrap();
    assert!(message(gradient(&mask, &a)).contains("expects a float tensor"));
}

#[test]
fn equal_extremes_share_a_gradient() {
    let values = watch(&tensor(
        float32(&[3.0, 3.0, 1.0, 2.0, 5.0, 5.0], &[2, 3]),
        TensorDevice::Cpu,
    ));
    let largest = reduce(&values, 2, Some(1)).unwrap();
    assert_eq!(
        array(&gradient(&largest, &values).unwrap()),
        float32(&[0.5, 0.5, 0.0, 0.0, 0.5, 0.5], &[2, 3])
    );
}

#[test]
fn a_variable_holds_a_watched_tensor_that_can_be_replaced() {
    let l = leaves(TensorDevice::Cpu);
    let variable = node_op(&l.a, 6, &[]).unwrap();
    let value = node_op(&variable, 7, &[]).unwrap();
    assert!(node(&value).is_tracked());
    assert_eq!(array(&value), array(&l.a));
    // Reading twice gives the same tensor, so a gradient finds it.
    assert_eq!(node(&value), node(&node_op(&variable, 7, &[]).unwrap()));
    let doubled = binary(&value, &l.two, 2).unwrap();
    assert_eq!(
        array(&gradient(&doubled, &value).unwrap()),
        float32(&[2.0; 6], &[2, 3])
    );
    // Another handle to the variable sees an assignment.
    let shared = host_tensor_share(vec![variable.clone()], None).unwrap();
    pair_op(&variable, &doubled, 4, &[]).unwrap();
    let replaced = node_op(&shared, 7, &[]).unwrap();
    assert_eq!(array(&replaced), array(&doubled));
    assert!(node(&replaced).is_tracked() && node(&replaced) != node(&doubled));

    let message = |result: Result<Value>| result.unwrap_err().message;
    assert!(message(node_op(&l.counts, 6, &[])).contains("expects a float tensor"));
    assert!(message(node_op(&l.a, 7, &[])).contains("expects a parameter"));
    assert!(message(pair_op(&l.a, &l.b, 4, &[])).contains("expects a parameter"));
    assert!(message(pair_op(&variable, &l.row, 4, &[]))
        .contains("the parameter's dtype and shape [2, 3]"));
    // Making a variable of a variable gives another handle to it.
    let again = node_op(&variable, 6, &[]).unwrap();
    assert_eq!(node(&again), node(&variable));
}

#[test]
fn backward_adds_gradients_to_the_watched_tensors_until_they_are_cleared() {
    let l = leaves(TensorDevice::Cpu);
    let weight = watch(&l.a);
    let grad = |tensor: &Value| array(&node_op(tensor, 11, &[]).unwrap());
    // No gradient yet reads as zeros.
    assert_eq!(grad(&weight), float32(&[0.0; 6], &[2, 3]));
    let doubled = binary(&weight, &l.two, 2).unwrap();
    node_op(&doubled, 10, &[]).unwrap();
    assert_eq!(grad(&weight), float32(&[2.0; 6], &[2, 3]));
    // A second result adds to it.
    let squared = binary(&weight, &weight, 2).unwrap();
    node_op(&squared, 10, &[]).unwrap();
    let expected: Vec<f32> = [1.0_f32, -2.0, 3.0, 4.5, 0.25, 6.0]
        .iter()
        .map(|value| 2.0 + 2.0 * value)
        .collect();
    assert_eq!(grad(&weight), float32(&expected, &[2, 3]));
    // The results wait to run with the next group, which runs them.
    node_op(&l.b, 9, &[]).unwrap();
    assert!(matches!(&*node(&squared).state(), State::Ready(_)));
    node_op(&weight, 12, &[]).unwrap();
    assert_eq!(grad(&weight), float32(&[0.0; 6], &[2, 3]));
    // A new tensor in a variable starts with no gradient.
    let variable = node_op(&l.a, 6, &[]).unwrap();
    let held = node_op(&variable, 7, &[]).unwrap();
    node_op(&binary(&held, &l.two, 2).unwrap(), 10, &[]).unwrap();
    assert_eq!(grad(&held), float32(&[2.0; 6], &[2, 3]));
    pair_op(&variable, &l.b, 4, &[]).unwrap();
    assert_eq!(
        grad(&node_op(&variable, 7, &[]).unwrap()),
        float32(&[0.0; 6], &[2, 3])
    );
    node_op(&l.b, 9, &[]).unwrap();

    let message = |result: Result<Value>| result.unwrap_err().message;
    assert!(message(node_op(&l.counts, 10, &[])).contains("expects a float tensor"));
    assert!(message(node_op(&l.a, 11, &[])).contains("held by a `Parameter`"));
}

fn text(value: &str) -> Value {
    Value::string(value)
}

fn outcome(value: Value) -> std::result::Result<Value, String> {
    let Value::EnumVariant(variant) = value else {
        panic!("expected a Result");
    };
    let payload = variant.payloads[0].clone();
    if variant.variant_name.as_str() == "Ok" {
        Ok(payload)
    } else {
        match payload {
            Value::String(message) => Err(message.to_string()),
            other => panic!("expected a message, found {other:?}"),
        }
    }
}

#[test]
fn tensors_are_saved_and_loaded_as_safetensors() {
    let directory =
        std::env::temp_dir().join(format!("aura-tensor-weights-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("model.safetensors");
    let path_text = text(path.to_str().unwrap());
    let l = leaves(TensorDevice::Cpu);
    let put = |name: &str, tensor: &Value| {
        host_tensor_weights_put(vec![path_text.clone(), text(name), tensor.clone()], None).unwrap();
    };
    put("a", &l.a);
    put("counts", &l.counts);
    put("small", &l.small);
    let length =
        outcome(host_tensor_weights_write(vec![path_text.clone()], None).unwrap()).unwrap();
    assert_eq!(length, int(std::fs::metadata(&path).unwrap().len() as i64));

    let names = outcome(host_tensor_weights_names(vec![path_text.clone()], None).unwrap()).unwrap();
    let Value::Vec(names) = names else {
        panic!("expected a list")
    };
    let names: Vec<String> = names
        .elements
        .iter()
        .map(|name| match &*name {
            Value::String(name) => name.to_string(),
            _ => panic!("expected a name"),
        })
        .collect();
    assert_eq!(names, ["a", "counts", "small"]);

    let load = |name: &str, dtype: ArrayDType| {
        outcome(
            host_tensor_weights_load(
                vec![path_text.clone(), text(name), int(dtype as i64), int(0)],
                None,
            )
            .unwrap(),
        )
    };
    assert_eq!(array(&load("a", ArrayDType::Float32).unwrap()), array(&l.a));
    assert_eq!(
        floats(&array(&load("a", ArrayDType::Float64).unwrap())),
        floats(&array(&l.a))
    );
    assert_eq!(
        array(&load("counts", ArrayDType::Int64).unwrap()),
        array(&l.counts)
    );
    assert_eq!(
        array(&load("small", ArrayDType::Int32).unwrap()),
        array(&l.small)
    );
    assert_eq!(
        places(&array(&load("small", ArrayDType::Int64).unwrap())),
        [1, -2, 3, 40]
    );
    assert!(load("missing", ArrayDType::Float32)
        .unwrap_err()
        .contains("has no tensor `missing`"));
    assert!(load("a", ArrayDType::Int64)
        .unwrap_err()
        .contains("has dtype F32, which does not load as `int64`"));
    assert!(load("counts", ArrayDType::Int32)
        .unwrap_err()
        .contains("which `int32` cannot hold"));

    // Half and brain floats widen, and a rank-zero tensor is one element.
    let header = br#"{"h":{"dtype":"F16","shape":[4],"data_offsets":[0,8]},"b":{"dtype":"BF16","shape":[],"data_offsets":[8,10]},"__metadata__":{"format":"pt"}}"#;
    let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
    bytes.extend_from_slice(header);
    for half in [0x3C00_u16, 0xC000, 0x0001, 0x7C00] {
        bytes.extend(half.to_le_bytes());
    }
    bytes.extend(0x3FC0_u16.to_le_bytes());
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(
        floats(&array(&load("h", ArrayDType::Float32).unwrap())),
        [1.0, -2.0, f64::from(2.0_f32.powi(-24)), f64::INFINITY]
    );
    let brain = load("b", ArrayDType::Float32).unwrap();
    assert_eq!(node(&brain).shape(), [1]);
    assert_eq!(floats(&array(&brain)), [1.5]);

    std::fs::write(&path, b"short").unwrap();
    assert!(load("a", ArrayDType::Float32)
        .unwrap_err()
        .contains("cannot read"));
    std::fs::write(&path, [255_u8; 16]).unwrap();
    assert!(load("a", ArrayDType::Float32)
        .unwrap_err()
        .contains("is not a safetensors file"));
    let missing = text(directory.join("none.safetensors").to_str().unwrap());
    assert!(
        outcome(host_tensor_weights_names(vec![missing], None).unwrap())
            .unwrap_err()
            .contains("cannot open")
    );
    put("twice", &l.a);
    put("twice", &l.b);
    assert!(
        outcome(host_tensor_weights_write(vec![path_text.clone()], None).unwrap())
            .unwrap_err()
            .contains("two tensors named `twice`")
    );
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn tensors_set_aside_run_as_one_plan() {
    let l = leaves(TensorDevice::Cpu);
    let expected_sum = array(&binary(&l.a, &l.b, 0).unwrap());
    let expected_product = array(
        &reduce(
            &binary(&expected_sum_of(&l), &l.row, 2).unwrap(),
            0,
            Some(0),
        )
        .unwrap(),
    );
    deferred(|| {
        let shared = binary(&l.a, &l.b, 0).unwrap();
        let product = reduce(&binary(&shared, &l.row, 2).unwrap(), 0, Some(0)).unwrap();
        let (plan, storage) = Plan::collect_all(&[node(&shared), node(&product)]);
        assert_eq!(plan.outputs.len(), 2);
        if let Some(generated) = generated_kernels::run_all(&plan, &storage, 2000) {
            close("shared", &generated[0], &expected_sum);
            close("product", &generated[1], &expected_product);
        }
        node_op(&shared, 8, &[]).unwrap();
        node_op(&shared, 8, &[]).unwrap();
        node_op(&product, 9, &[]).unwrap();
        assert!(node(&shared).is_ready() && node(&product).is_ready());
        assert_eq!(array(&shared), expected_sum);
        assert_eq!(array(&product), expected_product);
        // Nothing waits, so there is nothing to run.
        node_op(&product, 9, &[]).unwrap();
        // One waiting tensor runs by itself.
        let alone = binary(&l.a, &l.b, 2).unwrap();
        node_op(&alone, 9, &[]).unwrap();
        assert!(node(&alone).is_ready());
    });
}

fn expected_sum_of(l: &Leaves) -> Value {
    binary(&l.a, &l.b, 0).unwrap()
}

#[test]
fn a_number_becomes_a_one_element_tensor_of_the_other_dtype() {
    let l = leaves(TensorDevice::Cpu);
    let like =
        |tensor: &Value, value: Value| host_tensor_scalar_like(vec![tensor.clone(), value], None);
    let double = cast(&l.a, ArrayDType::Float64).unwrap();
    assert_eq!(
        floats(&array(&like(&l.a, Value::Float(1.5)).unwrap())),
        [1.5]
    );
    assert_eq!(
        floats(&array(&like(&double, Value::Float(2.5)).unwrap())),
        [2.5]
    );
    assert_eq!(floats(&array(&like(&l.a, int(3)).unwrap())), [3.0]);
    assert_eq!(floats(&array(&like(&double, int(4)).unwrap())), [4.0]);
    assert_eq!(places(&array(&like(&l.counts, int(5)).unwrap())), [5]);
    assert_eq!(array(&like(&l.small, int(6)).unwrap()), int32(&[6], &[1]));
    let message = |result: Result<Value>| result.unwrap_err().message;
    assert!(message(like(&l.small, int(1 << 40))).contains("which `int32` cannot hold"));
    assert!(message(like(&l.counts, Value::Float(1.0))).contains("cannot use a float"));
    let mask = compare(&l.a, &l.b, 0).unwrap();
    assert!(message(like(&mask, int(1))).contains("`Tensor[bool]`"));
}

#[test]
fn the_indexed_kernels_take_every_dtype_they_document() {
    let l = leaves(TensorDevice::Cpu);
    let mask = compare(&l.a, &l.b, 0).unwrap();
    // Joins of integers and of masks.
    assert_eq!(
        places(&array(&pair_op(&l.counts, &l.counts, 0, &[0]).unwrap())).len(),
        2 * node(&l.counts).len()
    );
    assert_eq!(
        array(&pair_op(&l.small, &l.small, 0, &[0]).unwrap()).len(),
        2 * node(&l.small).len()
    );
    assert_eq!(array(&pair_op(&mask, &mask, 0, &[0]).unwrap()).len(), 12);
    // Sums by index of both integer dtypes, with `int32` indices.
    let flat = |tensor: &Value| reshape(tensor, &[-1]).unwrap();
    let counts = flat(&l.counts);
    let small = flat(&l.small);
    let zeros = |length: usize| indices(&vec![0; length], &[length], &l.a);
    let short = tensor(
        int32(&vec![0; node(&small).len()], &[node(&small).len()]),
        TensorDevice::Cpu,
    );
    let total = array(&pair_op(&counts, &zeros(node(&counts).len()), 2, &[0, 2]).unwrap());
    assert_eq!(
        places(&total)[0],
        places(&array(&counts))
            .iter()
            .fold(0_i64, |sum, &value| sum.wrapping_add(value))
    );
    assert_eq!(
        array(&pair_op(&small, &short, 2, &[0, 2]).unwrap()).len(),
        2
    );
    // The largest of `int32` elements, and of `float64` elements with a NaN.
    assert_eq!(array(&node_op(&small, 2, &[0]).unwrap()).len(), 1);
    let nan = tensor(float64(&[1.0, f64::NAN, 3.0], &[3]), TensorDevice::Cpu);
    assert_eq!(array(&node_op(&nan, 2, &[0]).unwrap()).len(), 1);
    // Counting up in `float64`.
    assert_eq!(
        floats(&array(&filled(&[3], 2, 0, ArrayDType::Float64).unwrap())),
        [0.0, 1.0, 2.0]
    );
    // The kernels refuse masks and float indices themselves.
    let bools = array(&mask);
    let numbers = array(&l.a);
    assert!(tensor_kernels::arg_reduce(&bools, 0, true)
        .unwrap_err()
        .message
        .contains("found a `Tensor[bool]`"));
    assert!(tensor_kernels::index_add(&bools, &array(&zeros(2)), 0, 2)
        .unwrap_err()
        .message
        .contains("found a `Tensor[bool]`"));
    assert!(tensor_kernels::take(&numbers, &numbers, 0)
        .unwrap_err()
        .message
        .contains("integer indices"));
}

#[test]
fn a_gradient_passes_through_a_conversion_and_a_permutation() {
    let double = |x: &[Value]| cast(&x[0], ArrayDType::Float64);
    check_gradients(
        "cast",
        &[float32(&[1.0, -2.0, 3.0, 4.5, 0.25, 6.0], &[2, 3])],
        double,
    );
    check_gradients(
        "permute from the end",
        &[float32(&[1.0, -2.0, 3.0, 4.5, 0.25, 6.0], &[2, 3])],
        |x| binary(&permute(&x[0], &[-1, -2])?, &permute(&x[0], &[1, 0])?, 2),
    );
}

#[test]
fn safetensors_files_hold_the_narrow_and_wide_dtypes() {
    let directory = std::env::temp_dir().join(format!("aura-tensor-dtypes-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("model.safetensors");
    let path_text = text(path.to_str().unwrap());
    let l = leaves(TensorDevice::Cpu);
    let load = |name: &str, dtype: ArrayDType| {
        outcome(
            host_tensor_weights_load(
                vec![path_text.clone(), text(name), int(dtype as i64), int(0)],
                None,
            )
            .unwrap(),
        )
    };
    // A `float64` tensor and a mask are written under their own dtypes.
    let double = cast(&l.a, ArrayDType::Float64).unwrap();
    let mask = compare(&l.a, &l.b, 0).unwrap();
    for (name, tensor) in [("double", &double), ("mask", &mask)] {
        host_tensor_weights_put(vec![path_text.clone(), text(name), tensor.clone()], None).unwrap();
    }
    outcome(host_tensor_weights_write(vec![path_text.clone()], None).unwrap()).unwrap();
    assert_eq!(
        floats(&array(&load("double", ArrayDType::Float64).unwrap())),
        floats(&array(&l.a))
    );
    assert!(load("mask", ArrayDType::Int64)
        .unwrap_err()
        .contains("has dtype BOOL, which does not load as `int64`"));

    // Narrow integers widen, and a zero half keeps its sign.
    let header = br#"{"w":{"dtype":"I16","shape":[2],"data_offsets":[0,4]},"s":{"dtype":"I8","shape":[2],"data_offsets":[4,6]},"u":{"dtype":"U8","shape":[2],"data_offsets":[6,8]},"z":{"dtype":"F16","shape":[2],"data_offsets":[8,12]},"c":{"dtype":"C64","shape":[1],"data_offsets":[12,20]},"bad":{"dtype":"I16","shape":[3],"data_offsets":[0,4]}}"#;
    let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
    bytes.extend_from_slice(header);
    bytes.extend((-300_i16).to_le_bytes());
    bytes.extend(7_i16.to_le_bytes());
    bytes.extend([0xFF, 0x02, 0xFF, 0x02]);
    bytes.extend(0x0000_u16.to_le_bytes());
    bytes.extend(0x8000_u16.to_le_bytes());
    bytes.extend([0_u8; 8]);
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(
        places(&array(&load("w", ArrayDType::Int64).unwrap())),
        [-300, 7]
    );
    assert_eq!(
        places(&array(&load("s", ArrayDType::Int64).unwrap())),
        [-1, 2]
    );
    assert_eq!(
        places(&array(&load("u", ArrayDType::Int64).unwrap())),
        [255, 2]
    );
    let zeros = floats(&array(&load("z", ArrayDType::Float32).unwrap()));
    assert!(zeros[0] == 0.0 && zeros[0].is_sign_positive());
    assert!(zeros[1] == 0.0 && zeros[1].is_sign_negative());
    assert!(load("c", ArrayDType::Float32)
        .unwrap_err()
        .contains("has dtype C64, which is not read"));
    assert!(load("bad", ArrayDType::Int64)
        .unwrap_err()
        .contains("describes tensor `bad` wrongly"));

    // A header that is JSON and not an object.
    let mut bytes = 2_u64.to_le_bytes().to_vec();
    bytes.extend_from_slice(b"[]");
    std::fs::write(&path, &bytes).unwrap();
    assert!(load("w", ArrayDType::Int64)
        .unwrap_err()
        .contains("is not a safetensors file"));
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn a_waiting_tensor_the_program_holds_keeps_its_memory() {
    deferred(|| {
        let l = leaves(TensorDevice::Cpu);
        let held = binary(&l.a, &l.b, 0).unwrap();
        let total = {
            let dropped = binary(&held, &l.b, 2).unwrap();
            reduce(&dropped, 0, None).unwrap()
        };
        assert!(!node(&held).is_ready());
        let expected = {
            let sum = binary(&l.a, &l.b, 0).unwrap();
            array(&reduce(&binary(&sum, &l.b, 2).unwrap(), 0, None).unwrap())
        };
        // Reading the total runs the held tensor's work and keeps it.
        assert_eq!(array(&total), expected);
        assert!(node(&held).is_ready());
        // The next value computed from it starts from that memory: its
        // plan is two leaves and one step.
        let next = binary(&held, &l.a, 1).unwrap();
        let (plan, _) = Plan::collect(node(&next));
        assert_eq!(plan.slots.len(), 3);

        // A loop that computes each value from the last runs one step
        // per pass, with or without a tensor read on the way.
        let mut value = binary(&l.a, &l.b, 0).unwrap();
        for _ in 0..4 {
            let following = binary(&value, &l.b, 0).unwrap();
            let moved = reduce(&binary(&following, &value, 1).unwrap(), 0, None).unwrap();
            let (plan, _, held) = Plan::collect_held(&[node(&moved)]);
            assert!(
                plan.slots.len() <= 7,
                "the plan grew to {}",
                plan.slots.len()
            );
            // The value of this pass, and on the first pass the one before it.
            assert!(!held.is_empty() && held.len() <= 2);
            array(&moved);
            assert!(node(&following).is_ready());
            value = following;
        }

        // Tensors read together keep a held tensor too.
        let shared = binary(&l.a, &l.b, 2).unwrap();
        let first = binary(&shared, &l.a, 0).unwrap();
        let second = binary(&shared, &l.b, 0).unwrap();
        evaluate_together(&[node(&first).clone(), node(&second).clone()]).unwrap();
        assert!(node(&shared).is_ready());
    });
}

#[test]
fn slices_that_differ_in_their_start_share_a_plan() {
    deferred(|| {
        let l = leaves(TensorDevice::Cpu);
        let plan_of = |start: i64| {
            let rows = slice(&l.tall, 0, start, start + 2, 1).unwrap();
            let total = reduce(&binary(&rows, &rows, 2).unwrap(), 0, None).unwrap();
            let plan = Plan::collect(node(&total)).0;
            (plan, floats(&array(&total)))
        };
        let (first, first_sum) = plan_of(0);
        let (second, second_sum) = plan_of(1);
        assert_eq!(first, second);
        // tall is [[1, 2], [3, 4], [5, 6]].
        assert_eq!(first_sum, [30.0]);
        assert_eq!(second_sum, [86.0]);
    });
}

#[test]
fn a_long_tracked_chain_is_freed_without_recursion() {
    let l = leaves(TensorDevice::Cpu);
    let watched = node_op(&l.a, 4, &[]).unwrap();
    let mut total = watched.clone();
    // Each tensor holds the one before it as its history. Freeing them
    // one inside another would need a stack frame for each.
    for _ in 0..500_000 {
        total = binary(&total, &l.two, 0).unwrap();
    }
    drop(total);
    assert!(node(&watched).is_ready());
}
