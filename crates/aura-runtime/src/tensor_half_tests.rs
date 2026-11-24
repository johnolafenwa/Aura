//! Tests of `float16` tensors and of products that read the heads of an
//! attention in place.

use super::*;

/// `value` as a `float16` tensor.
fn half(value: &Value) -> Value {
    host_tensor_cast(vec![value.clone(), int(HALF_DTYPE)], None).unwrap()
}

fn halves(values: &[f32], shape: &[usize], device: TensorDevice) -> Value {
    half(&tensor(float32(values, shape), device))
}

fn elements(value: &Value) -> Vec<f32> {
    let elements = array(value);
    (0..elements.len())
        .map(|index| match elements.value_at_flat(index) {
            Value::Float(value) => value as f32,
            other => panic!("expected a float, found {other:?}"),
        })
        .collect()
}

/// How far a `float16` result of the generated kernels may be from the
/// CPU device's: a few half-precision steps, for a sum whose order
/// differs and for the library's functions.
fn close_halves(name: &str, generated: &ArrayValue, host: &ArrayValue) {
    assert_eq!(generated.shape, host.shape, "{name}: shape");
    assert_eq!(generated.dtype(), host.dtype(), "{name}: dtype");
    for index in 0..host.len() {
        match (generated.value_at_flat(index), host.value_at_flat(index)) {
            (Value::Float(generated), Value::Float(host)) => {
                let tolerance = 4.0 / 1024.0 * host.abs().max(1.0 / 64.0);
                assert!(
                    generated == host
                        || (generated.is_nan() && host.is_nan())
                        || (generated - host).abs() <= tolerance,
                    "{name}: element {index} is {generated} from the kernels and {host} on the host"
                );
            }
            (generated, host) => assert_eq!(generated, host, "{name}: element {index}"),
        }
    }
}

#[test]
fn a_half_is_the_nearest_one_to_a_float32() {
    use tensor_kernels::{half_to_single, rounded_to_half, single_to_half};
    // Every half is a float32 exactly, and comes back as itself.
    for bits in 0..=u16::MAX {
        let value = half_to_single(bits);
        if value.is_nan() {
            assert!(half_to_single(single_to_half(value)).is_nan());
        } else {
            assert_eq!(single_to_half(value), bits, "half {bits:#06x}");
        }
    }
    assert_eq!(half_to_single(0x3C00), 1.0);
    assert_eq!(half_to_single(0xC000), -2.0);
    assert_eq!(half_to_single(0x0001), 2.0_f32.powi(-24));
    assert_eq!(half_to_single(0x7BFF), 65504.0);
    assert_eq!(half_to_single(0x7C00), f32::INFINITY);
    assert!(single_to_half(-0.0) == 0x8000 && single_to_half(0.0) == 0);
    // Halfway between two halves goes to the even one.
    let step = 2.0_f32.powi(-10);
    assert_eq!(rounded_to_half(1.0 + step / 2.0), 1.0);
    assert_eq!(rounded_to_half(1.0 + step * 1.5), 1.0 + step * 2.0);
    assert_eq!(rounded_to_half(1.0 + step * 0.75), 1.0 + step);
    assert_eq!(rounded_to_half(2049.0), 2048.0);
    assert_eq!(rounded_to_half(2051.0), 2052.0);
    // Rounding up can carry into the next exponent, and past the largest
    // half into an infinity.
    assert_eq!(rounded_to_half(2.0 - step / 4.0), 2.0);
    assert_eq!(rounded_to_half(65519.0), 65504.0);
    assert_eq!(rounded_to_half(65520.0), f32::INFINITY);
    assert_eq!(rounded_to_half(-1.0e9), f32::NEG_INFINITY);
    // Below the smallest normal half the steps are `2^-24` wide.
    let tiny = 2.0_f32.powi(-24);
    assert_eq!(rounded_to_half(tiny * 2.5), tiny * 2.0);
    assert_eq!(rounded_to_half(tiny * 3.5), tiny * 4.0);
    assert_eq!(rounded_to_half(tiny * 0.5), 0.0);
    assert_eq!(rounded_to_half(tiny * 0.75), tiny);
    assert_eq!(rounded_to_half(tiny / 1024.0), 0.0);
    assert_eq!(rounded_to_half(1.0e-30), 0.0);
    assert_eq!(rounded_to_half(2.0_f32.powi(-14) - tiny / 2.0), 2.0_f32.powi(-14));
    assert!(rounded_to_half(f32::NAN).is_nan());

    let ints = int64(&[1], &[1]);
    assert!(tensor_kernels::round_half(&ints).is_err());
    assert!(tensor_kernels::half_bytes(&ints).is_err());
    let values = float32(&[1.0, -2.0, 0.1], &[3]);
    let bytes = tensor_kernels::half_bytes(&values).unwrap();
    assert_eq!(bytes[..4], [0x00, 0x3C, 0x00, 0xC0]);
    assert_eq!(
        tensor_kernels::from_half_bytes(&[3], &bytes).unwrap(),
        tensor_kernels::round_half(&values).unwrap()
    );
}

