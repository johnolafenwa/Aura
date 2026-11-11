use super::*;

fn layout(offset: usize, strides: &[isize]) -> Strides {
    Strides {
        offset,
        strides: strides.into(),
    }
}

#[test]
fn a_buffer_window_shares_storage_until_it_is_written() {
    let whole: ArrayBuf<i32> = vec![1, 2, 3, 4, 5].into();
    assert!(!whole.is_shared());
    let mut middle = whole.window(1, 3);
    assert_eq!(&*middle, &[2, 3, 4]);
    assert!(whole.is_shared() && middle.is_shared());
    assert!(whole.shares_buffer_with(&middle));
    assert_eq!(middle.as_ptr(), whole[1..].as_ptr());

    middle.try_make_mut("test write").unwrap()[0] = 20;
    assert_eq!(&*middle, &[20, 3, 4]);
    assert_eq!(&*whole, &[1, 2, 3, 4, 5], "the write stayed private");
    assert!(!whole.shares_buffer_with(&middle));
    assert!(!whole.is_shared() && !middle.is_shared());

    // A window that is the last holder of its buffer is written in place.
    let mut tail = whole.window(3, 2);
    drop(whole);
    let before = tail.as_ptr();
    tail.try_make_mut("test write").unwrap()[1] = 50;
    assert_eq!(tail.as_ptr(), before);
    assert_eq!(&*tail, &[4, 50]);

    let copy = tail.clone();
    assert_eq!(copy, tail);
    assert_eq!(format!("{copy:?}"), "[4, 50]");
    assert_eq!(&*ArrayBuf::from([7_i64, 8]), &[7, 8]);
    assert_eq!(&*(1..4).collect::<ArrayBuf<i32>>(), &[1, 2, 3]);
}

#[test]
fn a_shared_window_reports_a_failed_copy_and_keeps_its_elements() {
    let whole: ArrayBuf<f64> = vec![1.0, 2.0].into();
    let mut view = whole.clone();
    let error = crate::runtime_value::with_array_allocation_budget(0, || {
        view.try_make_mut("test write").map(|_| ())
    })
    .unwrap_err();
    assert_eq!(error.code, "AU4005");
    assert_eq!(&*view, &[1.0, 2.0]);
}

#[test]
#[should_panic(expected = "an Array window stays inside its buffer")]
fn a_window_past_the_buffer_is_refused() {
    let whole: ArrayBuf<i32> = vec![1, 2].into();
    let _ = whole.window(1, 2);
}

#[test]
fn row_major_strides_and_the_contiguity_test_agree() {
    assert_eq!(row_major_strides(&[4]), [1]);
    assert_eq!(row_major_strides(&[2, 3, 4]), [12, 4, 1]);
    assert!(is_row_major(&[2, 3, 4], &[12, 4, 1]));
    assert!(!is_row_major(&[2, 3, 4], &[1, 2, 6]));
    assert!(!is_row_major(&[2, 3], &[3, 2]));
    // A length-one axis never steps, and an empty layout has no elements.
    assert!(is_row_major(&[1, 3], &[99, 1]));
    assert!(is_row_major(&[3, 1], &[1, -5]));
    assert!(is_row_major(&[0, 3], &[7, 7]));
}

#[test]
fn gather_reads_strided_layouts_in_row_major_order() {
    let base: Vec<i32> = (0..12).collect();
    let read = |shape: &[usize], layout: Strides| {
        let mut output = Vec::new();
        gather(&base, shape, &layout, &mut output);
        output
    };
    // The whole 3 by 4 block is one run.
    assert_eq!(read(&[3, 4], layout(0, &[4, 1])), base);
    // Its transpose.
    assert_eq!(
        read(&[4, 3], layout(0, &[1, 4])),
        [0, 4, 8, 1, 5, 9, 2, 6, 10, 3, 7, 11]
    );
    // Every second column, backwards.
    assert_eq!(read(&[3, 2], layout(3, &[4, -2])), [3, 1, 7, 5, 11, 9]);
    // One element repeated over a 2 by 3 shape.
    assert_eq!(read(&[2, 3], layout(5, &[0, 0])), [5; 6]);
    // A row repeated down two rows.
    assert_eq!(read(&[2, 4], layout(4, &[0, 1])), [4, 5, 6, 7, 4, 5, 6, 7]);
    assert_eq!(read(&[2, 0], layout(0, &[4, 1])), [] as [i32; 0]);
}

#[test]
fn walk_runs_joins_trailing_axes_and_stops_on_request() {
    let mut runs = Vec::new();
    walk_runs(
        &[2, 3, 4],
        [&layout(0, &[12, 4, 1])],
        |[start], [step], count| {
            runs.push((start, step, count));
            true
        },
    );
    assert_eq!(runs, [(0, 1, 24)]);

    // The second operand repeats a row, so the run is one row long.
    let mut runs = Vec::new();
    walk_runs(
        &[2, 3],
        [&layout(0, &[3, 1]), &layout(10, &[0, 1])],
        |starts, steps, count| {
            runs.push((starts, steps, count));
            true
        },
    );
    assert_eq!(runs, [([0, 10], [1, 1], 3), ([3, 10], [1, 1], 3)]);

    let mut visited = 0;
    walk_runs(&[4, 2], [&layout(0, &[1, 4])], |_, _, _| {
        visited += 1;
        visited < 2
    });
    assert_eq!(visited, 2);
}

