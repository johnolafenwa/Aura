use crate::array_views::ArrayAxisSelection::{self, Index, Slice};
use crate::integer::IntegerValue;
use crate::runtime_value::{ArrayBinaryOp, ArrayStorage, ArrayValue, IntegerArithmeticMode, Value};

const CHECKED: IntegerArithmeticMode = IntegerArithmeticMode::Checked;

fn ints(shape: &[usize], values: impl IntoIterator<Item = i64>) -> ArrayValue {
    ArrayValue::new(
        shape.into(),
        ArrayStorage::Int64(values.into_iter().collect()),
    )
    .unwrap()
}

fn floats(shape: &[usize], values: impl IntoIterator<Item = f64>) -> ArrayValue {
    ArrayValue::new(
        shape.into(),
        ArrayStorage::Float64(values.into_iter().collect()),
    )
    .unwrap()
}

fn all() -> ArrayAxisSelection {
    Slice {
        start: None,
        end: None,
        step: 1,
    }
}

fn stepped(step: i64) -> ArrayAxisSelection {
    Slice {
        start: None,
        end: None,
        step,
    }
}

/// The Array a copy of `array` in row-major order would be.
fn packed(array: &ArrayValue) -> ArrayValue {
    array.detached().unwrap()
}

#[test]
fn shapes_broadcast_from_the_last_axis() {
    let grid = ints(&[2, 3], 0..6);
    let row = ints(&[3], [10, 20, 30]);
    let column = ints(&[2, 1], [100, 200]);

    let sum = grid.binary(&row, ArrayBinaryOp::Add, CHECKED).unwrap();
    assert_eq!(sum, ints(&[2, 3], [10, 21, 32, 13, 24, 35]));
    let sum = row.binary(&grid, ArrayBinaryOp::Sub, CHECKED).unwrap();
    assert_eq!(sum, ints(&[2, 3], [10, 19, 28, 7, 16, 25]));
    let sum = grid.binary(&column, ArrayBinaryOp::Add, CHECKED).unwrap();
    assert_eq!(sum, ints(&[2, 3], [100, 101, 102, 203, 204, 205]));
    // A column and a row make a table.
    let table = column.binary(&row, ArrayBinaryOp::Mul, CHECKED).unwrap();
    assert_eq!(table, ints(&[2, 3], [1000, 2000, 3000, 2000, 4000, 6000]));
    // One element broadcasts to any shape.
    let one = ints(&[1], [7]);
    let sum = one.binary(&grid, ArrayBinaryOp::Add, CHECKED).unwrap();
    assert_eq!(sum, ints(&[2, 3], 7..13));
    // An empty axis stays empty.
    let empty = ints(&[0, 3], []);
    let sum = empty.binary(&row, ArrayBinaryOp::Add, CHECKED).unwrap();
    assert_eq!(sum, ints(&[0, 3], []));
}

#[test]
fn operands_that_do_not_broadcast_or_differ_in_dtype_are_refused() {
    let grid = ints(&[2, 3], 0..6);
    for other in [
        ints(&[2], [1, 2]),
        ints(&[3, 2], 0..6),
        ints(&[2, 2, 3], 0..12)
            .select(&[Index(0), all(), stepped(2)])
            .unwrap(),
    ] {
        let error = grid
            .binary(&other, ArrayBinaryOp::Add, CHECKED)
            .unwrap_err();
        assert_eq!(error.code, "AU4007");
        assert!(
            error
                .message
                .starts_with("array operands require the same dtype and shapes that broadcast"),
            "{}",
            error.message
        );
    }
    let error = grid
        .binary(&floats(&[2, 3], [0.0; 6]), ArrayBinaryOp::Add, CHECKED)
        .unwrap_err();
    assert_eq!(error.code, "AU4007");
}