#[test]
fn a_float16_tensor_rounds_every_result_to_half_precision() {
    let cpu = TensorDevice::Cpu;
    let a = halves(&[2048.0, 60000.0, 0.1, -3.0], &[4], cpu);
    let b = halves(&[1.0, 60000.0, 0.2, 2.0], &[4], cpu);
    assert!(node(&a).is_half() && node(&a).dtype() == ArrayDType::Float32);
    assert_eq!(node(&a).dtype_name(), "float16");
    assert_eq!(node(&tensor(float32(&[1.0], &[1]), cpu)).dtype_name(), "float32");
    let tenth = tensor_kernels::rounded_to_half(0.1);
    let fifth = tensor_kernels::rounded_to_half(0.2);
    assert_eq!(elements(&a), [2048.0, 60000.0, tenth, -3.0]);
    // A sum is rounded, and one past the largest half is an infinity.
    let sum = binary(&a, &b, 0).unwrap();
    assert!(node(&sum).is_half());
    assert_eq!(
        elements(&sum),
        [
            2048.0,
            f32::INFINITY,
            tensor_kernels::rounded_to_half(tenth + fifth),
            -1.0
        ]
    );
    let quotient = binary(&a, &b, 3).unwrap();
    assert_eq!(elements(&quotient)[2], tensor_kernels::rounded_to_half(tenth / fifth));
    let root = unary(&unary(&a, 0).unwrap(), 1).unwrap();
    assert_eq!(elements(&root)[3], tensor_kernels::rounded_to_half(3.0_f32.sqrt()));
    // A comparison gives a mask, and the place of a largest element an
    // integer.
    let mask = compare(&a, &b, 2).unwrap();
    assert!(!node(&mask).is_half() && node(&mask).dtype() == ArrayDType::Bool);
    let place = node_op(&a, 2, &[0]).unwrap();
    assert!(!node(&place).is_half() && node(&place).dtype() == ArrayDType::Int64);
    assert!(node(&choose(&mask, &a, &b).unwrap()).is_half());
    // Reductions, reshapes and products keep the dtype.
    let total = reduce(&halves(&[0.1; 64], &[64], cpu), 0, None).unwrap();
    assert!(node(&total).is_half());
    assert_eq!(elements(&total), [tensor_kernels::rounded_to_half(tenth * 64.0)]);
    let grid = reshape(&a, &[2, 2]).unwrap();
    let squared = matmul(&grid, &grid).unwrap();
    assert!(node(&grid).is_half() && node(&squared).is_half());
    assert!(elements(&squared)[1].is_infinite());

    // The conversions: to itself, to wider floats exactly, and from
    // integers.
    assert!(std::ptr::eq(node(&half(&a)).0.as_ref(), node(&a).0.as_ref()));
    let single = cast(&a, ArrayDType::Float32).unwrap();
    assert!(!node(&single).is_half());
    assert_eq!(elements(&single), elements(&a));
    let double = cast(&a, ArrayDType::Float64).unwrap();
    assert_eq!(node(&double).dtype(), ArrayDType::Float64);
    let counted = half(&tensor(int64(&[3, 2049], &[2]), cpu));
    assert_eq!(elements(&counted), [3.0, 2048.0]);
    assert_eq!(
        array(&cast(&halves(&[2.5, -7.0], &[2], cpu), ArrayDType::Int64).unwrap()),
        int64(&[2, -7], &[2])
    );
    assert!(host_tensor_cast(vec![a.clone(), int(9)], None).is_err());

    // A float16 tensor and a float32 one do not mix.
    let plain = tensor(float32(&[1.0, 2.0, 3.0, 4.0], &[4]), cpu);
    for mixed in [
        binary(&a, &plain, 0),
        compare(&plain, &a, 0),
        choose(&mask, &a, &plain),
        matmul(&grid, &reshape(&plain, &[2, 2]).unwrap()),
        pair_op(&a, &plain, 0, &[0]),
    ] {
        let message = mixed.unwrap_err().message;
        assert!(
            message.contains("one dtype"),
            "a mixed operation reports its dtypes: {message}"
        );
    }
    assert!(binary(&a, &plain, 0)
        .unwrap_err()
        .message
        .contains("found float16 and float32"));
}