#[test]
fn scatter_writes_runs_fills_and_strided_elements() {
    let source: Vec<i32> = (100..106).collect();
    // Contiguous rows into contiguous rows.
    let mut target = vec![0; 12];
    scatter(
        &mut target,
        &layout(4, &[4, 1]),
        &source,
        &layout(0, &[3, 1]),
        &[2, 3],
    );
    assert_eq!(target, [0, 0, 0, 0, 100, 101, 102, 0, 103, 104, 105, 0]);
    // One source element stretched along each row.
    let mut target = vec![0; 6];
    scatter(
        &mut target,
        &layout(0, &[3, 1]),
        &source,
        &layout(0, &[1, 0]),
        &[2, 3],
    );
    assert_eq!(target, [100, 100, 100, 101, 101, 101]);
    // A column of the target, from a reversed source.
    let mut target = vec![0; 6];
    scatter(
        &mut target,
        &layout(1, &[2]),
        &source,
        &layout(2, &[-1]),
        &[3],
    );
    assert_eq!(target, [0, 102, 0, 101, 0, 100]);

    let mut target = vec![0; 6];
    scatter_fill(&mut target, &layout(0, &[3, 1]), &[2, 2], 9);
    assert_eq!(target, [9, 9, 0, 9, 9, 0]);
    scatter_fill(&mut target, &layout(2, &[3]), &[2], 4);
    assert_eq!(target, [9, 9, 4, 9, 9, 4]);
}

#[test]
fn shapes_broadcast_from_their_last_axes() {
    assert_eq!(broadcast_shape(&[2, 3], &[2, 3]), Some(vec![2, 3]));
    assert_eq!(broadcast_shape(&[2, 3], &[3]), Some(vec![2, 3]));
    assert_eq!(broadcast_shape(&[3], &[2, 3]), Some(vec![2, 3]));
    assert_eq!(broadcast_shape(&[2, 1], &[1, 3]), Some(vec![2, 3]));
    assert_eq!(broadcast_shape(&[4, 1, 3], &[2, 1]), Some(vec![4, 2, 3]));
    assert_eq!(broadcast_shape(&[0, 3], &[1]), Some(vec![0, 3]));
    assert_eq!(broadcast_shape(&[1], &[0]), Some(vec![0]));
    assert_eq!(broadcast_shape(&[2, 3], &[2]), None);
    assert_eq!(broadcast_shape(&[2], &[3]), None);

    let row = layout(7, &[1]);
    assert_eq!(
        broadcast_layout(&[3], &row, &[2, 3]),
        layout(7, &[0, 1]),
        "a missing leading axis repeats the operand"
    );
    assert_eq!(
        broadcast_layout(&[2, 1], &layout(0, &[1, 1]), &[2, 3]),
        layout(0, &[1, 0]),
        "a stretched axis repeats its one element"
    );
    let error = broadcast_error("array addition", &[2, 3], &[2]);
    assert_eq!(error.code, "AU4007");
    assert_eq!(
        error.message,
        "array addition requires shapes that broadcast together, found [2, 3] and [2]"
    );
}

#[test]
fn per_axis_values_stay_inline_until_they_outgrow_it() {
    let mut values = AxisVec::<usize>::default();
    assert!(values.is_empty());
    for value in 0..INLINE_AXES {
        values.push(value);
    }
    assert_eq!(values.as_slice(), &[0, 1, 2, 3, 4, 5]);
    values.push(6);
    values.push(7);
    assert_eq!(&*values, &[0, 1, 2, 3, 4, 5, 6, 7]);

    for shape in [
        &[][..],
        &[4],
        &[2, 3],
        &[2, 0, 3],
        &[2, 1, 2, 1, 2, 1, 2, 3],
    ] {
        assert_eq!(
            row_major_axis_strides(shape).as_slice(),
            &row_major_strides(shape)[..],
            "{shape:?}"
        );
    }
}

#[test]
fn a_window_copies_inside_a_buffer_it_shares_with_one_other_window() {
    let mut whole: ArrayBuf<i64> = (0..6).collect();
    let tail = whole.window(2, 4);
    let third = whole.window(0, 1);
    assert!(!whole.is_shared_only_with(&tail));
    assert!(
        !whole.copy_within_from(tail, 0),
        "a third window still reads"
    );
    assert_eq!(&*whole, &[0, 1, 2, 3, 4, 5]);
    drop(third);

    let address = whole.as_ptr();
    let tail = whole.window(2, 4);
    assert!(whole.is_shared_only_with(&tail));
    assert!(whole.copy_within_from(tail, 1));
    assert_eq!(&*whole, &[0, 2, 3, 4, 5, 5]);
    assert_eq!(whole.as_ptr(), address);

    let other: ArrayBuf<i64> = (0..2).collect();
    assert!(!whole.copy_within_from(other, 0));
}
