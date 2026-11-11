use crate::array_views::ArrayAxisSelection::{self, Index, Slice};
use crate::runtime_value::{ArrayReduction, ArrayStorage, ArrayValue, Value};

const REDUCTIONS: [ArrayReduction; 6] = [
    ArrayReduction::Sum,
    ArrayReduction::Min,
    ArrayReduction::Max,
    ArrayReduction::Mean,
    ArrayReduction::FastSum,
    ArrayReduction::FastMean,
];

fn all() -> ArrayAxisSelection {
    Slice {
        start: None,
        end: None,
        step: 1,
    }
}

fn floats(shape: &[usize], values: impl IntoIterator<Item = f64>) -> ArrayValue {
    ArrayValue::new(
        shape.into(),
        ArrayStorage::Float64(values.into_iter().collect()),
    )
    .unwrap()
}

fn ints(shape: &[usize], values: impl IntoIterator<Item = i64>) -> ArrayValue {
    ArrayValue::new(
        shape.into(),
        ArrayStorage::Int64(values.into_iter().collect()),
    )
    .unwrap()
}

/// The reduction of every lane along `axis`, taken as a view and reduced
/// whole: what `reduce_axis` must equal bit for bit.
fn by_views(
    array: &ArrayValue,
    reduction: ArrayReduction,
    axis: usize,
) -> Result<Vec<Value>, String> {
    let mut shape: Vec<usize> = array.shape.to_vec();
    shape.remove(axis);
    let mut results = Vec::new();
    let mut coordinates = vec![0_usize; shape.len()];
    let lanes = shape.iter().product::<usize>();
    for _ in 0..lanes {
        let mut selection = Vec::new();
        let mut next = coordinates.iter();
        for position in 0..array.rank() {
            if position == axis {
                selection.push(all());
            } else {
                selection.push(Index(*next.next().unwrap() as i64));
            }
        }
        let lane = array.select(&selection).unwrap();
        results.push(lane.reduce(reduction).map_err(|error| error.message)?);
        for position in (0..coordinates.len()).rev() {
            coordinates[position] += 1;
            if coordinates[position] < shape[position] {
                break;
            }
            coordinates[position] = 0;
        }
    }
    Ok(results)
}

fn elements(array: &ArrayValue) -> Vec<Value> {
    (0..array.len())
        .map(|index| {
            let flat = array.reshape(&[-1]).unwrap();
            flat.get(&[index as i64]).unwrap()
        })
        .collect()
}

fn same_bits(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Float(left), Value::Float(right)) => left.to_bits() == right.to_bits(),
        _ => left == right,
    }
}

fn assert_matches_views(array: &ArrayValue) {
    for reduction in REDUCTIONS {
        for axis in 0..array.rank() {
            let expected = by_views(array, reduction, axis);
            let actual = array.reduce_axis(reduction, axis as i64);
            match (expected, actual) {
                (Ok(expected), Ok(actual)) => {
                    let mut shape = array.shape.to_vec();
                    shape.remove(axis);
                    if shape.is_empty() {
                        shape.push(1);
                    }
                    assert_eq!(&*actual.shape, &shape[..]);
                    let actual = elements(&actual);
                    assert_eq!(actual.len(), expected.len());
                    for (index, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
                        assert!(
                            same_bits(actual, expected),
                            "{reduction:?} axis {axis} lane {index}: {actual:?} != {expected:?}"
                        );
                    }
                    // A negative axis names the same one.
                    let negative = array
                        .reduce_axis(reduction, axis as i64 - array.rank() as i64)
                        .unwrap();
                    assert_eq!(elements(&negative).len(), expected.len());
                }
                (Err(expected), Err(actual)) => assert_eq!(actual.message, expected),
                (expected, actual) => {
                    panic!("{reduction:?} axis {axis}: expected {expected:?}, found {actual:?}")
                }
            }
        }
    }
}

