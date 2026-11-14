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
    let with_header = |header: &str| {
        let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
        bytes.extend_from_slice(header.as_bytes());
        std::fs::write(&path, bytes).unwrap();
    };
    with_header("not json");
    assert!(load("a", ArrayDType::Float32)
        .unwrap_err()
        .contains("has a header that is not JSON"));
    with_header(r#"{"a":{"dtype":"F32","shape":[2],"data_offsets":[0,8]}}"#);
    assert!(load("a", ArrayDType::Float32)
        .unwrap_err()
        .contains("describes tensor `a` wrongly"));
    with_header(r#"{"a":{"dtype":"F32","shape":[2]}}"#);
    assert!(load("a", ArrayDType::Float32)
        .unwrap_err()
        .contains("describes tensor `a` wrongly"));
    let missing = text(directory.join("none.safetensors").to_str().unwrap());
    assert!(
        outcome(host_tensor_weights_names(vec![missing], None).unwrap())
            .unwrap_err()
            .contains("cannot open")
    );
    let nowhere = text(
        directory
            .join("none")
            .join("model.safetensors")
            .to_str()
            .unwrap(),
    );
    host_tensor_weights_put(vec![nowhere.clone(), text("a"), l.a.clone()], None).unwrap();
    assert!(
        outcome(host_tensor_weights_write(vec![nowhere], None).unwrap())
            .unwrap_err()
            .contains("cannot write")
    );
    assert!(
        host_tensor_weights_load(vec![path_text.clone(), text("a"), int(99), int(0)], None)
            .is_err()
    );
    assert!(host_tensor_filled(vec![ints(&[2]), int(0), int(1), int(99), int(0)], None).is_err());
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

#[test]
fn the_trace_is_off_unless_asked_for_and_measures_gaps() {
    // The tests run without `AURA_TENSOR_TRACE`.
    assert_eq!(trace_level(), 0);
    trace_gap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    assert!(trace_gap() >= 2.0);
}

/// Rows of 100 elements with ties, a row with a `NaN` and a row of one
/// value, so a lane is long enough for a group of threads.
fn wide_rows() -> ArrayValue {
    let mut values: Vec<f32> = (0..500)
        .map(|index| ((index * 53) % 41) as f32 * 0.5 - 7.0)
        .collect();
    values[100 + 77] = f32::NAN;
    values[100 + 93] = f32::NAN;
    for value in &mut values[200..300] {
        *value = 2.5;
    }
    values[300 + 99] = -90.0;
    values[400] = 90.0;
    float32(&values, &[5, 100])
}

type Wide = fn(&Value) -> Result<Value>;

const WIDE: &[(&str, Wide)] = &[
    ("sum", |rows| reduce(rows, 0, Some(1))),
    ("min", |rows| reduce(rows, 1, Some(-1))),
    ("max", |rows| reduce(rows, 2, Some(1))),
    ("mean", |rows| reduce(rows, 3, Some(1))),
    ("argmax", |rows| node_op(rows, 2, &[1])),
    ("argmin", |rows| node_op(rows, 3, &[1])),
    ("argmin of an expression", |rows| {
        node_op(&binary(rows, rows, 2)?, 3, &[1])
    }),
    ("sum of an expression", |rows| {
        reduce(&binary(rows, rows, 2)?, 0, Some(1))
    }),
];

#[test]
fn a_long_last_axis_is_reduced_by_a_group_of_threads_for_each_lane() {
    deferred(|| {
        for (tag, (name, expression)) in WIDE.iter().enumerate() {
            let rows = tensor(wide_rows(), TensorDevice::Cpu);
            let waiting = expression(&rows).unwrap();
            let (plan, storage) = Plan::collect(node(&waiting));
            let program = tensor_codegen::generate(&plan);
            let threads: Vec<usize> = program
                .actions
                .iter()
                .map(|action| match action {
                    tensor_codegen::Action::Kernel { threads, .. } => *threads,
                    tensor_codegen::Action::Matmul { .. }
                    | tensor_codegen::Action::Alias { .. } => panic!("only kernels here"),
                })
                .collect();
            assert_eq!(
                threads[threads.len() - 2..],
                [5 * 16, 5],
                "{name}: 16 threads share a lane of 100, then one thread joins them"
            );
            let Some(generated) = generated_kernels::run(&plan, &storage, 1100 + tag) else {
                eprintln!("SKIPPED: this machine has no C++ compiler");
                return;
            };
            close(name, &generated, &array(&waiting));
        }
        // A long lane of another axis is shared too: 600 rows of 5 and
        // of 2 by 3, with 32 threads for each lane.
        let values: Vec<f32> = (0..3600)
            .map(|index| ((index * 37) % 101) as f32 * 0.25 - 12.0)
            .collect();
        for (tag, shape, axis, lanes) in [
            (0, &[600, 5][..], 0, 5),
            (1, &[2, 600, 3][..], 1, 6),
            (2, &[600, 5][..], -2, 5),
        ] {
            let tall = tensor(float32(&values[..lanes * 600], shape), TensorDevice::Cpu);
            for reduction in 0..4 {
                let waiting =
                    reduce(&binary(&tall, &tall, 2).unwrap(), reduction, Some(axis)).unwrap();
                let (plan, storage) = Plan::collect(node(&waiting));
                let program = tensor_codegen::generate(&plan);
                let threads: Vec<usize> = program
                    .actions
                    .iter()
                    .map(|action| match action {
                        tensor_codegen::Action::Kernel { threads, .. } => *threads,
                        _ => panic!("only kernels here"),
                    })
                    .collect();
                assert_eq!(threads, [lanes * 32, lanes]);
                let Some(generated) =
                    generated_kernels::run(&plan, &storage, 1150 + tag * 4 + reduction as usize)
                else {
                    eprintln!("SKIPPED: this machine has no C++ compiler");
                    return;
                };
                close("a long lane of another axis", &generated, &array(&waiting));
            }
        }
        // A short lane and a lane that is not the last axis keep one thread.
        let rows = tensor(wide_rows(), TensorDevice::Cpu);
        for (waiting, lanes) in [
            (reduce(&rows, 0, Some(0)).unwrap(), 100),
            (
                reduce(&reshape(&rows, &[25, 20]).unwrap(), 0, Some(1)).unwrap(),
                25,
            ),
        ] {
            let (plan, _) = Plan::collect(node(&waiting));
            let program = tensor_codegen::generate(&plan);
            let Some(tensor_codegen::Action::Kernel { threads, .. }) = program.actions.last()
            else {
                panic!("a reduction ends in a kernel");
            };
            assert_eq!(*threads, lanes);
            assert_eq!(program.buffers, plan.slots.len(), "no scratch buffer");
        }
    });
}

#[test]
fn a_gpu_device_agrees_on_long_lanes() {
    for device in gpus() {
        for (name, expression) in WIDE {
            let host = expression(&tensor(wide_rows(), TensorDevice::Cpu)).unwrap();
            let gpu = expression(&tensor(wide_rows(), device)).unwrap();
            close(name, &array(&gpu), &array(&host));
        }
        println!("RAN: {} long lane expressions on a GPU device", WIDE.len());
    }
}

/// A stack of two 3 by 4 matrices, a 4 by 3 matrix and a stack of two.
fn matrices(device: TensorDevice) -> (Value, Value, Value) {
    let numbers = |count: usize, scale: f32| -> Vec<f32> {
        (0..count)
            .map(|index| ((index * 7) % 11) as f32 * scale - 2.0)
            .collect()
    };
    (
        tensor(float32(&numbers(24, 0.5), &[2, 3, 4]), device),
        tensor(float32(&numbers(12, 0.25), &[4, 3]), device),
        tensor(float32(&numbers(24, 0.75), &[2, 4, 3]), device),
    )
}

type Product = fn(&(Value, Value, Value)) -> Result<Value>;

/// Products over transposes and reshapes, with how many kernels and
/// aliases the plan has.
const PRODUCTS: &[(&str, Product, usize, usize)] = &[
    (
        "a transpose of a transpose",
        |(stack, flat, _)| matmul(stack, &permute(&permute(flat, &[1, 0])?, &[1, 0])?),
        1,
        0,
    ),
    (
        "left transpose of a leaf",
        |(_, flat, _)| matmul(&permute(flat, &[1, 0])?, flat),
        0,
        0,
    ),
    (
        "right transpose of a leaf",
        |(_, flat, _)| matmul(flat, &permute(flat, &[])?),
        0,
        0,
    ),
    (
        "stacks, both transposed",
        |(stack, _, other)| matmul(&permute(other, &[0, 2, 1])?, &permute(stack, &[0, 2, 1])?),
        0,
        0,
    ),
    (
        "a transposed stack times one transposed matrix",
        |(stack, flat, _)| matmul(&permute(stack, &[0, 2, 1])?, &permute(flat, &[1, 0])?),
        0,
        0,
    ),
    (
        "a stack times the transpose of a reshape",
        |(stack, flat, _)| matmul(stack, &permute(&reshape(flat, &[3, 4])?, &[1, 0])?),
        0,
        1,
    ),
    (
        "a transpose two products read",
        |(_, flat, _)| {
            let turned = permute(flat, &[1, 0])?;
            binary(
                &matmul(&turned, flat)?,
                &reduce(&matmul(flat, &turned)?, 0, None)?,
                0,
            )
        },
        2,
        0,
    ),
    (
        "a transpose also read outside a product",
        |(_, flat, _)| {
            let turned = permute(flat, &[1, 0])?;
            binary(&matmul(&turned, flat)?, &reduce(&turned, 0, None)?, 0)
        },
        3,
        0,
    ),
    (
        "a transpose of an expression",
        |(_, flat, _)| matmul(&permute(&binary(flat, flat, 0)?, &[1, 0])?, flat),
        1,
        0,
    ),
    (
        "a permute that is no transpose",
        |(stack, flat, _)| matmul(&permute(stack, &[1, 0, 2])?, flat),
        1,
        0,
    ),
    (
        "reshapes of a product and a leaf",
        |(stack, flat, _)| {
            let product = reshape(&matmul(stack, flat)?, &[6, 3])?;
            matmul(&product, &reshape(flat, &[3, 4])?)
        },
        0,
        2,
    ),
    (
        "a reshape of an expression",
        |(stack, flat, _)| matmul(&reshape(&binary(stack, stack, 0)?, &[6, 4])?, flat),
        1,
        0,
    ),
];

#[test]
fn a_product_reads_a_transpose_in_place_and_a_reshape_shares_memory() {
    deferred(|| {
        for (tag, (name, expression, kernels, aliases)) in PRODUCTS.iter().enumerate() {
            let waiting = expression(&matrices(TensorDevice::Cpu)).unwrap();
            let (plan, storage) = Plan::collect(node(&waiting));
            let program = tensor_codegen::generate(&plan);
            let count = |wanted: fn(&tensor_codegen::Action) -> bool| {
                program
                    .actions
                    .iter()
                    .filter(|action| wanted(action))
                    .count()
            };
            assert_eq!(
                (
                    count(|action| matches!(action, tensor_codegen::Action::Kernel { .. })),
                    count(|action| matches!(action, tensor_codegen::Action::Alias { .. })),
                ),
                (*kernels, *aliases),
                "{name}: kernels and aliases"
            );
            let Some(generated) = generated_kernels::run(&plan, &storage, 1200 + tag) else {
                eprintln!("SKIPPED: this machine has no C++ compiler");
                return;
            };
            close(name, &generated, &array(&waiting));
        }
    });
}

#[test]
fn a_gpu_device_agrees_on_products_of_transposes() {
    for device in gpus() {
        for (name, expression, _, _) in PRODUCTS {
            let host = expression(&matrices(TensorDevice::Cpu)).unwrap();
            let gpu = expression(&matrices(device)).unwrap();
            close(name, &array(&gpu), &array(&host));
        }
        println!(
            "RAN: {} products of transposes on a GPU device",
            PRODUCTS.len()
        );
    }
}

#[test]
fn a_reshape_that_shares_memory_can_be_a_result() {
    deferred(|| {
        let (stack, _, _) = matrices(TensorDevice::Cpu);
        let total = reduce(&stack, 0, Some(2)).unwrap();
        let waiting = reshape(&total, &[3, 2]).unwrap();
        let (plan, storage) = Plan::collect(node(&waiting));
        let program = tensor_codegen::generate(&plan);
        assert!(matches!(
            program.actions.last(),
            Some(tensor_codegen::Action::Alias { .. })
        ));
        let Some(generated) = generated_kernels::run(&plan, &storage, 1290) else {
            eprintln!("SKIPPED: this machine has no C++ compiler");
            return;
        };
        close("a reshaped sum", &generated, &array(&waiting));
    });
}

/// A moving average, a second one and the new weight made from both, as
/// an optimizer step makes them.
fn averaged(l: &Leaves) -> (Value, Value, Value) {
    let average = binary(
        &binary(&l.a, &l.two, 2).unwrap(),
        &binary(&l.b, &l.b, 2).unwrap(),
        0,
    )
    .unwrap();
    let second = unary(&binary(&l.b, &l.row, 0).unwrap(), 1).unwrap();
    let weight = binary(&binary(&l.a, &average, 1).unwrap(), &second, 2).unwrap();
    (average, second, weight)
}

#[test]
fn a_kernel_writes_the_kept_results_only_it_reads() {
    let l = leaves(TensorDevice::Cpu);
    let (average, second, weight) = averaged(&l);
    let expected = [array(&average), array(&second), array(&weight)];
    deferred(|| {
        let (average, second, weight) = averaged(&l);
        let (plan, storage) = Plan::collect_all(&[node(&average), node(&second), node(&weight)]);
        let program = tensor_codegen::generate(&plan);
        let [tensor_codegen::Action::Kernel { more, inputs, .. }] = &program.actions[..] else {
            panic!("one kernel writes all three: {:?}", program.actions);
        };
        assert_eq!((more.len(), inputs.len()), (2, 4));
        if let Some(generated) = generated_kernels::run_all(&plan, &storage, 2100) {
            for (index, name) in ["average", "second", "weight"].into_iter().enumerate() {
                close(name, &generated[index], &expected[index]);
            }
        }

        // A kept result of another shape, or one a reduction also reads,
        // has its own kernel.
        let narrow = binary(&l.row, &l.row, 0).unwrap();
        let wide = binary(&l.a, &narrow, 2).unwrap();
        let (plan, _) = Plan::collect_all(&[node(&narrow), node(&wide)]);
        assert_eq!(tensor_codegen::generate(&plan).actions.len(), 2);
        let (average, _, weight) = averaged(&l);
        let total = reduce(&average, 0, None).unwrap();
        let (plan, storage) = Plan::collect_all(&[node(&average), node(&weight), node(&total)]);
        let program = tensor_codegen::generate(&plan);
        assert!(program.actions.iter().all(|action| matches!(
            action,
            tensor_codegen::Action::Kernel { more, .. } if more.is_empty()
        )));
        if let Some(generated) = generated_kernels::run_all(&plan, &storage, 2101) {
            close("average", &generated[0], &expected[0]);
            close("weight", &generated[1], &expected[2]);
        }
    });
}

#[test]
fn a_gpu_device_agrees_on_results_written_beside_another() {
    for device in gpus() {
        let (average, second, weight) = averaged(&leaves(TensorDevice::Cpu));
        let expected = [array(&average), array(&second), array(&weight)];
        let (average, second, weight) = averaged(&leaves(device));
        for kept in [&average, &second, &weight] {
            node_op(kept, 8, &[]).unwrap();
        }
        node_op(&weight, 9, &[]).unwrap();
        for (index, kept) in [&average, &second, &weight].into_iter().enumerate() {
            assert!(node(kept).is_ready());
            close("kept result", &array(kept), &expected[index]);
        }
        println!("RAN: results written beside another on a GPU device");
    }
}

#[test]
fn node_addresses_hash_to_different_values() {
    use std::hash::{BuildHasher, Hasher};
    let hashed = |address: usize| {
        let mut hasher = PointerHasher::default().build_hasher();
        hasher.write_usize(address);
        hasher.finish()
    };
    assert_ne!(hashed(0x7f00_1000), hashed(0x7f00_1040));
    // Bytes hash too, for a key that is not an address.
    let bytes = |text: &[u8]| {
        let mut hasher = PointerHasher::default();
        hasher.write(text);
        hasher.finish()
    };
    assert_ne!(bytes(b"ab"), bytes(b"ba"));
}

#[test]
fn small_kernels_that_do_not_read_each_other_share_a_launch() {
    let l = leaves(TensorDevice::Cpu);
    let make = |l: &Leaves| {
        vec![
            binary(&l.a, &l.b, 0).unwrap(),
            binary(&l.a, &l.b, 2).unwrap(),
            binary(&l.row, &l.row, 0).unwrap(),
            unary(&l.tall, 1).unwrap(),
            binary(&l.b, &l.row, 1).unwrap(),
            binary(&l.counts, &l.counts, 0).unwrap(),
        ]
    };
    let expected: Vec<ArrayValue> = make(&l).iter().map(array).collect();
    deferred(|| {
        let kept = make(&l);
        let roots: Vec<&TensorValue> = kept.iter().map(node).collect();
        let (plan, storage) = Plan::collect_all(&roots);
        let program = tensor_codegen::generate(&plan);
        let [tensor_codegen::Action::Kernel {
            name,
            more,
            inputs,
            threads,
            ..
        }] = &program.actions[..]
        else {
            panic!("six small kernels are one launch: {:?}", program.actions);
        };
        assert!(name.starts_with('g'));
        assert_eq!((more.len(), inputs.len(), *threads), (5, 5, 31));
        if let Some(generated) = generated_kernels::run_all(&plan, &storage, 2200) {
            for (index, expected) in expected.iter().enumerate() {
                close("a kernel of a shared launch", &generated[index], expected);
            }
        }

        // Small kernels whose results nothing later reads move to the
        // end, past a reduction between them, and share a launch there.
        let first = binary(&l.a, &l.b, 0).unwrap();
        let total = reduce(&binary(&l.a, &l.b, 2).unwrap(), 0, Some(1)).unwrap();
        let second = binary(&l.row, &l.row, 0).unwrap();
        let (plan, storage) = Plan::collect_all(&[node(&first), node(&total), node(&second)]);
        let program = tensor_codegen::generate(&plan);
        let [tensor_codegen::Action::Kernel { more: none, .. }, tensor_codegen::Action::Kernel { name, more, .. }] =
            &program.actions[..]
        else {
            panic!("a reduction, then one shared launch: {:?}", program.actions);
        };
        assert!(none.is_empty() && name.starts_with('g') && more.len() == 1);
        if let Some(generated) = generated_kernels::run_all(&plan, &storage, 2202) {
            close("first", &generated[0], &expected[0]);
            close("second", &generated[2], &expected[2]);
        }

        // A kernel that reads another's result is launched after it.
        let sum = binary(&l.a, &l.b, 0).unwrap();
        let square = binary(&sum, &sum, 2).unwrap();
        let (plan, storage) = Plan::collect_all(&[node(&sum), node(&square)]);
        let program = tensor_codegen::generate(&plan);
        let names: Vec<&str> = program
            .actions
            .iter()
            .map(|action| match action {
                tensor_codegen::Action::Kernel { name, .. } => name.as_str(),
                _ => panic!("only kernels here"),
            })
            .collect();
        assert_eq!(names.len(), 2);
        assert!(names.iter().all(|name| name.starts_with('k')));
        if let Some(generated) = generated_kernels::run_all(&plan, &storage, 2201) {
            close(
                "sum",
                &generated[0],
                &array(&binary(&l.a, &l.b, 0).unwrap()),
            );
        }
    });
}

#[test]
fn a_gpu_device_agrees_on_kernels_that_share_a_launch() {
    for device in gpus() {
        let make = |l: &Leaves| {
            vec![
                binary(&l.a, &l.b, 0).unwrap(),
                binary(&l.a, &l.b, 2).unwrap(),
                binary(&l.row, &l.row, 0).unwrap(),
                unary(&l.tall, 1).unwrap(),
                binary(&l.b, &l.row, 1).unwrap(),
            ]
        };
        let expected: Vec<ArrayValue> =
            make(&leaves(TensorDevice::Cpu)).iter().map(array).collect();
        let kept = make(&leaves(device));
        for value in &kept {
            node_op(value, 8, &[]).unwrap();
        }
        node_op(&kept[0], 9, &[]).unwrap();
        for (value, expected) in kept.iter().zip(&expected) {
            assert!(node(value).is_ready());
            close("a kernel of a shared launch", &array(value), expected);
        }
        println!("RAN: kernels that share a launch on a GPU device");
    }
}

#[test]
fn a_waiting_tensor_assigned_to_a_variable_runs_with_the_next_work() {
    deferred(|| {
        let l = leaves(TensorDevice::Cpu);
        let variable = node_op(&l.a, 6, &[]).unwrap();
        let sum = binary(&l.a, &l.b, 0).unwrap();
        pair_op(&variable, &sum, 4, &[]).unwrap();
        let held = node_op(&variable, 7, &[]).unwrap();
        assert!(node(&held).is_tracked() && !node(&held).is_ready());
        // Other work computes the assigned tensor in the same run.
        let other = binary(&l.b, &l.two, 2).unwrap();
        let expected = array(&other);
        assert!(node(&held).is_ready());
        assert_eq!(array(&held), array(&sum));
        assert_eq!(array(&other), expected);

        // So does work on several tensors at once.
        let next = binary(&l.a, &l.two, 2).unwrap();
        pair_op(&variable, &next, 4, &[]).unwrap();
        let held = node_op(&variable, 7, &[]).unwrap();
        let (one, two) = (
            binary(&l.a, &l.a, 0).unwrap(),
            binary(&l.b, &l.b, 0).unwrap(),
        );
        evaluate_together(&[node(&one).clone(), node(&two).clone()]).unwrap();
        assert!(node(&held).is_ready());
        assert_eq!(array(&held), array(&next));

        // An assigned tensor that is read first runs by itself, and one of a
        // dtype without gradients is refused as before.
        let last = binary(&l.b, &l.two, 2).unwrap();
        pair_op(&variable, &last, 4, &[]).unwrap();
        assert_eq!(array(&node_op(&variable, 7, &[]).unwrap()), array(&last));
        assert!(TensorValue::deferred(TensorDevice::Cpu, &[]).is_empty());
    });
}

#[test]
fn window_indices_name_each_window_and_are_kept() {
    let l = leaves(TensorDevice::Cpu);
    let places = |value: &Value| places(&array(value));
    // A 2 by 3 image, a 2 by 2 kernel, stride 1, no padding: two windows.
    let plain = node_op(&l.a, 13, &[2, 3, 2, 2, 1, 0]).unwrap();
    assert_eq!(places(&plain), [0, 1, 3, 4, 1, 2, 4, 5]);
    // The same window again is the same tensor.
    assert_eq!(
        node(&plain),
        node(&node_op(&l.b, 13, &[2, 3, 2, 2, 1, 0]).unwrap())
    );
    // Padding names the place after the image, 4, and a stride skips.
    let padded = node_op(&l.a, 13, &[2, 2, 2, 2, 2, 1]).unwrap();
    assert_eq!(
        places(&padded),
        [4, 4, 4, 0, 4, 4, 1, 4, 4, 2, 4, 4, 3, 4, 4, 4]
    );
    // The list empties when it is full, and the indices are made again.
    for width in 0..KEPT_WINDOWS as i64 + 1 {
        node_op(&l.a, 13, &[1, width + 1, 1, 1, 1, 0]).unwrap();
    }
    assert_eq!(
        places(&node_op(&l.a, 13, &[2, 3, 2, 2, 1, 0]).unwrap()),
        places(&plain)
    );

    let message = |parameters: &[i64]| node_op(&l.a, 13, parameters).unwrap_err().message;
    assert!(message(&[0, 3, 1, 1, 1, 0]).contains("at least one element"));
    assert!(message(&[2, 3, 0, 1, 1, 0]).contains("at least one element"));
    assert!(message(&[2, 3, 1, 1, 0, 0]).contains("a stride of at least 1"));
    assert!(message(&[2, 3, 1, 1, 1, -1]).contains("a padding of at least 0"));
    assert!(message(&[2, 3, 3, 1, 1, 0]).contains("no larger than the padded image"));
    assert!(message(&[2, 3, 1, 4, 1, 0]).contains("the kernel is 1 by 4"));
    assert!(message(&[2, 3, 1]).contains("expects 6 parameters"));
}

/// The softmax of each row of `x`, built as `std/tensor.au` builds it.
fn row_softmax(x: &Value, rows: i64) -> Value {
    let largest = reshape(&reduce(x, 2, Some(1)).unwrap(), &[rows, 1]).unwrap();
    let powers = unary(&binary(x, &largest, 1).unwrap(), 2).unwrap();
    let total = reshape(&reduce(&powers, 0, Some(1)).unwrap(), &[rows, 1]).unwrap();
    binary(&powers, &total, 3).unwrap()
}

fn wave(count: usize, shape: &[usize], device: TensorDevice) -> Value {
    let values: Vec<f32> = (0..count)
        .map(|at| ((at * 37 % 101) as f32 - 50.0) / 16.0)
        .collect();
    tensor(float32(&values, shape), device)
}

#[test]
fn large_shared_arithmetic_is_computed_again_by_each_reader() {
    let kernels = |plan: &Plan| {
        tensor_codegen::generate(plan)
            .actions
            .iter()
            .filter(|action| matches!(action, tensor_codegen::Action::Kernel { .. }))
            .count()
    };
    let expected = array(&row_softmax(
        &wave(256 * 256, &[256, 256], TensorDevice::Cpu),
        256,
    ));
    deferred(|| {
        // The powers have two readers and are not written, and one
        // kernel reduces each lane twice and writes the result.
        let x = wave(256 * 256, &[256, 256], TensorDevice::Cpu);
        let result = row_softmax(&x, 256);
        let (plan, storage) = Plan::collect_all(&[node(&result)]);
        assert_eq!(kernels(&plan), 1);
        if let Some(generated) = generated_kernels::run_all(&plan, &storage, 2300) {
            close("softmax", &generated[0], &expected);
        }

        // A small tensor keeps the powers in memory: one kernel writes
        // them and one the result.
        let small = wave(64 * 64, &[64, 64], TensorDevice::Cpu);
        let (plan, _) = Plan::collect_all(&[node(&row_softmax(&small, 64))]);
        assert_eq!(kernels(&plan), 2);

        // So does a step the program keeps, one with more readers, and
        // one that reads two large buffers.
        // Each whole sum below is three kernels.
        let y = wave(256 * 256, &[256, 256], TensorDevice::Cpu);
        let total =
            |operand: &Value, code: i64| reduce(&unary(operand, code).unwrap(), 0, None).unwrap();
        let doubled = binary(&x, &x, 0).unwrap();
        let (one, two) = (total(&doubled, 2), total(&doubled, 0));
        let (plan, _) = Plan::collect_all(&[node(&one), node(&two)]);
        assert_eq!(kernels(&plan), 6);
        let (plan, _) = Plan::collect_all(&[node(&one), node(&two), node(&doubled)]);
        assert_eq!(kernels(&plan), 7);
        let many: Vec<Value> = (0..4).map(|_| total(&doubled, 2)).collect();
        let many: Vec<&TensorValue> = many.iter().map(node).collect();
        assert_eq!(kernels(&Plan::collect_all(&many).0), 13);
        let mixed = binary(&x, &y, 0).unwrap();
        let (one, two) = (total(&mixed, 2), total(&mixed, 0));
        let (plan, _) = Plan::collect_all(&[node(&one), node(&two)]);
        assert_eq!(kernels(&plan), 7);
    });
}

#[test]
fn a_gpu_device_agrees_on_arithmetic_computed_again() {
    for device in gpus() {
        let expected = array(&row_softmax(
            &wave(256 * 256, &[256, 256], TensorDevice::Cpu),
            256,
        ));
        let found = array(&row_softmax(&wave(256 * 256, &[256, 256], device), 256));
        close("softmax of long rows", &found, &expected);
        println!("RAN: arithmetic computed again on a GPU device");
    }
}

#[test]
fn a_product_starts_deep_waiting_operands_before_it_is_read() {
    assert_eq!(early_depth_from(None), EARLY_DEPTH);
    assert_eq!(early_depth_from(Some("12")), 12);
    assert_eq!(early_depth_from(Some("0")), 0);
    assert_eq!(early_depth_from(Some("deep")), EARLY_DEPTH);
    assert!(!runs_early());
    // `steps` additions of a matrix to itself.
    let chain = |device: TensorDevice, steps: usize| {
        let mut value = wave(16, &[4, 4], device);
        for _ in 0..steps {
            value = binary(&value, &wave(16, &[4, 4], device), 0).unwrap();
        }
        value
    };
    let product = |left: &Value, right: &Value| {
        host_tensor_matmul(vec![left.clone(), right.clone()], None).unwrap()
    };
    let turned = |value: &Value| permute(value, &[1, 0]);
    let expected = array(&product(
        &chain(TensorDevice::Cpu, EARLY_DEPTH),
        &turned(&chain(TensorDevice::Cpu, 3)).unwrap(),
    ));
    deferred(|| {
        // Shallow operands wait for the product to be read.
        let (left, right) = (chain(TensorDevice::Cpu, 3), chain(TensorDevice::Cpu, 3));
        let shallow = product(&left, &right);
        assert!(!node(&left).is_ready() && !node(&right).is_ready());
        assert!(!node(&shallow).is_ready());

        // A deep operand runs when the product is made, with the other
        // operand. Of a transposed operand, the memory the product reads
        // in place runs, and the transpose waits.
        let deep = chain(TensorDevice::Cpu, EARLY_DEPTH);
        let plain = chain(TensorDevice::Cpu, 3);
        let swapped = turned(&plain).unwrap();
        let made = product(&deep, &swapped);
        assert!(node(&deep).is_ready() && node(&plain).is_ready());
        assert!(!node(&swapped).is_ready() && !node(&made).is_ready());
        assert!(!runs_early());
        assert_eq!(array(&made), expected);

        // Operands that are ready start nothing, and a depth of zero
        // turns early starts off.
        start_operands(&[node(&deep), node(&plain)], 1).unwrap();
        let waiting = chain(TensorDevice::Cpu, 3);
        start_operands(&[node(&waiting)], 0).unwrap();
        assert!(!node(&waiting).is_ready());
        start_operands(&[node(&waiting)], 1).unwrap();
        assert!(node(&waiting).is_ready());

        // While work with gradients waits, nothing starts early: the
        // backward pass shares the work.
        let weight = node_op(&wave(16, &[4, 4], TensorDevice::Cpu), 4, &[]).unwrap();
        let tracked = binary(&weight, &wave(16, &[4, 4], TensorDevice::Cpu), 0).unwrap();
        assert!(node(&tracked).is_tracked() && !node(&tracked).is_ready());
        let deep = chain(TensorDevice::Cpu, EARLY_DEPTH);
        let held_back = product(&deep, &right);
        assert!(!node(&deep).is_ready());
        // A run the program waits for ends that.
        array(&held_back);
        array(&tracked);
        let deep = chain(TensorDevice::Cpu, EARLY_DEPTH);
        product(&deep, &right);
        assert!(node(&deep).is_ready());

        // With gradients off, a result of a tracked tensor is not
        // tracked, so a model that only predicts starts early.
        node_op(&weight, 14, &[0]).unwrap();
        let plain = binary(&weight, &wave(16, &[4, 4], TensorDevice::Cpu), 0).unwrap();
        assert!(!node(&plain).is_tracked());
        let deep = chain(TensorDevice::Cpu, EARLY_DEPTH);
        product(&deep, &weight);
        assert!(node(&deep).is_ready());
        assert!(node_op(&weight, 14, &[]).is_err());
        node_op(&weight, 14, &[1]).unwrap();
        let again = binary(&weight, &wave(16, &[4, 4], TensorDevice::Cpu), 0).unwrap();
        assert!(node(&again).is_tracked());
        array(&again);
    });
}

/// A layer norm over the last axis of `x`, as `std/tensor.au` builds it.
fn lane_norm(x: &Value, kept: &[i64], weight: &Value, bias: &Value) -> Value {
    let mean = |value: &Value| reshape(&reduce(value, 3, Some(-1)).unwrap(), kept).unwrap();
    let centered = binary(x, &mean(x), 1).unwrap();
    let spread = mean(&binary(&centered, &centered, 2).unwrap());
    let eps = host_tensor_scalar_like(vec![spread.clone(), Value::Float(0.00001)], None).unwrap();
    let deviation = unary(&binary(&spread, &eps, 0).unwrap(), 0).unwrap();
    let scaled = binary(&binary(&centered, &deviation, 3).unwrap(), weight, 2).unwrap();
    binary(&scaled, bias, 0).unwrap()
}

/// The inputs of `lane_norm` over `[rows, length]` on `device`.
fn lane_norm_of(rows: usize, length: usize, device: TensorDevice) -> Value {
    let x = wave(rows * length, &[rows, length], device);
    let (weight, bias) = (
        wave(length, &[length], device),
        wave(length, &[length], device),
    );
    lane_norm(&x, &[rows as i64, 1], &weight, &bias)
}

#[test]
fn one_kernel_reduces_each_lane_and_writes_the_arithmetic_that_reads_it() {
    let actions = |plan: &Plan| {
        tensor_codegen::generate(plan)
            .actions
            .iter()
            .filter(|action| !matches!(action, tensor_codegen::Action::Alias { .. }))
            .count()
    };
    // Lanes longer than a block, a whole block, and shorter than one,
    // whose length no thread count divides.
    for (tag, (rows, length)) in [(96, 700), (256, 256), (700, 100)].into_iter().enumerate() {
        let expected = array(&lane_norm_of(rows, length, TensorDevice::Cpu));
        deferred(|| {
            let result = lane_norm_of(rows, length, TensorDevice::Cpu);
            let (plan, storage) = Plan::collect_all(&[node(&result)]);
            assert_eq!(actions(&plan), 1, "{rows} lanes of {length}");
            if let Some(generated) = generated_kernels::run_all(&plan, &storage, 2400 + tag) {
                close("layer norm", &generated[0], &expected);
            }
        });
    }
    let expected = array(&row_softmax(
        &wave(300 * 300, &[300, 300], TensorDevice::Cpu),
        300,
    ));
    deferred(|| {
        // The largest element and the sum use their own folds.
        let x = wave(300 * 300, &[300, 300], TensorDevice::Cpu);
        let (plan, storage) = Plan::collect_all(&[node(&row_softmax(&x, 300))]);
        assert_eq!(actions(&plan), 1);
        // The powers are written while they are summed and divided in
        // place, so each is computed once.
        let source = tensor_codegen::generate(&plan).source;
        assert!(source.contains("out[start + k] = (out[start + k] / p"));
        if let Some(generated) = generated_kernels::run_all(&plan, &storage, 2410) {
            close("softmax", &generated[0], &expected);
        }

        // A reduction the program keeps, or that another kernel reads,
        // has memory of its own, and so does one of a short lane.
        let x = wave(256 * 256, &[256, 256], TensorDevice::Cpu);
        let largest = reshape(&reduce(&x, 2, Some(1)).unwrap(), &[256, 1]).unwrap();
        let moved = binary(&x, &largest, 1).unwrap();
        let (plan, _) = Plan::collect_all(&[node(&moved)]);
        assert_eq!(actions(&plan), 1);
        let (plan, _) = Plan::collect_all(&[node(&moved), node(&largest)]);
        assert_eq!(actions(&plan), 3);
        let again = binary(&largest, &largest, 0).unwrap();
        let (plan, _) = Plan::collect_all(&[node(&moved), node(&again)]);
        assert_eq!(actions(&plan), 3);
        let short = wave(4096 * 32, &[4096, 32], TensorDevice::Cpu);
        let largest = reshape(&reduce(&short, 2, Some(1)).unwrap(), &[4096, 1]).unwrap();
        let (plan, _) = Plan::collect_all(&[node(&binary(&short, &largest, 1).unwrap())]);
        assert_eq!(actions(&plan), 3);
    });
}

/// A softmax whose largest element is read through two reshapes, as
/// `std/tensor.au` builds it, over a `[2, 4, 64, 128]` tensor.
fn stacked_softmax(device: TensorDevice) -> Value {
    stacked_softmax_of(64, 128, device)
}

/// The same softmax with lanes of `length` elements. A lane longer than
/// a block of threads is divided in place; a shorter one is held by its
/// threads.
fn stacked_softmax_of(rows: usize, length: usize, device: TensorDevice) -> Value {
    let rows_i = rows as i64;
    let x = wave(2 * 4 * rows * length, &[2, 4, rows, length], device);
    let kept = |value: &Value| reshape(&reduce(value, 2, Some(-1)).unwrap(), &[2, 4, rows_i, 1]);
    let largest = reshape(&kept(&x).unwrap(), &[2, 4, rows_i, 1]).unwrap();
    let powers = unary(&binary(&x, &largest, 1).unwrap(), 2).unwrap();
    let total = reshape(&reduce(&powers, 0, Some(-1)).unwrap(), &[2, 4, rows_i, 1]).unwrap();
    binary(&powers, &total, 3).unwrap()
}

#[test]
fn a_lane_kernel_reads_through_reshapes_and_feeds_products() {
    let count = |plan: &Plan| {
        let (mut kernels, mut products) = (0, 0);
        for action in tensor_codegen::generate(plan).actions {
            match action {
                tensor_codegen::Action::Kernel { .. } => kernels += 1,
                tensor_codegen::Action::Matmul { .. } => products += 1,
                tensor_codegen::Action::Alias { .. } => {}
            }
        }
        (kernels, products)
    };
    // A layer norm over `[8, 32, 256]`, flattened for a product.
    let normed = |device: TensorDevice| {
        let x = wave(8 * 32 * 256, &[8, 32, 256], device);
        let (weight, bias) = (wave(256, &[256], device), wave(256, &[256], device));
        lane_norm(&x, &[8, 32, 1], &weight, &bias)
    };
    let flat = |value: &Value| reshape(value, &[-1, 256]).unwrap();
    let product = |left: &Value, device: TensorDevice| {
        host_tensor_matmul(vec![left.clone(), wave(256 * 4, &[256, 4], device)], None).unwrap()
    };
    let cpu = TensorDevice::Cpu;
    let expected = array(&flat(&normed(cpu)));
    let expected_softmax = array(&stacked_softmax(cpu));
    let expected_long = array(&stacked_softmax_of(32, 700, cpu));
    deferred(|| {
        // One reshape reads the layer norm: the kernel writes the
        // flattened lanes.
        // The lanes are compared before the product, whose additions
        // run in another order on each device.
        let lanes = flat(&normed(cpu));
        let result = product(&lanes, cpu);
        let (plan, _) = Plan::collect_all(&[node(&result)]);
        assert_eq!(count(&plan), (1, 1));
        let (plan, storage) = Plan::collect_all(&[node(&result), node(&lanes)]);
        assert_eq!(count(&plan), (1, 1));
        if let Some(generated) = generated_kernels::run_all(&plan, &storage, 2420) {
            close("layer norm into a product", &generated[1], &expected);
        }

        // Two reshapes read it: it is written once and both are that
        // memory.
        let shared = normed(cpu);
        let (one, two) = (product(&flat(&shared), cpu), product(&flat(&shared), cpu));
        let (plan, _) = Plan::collect_all(&[node(&one), node(&two)]);
        assert_eq!(count(&plan), (1, 2));
        let (plan, storage) = Plan::collect_all(&[node(&one), node(&two), node(&shared)]);
        assert_eq!(count(&plan), (1, 2));
        if let Some(generated) = generated_kernels::run_all(&plan, &storage, 2421) {
            close(
                "layer norm into two products",
                &generated[2],
                &array(&normed(cpu)),
            );
        }

        // Reshapes between a reduction and its reader are no memory.
        let (plan, storage) = Plan::collect_all(&[node(&stacked_softmax(cpu))]);
        assert_eq!(count(&plan), (1, 0));
        if let Some(generated) = generated_kernels::run_all(&plan, &storage, 2422) {
            close("softmax through reshapes", &generated[0], &expected_softmax);
        }
        // Lanes longer than a block of threads are divided in place.
        let (plan, storage) = Plan::collect_all(&[node(&stacked_softmax_of(32, 700, cpu))]);
        assert_eq!(count(&plan), (1, 0));
        if let Some(generated) = generated_kernels::run_all(&plan, &storage, 2423) {
            close("softmax of long lanes", &generated[0], &expected_long);
        }

        // A reader that moves elements between lanes needs the
        // reduction in memory.
        let x = wave(256 * 256, &[256, 256], cpu);
        let largest = reshape(&reduce(&x, 2, Some(1)).unwrap(), &[256, 1]).unwrap();
        let moved = binary(&x, &largest, 1).unwrap();
        let turned = binary(&moved, &permute(&moved, &[1, 0]).unwrap(), 0).unwrap();
        let (plan, _) = Plan::collect_all(&[node(&turned)]);
        assert_eq!(count(&plan), (3, 0));
    });
}

#[test]
fn a_gpu_device_agrees_on_a_lane_reduced_inside_one_kernel() {
    for device in gpus() {
        close(
            "softmax through reshapes",
            &array(&stacked_softmax(device)),
            &array(&stacked_softmax(TensorDevice::Cpu)),
        );
        close(
            "softmax of long lanes",
            &array(&stacked_softmax_of(32, 700, device)),
            &array(&stacked_softmax_of(32, 700, TensorDevice::Cpu)),
        );
    }
    for device in gpus() {
        for (rows, length) in [(96, 700), (256, 256), (700, 100)] {
            let expected = array(&lane_norm_of(rows, length, TensorDevice::Cpu));
            let found = array(&lane_norm_of(rows, length, device));
            close("layer norm of lanes", &found, &expected);
        }
        println!("RAN: a lane reduced inside one kernel on a GPU device");
    }
}

/// A linear layer, `gelu` as `std/tensor.au` builds it, and a second
/// product, over a `[4, 64, 512]` hidden tensor.
fn gelu_between_products(device: TensorDevice, one_sign: bool) -> (Value, Value) {
    let x = wave(256 * 64, &[256, 64], device);
    let weight = wave(64 * 512, &[64, 512], device);
    let bias = wave(512, &[512], device);
    let narrow = wave(64 * 512, &[512, 64], device);
    // Squares make the second product add terms of one sign, so the
    // order of the additions changes less of the result.
    let squared = |value: Value| match one_sign {
        true => binary(&value, &value, 2).unwrap(),
        false => value,
    };
    let (x, weight, narrow) = (squared(x), squared(weight), squared(narrow));
    let scalar = |like: &Value, value: f64| {
        host_tensor_scalar_like(vec![like.clone(), Value::Float(value)], None).unwrap()
    };
    let flat = binary(
        &host_tensor_matmul(vec![x, weight], None).unwrap(),
        &bias,
        0,
    )
    .unwrap();
    let hidden = reshape(&flat, &[4, 64, 512]).unwrap();
    let scaled = binary(&hidden, &scalar(&hidden, 0.001), 2).unwrap();
    let raised = binary(&unary(&scaled, 67).unwrap(), &scalar(&hidden, 1.0), 0).unwrap();
    let halved = binary(&hidden, &scalar(&hidden, 0.5), 2).unwrap();
    let activated = binary(&halved, &raised, 2).unwrap();
    let product =
        host_tensor_matmul(vec![reshape(&activated, &[-1, 512]).unwrap(), narrow], None).unwrap();
    (activated, product)
}

#[test]
fn a_reshape_with_two_readers_is_computed_again_by_each() {
    let expected = array(&gelu_between_products(TensorDevice::Cpu, true).1);
    deferred(|| {
        // The bias sum is read through a reshape by both halves of the
        // `gelu`. One kernel between the two products computes it all.
        let result = gelu_between_products(TensorDevice::Cpu, false).1;
        let (plan, _) = Plan::collect_all(&[node(&result)]);
        let kernels = tensor_codegen::generate(&plan)
            .actions
            .iter()
            .filter(|action| matches!(action, tensor_codegen::Action::Kernel { .. }))
            .count();
        assert_eq!(kernels, 1);
        let result = gelu_between_products(TensorDevice::Cpu, true).1;
        let (plan, storage) = Plan::collect_all(&[node(&result)]);
        if let Some(generated) = generated_kernels::run_all(&plan, &storage, 2301) {
            close("gelu between products", &generated[0], &expected);
        }
    });
}

#[test]
fn a_gpu_device_agrees_on_a_reshape_computed_again() {
    for device in gpus() {
        // The `gelu` is compared, and of the second product only its
        // shape: a library adds a product's terms in its own order, and
        // the sums then differ by more than `close` allows. The first
        // product is exact in any order: its terms are multiples of
        // 1/256 and its sums are below 700.
        let expected = array(&gelu_between_products(TensorDevice::Cpu, false).0);
        let (activated, product) = gelu_between_products(device, false);
        assert_eq!(&*array(&product).shape, &[256, 64]);
        close("gelu after a product", &array(&activated), &expected);
        println!("RAN: a reshape computed again on a GPU device");
    }
}

/// The products of an attention with their heads merged, as
/// `std/tensor.au` writes them: `[2, 3, 8, 4]` products, the two middle
/// axes swapped, then rows of all three heads, into a last product.
fn merged_heads(device: TensorDevice) -> Value {
    let weights = wave(2 * 3 * 8 * 8, &[2, 3, 8, 8], device);
    let values = wave(2 * 3 * 8 * 4, &[2, 3, 8, 4], device);
    let mixed = host_tensor_matmul(vec![weights, values], None).unwrap();
    let merged = reshape(&permute(&mixed, &[0, 2, 1, 3]).unwrap(), &[2, 8, 12]).unwrap();
    let flat = reshape(&merged, &[-1, 12]).unwrap();
    host_tensor_matmul(vec![flat, wave(12 * 5, &[12, 5], device)], None).unwrap()
}

#[test]
fn stacked_products_write_their_merged_heads_in_place() {
    let cpu = TensorDevice::Cpu;
    let expected = array(&merged_heads(cpu));
    deferred(|| {
        let result = merged_heads(cpu);
        let (plan, storage) = Plan::collect_all(&[node(&result)]);
        let actions = tensor_codegen::generate(&plan).actions;
        // No kernel moves the heads: the first product writes the rows
        // the second reads.
        assert!(actions
            .iter()
            .all(|action| !matches!(action, tensor_codegen::Action::Kernel { .. })));
        let spread: Vec<_> = actions
            .iter()
            .filter_map(|action| match action {
                tensor_codegen::Action::Matmul { spread, .. } => Some(spread.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(spread, [Some((12, vec![0, 4, 8, 96, 100, 104])), None]);
        if let Some(generated) = generated_kernels::run_all(&plan, &storage, 2430) {
            close("merged heads", &generated[0], &expected);
        }

        // Heads the program also reads are written as they are.
        let mixed = host_tensor_matmul(
            vec![
                wave(2 * 3 * 8 * 8, &[2, 3, 8, 8], cpu),
                wave(2 * 3 * 8 * 4, &[2, 3, 8, 4], cpu),
            ],
            None,
        )
        .unwrap();
        let merged = reshape(&permute(&mixed, &[0, 2, 1, 3]).unwrap(), &[2, 8, 12]).unwrap();
        let (plan, _) = Plan::collect_all(&[node(&merged), node(&mixed)]);
        assert!(tensor_codegen::generate(&plan)
            .actions
            .iter()
            .all(|action| !matches!(
                action,
                tensor_codegen::Action::Matmul {
                    spread: Some(_),
                    ..
                }
            )));
    });
}

#[test]
fn a_gpu_device_agrees_on_merged_heads() {
    for device in gpus() {
        close(
            "merged heads",
            &array(&merged_heads(device)),
            &array(&merged_heads(TensorDevice::Cpu)),
        );
        println!("RAN: merged heads written in place on a GPU device");
    }
}