#[test]
fn strided_operands_give_the_elements_of_their_copies() {
    let cube = ints(&[3, 4, 5], 0..60);
    let views = [
        cube.permute(&[2, 0, 1]).unwrap(),
        cube.select(&[all(), stepped(-1)])
            .unwrap()
            .permute(&[2, 0, 1])
            .unwrap(),
        cube.permute(&[2, 0, 1])
            .unwrap()
            .select(&[stepped(-2), all(), stepped(1)])
            .unwrap(),
        cube.permute(&[2, 0, 1])
            .unwrap()
            .select(&[stepped(2)])
            .unwrap(),
    ];
    let operations = [ArrayBinaryOp::Add, ArrayBinaryOp::Sub, ArrayBinaryOp::Mul];
    for left in &views {
        for right in &views {
            if left.shape != right.shape {
                continue;
            }
            for operation in operations {
                let expected = packed(left)
                    .binary(&packed(right), operation, CHECKED)
                    .unwrap();
                assert_eq!(left.binary(right, operation, CHECKED).unwrap(), expected);
                assert_eq!(
                    left.binary(&packed(right), operation, CHECKED).unwrap(),
                    expected
                );
                assert_eq!(
                    packed(left).binary(right, operation, CHECKED).unwrap(),
                    expected
                );
            }
        }
        for scalar_left in [false, true] {
            for operation in operations {
                let scalar = Value::Int(IntegerValue::from_i64(3));
                let expected = packed(left)
                    .scalar_binary(&scalar, scalar_left, operation, CHECKED)
                    .unwrap();
                assert_eq!(
                    left.scalar_binary(&scalar, scalar_left, operation, CHECKED)
                        .unwrap(),
                    expected
                );
            }
        }
    }
}

#[test]
fn every_dtype_runs_through_the_strided_walk() {
    let storages = [
        ArrayStorage::Int32((1..7).collect()),
        ArrayStorage::Int64((1..7).collect()),
        ArrayStorage::Float32((1..7).map(|value| value as f32).collect()),
        ArrayStorage::Float64((1..7).map(f64::from).collect()),
    ];
    for storage in storages {
        let float = matches!(storage, ArrayStorage::Float32(_) | ArrayStorage::Float64(_));
        let grid = ArrayValue::new([2, 3].into(), storage).unwrap();
        let turned = grid.transpose().unwrap();
        let scalar = grid.get(&[0, 1]).unwrap();
        let mut operations = vec![ArrayBinaryOp::Add, ArrayBinaryOp::Sub, ArrayBinaryOp::Mul];
        if float {
            operations.push(ArrayBinaryOp::Div);
        }
        for operation in operations {
            let expected = packed(&turned)
                .binary(&packed(&turned), operation, CHECKED)
                .unwrap();
            assert_eq!(
                turned.binary(&turned, operation, CHECKED).unwrap(),
                expected
            );
            for scalar_left in [false, true] {
                let expected = packed(&turned)
                    .scalar_binary(&scalar, scalar_left, operation, CHECKED)
                    .unwrap();
                assert_eq!(
                    turned
                        .scalar_binary(&scalar, scalar_left, operation, CHECKED)
                        .unwrap(),
                    expected
                );
            }
        }
    }
}

