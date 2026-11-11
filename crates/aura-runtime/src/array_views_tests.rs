use super::ArrayAxisSelection::{Index, Slice};
use super::*;
use crate::integer::IntegerValue;
use crate::runtime_value::with_array_allocation_budget;

fn int_array(shape: &[usize]) -> ArrayValue {
    let len: usize = shape.iter().product();
    ArrayValue::new(shape.into(), ArrayStorage::Int64((0..len as i64).collect())).unwrap()
}

fn ints(array: &ArrayValue) -> Vec<i64> {
    match &*array.elements().unwrap() {
        ArrayStorage::Int64(values) => values.to_vec(),
        other => panic!("expected int64 elements, found {other:?}"),
    }
}

fn int(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

fn all() -> ArrayAxisSelection {
    Slice {
        start: None,
        end: None,
        step: 1,
    }
}

fn span(start: i64, end: i64, step: i64) -> ArrayAxisSelection {
    Slice {
        start: Some(start),
        end: Some(end),
        step,
    }
}

fn base_address(array: &ArrayValue) -> *const i64 {
    match &array.base {
        ArrayStorage::Int64(values) => values.as_ptr(),
        other => panic!("expected int64 storage, found {other:?}"),
    }
}

#[test]
fn selection_words_decode_to_indexes_and_slices() {
    let words = [
        ARRAY_SELECTION_INDEX,
        -2,
        0,
        0,
        ARRAY_SELECTION_SLICE,
        0,
        0,
        1,
        ARRAY_SELECTION_SLICE | ARRAY_SELECTION_HAS_START,
        3,
        0,
        2,
        ARRAY_SELECTION_SLICE | ARRAY_SELECTION_HAS_START | ARRAY_SELECTION_HAS_END,
        5,
        1,
        -1,
        ARRAY_SELECTION_SLICE | ARRAY_SELECTION_HAS_END,
        0,
        4,
        1,
    ];
    assert_eq!(
        ArrayAxisSelection::decode(&words).unwrap(),
        [
            Index(-2),
            all(),
            Slice {
                start: Some(3),
                end: None,
                step: 2
            },
            span(5, 1, -1),
            Slice {
                start: None,
                end: Some(4),
                step: 1
            },
        ]
    );
    for malformed in [
        &[][..],
        &[0, 1, 2][..],
        &[2, 0, 0, 1][..],
        &[9, 0, 0, 1][..],
    ] {
        let error = ArrayAxisSelection::decode(malformed).unwrap_err();
        assert_eq!(error.code, "AU4001");
        assert_eq!(error.message, "malformed Array selection words");
    }
}

#[test]
fn a_selection_of_whole_rows_is_a_contiguous_window_of_the_source() {
    let matrix = int_array(&[4, 3]);
    let rows = matrix.select(&[span(1, 3, 1)]).unwrap();
    assert!(rows.is_contiguous());
    assert_eq!(&*rows.shape, &[2, 3]);
    assert_eq!(ints(&rows), [3, 4, 5, 6, 7, 8]);
    assert_eq!(
        base_address(&rows),
        unsafe { base_address(&matrix).add(3) },
        "the view reads the source's buffer in place"
    );
    // One row by index drops the axis and stays contiguous.
    let row = matrix.select(&[Index(-1)]).unwrap();
    assert!(row.is_contiguous());
    assert_eq!(&*row.shape, &[3]);
    assert_eq!(ints(&row), [9, 10, 11]);
    assert_eq!(matrix.slice_first_axis(Some(1), Some(3)).unwrap(), rows);
}

#[test]
fn a_selection_across_columns_is_a_strided_view() {
    let matrix = int_array(&[3, 4]);
    let column = matrix.select(&[all(), Index(1)]).unwrap();
    assert!(!column.is_contiguous());
    assert_eq!(&*column.shape, &[3]);
    assert_eq!(column.len(), 3);
    assert!(!column.is_empty());
    assert_eq!(ints(&column), [1, 5, 9]);
    assert_eq!(base_address(&column), base_address(&matrix));
    assert_eq!(column.get(&[2]).unwrap(), int(9));
    assert_eq!(column.get(&[-3]).unwrap(), int(1));
    assert_eq!(column.get_optional(&[3]).unwrap(), None);
    assert_eq!(column.value_at_flat(1), int(5));
    assert_eq!(column.render(), "Array[int64](shape=[3], values=[1, 5, 9])");

    let stepped = matrix.select(&[span(2, 0, -1), span(0, 4, 2)]).unwrap();
    assert_eq!(&*stepped.shape, &[2, 2]);
    assert_eq!(ints(&stepped), [8, 10, 4, 6]);
    assert_eq!(stepped.value_at_flat(3), int(6));
    assert_eq!(stepped.get(&[1, 0]).unwrap(), int(4));
    // A view of a view composes offsets and strides.
    let corner = stepped.select(&[Index(0), span(1, 2, 1)]).unwrap();
    assert_eq!(ints(&corner), [10]);
    // Kernels read a view through its packed elements.
    assert_eq!(
        column
            .reduce(crate::runtime_value::ArrayReduction::Sum)
            .unwrap(),
        int(15)
    );
    assert_eq!(column.dot(&column).unwrap(), int(1 + 25 + 81));
    let doubled = column
        .binary(
            &column,
            crate::runtime_value::ArrayBinaryOp::Add,
            crate::runtime_value::IntegerArithmeticMode::Checked,
        )
        .unwrap();
    assert_eq!(ints(&doubled), [2, 10, 18]);
    assert_eq!(column, column.try_clone().unwrap());
    assert_ne!(column, matrix.select(&[all(), Index(2)]).unwrap());
    assert_ne!(column, matrix);
}

#[test]
fn selections_report_rank_range_and_step_errors() {
    let matrix = int_array(&[2, 3]);
    let error = matrix.select(&[all(), all(), all()]).unwrap_err();
    assert_eq!(error.code, "AU4007");
    assert_eq!(
        error.message,
        "array rank 2 accepts at most 2 selectors, found 3"
    );
    let error = matrix.select(&[all(), Index(3)]).unwrap_err();
    assert_eq!(error.code, "AU4003");
    assert_eq!(
        error.message,
        "array coordinate 3 on axis 1 normalizes to 3, outside 0..3"
    );
    let error = matrix.select(&[span(0, 2, 0)]).unwrap_err();
    assert_eq!(error.code, "AU4003");
    assert_eq!(error.message, "slice step cannot be zero");
    let error = matrix.select(&[span(0, 3, 1)]).unwrap_err();
    assert_eq!(error.code, "AU4003");

    // Empty selections keep their shape and read no element.
    let empty = matrix.select(&[span(1, 1, 1)]).unwrap();
    assert_eq!(&*empty.shape, &[0, 3]);
    assert!(empty.is_empty() && empty.is_contiguous());
    let empty = matrix.select(&[all(), span(2, 2, -1)]).unwrap();
    assert_eq!(&*empty.shape, &[2, 0]);
    assert_eq!(ints(&empty), [] as [i64; 0]);
    // A step longer than the axis selects one element.
    let single = matrix.select(&[all(), span(1, 3, i64::MAX)]).unwrap();
    assert_eq!(ints(&single), [1, 4]);
    // An index on every axis leaves the one element as a rank-1 Array.
    let element = matrix.select(&[Index(1), Index(2)]).unwrap();
    assert_eq!(&*element.shape, &[1]);
    assert_eq!(ints(&element), [5]);
}

#[test]
fn reshape_shares_a_contiguous_buffer_and_packs_a_strided_view() {
    let values = int_array(&[2, 6]);
    let cube = values.reshape(&[2, 3, 2]).unwrap();
    assert_eq!(&*cube.shape, &[2, 3, 2]);
    assert_eq!(base_address(&cube), base_address(&values));
    assert_eq!(ints(&cube), ints(&values));
    assert_eq!(&*values.reshape(&[-1]).unwrap().shape, &[12]);
    assert_eq!(&*values.reshape(&[3, -1]).unwrap().shape, &[3, 4]);
    assert_eq!(&*values.reshape(&[-1, 12, 1]).unwrap().shape, &[1, 12, 1]);

    let transposed = values.transpose().unwrap();
    let flat = transposed.reshape(&[12]).unwrap();
    assert!(flat.is_contiguous());
    assert_eq!(ints(&flat), [0, 6, 1, 7, 2, 8, 3, 9, 4, 10, 5, 11]);
    assert_ne!(base_address(&flat), base_address(&values));

    let empty = int_array(&[0, 3]);
    assert_eq!(&*empty.reshape(&[3, 0]).unwrap().shape, &[3, 0]);

    for (shape, message) in [
        (&[][..], "array rank must be at least one"),
        (
            &[5][..],
            "array reshape requires 12 elements, but shape [5] does not hold that many",
        ),
        (
            &[5, -1][..],
            "array reshape requires 12 elements, but shape [5, -1] does not hold that many",
        ),
        (
            &[0, -1][..],
            "array reshape requires 12 elements, but shape [0, -1] does not hold that many",
        ),
        (
            &[-1, -1][..],
            "array reshape accepts one inferred dimension `-1`, found another at axis 1",
        ),
        (&[-2, 6][..], "Array shape axis 0 cannot be negative, found -2"),
        (
            &[i64::MAX, i64::MAX][..],
            "array reshape requires 12 elements, but shape [9223372036854775807, 9223372036854775807] does not hold that many",
        ),
    ] {
        let error = values.reshape(shape).unwrap_err();
        assert_eq!(error.code, "AU4007");
        assert_eq!(error.message, message);
    }
}

#[test]
fn permute_and_transpose_reorder_axes_without_copying() {
    let cube = int_array(&[2, 3, 4]);
    let transposed = cube.transpose().unwrap();
    assert_eq!(&*transposed.shape, &[4, 3, 2]);
    assert_eq!(base_address(&transposed), base_address(&cube));
    assert_eq!(
        transposed.get(&[3, 2, 1]).unwrap(),
        cube.get(&[1, 2, 3]).unwrap()
    );
    let permuted = cube.permute(&[1, -1, 0]).unwrap();
    assert_eq!(&*permuted.shape, &[3, 4, 2]);
    for (i, j, k) in [(0, 0, 0), (2, 3, 1), (1, 2, 0)] {
        assert_eq!(
            permuted.get(&[i, j, k]).unwrap(),
            cube.get(&[k, i, j]).unwrap()
        );
    }
    // The identity permutation, and a transpose back, are contiguous.
    assert!(cube.permute(&[0, 1, 2]).unwrap().is_contiguous());
    assert_eq!(transposed.transpose().unwrap(), cube);
    assert!(transposed.transpose().unwrap().is_contiguous());
    // A vector's transpose is itself.
    assert_eq!(int_array(&[3]).transpose().unwrap(), int_array(&[3]));

    let matrix = int_array(&[2, 3]);
    let product = matrix.matmul(&matrix.transpose().unwrap()).unwrap();
    assert_eq!(ints(&product), [5, 14, 14, 50]);

    for (axes, message) in [
        (
            &[0, 1][..],
            "array permute requires one axis per dimension of rank 3, found 2",
        ),
        (&[0, 1, 3][..], "array axis 3 is outside rank 3"),
        (&[0, -4, 1][..], "array axis -4 is outside rank 3"),
        (&[0, 2, -1][..], "array permute names axis 2 more than once"),
    ] {
        let error = cube.permute(axes).unwrap_err();
        assert_eq!(error.code, "AU4007");
        assert_eq!(error.message, message);
    }
}

#[test]
fn a_write_to_a_view_or_its_source_never_reaches_the_other() {
    let mut matrix = int_array(&[2, 3]);
    let mut column = matrix.select(&[all(), Index(0)]).unwrap();
    let mut rows = matrix.select(&[span(0, 1, 1)]).unwrap();
    assert_eq!(column.set(&[1], int(30)).unwrap(), int(3));
    assert!(
        column.is_contiguous(),
        "a written view owns a packed buffer"
    );
    assert_eq!(ints(&column), [0, 30]);
    assert_eq!(ints(&matrix), [0, 1, 2, 3, 4, 5]);

    matrix.set(&[0, 0], int(-1)).unwrap();
    assert_eq!(ints(&matrix), [-1, 1, 2, 3, 4, 5]);
    assert_eq!(
        ints(&rows),
        [0, 1, 2],
        "the earlier view kept the old elements"
    );
    rows.fill(int(7)).unwrap();
    assert_eq!(ints(&rows), [7, 7, 7]);
    assert_eq!(ints(&matrix), [-1, 1, 2, 3, 4, 5]);

    // A strided view is filled without reading its old elements.
    let mut diagonal_step = matrix.select(&[all(), span(0, 3, 2)]).unwrap();
    diagonal_step.fill(int(8)).unwrap();
    assert!(diagonal_step.is_contiguous());
    assert_eq!(ints(&diagonal_step), [8, 8, 8, 8]);
    // An unshared Array is filled and written in place.
    let before = base_address(&diagonal_step);
    diagonal_step.fill(int(9)).unwrap();
    diagonal_step.set(&[0, 1], int(1)).unwrap();
    assert_eq!(base_address(&diagonal_step), before);
    assert_eq!(ints(&diagonal_step), [9, 1, 9, 9]);

    // Bad coordinates are reported before a view is packed.
    let view = matrix.transpose().unwrap();
    let mut written = view.try_clone().unwrap();
    let error = written.set(&[5, 0], int(1)).unwrap_err();
    assert_eq!(error.code, "AU4003");
    assert!(!written.is_contiguous());
}

#[test]
fn fill_and_set_cover_every_dtype_on_shared_storage() {
    let cases = [
        (
            ArrayStorage::Int32([1, 2].into()),
            Value::Int(IntegerValue::from_i32(5)),
            "Array[int32](shape=[2], values=[5, 5])",
        ),
        (
            ArrayStorage::Int64([1, 2].into()),
            int(5),
            "Array[int64](shape=[2], values=[5, 5])",
        ),
        (
            ArrayStorage::Float32([1.0, 2.0].into()),
            Value::Float(0.5),
            "Array[float32](shape=[2], values=[0.5, 0.5])",
        ),
        (
            ArrayStorage::Float64([1.0, 2.0].into()),
            Value::Float(0.5),
            "Array[float64](shape=[2], values=[0.5, 0.5])",
        ),
    ];
    for (storage, value, rendered) in cases {
        let source = ArrayValue::new([2].into(), storage).unwrap();
        let before = source.render();
        let mut shared = source.try_clone().unwrap();
        shared.fill(value.clone()).unwrap();
        assert_eq!(shared.render(), rendered);
        assert_eq!(source.render(), before);
        let mut written = source.try_clone().unwrap();
        written.set(&[0], value.clone()).unwrap();
        written.set(&[1], value.clone()).unwrap();
        assert_eq!(written.render(), rendered);
        assert_eq!(source.render(), before);
        let mut assigned = source.try_clone().unwrap();
        assigned.assign(&[all()], &value).unwrap();
        assert_eq!(assigned.render(), rendered);
        let mut copied = source.try_clone().unwrap();
        copied.assign(&[all()], &Value::Array(shared)).unwrap();
        assert_eq!(copied.render(), rendered);
        assert_eq!(source.render(), before);
        let error = written.assign(&[all()], &Value::Bool(true)).unwrap_err();
        assert_eq!(error.code, "AU4007");
    }
}

#[test]
fn assignment_writes_scalars_arrays_and_broadcast_rows_through_a_selection() {
    let mut matrix = int_array(&[3, 4]);
    matrix.assign(&[Index(1)], &int(-1)).unwrap();
    assert_eq!(ints(&matrix), [0, 1, 2, 3, -1, -1, -1, -1, 8, 9, 10, 11]);

    let column = ArrayValue::new([3].into(), ArrayStorage::Int64([70, 80, 90].into())).unwrap();
    matrix
        .assign(&[all(), Index(-1)], &Value::Array(column))
        .unwrap();
    assert_eq!(ints(&matrix), [0, 1, 2, 70, -1, -1, -1, 80, 8, 9, 10, 90]);

    // One row broadcast down two rows, into every second column.
    let pair = ArrayValue::new([2].into(), ArrayStorage::Int64([5, 6].into())).unwrap();
    matrix
        .assign(&[span(0, 2, 1), span(0, 4, 2)], &Value::Array(pair))
        .unwrap();
    assert_eq!(ints(&matrix), [5, 1, 6, 70, 5, -1, 6, 80, 8, 9, 10, 90]);

    // A source that is a view of the target reads the old elements.
    let mut values = int_array(&[5]);
    let head = values.select(&[span(0, 4, 1)]).unwrap();
    values
        .assign(
            &[Slice {
                start: Some(1),
                end: None,
                step: 1,
            }],
            &Value::Array(head),
        )
        .unwrap();
    assert_eq!(ints(&values), [0, 0, 1, 2, 3]);
    let reversed = values.select(&[span(4, 0, -1)]).unwrap();
    values
        .assign(&[span(0, 4, 1)], &Value::Array(reversed))
        .unwrap();
    assert_eq!(ints(&values), [3, 2, 1, 0, 3]);

    // Assigning into a strided view packs it first.
    let mut view = int_array(&[2, 3]).transpose().unwrap();
    view.assign(&[Index(0)], &int(9)).unwrap();
    assert!(view.is_contiguous());
    assert_eq!(ints(&view), [9, 9, 1, 4, 2, 5]);

    // An empty selection writes nothing.
    let mut untouched = int_array(&[3]);
    untouched.assign(&[span(1, 1, 1)], &int(4)).unwrap();
    assert_eq!(ints(&untouched), [0, 1, 2]);
}

#[test]
fn assignment_rejects_values_that_do_not_fit_before_copying() {
    let mut matrix = int_array(&[2, 3]);
    let shared = matrix.try_clone().unwrap();
    let wide = int_array(&[4]);
    let error = matrix.assign(&[all()], &Value::Array(wide)).unwrap_err();
    assert_eq!(error.code, "AU4007");
    assert_eq!(
        error.message,
        "array slice assignment requires a int64 value that broadcasts to the selected shape [2, 3], found int64 [4]"
    );
    // A value with more axes than the selection does not broadcast to it.
    let tall = int_array(&[2, 2, 3]);
    assert!(matrix.assign(&[all()], &Value::Array(tall)).is_err());
    let floats = ArrayValue::new([3].into(), ArrayStorage::Float64([1.0; 3].into())).unwrap();
    let error = matrix.assign(&[all()], &Value::Array(floats)).unwrap_err();
    assert_eq!(
        error.message,
        "array slice assignment requires a int64 value that broadcasts to the selected shape [2, 3], found float64 [3]"
    );
    assert!(matrix.assign(&[Index(2)], &int(1)).is_err());
    assert_eq!(
        base_address(&matrix),
        base_address(&shared),
        "a refused assignment leaves the shared buffer alone"
    );
    let error = with_array_allocation_budget(0, || matrix.assign(&[all()], &int(1))).unwrap_err();
    assert_eq!(error.code, "AU4005");
    assert_eq!(ints(&matrix), ints(&shared));
}

/// Checks views against a model that keeps every element with its source
/// coordinates, over a few hundred generated selections and permutations.
#[test]
fn generated_views_match_an_index_by_index_model() {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % bound
    };
    for _ in 0..300 {
        let rank = 1 + next(3) as usize;
        let shape: Vec<usize> = (0..rank).map(|_| 1 + next(4) as usize).collect();
        let source = int_array(&shape);
        // Each axis: an index, or a stepped slice in either direction.
        let mut selection = Vec::new();
        let mut kept: Vec<Vec<usize>> = Vec::new();
        for &dimension in &shape {
            if next(4) == 0 {
                let position = next(dimension as u64) as usize;
                selection.push(Index(
                    position as i64 - if next(2) == 0 { dimension as i64 } else { 0 },
                ));
                kept.push(vec![position]);
                continue;
            }
            let step = 1 + next(2) as usize;
            let low = next(dimension as u64) as usize;
            let high = low + next((dimension - low) as u64 + 1) as usize;
            let mut positions: Vec<usize> = (low..high).step_by(step).collect();
            if next(2) == 0 || positions.is_empty() {
                selection.push(span(low as i64, high as i64, step as i64));
            } else {
                // The same positions walked down from the last to the first.
                let last = *positions.last().unwrap() as i64;
                let stop = positions[0] as i64 - 1;
                selection.push(Slice {
                    start: Some(last),
                    end: (stop >= 0).then_some(stop),
                    step: -(step as i64),
                });
                positions.reverse();
            }
            kept.push(positions);
        }
        let view = source.select(&selection).unwrap();
        let mut expected = vec![0_i64];
        for (positions, &dimension) in kept.iter().zip(&shape) {
            expected = expected
                .iter()
                .flat_map(|&prefix| {
                    positions
                        .iter()
                        .map(move |&position| prefix * dimension as i64 + position as i64)
                })
                .collect();
        }
        assert_eq!(ints(&view), expected, "{shape:?} {selection:?}");
        assert_eq!(view.len(), expected.len());
        for (flat, value) in expected.iter().enumerate() {
            assert_eq!(view.value_at_flat(flat), int(*value));
        }

        // A permutation of the view, read back through coordinates.
        let view_rank = view.rank();
        let mut axes: Vec<i64> = (0..view_rank as i64).collect();
        for index in (1..view_rank).rev() {
            axes.swap(index, next(index as u64 + 1) as usize);
        }
        let permuted = view.permute(&axes).unwrap();
        let packed = permuted.reshape(&[-1]).unwrap();
        let mut coordinates = vec![0_i64; view_rank];
        for flat in 0..permuted.len() {
            let mut remaining = flat;
            for axis in (0..view_rank).rev() {
                coordinates[axis] = (remaining % permuted.shape[axis]) as i64;
                remaining /= permuted.shape[axis];
            }
            let mut source_coordinates = vec![0_i64; view_rank];
            for (axis, &from) in axes.iter().enumerate() {
                source_coordinates[from as usize] = coordinates[axis];
            }
            let expected = view.get(&source_coordinates).unwrap();
            assert_eq!(permuted.get(&coordinates).unwrap(), expected);
            assert_eq!(packed.get(&[flat as i64]).unwrap(), expected);
        }

        // Writing the selection back through assignment restores the
        // selected elements exactly where they were read from.
        let mut target = ArrayValue::zeros(
            crate::runtime_value::ArrayDType::Int64,
            shape.clone().into(),
        )
        .unwrap();
        target.assign(&selection, &Value::Array(view)).unwrap();
        let written = ints(&target);
        for (flat, value) in written.iter().enumerate() {
            let selected = expected.contains(&(flat as i64));
            assert_eq!(*value, if selected { flat as i64 } else { 0 });
        }
    }
}