#[test]
fn float16_tensors_are_made_filled_moved_and_kept() {
    let cpu = TensorDevice::Cpu;
    let full = host_tensor_full(
        vec![ints(&[2, 2]), Value::Float(0.1), int(HALF_DTYPE), int(0)],
        None,
    )
    .unwrap();
    assert!(node(&full).is_half());
    assert_eq!(elements(&full), [tensor_kernels::rounded_to_half(0.1); 4]);
    assert!(host_tensor_full(vec![ints(&[1]), Value::Float(0.1), int(8), int(0)], None).is_err());
    // A number beside a float16 tensor is one.
    let number = host_tensor_scalar_like(vec![full.clone(), Value::Float(0.3)], None).unwrap();
    assert!(node(&number).is_half());
    assert_eq!(elements(&number), [tensor_kernels::rounded_to_half(0.3)]);
    let whole = host_tensor_scalar_like(vec![full.clone(), int(7)], None).unwrap();
    assert!(node(&whole).is_half());
    assert!(node(&binary(&full, &number, 2).unwrap()).is_half());
    // Random halves are the float32 ones, rounded.
    let random = |dtype: i64| {
        host_tensor_filled(vec![ints(&[5]), int(0), int(11), int(dtype), int(0)], None).unwrap()
    };
    let (drawn, wide) = (random(HALF_DTYPE), random(2));
    assert!(node(&drawn).is_half() && !node(&wide).is_half());
    let rounded: Vec<f32> = elements(&wide)
        .into_iter()
        .map(tensor_kernels::rounded_to_half)
        .collect();
    assert_eq!(elements(&drawn), rounded);
    assert!(host_tensor_filled(vec![ints(&[5]), int(0), int(11), int(7), int(0)], None).is_err());
    // The same device gives the same tensor, and another device a
    // float16 one.
    let moved = host_tensor_to_device(vec![full.clone(), int(0)], None).unwrap();
    assert!(node(&moved).is_half());
    // Watching, detaching and holding in a parameter keep the dtype.
    let watched = node_op(&full, 4, &[]).unwrap();
    assert!(node(&watched).is_half() && node(&watched).is_tracked());
    let doubled = binary(&watched, &watched, 0).unwrap();
    assert!(node(&node_op(&doubled, 5, &[]).unwrap()).is_half());
    let parameter = node_op(&full, 6, &[]).unwrap();
    assert!(node(&parameter).is_half());
    assert!(node(&node_op(&parameter, 7, &[]).unwrap()).is_half());
    // A parameter of float16 does not take a float32 tensor.
    let plain = tensor(float32(&[1.0; 4], &[2, 2]), cpu);
    assert!(pair_op(&parameter, &plain, 4, &[]).is_err());
    pair_op(&parameter, &binary(&full, &full, 0).unwrap(), 4, &[]).unwrap();
    deferred(|| {
        let waiting = binary(&full, &full, 0).unwrap();
        assert!(!node(&waiting).is_ready());
        assert!(node(&node_op(&node_op(&waiting, 4, &[]).unwrap(), 5, &[]).unwrap()).is_half());
        pair_op(&parameter, &binary(&full, &full, 0).unwrap(), 4, &[]).unwrap();
        assert!(node(&node_op(&parameter, 7, &[]).unwrap()).is_half());
    });
}