#[test]
fn an_axis_reduction_reduces_each_lane() {
    let grid = floats(&[2, 3], [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    assert_eq!(
        grid.reduce_axis(ArrayReduction::Sum, 0).unwrap(),
        floats(&[3], [5.0, 7.0, 9.0])
    );
    assert_eq!(
        grid.reduce_axis(ArrayReduction::Sum, 1).unwrap(),
        floats(&[2], [6.0, 15.0])
    );
    assert_eq!(
        grid.reduce_axis(ArrayReduction::Max, -1).unwrap(),
        floats(&[2], [3.0, 6.0])
    );
    assert_eq!(
        grid.reduce_axis(ArrayReduction::Mean, 0).unwrap(),
        floats(&[3], [2.5, 3.5, 4.5])
    );
    // A rank-1 Array gives one element.
    let row = ints(&[4], [4, 1, 3, 2]);
    assert_eq!(
        row.reduce_axis(ArrayReduction::Min, 0).unwrap(),
        ints(&[1], [1])
    );
    // An integer mean is a float64 Array.
    assert_eq!(
        ints(&[2, 2], [1, 2, 3, 4])
            .reduce_axis(ArrayReduction::Mean, 1)
            .unwrap(),
        floats(&[2], [1.5, 3.5])
    );
}

#[test]
fn every_dtype_layout_and_reduction_matches_its_lanes() {
    let count = 2 * 3 * 37;
    let pattern = |index: usize| ((index * 7919) % 23) as i64 - 11;
    let storages = [
        ArrayStorage::Int32((0..count).map(|index| pattern(index) as i32).collect()),
        ArrayStorage::Int64((0..count).map(pattern).collect()),
        ArrayStorage::Float32(
            (0..count)
                .map(|index| pattern(index) as f32 * 0.37)
                .collect(),
        ),
        ArrayStorage::Float64(
            (0..count)
                .map(|index| pattern(index) as f64 * 0.37)
                .collect(),
        ),
    ];
    for storage in storages {
        let cube = ArrayValue::new([2, 3, 37].into(), storage).unwrap();
        assert_matches_views(&cube);
        assert_matches_views(&cube.transpose().unwrap());
        assert_matches_views(&cube.permute(&[1, 2, 0]).unwrap());
        let stepped = Slice {
            start: None,
            end: None,
            step: -2,
        };
        assert_matches_views(&cube.select(&[all(), all(), stepped]).unwrap());
    }
}

#[test]
fn float_lanes_keep_the_kernel_bits_for_nan_and_signed_zero() {
    let values = [
        0.0,
        -0.0,
        f64::NAN,
        1.0,
        -0.0,
        0.0,
        f64::INFINITY,
        2.0,
        0.0,
        0.0,
        f64::NEG_INFINITY,
        -3.0,
    ];
    let grid = floats(&[3, 4], values);
    assert_matches_views(&grid);
    assert_matches_views(&grid.transpose().unwrap());
    let narrow = ArrayValue::new(
        [3, 4].into(),
        ArrayStorage::Float32(values.iter().map(|value| *value as f32).collect()),
    )
    .unwrap();
    assert_matches_views(&narrow);
    assert_matches_views(&narrow.transpose().unwrap());
}

#[test]
fn an_integer_sum_reports_the_overflow_of_its_first_lane() {
    let grid = ints(&[2, 2], [i64::MAX, 1, 1, i64::MAX]);
    assert_matches_views(&grid);
    let error = grid.reduce_axis(ArrayReduction::Sum, 0).unwrap_err();
    assert_eq!(error.code, "AU4002");
    let narrow = ArrayValue::new(
        [2, 2].into(),
        ArrayStorage::Int32([i32::MIN, 5, -1, 6].into()),
    )
    .unwrap();
    assert_matches_views(&narrow);
    assert!(narrow.reduce_axis(ArrayReduction::Sum, 0).is_err());
    assert!(narrow.reduce_axis(ArrayReduction::Sum, 1).is_ok());
}

#[test]
fn empty_lanes_and_bad_axes_follow_the_whole_array_rules() {
    let empty = floats(&[0, 3], []);
    // Three lanes of no elements.
    assert_eq!(
        empty.reduce_axis(ArrayReduction::Sum, 0).unwrap(),
        floats(&[3], [0.0, 0.0, 0.0])
    );
    for reduction in [
        ArrayReduction::Min,
        ArrayReduction::Max,
        ArrayReduction::Mean,
        ArrayReduction::FastMean,
    ] {
        let error = empty.reduce_axis(reduction, 0).unwrap_err();
        assert_eq!(error.code, "AU4007");
    }
    // No lanes at all.
    for reduction in REDUCTIONS {
        assert_eq!(empty.reduce_axis(reduction, 1).unwrap().len(), 0);
    }
    for (storage, zero) in [
        (
            ArrayStorage::Int32([].into()),
            ArrayStorage::Int32([0, 0].into()),
        ),
        (
            ArrayStorage::Int64([].into()),
            ArrayStorage::Int64([0, 0].into()),
        ),
        (
            ArrayStorage::Float32([].into()),
            ArrayStorage::Float32([0.0, 0.0].into()),
        ),
    ] {
        let empty = ArrayValue::new([2, 0].into(), storage).unwrap();
        assert_eq!(
            empty.reduce_axis(ArrayReduction::FastSum, 1).unwrap(),
            ArrayValue::new([2].into(), zero).unwrap()
        );
    }
    let grid = floats(&[2, 3], [1.0; 6]);
    for axis in [2, -3] {
        let error = grid.reduce_axis(ArrayReduction::Sum, axis).unwrap_err();
        assert_eq!(error.code, "AU4007");
    }
}

#[test]
fn strided_reordered_sums_match_the_packed_elements_bit_for_bit() {
    for len in [0_usize, 1, 2, 31, 32, 33, 64, 95, 200] {
        let source = floats(
            &[len],
            (0..len).map(|index| 0.1 + (index as f64).sin() * 1e3),
        );
        for step in [1_i64, 2, 3, -1, -2, 7, -33] {
            let view = source
                .select(&[Slice {
                    start: None,
                    end: None,
                    step,
                }])
                .unwrap();
            let packed = floats(
                &[view.len()],
                match &*view.elements().unwrap() {
                    ArrayStorage::Float64(values) => values.to_vec(),
                    _ => unreachable!(),
                },
            );
            for reduction in [ArrayReduction::FastSum, ArrayReduction::FastMean] {
                let (strided, whole) = (view.reduce(reduction), packed.reduce(reduction));
                match (strided, whole) {
                    (Ok(Value::Float(left)), Ok(Value::Float(right))) => {
                        assert_eq!(left.to_bits(), right.to_bits(), "len {len} step {step}");
                    }
                    (Err(left), Err(right)) => assert_eq!(left.message, right.message),
                    other => panic!("len {len} step {step}: {other:?}"),
                }
            }
        }
    }
}