#[test]
fn a_fault_reports_the_row_major_index_of_the_result() {
    // Overflow at result element [1, 0], flat index 2, read from a
    // transposed operand.
    let grid = ints(&[2, 2], [1, 2, i64::MAX, 4]).transpose().unwrap();
    let ones = ints(&[2], [1, 1]);
    let error = grid.binary(&ones, ArrayBinaryOp::Add, CHECKED).unwrap_err();
    assert_eq!(error.code, "AU4002");
    assert_eq!(
        error.message,
        packed(&grid)
            .binary(&ints(&[2, 2], [1; 4]), ArrayBinaryOp::Add, CHECKED)
            .unwrap_err()
            .message
    );
    let wrapped = grid
        .binary(&ones, ArrayBinaryOp::Add, IntegerArithmeticMode::Wrapping)
        .unwrap();
    assert_eq!(wrapped, ints(&[2, 2], [2, i64::MIN, 3, 5]));

    // A zero divisor in a broadcast row, and as a scalar.
    let values = floats(&[2, 3], [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    let divisors = floats(&[3], [1.0, 2.0, 0.0]);
    let error = values
        .binary(&divisors, ArrayBinaryOp::Div, CHECKED)
        .unwrap_err();
    assert_eq!(error.code, "AU4004");
    assert_eq!(
        error.message,
        "array division has a zero divisor at flat index 2"
    );
    let column = values.select(&[all(), Index(1)]).unwrap();
    let error = column
        .scalar_binary(&Value::Float(0.0), false, ArrayBinaryOp::Div, CHECKED)
        .unwrap_err();
    assert_eq!(error.code, "AU4004");
    let error = Value::Float(1.0);
    let error = floats(&[2, 2], [1.0, 2.0, 0.0, 4.0])
        .transpose()
        .unwrap()
        .scalar_binary(&error, true, ArrayBinaryOp::Div, CHECKED)
        .unwrap_err();
    assert_eq!(
        error.message,
        "array division has a zero divisor at flat index 1"
    );

    // An integer Array has no division, and a float Array no wrapping.
    let error = grid.binary(&ones, ArrayBinaryOp::Div, CHECKED).unwrap_err();
    assert_eq!(error.code, "AU4001");
    let error = values
        .binary(
            &divisors,
            ArrayBinaryOp::Add,
            IntegerArithmeticMode::Wrapping,
        )
        .unwrap_err();
    assert_eq!(error.code, "AU4001");
}

fn base_address(array: &ArrayValue) -> *const f64 {
    match array.storage() {
        ArrayStorage::Float64(values) => values.as_ptr(),
        other => panic!("expected float64 storage, found {other:?}"),
    }
}

#[test]
fn a_temporary_lends_its_buffer_to_the_result() {
    let operations = [
        ArrayBinaryOp::Add,
        ArrayBinaryOp::Sub,
        ArrayBinaryOp::Mul,
        ArrayBinaryOp::Div,
    ];
    let other = floats(&[2, 3], [2.0, 4.0, 8.0, 16.0, 32.0, 64.0]);
    for operation in operations {
        for temporary_left in [true, false] {
            let plain = floats(&[2, 3], (1..7).map(f64::from));
            // Against an Array of the same shape.
            let mut temporary = packed(&plain);
            let address = base_address(&temporary);
            let result = temporary
                .binary_reusing(
                    &Value::Array(other.clone()),
                    temporary_left,
                    operation,
                    CHECKED,
                )
                .expect("a private block of the same shape is reused")
                .unwrap();
            let expected = if temporary_left {
                plain.binary(&other, operation, CHECKED)
            } else {
                other.binary(&plain, operation, CHECKED)
            }
            .unwrap();
            assert_eq!(result, expected);
            assert_eq!(base_address(&result), address);
            assert_eq!(temporary.len(), 0, "the temporary gave its buffer away");

            // Against a scalar.
            let mut temporary = packed(&plain);
            let address = base_address(&temporary);
            let result = temporary
                .binary_reusing(&Value::Float(4.0), temporary_left, operation, CHECKED)
                .unwrap()
                .unwrap();
            let expected = plain
                .scalar_binary(&Value::Float(4.0), !temporary_left, operation, CHECKED)
                .unwrap();
            assert_eq!(result, expected);
            assert_eq!(base_address(&result), address);
        }
    }

    // More elements than one block of the in-place kernel.
    let long = floats(&[3000], (0..3000).map(f64::from));
    let mut temporary = packed(&long);
    let doubled = temporary
        .binary_reusing(
            &Value::Array(long.clone()),
            true,
            ArrayBinaryOp::Add,
            CHECKED,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        doubled,
        long.binary(&long, ArrayBinaryOp::Add, CHECKED).unwrap()
    );

    // Integer Arrays reuse their buffer too, and report a fault as the
    // allocating kernel does.
    let mut temporary = ints(&[3], [1, 2, 3]);
    let result = temporary
        .binary_reusing(
            &Value::Int(IntegerValue::from_i64(10)),
            false,
            ArrayBinaryOp::Sub,
            CHECKED,
        )
        .unwrap()
        .unwrap();
    assert_eq!(result, ints(&[3], [9, 8, 7]));
    let mut temporary = ints(&[3], [1, i64::MAX, 3]);
    let error = temporary
        .binary_reusing(
            &Value::Array(ints(&[3], [1, 1, 1])),
            true,
            ArrayBinaryOp::Add,
            CHECKED,
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, "AU4002");
    assert_eq!(error.message, "array addition overflowed at flat index 1");
    for storage in [
        ArrayStorage::Int32([1, 2].into()),
        ArrayStorage::Float32([1.0, 2.0].into()),
    ] {
        let scalar = match &storage {
            ArrayStorage::Int32(_) => Value::Int(IntegerValue::from_i32(3)),
            _ => Value::Float(3.0),
        };
        let plain = ArrayValue::new([2].into(), storage).unwrap();
        let mut temporary = packed(&plain);
        let result = temporary
            .binary_reusing(
                &Value::Array(plain.clone()),
                true,
                ArrayBinaryOp::Mul,
                CHECKED,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            result,
            plain.binary(&plain, ArrayBinaryOp::Mul, CHECKED).unwrap()
        );
        let mut temporary = packed(&plain);
        let result = temporary
            .binary_reusing(&scalar, true, ArrayBinaryOp::Add, CHECKED)
            .unwrap()
            .unwrap();
        assert_eq!(
            result,
            plain
                .scalar_binary(&scalar, false, ArrayBinaryOp::Add, CHECKED)
                .unwrap()
        );
    }
}

#[test]
fn a_temporary_that_cannot_hold_the_result_is_left_alone() {
    let plain = floats(&[2, 3], (1..7).map(f64::from));
    // Another Array reads the buffer.
    let mut shared = plain.clone();
    assert!(shared
        .binary_reusing(&Value::Float(2.0), true, ArrayBinaryOp::Mul, CHECKED)
        .is_none());
    // A strided view, a shape to broadcast, a strided operand, a dtype.
    let mut turned = packed(&plain).transpose().unwrap();
    assert!(turned
        .binary_reusing(&Value::Float(2.0), true, ArrayBinaryOp::Mul, CHECKED)
        .is_none());
    for other in [
        floats(&[3], [1.0, 2.0, 3.0]),
        floats(&[3, 2], (1..7).map(f64::from)).transpose().unwrap(),
        ints(&[2, 3], 0..6),
    ] {
        let mut temporary = packed(&plain);
        assert!(temporary
            .binary_reusing(&Value::Array(other), true, ArrayBinaryOp::Add, CHECKED)
            .is_none());
        assert_eq!(temporary, plain);
    }
}

#[test]
fn a_selection_updates_in_place_and_matches_select_compute_assign() {
    let selections: Vec<Vec<ArrayAxisSelection>> = vec![
        vec![Index(1)],
        vec![all(), Index(2)],
        vec![stepped(-1), stepped(2)],
        vec![
            Index(0),
            Slice {
                start: Some(1),
                end: Some(3),
                step: 1,
            },
        ],
        vec![all()],
    ];
    let operations = [
        ArrayBinaryOp::Add,
        ArrayBinaryOp::Sub,
        ArrayBinaryOp::Mul,
        ArrayBinaryOp::Div,
    ];
    let model =
        |target: &ArrayValue, selection: &[ArrayAxisSelection], value: &Value, operation| {
            let mut expected = packed(target);
            let view = expected.select(selection).unwrap();
            let computed = match value {
                Value::Array(other) => view.binary(other, operation, CHECKED),
                scalar => view.scalar_binary(scalar, false, operation, CHECKED),
            }
            .unwrap();
            drop(view);
            expected.assign(selection, &Value::Array(computed)).unwrap();
            expected
        };
    for selection in &selections {
        for operation in operations {
            let base = floats(&[3, 4], (1..13).map(f64::from));
            let selected_shape = base.select(selection).unwrap().shape.to_vec();
            let last = *selected_shape.last().unwrap();
            let values = [
                Value::Float(2.5),
                Value::Array(floats(&[last], (0..last).map(|value| value as f64 + 2.0))),
                // A view of the target itself.
                Value::Array(base.select(selection).unwrap()),
            ];
            for value in values {
                let expected = model(&base, selection, &value, operation);
                let mut target = packed(&base);
                let address = base_address(&target);
                let value = match value {
                    Value::Array(view) if view.shares_buffer_with(&base) => {
                        Value::Array(target.select(selection).unwrap())
                    }
                    other => other,
                };
                target.update(selection, value, operation, CHECKED).unwrap();
                assert_eq!(target, expected, "{selection:?} {operation:?}");
                assert_eq!(
                    base_address(&target),
                    address,
                    "{selection:?} {operation:?}"
                );
            }
            // A transposed target is repacked first.
            let turned = floats(&[4, 3], (1..13).map(f64::from)).transpose().unwrap();
            let expected = model(&turned, selection, &Value::Float(2.5), operation);
            let mut target = turned.clone();
            target
                .update(selection, Value::Float(2.5), operation, CHECKED)
                .unwrap();
            assert_eq!(target, expected);
        }
    }

    // float32 updates in place too.
    let mut narrow = ArrayValue::new(
        [2, 2].into(),
        ArrayStorage::Float32([1.0, 2.0, 3.0, 4.0].into()),
    )
    .unwrap();
    narrow
        .update(
            &[all(), Index(1)],
            Value::Float(0.5),
            ArrayBinaryOp::Mul,
            CHECKED,
        )
        .unwrap();
    narrow
        .update(
            &[Index(0)],
            Value::Array(
                ArrayValue::new([2].into(), ArrayStorage::Float32([10.0, 20.0].into())).unwrap(),
            ),
            ArrayBinaryOp::Add,
            CHECKED,
        )
        .unwrap();
    assert_eq!(
        narrow,
        ArrayValue::new(
            [2, 2].into(),
            ArrayStorage::Float32([11.0, 21.0, 3.0, 2.0].into())
        )
        .unwrap()
    );
}

#[test]
fn an_update_that_faults_or_does_not_fit_writes_nothing() {
    // A zero divisor: scalar, and one element of an Array.
    let base = floats(&[2, 2], [1.0, 2.0, 3.0, 4.0]);
    let mut target = packed(&base);
    let error = target
        .update(
            &[all(), Index(1)],
            Value::Float(0.0),
            ArrayBinaryOp::Div,
            CHECKED,
        )
        .unwrap_err();
    assert_eq!(error.code, "AU4004");
    let error = target
        .update(
            &[Index(1)],
            Value::Array(floats(&[2], [2.0, 0.0])),
            ArrayBinaryOp::Div,
            CHECKED,
        )
        .unwrap_err();
    assert_eq!(
        error.message,
        "array division has a zero divisor at flat index 1"
    );
    assert_eq!(target, base);

    // An integer overflow, after an element that would have been written.
    let numbers = ints(&[3], [1, i64::MAX, 3]);
    let mut target = packed(&numbers);
    let error = target
        .update(
            &[all()],
            Value::Int(IntegerValue::from_i64(1)),
            ArrayBinaryOp::Add,
            CHECKED,
        )
        .unwrap_err();
    assert_eq!(error.code, "AU4002");
    assert_eq!(target, numbers);
    target
        .update(
            &[Index(0)],
            Value::Int(IntegerValue::from_i64(4)),
            ArrayBinaryOp::Mul,
            CHECKED,
        )
        .unwrap();
    assert_eq!(target, ints(&[3], [4, i64::MAX, 3]));

    // A shape that does not broadcast to the selection, a dtype, a bad
    // selection, and a value of another kind.
    let mut target = packed(&base);
    for value in [
        Value::Array(floats(&[3], [1.0, 2.0, 3.0])),
        Value::Array(floats(&[2, 2, 2], [1.0; 8])),
        Value::Array(ints(&[2], [1, 2])),
    ] {
        let error = target
            .update(&[all()], value, ArrayBinaryOp::Add, CHECKED)
            .unwrap_err();
        assert_eq!(error.code, "AU4007", "{}", error.message);
    }
    let error = target
        .update(&[Index(5)], Value::Float(1.0), ArrayBinaryOp::Add, CHECKED)
        .unwrap_err();
    assert_eq!(error.code, "AU4003");
    assert!(target
        .update(&[all()], Value::Bool(true), ArrayBinaryOp::Add, CHECKED)
        .is_err());
    assert!(target
        .update(
            &[all()],
            Value::Float(1.0),
            ArrayBinaryOp::Add,
            IntegerArithmeticMode::Wrapping
        )
        .is_err());
    assert_eq!(target, base);
}