#[test]
fn a_gradient_of_float16_tensors_is_float16() {
    let x = node_op(&halves(&[1.0, -2.0, 0.5], &[3], TensorDevice::Cpu), 4, &[]).unwrap();
    let squares = reduce(&binary(&x, &x, 2).unwrap(), 0, None).unwrap();
    let slope = pair_op(&squares, &x, 3, &[]).unwrap();
    assert!(node(&slope).is_half());
    assert_eq!(elements(&slope), [2.0, -4.0, 1.0]);
    // A tensor the result does not depend on has a gradient of zeros of
    // its dtype.
    let other = node_op(&halves(&[1.0], &[1], TensorDevice::Cpu), 4, &[]).unwrap();
    let none = pair_op(&squares, &other, 3, &[]).unwrap();
    assert!(node(&none).is_half());
    assert_eq!(elements(&none), [0.0]);
}

#[test]
fn float16_tensors_are_saved_as_halves_and_loaded_from_any_float() {
    let directory = std::env::temp_dir().join(format!("aura-tensor-halves-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("halves.safetensors");
    let path_text = text(path.to_str().unwrap());
    let cpu = TensorDevice::Cpu;
    let small = halves(&[0.1, -2.0, 65504.0], &[3], cpu);
    let wide = tensor(float32(&[0.1, 100000.0], &[2]), cpu);
    for (name, value) in [("small", &small), ("wide", &wide)] {
        host_tensor_weights_put(
            vec![path_text.clone(), text(name), value.clone()],
            None,
        )
        .unwrap();
    }
    outcome(host_tensor_weights_write(vec![path_text.clone()], None).unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let header = String::from_utf8_lossy(&bytes[8..]).to_string();
    assert!(header.contains(r#""dtype":"F16""#) && header.contains(r#""data_offsets":[0,6]"#));
    assert!(header.contains(r#""dtype":"F32""#));
    let load = |name: &str, dtype: i64| {
        outcome(
            host_tensor_weights_load(
                vec![path_text.clone(), text(name), int(dtype), int(0)],
                None,
            )
            .unwrap(),
        )
        .unwrap()
    };
    let unwrap = |value: Value| value;
    let again = unwrap(load("small", HALF_DTYPE));
    assert!(node(&again).is_half());
    assert_eq!(elements(&again), elements(&small));
    // A float32 tensor loads as float16, rounded.
    let narrowed = unwrap(load("wide", HALF_DTYPE));
    assert!(node(&narrowed).is_half());
    assert_eq!(
        elements(&narrowed),
        [tensor_kernels::rounded_to_half(0.1), f32::INFINITY]
    );
    // And a float16 one as float32.
    let widened = unwrap(load("small", 2));
    assert!(!node(&widened).is_half());
    assert_eq!(elements(&widened), elements(&small));
    std::fs::remove_dir_all(&directory).unwrap();
}

/// The leaves of the planned expressions, with the float ones as
/// `float16`.
fn half_leaves(device: TensorDevice) -> Leaves {
    let l = leaves(device);
    Leaves {
        a: half(&l.a),
        b: half(&l.b),
        row: half(&l.row),
        tall: half(&l.tall),
        two: half(&l.two),
        ..l
    }
}

/// The planned expressions that keep `float16` operands apart from
/// `float32` ones.
fn half_expressions() -> impl Iterator<Item = &'static (&'static str, Expression)> {
    PLANNED
        .iter()
        .chain(INDEXED)
        .filter(|(_, expression)| expression(&half_leaves(TensorDevice::Cpu)).is_ok())
}

#[test]
fn a_plan_of_float16_tensors_gives_what_each_operation_gives_at_once() {
    let mut halves = 0;
    for (name, expression) in half_expressions() {
        let eager = expression(&half_leaves(TensorDevice::Cpu)).unwrap();
        let waiting = deferred(|| expression(&half_leaves(TensorDevice::Cpu)).unwrap());
        assert_eq!(
            node(&waiting).is_half(),
            node(&eager).is_half(),
            "{name}: dtype"
        );
        assert_eq!(node(&waiting).shape(), node(&eager).shape(), "{name}: shape");
        halves += usize::from(node(&eager).is_half());
        close(name, &array(&waiting), &array(&eager));
    }
    assert!(halves > 30, "the expressions cover float16: {halves}");
}

#[test]
fn the_generated_kernels_of_float16_tensors_agree_with_the_cpu_device() {
    deferred(|| {
        for (tag, (name, expression)) in half_expressions().enumerate() {
            let waiting = expression(&half_leaves(TensorDevice::Cpu)).unwrap();
            if node(&waiting).is_ready() {
                // An operation no GPU library runs has no plan.
                continue;
            }
            let (plan, storage) = Plan::collect(node(&waiting));
            let Some(generated) = generated_kernels::run(&plan, &storage, 5000 + tag) else {
                eprintln!("SKIPPED: this machine has no C++ compiler");
                return;
            };
            close_halves(name, &generated, &array(&waiting));
        }
    });
}

/// A softmax over the rows of a `[rows, length]` tensor of `float16`.
fn half_softmax(rows: usize, length: usize) -> Value {
    let x = half(&wave(rows * length, &[rows, length], TensorDevice::Cpu));
    row_softmax(&x, rows as i64)
}

/// A layer norm over the rows of a `[rows, length]` tensor of `float16`.
fn half_norm(rows: usize, length: usize) -> Value {
    let cpu = TensorDevice::Cpu;
    let x = half(&wave(rows * length, &[rows, length], cpu));
    let (weight, bias) = (
        half(&wave(length, &[length], cpu)),
        half(&wave(length, &[length], cpu)),
    );
    lane_norm(&x, &[rows as i64, 1], &weight, &bias)
}

#[test]
fn float16_lanes_and_long_reductions_add_in_float32() {
    let cpu = TensorDevice::Cpu;
    type Made = fn() -> Value;
    let cases: [(&str, Made); 7] = [
        ("softmax of short lanes", || half_softmax(3, 96)),
        ("softmax of long lanes", || half_softmax(2, 400)),
        ("layer norm", || half_norm(3, 128)),
        ("whole sum in stages", || {
            let wide = half(&wave(20_000, &[100, 200], TensorDevice::Cpu));
            reduce(&binary(&wide, &wide, 2).unwrap(), 3, None).unwrap()
        }),
        ("split lane sum", || {
            reduce(&half(&wave(4 * 300, &[4, 300], TensorDevice::Cpu)), 0, Some(1)).unwrap()
        }),
        ("split lane argmax", || {
            node_op(&half(&wave(4 * 300, &[4, 300], TensorDevice::Cpu)), 2, &[1]).unwrap()
        }),
        ("stacked product", || {
            let left = half(&wave(2 * 3 * 4, &[2, 3, 4], TensorDevice::Cpu));
            let right = half(&wave(2 * 4 * 5, &[2, 4, 5], TensorDevice::Cpu));
            matmul(&left, &right).unwrap()
        }),
    ];
    let _ = cpu;
    for (tag, (name, made)) in cases.iter().enumerate() {
        let eager = array(&made());
        deferred(|| {
            let waiting = made();
            assert!(!node(&waiting).is_ready(), "{name} waits");
            let (plan, storage) = Plan::collect(node(&waiting));
            let program = tensor_codegen::generate(&plan);
            assert!(program.source.contains("struct aura_h"), "{name}");
            if let Some(generated) = generated_kernels::run(&plan, &storage, 5200 + tag) {
                close_halves(name, &generated, &eager);
            }
            close_halves(name, &array(&waiting), &eager);
        });
    }
    // A plan with no float16 value does not carry the type.
    deferred(|| {
        let l = leaves(cpu);
        let (plan, _) = Plan::collect(node(&binary(&l.a, &l.b, 0).unwrap()));
        assert!(!tensor_codegen::generate(&plan).source.contains("aura_h"));
    });
}

#[test]
fn a_gpu_device_agrees_on_float16_tensors() {
    for device in gpus() {
        let mut planned = 0;
        for (name, expression) in half_expressions() {
            let host = expression(&half_leaves(TensorDevice::Cpu)).unwrap();
            let gpu = expression(&half_leaves(device)).unwrap();
            assert_eq!(node(&gpu).is_half(), node(&host).is_half(), "{name}: dtype");
            planned += usize::from(!node(&gpu).is_ready());
            close_halves(name, &array(&gpu), &array(&host));
        }
        type Made = fn(TensorDevice) -> Value;
        let large: [(&str, Made); 4] = [
            ("softmax", |device| {
                let x = half(&wave(64 * 96, &[64, 96], device));
                row_softmax(&x, 64)
            }),
            ("long softmax", |device| {
                let x = half(&wave(8 * 400, &[8, 400], device));
                row_softmax(&x, 8)
            }),
            ("stacked product", |device| {
                let left = half(&wave(6 * 32 * 48, &[6, 32, 48], device));
                let right = half(&wave(6 * 48 * 16, &[6, 48, 16], device));
                let scale = host_tensor_scalar_like(vec![left.clone(), Value::Float(0.01)], None)
                    .unwrap();
                matmul(&binary(&left, &scale, 2).unwrap(), &right).unwrap()
            }),
            ("mean of many", |device| {
                reduce(&half(&wave(1 << 16, &[256, 256], device)), 3, None).unwrap()
            }),
        ];
        for (name, made) in large {
            close_halves(name, &array(&made(device)), &array(&made(TensorDevice::Cpu)));
        }
        // A large float16 tensor lives on the device as halves and comes
        // back as itself.
        let values = half(&wave(1 << 14, &[1 << 14], TensorDevice::Cpu));
        let placed = host_tensor_to_device(vec![values.clone(), int(device as i64)], None).unwrap();
        assert!(node(&placed).is_half());
        assert_eq!(array(&placed), array(&values));
        println!("RAN: {planned} float16 expressions on the {} device", device.name());
    }
}

/// A self-attention over `[2, 8, 12]` rows of three heads, as
/// `std/tensor.au` builds it: three linear layers, their heads split,
/// the scores, a function of them, the mix of the values, and the heads
/// merged into a last linear layer.
fn attention(device: TensorDevice, narrow: bool) -> Value {
    let dtype = |value: Value| if narrow { half(&value) } else { value };
    let x = dtype(wave(2 * 8 * 12, &[16, 12], device));
    let scale = host_tensor_scalar_like(vec![x.clone(), Value::Float(0.03125)], None).unwrap();
    let linear = |seed: usize| {
        let values: Vec<f32> = (0..144)
            .map(|at| (((at + seed) * 29 % 53) as f32 - 26.0) / 64.0)
            .collect();
        let weight = dtype(tensor(float32(&values, &[12, 12]), device));
        let bias = dtype(tensor(float32(&values[..12], &[12]), device));
        let flat = binary(&matmul(&x, &weight).unwrap(), &bias, 0).unwrap();
        reshape(&flat, &[2, 8, 12]).unwrap()
    };
    let split = |rows: &Value| {
        permute(&reshape(rows, &[2, 8, 3, 4]).unwrap(), &[0, 2, 1, 3]).unwrap()
    };
    let (query, key, value) = (split(&linear(1)), split(&linear(2)), split(&linear(3)));
    let scores = matmul(&query, &permute(&key, &[0, 1, 3, 2]).unwrap()).unwrap();
    let weights = unary(&binary(&scores, &scale, 2).unwrap(), TANH_OPERATION).unwrap();
    let mixed = matmul(&weights, &value).unwrap();
    let merged = reshape(&permute(&mixed, &[0, 2, 1, 3]).unwrap(), &[16, 12]).unwrap();
    let out: Vec<f32> = (0..60).map(|at| ((at * 7 % 23) as f32 - 11.0) / 32.0).collect();
    matmul(&merged, &dtype(tensor(float32(&out, &[12, 5]), device))).unwrap()
}

#[test]
fn products_read_the_heads_of_an_attention_in_place() {
    use tensor_codegen::Action;
    assert!(tensor_codegen::heads_in_place());
    let cpu = TensorDevice::Cpu;
    for (tag, narrow) in [false, true].into_iter().enumerate() {
        let expected = array(&attention(cpu, narrow));
        deferred(|| {
            let result = attention(cpu, narrow);
            let (plan, storage) = Plan::collect_all(&[node(&result)]);
            let program = tensor_codegen::generate(&plan);
            // The kernels are the three that add a bias and the one of
            // the scores, and for `float16` the three that convert the
            // leaves. None writes the heads apart.
            let kernels = program
                .actions
                .iter()
                .filter(|action| matches!(action, Action::Kernel { .. }))
                .count();
            assert_eq!(kernels, if narrow { 7 } else { 4 }, "{:?}", program.actions);
            let read: Vec<_> = program
                .actions
                .iter()
                .filter_map(|action| match action {
                    Action::Matmul {
                        wide,
                        swapped,
                        half,
                        pairs,
                        ..
                    } => {
                        assert_eq!(*half, narrow);
                        Some((*wide, *swapped, pairs.len()))
                    }
                    _ => None,
                })
                .collect();
            let whole = ((None, None), (false, false), 1);
            assert_eq!(
                read,
                [
                    whole,
                    whole,
                    // The scores: the queries and the transposed keys.
                    ((Some(12), Some(12)), (false, true), 6),
                    whole,
                    // The mix: the weights, and the values in place.
                    ((None, Some(12)), (false, false), 6),
                    whole,
                ]
            );
            let Some(Action::Matmul { pairs, .. }) = program
                .actions
                .iter()
                .find(|action| action.is_tabled())
            else {
                panic!("the scores take the address of each head")
            };
            // Head `h` of stack `b` starts four columns after the head
            // before it, in the rows of its stack.
            assert_eq!(
                pairs[..],
                [(0, 0), (4, 4), (8, 8), (96, 96), (100, 100), (104, 104)]
            );
            assert!(program.source.contains(tensor_codegen::ADDRESSES_KERNEL));
            if let Some(generated) = generated_kernels::run_all(&plan, &storage, 5400 + tag) {
                if narrow {
                    close_halves("attention", &generated[0], &expected);
                } else {
                    close("attention", &generated[0], &expected);
                }
            }
        });
    }

    deferred(|| {
        // A split the program also reads, or that arithmetic reads, is
        // written as it is.
        let rows = wave(2 * 8 * 12, &[2, 8, 12], cpu);
        let doubled = binary(&rows, &rows, 0).unwrap();
        let heads = permute(&reshape(&doubled, &[2, 8, 3, 4]).unwrap(), &[0, 2, 1, 3]).unwrap();
        let turned = permute(&heads, &[0, 1, 3, 2]).unwrap();
        let scores = matmul(&heads, &turned).unwrap();
        let wide = |roots: &[&Value]| {
            let nodes: Vec<&TensorValue> = roots.iter().map(|root| node(root)).collect();
            let (plan, _) = Plan::collect_all(&nodes);
            tensor_codegen::generate(&plan)
                .actions
                .iter()
                .find_map(|action| match action {
                    Action::Matmul { wide, .. } => Some(*wide),
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(wide(&[&scores]), (Some(12), Some(12)));
        assert_eq!(wide(&[&scores, &heads]), (None, None));
        assert_eq!(wide(&[&scores, &turned]), (None, None));
        let halved = binary(&heads, &heads, 0).unwrap();
        assert_eq!(wide(&[&scores, &halved]), (None, None));
        // A transpose that arithmetic reads is written, and the split
        // with it.
        let shifted = binary(&turned, &turned, 0).unwrap();
        assert_eq!(wide(&[&scores, &shifted]), (None, None));
        // Rows that something else reads exist already, and are read in
        // place all the same.
        assert_eq!(wide(&[&scores, &doubled]), (Some(12), Some(12)));
        // Rows of a leaf need no kernel at all.
        let leaf = permute(&reshape(&rows, &[2, 8, 3, 4]).unwrap(), &[0, 2, 1, 3]).unwrap();
        let product = matmul(&leaf, &permute(&leaf, &[0, 1, 3, 2]).unwrap()).unwrap();
        let (plan, storage) = Plan::collect(node(&product));
        let program = tensor_codegen::generate(&plan);
        assert!(
            !program
                .actions
                .iter()
                .any(|action| matches!(action, Action::Kernel { .. })),
            "{:?}",
            program.actions
        );
        let eager = {
            DEFER_ON_CPU.with(|flag| flag.set(false));
            let rows = wave(2 * 8 * 12, &[2, 8, 12], cpu);
            let leaf = permute(&reshape(&rows, &[2, 8, 3, 4]).unwrap(), &[0, 2, 1, 3]).unwrap();
            let made = matmul(&leaf, &permute(&leaf, &[0, 1, 3, 2]).unwrap()).unwrap();
            DEFER_ON_CPU.with(|flag| flag.set(true));
            array(&made)
        };
        if let Some(generated) = generated_kernels::run(&plan, &storage, 5410) {
            close("heads of a leaf", &generated, &eager);
        }
    });
}

#[test]
fn an_early_start_writes_the_rows_a_product_reads_heads_from() {
    let cpu = TensorDevice::Cpu;
    deferred(|| {
        let mut rows = wave(2 * 8 * 12, &[2, 8, 12], cpu);
        for _ in 0..EARLY_DEPTH {
            rows = binary(&rows, &wave(2 * 8 * 12, &[2, 8, 12], cpu), 0).unwrap();
        }
        let shaped = reshape(&rows, &[2, 8, 3, 4]).unwrap();
        let heads = permute(&shaped, &[0, 2, 1, 3]).unwrap();
        let turned = permute(&heads, &[0, 1, 3, 2]).unwrap();
        let scores = matmul(&heads, &turned).unwrap();
        // The rows ran when the product was made. The split and the
        // transpose wait: the product reads through them.
        assert!(node(&shaped).is_ready());
        assert!(!node(&heads).is_ready() && !node(&turned).is_ready());
        assert!(!node(&scores).is_ready());
        let (plan, _) = Plan::collect(node(&scores));
        assert!(matches!(
            tensor_codegen::generate(&plan).actions[..],
            [tensor_codegen::Action::Matmul {
                wide: (Some(12), Some(12)),
                ..
            }]
        ));
        assert_eq!(array(&scores).shape.as_ref(), &[2, 3, 8, 8]);
    });
}

#[test]
fn a_gpu_device_agrees_on_heads_read_in_place() {
    for device in gpus() {
        for narrow in [false, true] {
            let (gpu, host) = (
                array(&attention(device, narrow)),
                array(&attention(TensorDevice::Cpu, narrow)),
            );
            if narrow {
                close_halves("attention", &gpu, &host);
            } else {
                close("attention", &gpu, &host);
            }
        }
        println!("RAN: heads read in place on the {} device", device.name());
    }
}
