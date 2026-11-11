use super::*;
use crate::integer::IntegerValue;
use crate::runtime_value::{ArrayReduction, ArrayStorage, ArrayValue, Value};

/// Deterministic values with a wide range of magnitudes, so a different
/// order of additions rounds differently.
fn floats(len: usize, seed: u64) -> Vec<f64> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let unit = (state >> 11) as f64 / (1_u64 << 53) as f64;
            let exponent = ((state >> 3) % 40) as i32 - 20;
            (unit - 0.5) * 2.0_f64.powi(exponent)
        })
        .collect()
}

fn floats32(len: usize, seed: u64) -> Vec<f32> {
    floats(len, seed).into_iter().map(|v| v as f32).collect()
}

fn integers(len: usize, seed: u64, range: i64) -> Vec<i64> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((state >> 33) as i64 % (2 * range + 1)) - range
        })
        .collect()
}

/// The reordered sum, written as the contract states it.
fn reference_striped<T: Copy + Default + std::ops::Add<Output = T>>(terms: &[T]) -> T {
    let whole = terms.len() - terms.len() % STRIPES;
    let mut partial = [T::default(); STRIPES];
    for (index, term) in terms[..whole].iter().enumerate() {
        partial[index % STRIPES] = partial[index % STRIPES] + *term;
    }
    for width in [16, 8, 4, 2, 1] {
        for stripe in 0..width {
            partial[stripe] = partial[stripe] + partial[stripe + width];
        }
    }
    terms[whole..]
        .iter()
        .fold(partial[0], |total, term| total + *term)
}

const LEVELS: [KernelLevel; 2] = [KernelLevel::Baseline, KernelLevel::Avx2];
const LENGTHS: [usize; 12] = [0, 1, 2, 31, 32, 33, 63, 64, 65, 255, 1000, 4099];

fn array(shape: &[usize], storage: ArrayStorage) -> ArrayValue {
    ArrayValue::new(shape.to_vec().into_boxed_slice(), storage).unwrap()
}

fn float_array(shape: &[usize], values: Vec<f64>) -> ArrayValue {
    array(
        shape,
        ArrayStorage::Float64(values.into_boxed_slice().into()),
    )
}

fn float_bits(value: &Value) -> u64 {
    match value {
        Value::Float(value) => value.to_bits(),
        other => panic!("expected a float, found {other:?}"),
    }
}

#[test]
fn level_names_and_selection_follow_the_cpu_and_the_setting() {
    assert_eq!(KernelLevel::Baseline.name(), "baseline");
    assert_eq!(KernelLevel::Avx2.name(), "avx2");
    assert_eq!(level_for(true, None), KernelLevel::Avx2);
    assert_eq!(level_for(true, Some("avx2")), KernelLevel::Avx2);
    assert_eq!(level_for(true, Some("baseline")), KernelLevel::Baseline);
    assert_eq!(level_for(false, None), KernelLevel::Baseline);
    assert_eq!(level_for(false, Some("avx2")), KernelLevel::Baseline);
    // The process level is what the CPU has, unless the variable says
    // otherwise, and an override lasts only for its closure.
    let detected = kernel_level();
    assert_eq!(kernel_level(), detected);
    with_kernel_level(KernelLevel::Baseline, || {
        assert_eq!(kernel_level(), KernelLevel::Baseline);
        with_kernel_level(KernelLevel::Avx2, || {
            assert_eq!(kernel_level(), level_for(cpu_has_avx2(), None));
        });
        assert_eq!(kernel_level(), KernelLevel::Baseline);
    });
    assert_eq!(kernel_level(), detected);
}

#[test]
fn reordered_sums_follow_the_stated_order_at_every_level() {
    for len in LENGTHS {
        let values = floats(len, 7 + len as u64);
        let narrow = floats32(len, 11 + len as u64);
        let widened = narrow.iter().map(|v| *v as f64).collect::<Vec<_>>();
        let ints = integers(len, 13 + len as u64, 1 << 40);
        let ints32 = ints.iter().map(|v| *v as i32).collect::<Vec<_>>();
        let ints_as_floats = ints.iter().map(|v| *v as f64).collect::<Vec<_>>();
        let ints32_as_floats = ints32.iter().map(|v| *v as f64).collect::<Vec<_>>();
        for level in LEVELS {
            with_kernel_level(level, || {
                assert_eq!(
                    striped_sum(&values).to_bits(),
                    reference_striped(&values).to_bits(),
                    "float64 sum of {len} at {level:?}"
                );
                assert_eq!(
                    striped_sum(&narrow).to_bits(),
                    reference_striped(&narrow).to_bits(),
                    "float32 sum of {len} at {level:?}"
                );
                assert_eq!(
                    striped_sum_f64(&narrow).to_bits(),
                    reference_striped(&widened).to_bits()
                );
                assert_eq!(
                    striped_sum_i64_as_f64(&ints).to_bits(),
                    reference_striped(&ints_as_floats).to_bits()
                );
                assert_eq!(
                    striped_sum_i32_as_f64(&ints32).to_bits(),
                    reference_striped(&ints32_as_floats).to_bits()
                );
            });
        }
    }
}

#[test]
fn reordered_sum_differs_from_the_ordered_sum_where_rounding_differs() {
    let values = floats(4099, 99);
    let ordered = values.iter().fold(0.0, |sum, value| sum + value);
    assert_ne!(striped_sum(&values).to_bits(), ordered.to_bits());
}

#[test]
fn dot_rounds_each_product_and_follows_the_stated_order() {
    for len in LENGTHS {
        let left = floats(len, 3 + len as u64);
        let right = floats(len, 5 + len as u64);
        let products = left
            .iter()
            .zip(&right)
            .map(|(l, r)| l * r)
            .collect::<Vec<_>>();
        let left32 = floats32(len, 17 + len as u64);
        let right32 = floats32(len, 19 + len as u64);
        let products32 = left32
            .iter()
            .zip(&right32)
            .map(|(l, r)| l * r)
            .collect::<Vec<_>>();
        for level in LEVELS {
            with_kernel_level(level, || {
                assert_eq!(
                    striped_dot(&left, &right).to_bits(),
                    reference_striped(&products).to_bits(),
                    "float64 dot of {len} at {level:?}"
                );
                assert_eq!(
                    striped_dot(&left32, &right32).to_bits(),
                    reference_striped(&products32).to_bits(),
                    "float32 dot of {len} at {level:?}"
                );
            });
        }
    }
}

#[test]
fn float_extremes_match_a_scan_and_order_the_zeros() {
    for len in LENGTHS.into_iter().filter(|len| *len > 0) {
        let values = floats(len, 23 + len as u64);
        let least = values.iter().copied().fold(f64::INFINITY, f64::min);
        let greatest = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        for level in LEVELS {
            with_kernel_level(level, || {
                assert_eq!(float_min(&values), Some(least), "min of {len} at {level:?}");
                assert_eq!(float_max(&values), Some(greatest));
                for position in [0, len / 2, len - 1] {
                    let mut with_nan = values.clone();
                    with_nan[position] = f64::NAN;
                    assert_eq!(float_min(&with_nan), None);
                    assert_eq!(float_max(&with_nan), None);
                }
            });
        }
    }
    for level in LEVELS {
        with_kernel_level(level, || {
            for zeros in [[0.0_f64, -0.0], [-0.0, 0.0]] {
                let least = float_min(&zeros).unwrap();
                assert_eq!(least.to_bits(), (-0.0_f64).to_bits());
                let greatest = float_max(&zeros).unwrap();
                assert_eq!(greatest.to_bits(), 0.0_f64.to_bits());
            }
            // Zeros of both signs among forty elements, past one group.
            let mut mixed = vec![0.0_f64; 40];
            mixed[37] = -0.0;
            assert_eq!(float_min(&mixed).unwrap().to_bits(), (-0.0_f64).to_bits());
            assert_eq!(float_max(&mixed).unwrap().to_bits(), 0);
            let mut mixed = vec![-0.0_f64; 40];
            mixed[3] = 0.0;
            assert_eq!(float_min(&mixed).unwrap().to_bits(), (-0.0_f64).to_bits());
            assert_eq!(float_max(&mixed).unwrap().to_bits(), 0);
            assert_eq!(float_min(&[0.0_f32, 0.0]).unwrap().to_bits(), 0);
            assert_eq!(
                float_max(&[-0.0_f32, -0.0]).unwrap().to_bits(),
                (-0.0_f32).to_bits()
            );
            assert_eq!(
                float_min(&[2.0_f32, -0.0, 0.0, 1.0]).unwrap().to_bits(),
                (-0.0_f32).to_bits()
            );
            assert_eq!(
                float_max(&[-2.0_f32, -0.0, 0.0, -1.0]).unwrap().to_bits(),
                0
            );
            assert_eq!(
                float_min(&[f64::INFINITY, f64::NEG_INFINITY]),
                Some(f64::NEG_INFINITY)
            );
        });
    }
}

#[test]
fn integer_dot_is_exact_and_reports_the_first_overflow() {
    for len in LENGTHS {
        let left = integers(len, 29 + len as u64, 1 << 20);
        let right = integers(len, 31 + len as u64, 1 << 20);
        let exact = left
            .iter()
            .zip(&right)
            .map(|(l, r)| i128::from(*l) * i128::from(*r))
            .sum::<i128>();
        let left32 = left.iter().map(|v| (*v >> 8) as i32).collect::<Vec<_>>();
        let right32 = right.iter().map(|v| (*v >> 8) as i32).collect::<Vec<_>>();
        let exact32 = left32
            .iter()
            .zip(&right32)
            .map(|(l, r)| i128::from(*l) * i128::from(*r))
            .sum::<i128>();
        for level in LEVELS {
            with_kernel_level(level, || {
                assert_eq!(checked_integer_dot(&left, &right), Ok(exact as i64));
                assert_eq!(checked_integer_dot(&left32, &right32), Ok(exact32 as i32));
            });
        }
    }
    for level in LEVELS {
        with_kernel_level(level, || {
            // A product that overflows on its own.
            let mut left = vec![1_i64; 600];
            let right = vec![2_i64; 600];
            left[417] = i64::MAX;
            assert_eq!(checked_integer_dot(&left, &right), Err(417));
            // Products that fit whose running sum overflows.
            let left = vec![i64::MAX / 4; 10];
            let right = vec![1_i64; 10];
            assert_eq!(checked_integer_dot(&left, &right), Err(4));
            // A running sum that overflows and would come back in range.
            let left = [i32::MAX, 1, -5];
            let right = [1, 1, 1];
            assert_eq!(checked_integer_dot(&left, &right), Err(1));
            // The extremes fit.
            assert_eq!(checked_integer_dot(&[i32::MIN], &[1]), Ok(i32::MIN));
            assert_eq!(
                checked_integer_dot(&[i64::MIN, 1], &[1, 1]),
                Ok(i64::MIN + 1)
            );
            assert_eq!(checked_integer_dot(&[i64::MIN], &[-1]), Err(0));
            // A block with large products is added element by element.
            let left = vec![1_i64 << 61; 300];
            let mut right = vec![1_i64; 300];
            for value in right.iter_mut().skip(1).step_by(2) {
                *value = -1;
            }
            assert_eq!(checked_integer_dot(&left, &right), Ok(0));
        });
    }
}

/// The matrix product with each element's products added in ascending
/// order of the inner index.
fn reference_matmul(left: &[f64], right: &[f64], n: usize, k: usize, m: usize) -> Vec<f64> {
    let mut output = vec![0.0; n * m];
    for i in 0..n {
        for j in 0..m {
            let mut sum = 0.0;
            for inner in 0..k {
                sum += left[i * k + inner] * right[inner * m + j];
            }
            output[i * m + j] = sum;
        }
    }
    output
}

#[test]
fn float_matmul_adds_in_inner_order_at_every_level() {
    for (n, k, m) in [
        (0, 3, 2),
        (2, 0, 3),
        (3, 2, 0),
        (1, 1, 1),
        (2, 3, 4),
        (5, 7, 3),
        (9, 70, 11),
        (3, 130, 600),
        (17, 5, 1030),
        // Whole tiles only, a tile and a leftover column, and two blocks
        // of the inner index with an odd row left over.
        (4, 6, 32),
        (6, 9, 17),
        (7, 259, 50),
    ] {
        let left = floats(n * k, 37 + (n * k) as u64);
        let right = floats(k * m, 41 + (k * m) as u64);
        let expected = reference_matmul(&left, &right, n, k, m);
        let left32 = floats32(n * k, 43);
        let right32 = floats32(k * m, 47);
        let mut expected32 = vec![0.0_f32; n * m];
        for i in 0..n {
            for j in 0..m {
                let mut sum = 0.0_f32;
                for inner in 0..k {
                    sum += left32[i * k + inner] * right32[inner * m + j];
                }
                expected32[i * m + j] = sum;
            }
        }
        for level in LEVELS {
            with_kernel_level(level, || {
                let mut output = vec![0.0; n * m];
                float_matmul(&left, &right, &mut output, n, k, m);
                assert_eq!(
                    output.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                    expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                    "{n}x{k} by {k}x{m} at {level:?}"
                );
                let mut output32 = vec![0.0_f32; n * m];
                float_matmul(&left32, &right32, &mut output32, n, k, m);
                assert_eq!(
                    output32.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                    expected32.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
                );
                if m == 1 {
                    let mut by_vector = vec![0.0; n];
                    float_matrix_vector(&left, &right, &mut by_vector, n, k);
                    assert_eq!(by_vector, expected);
                }
            });
        }
    }
    for (n, k) in [(0, 4), (1, 5), (4, 9), (7, 33), (10, 0)] {
        let left = floats(n * k, 53);
        let right = floats(k, 59);
        let expected = reference_matmul(&left, &right, n, k, 1);
        for level in LEVELS {
            with_kernel_level(level, || {
                let mut output = vec![0.0; n];
                float_matrix_vector(&left, &right, &mut output, n, k);
                assert_eq!(
                    output.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                    expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
                );
            });
        }
    }
}

#[test]
fn float_matmul_gives_the_same_bits_on_any_number_of_threads() {
    for (n, k, m) in [(1, 4, 40), (2, 3, 16), (9, 140, 70), (33, 20, 35)] {
        let left = floats(n * k, 61 + n as u64);
        let right = floats(k * m, 67 + m as u64);
        let expected = reference_matmul(&left, &right, n, k, m);
        let left32 = floats32(n * k, 71);
        let right32 = floats32(k * m, 73);
        for level in LEVELS {
            with_kernel_level(level, || {
                let mut one32 = vec![0.0_f32; n * m];
                float_matmul_on(1, &left32, &right32, &mut one32, n, k, m);
                for threads in [1, 2, 3, 8, 64] {
                    let mut output = vec![0.0; n * m];
                    float_matmul_on(threads, &left, &right, &mut output, n, k, m);
                    assert_eq!(
                        output.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                        expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                        "{n}x{k} by {k}x{m} on {threads} threads at {level:?}"
                    );
                    let mut output32 = vec![0.0_f32; n * m];
                    float_matmul_on(threads, &left32, &right32, &mut output32, n, k, m);
                    assert_eq!(
                        output32.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                        one32.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
                    );
                }
            });
        }
    }
    // The tiles and the row kernel agree on every target, whichever one
    // the CPU's level picks.
    for (n, k, m) in [(5, 131, 70), (2, 3, 32), (3, 2, 15), (0, 4, 40), (4, 0, 40)] {
        let left = floats(n * k, 79);
        let right = floats(k * m, 83);
        let expected = reference_matmul(&left, &right, n, k, m);
        let left32 = floats32(n * k, 89);
        let right32 = floats32(k * m, 97);
        let mut by_row32 = vec![0.0_f32; n * m];
        float_matmul_rows(false, &left32, &right32, &mut by_row32, n, k, m);
        for tiled in [false, true] {
            let mut output = vec![0.0; n * m];
            float_matmul_rows(tiled, &left, &right, &mut output, n, k, m);
            assert_eq!(
                output.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                "{n}x{k} by {k}x{m}, tiled {tiled}"
            );
            let mut output32 = vec![0.0_f32; n * m];
            float_matmul_rows(tiled, &left32, &right32, &mut output32, n, k, m);
            assert_eq!(
                output32.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                by_row32.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
            );
        }
    }
    // An empty product starts no thread.
    let mut nothing: Vec<f64> = Vec::new();
    float_matmul_on(4, &[], &[], &mut nothing, 6, 0, 0);
    assert!(nothing.is_empty());
}

#[test]
fn matmul_threads_follow_the_work_and_the_limit() {
    // 64 by 64 by 64 is an eighth of one thread's share.
    assert_eq!(matmul_threads(64, 64, 64, 8), 1);
    // 256 cubed is eight shares.
    assert_eq!(matmul_threads(256, 256, 256, 16), 8);
    assert_eq!(matmul_threads(256, 256, 256, 4), 4);
    assert_eq!(matmul_threads(256, 256, 256, 1), 1);
    // Two rows are one tile, whatever the work.
    assert_eq!(matmul_threads(2, 4096, 4096, 8), 1);
    assert_eq!(matmul_threads(7, 4096, 4096, 8), 3);
    assert_eq!(matmul_threads(usize::MAX, usize::MAX, 2, 8), 8);
    assert_eq!(matmul_threads(0, 5, 5, 8), 1);

    assert_eq!(thread_limit_for(None, 6), 6);
    assert_eq!(thread_limit_for(Some("3"), 6), 3);
    assert_eq!(thread_limit_for(Some(" 1 "), 6), 1);
    assert_eq!(thread_limit_for(Some("0"), 6), 6);
    assert_eq!(thread_limit_for(Some("many"), 6), 6);
    assert_eq!(thread_limit_for(Some("500"), 6), 16);
    assert_eq!(thread_limit_for(None, 128), 16);
}

#[test]
fn integer_matmul_is_exact_and_reports_an_overflowing_element() {
    for (n, k, m) in [(0, 2, 2), (1, 1, 1), (3, 4, 5), (6, 9, 40)] {
        let left = integers(n * k, 61, 1000);
        let right = integers(k * m, 67, 1000);
        let mut expected = vec![0_i64; n * m];
        for i in 0..n {
            for j in 0..m {
                for inner in 0..k {
                    expected[i * m + j] += left[i * k + inner] * right[inner * m + j];
                }
            }
        }
        let left32 = left.iter().map(|v| *v as i32).collect::<Vec<_>>();
        let right32 = right.iter().map(|v| *v as i32).collect::<Vec<_>>();
        for level in LEVELS {
            with_kernel_level(level, || {
                let mut output = vec![0_i64; n * m];
                assert_eq!(integer_matmul(&left, &right, &mut output, n, k, m), Ok(()));
                assert_eq!(output, expected);
                let mut output32 = vec![0_i32; n * m];
                assert_eq!(
                    integer_matmul(&left32, &right32, &mut output32, n, k, m),
                    Ok(())
                );
                assert_eq!(
                    output32,
                    expected.iter().map(|v| *v as i32).collect::<Vec<_>>()
                );
            });
        }
    }
    for level in LEVELS {
        with_kernel_level(level, || {
            // The product at row 1, column 2 overflows.
            let left = [1_i32, 1, i32::MAX, 1];
            let right = [1_i32, 1, 2, 1, 1, 1];
            let mut output = [0_i32; 6];
            assert_eq!(integer_matmul(&left, &right, &mut output, 2, 2, 3), Err(5));
            // The running sum at row 0, column 1 overflows.
            let left = [i64::MAX, 1];
            let right = [0_i64, 1, 0, 1];
            let mut output = [0_i64; 2];
            assert_eq!(integer_matmul(&left, &right, &mut output, 1, 2, 2), Err(1));
        });
    }
}

#[test]
fn array_reductions_and_products_use_the_kernels() {
    let values = floats(1000, 71);
    let other = floats(1000, 73);
    let left = float_array(&[1000], values.clone());
    let right = float_array(&[1000], other.clone());
    let products = values
        .iter()
        .zip(&other)
        .map(|(l, r)| l * r)
        .collect::<Vec<_>>();
    assert_eq!(
        float_bits(&left.reduce(ArrayReduction::FastSum).unwrap()),
        reference_striped(&values).to_bits()
    );
    assert_eq!(
        float_bits(&left.reduce(ArrayReduction::FastMean).unwrap()),
        (reference_striped(&values) / 1000.0).to_bits()
    );
    assert_eq!(
        float_bits(&left.dot(&right).unwrap()),
        reference_striped(&products).to_bits()
    );
    // The ordered reductions keep their order.
    assert_eq!(
        float_bits(&left.reduce(ArrayReduction::Sum).unwrap()),
        values.iter().fold(0.0, |sum, value| sum + value).to_bits()
    );

    let narrow = floats32(100, 79);
    let narrow_array = array(&[100], ArrayStorage::Float32(narrow.clone().into()));
    assert_eq!(
        float_bits(&narrow_array.reduce(ArrayReduction::FastSum).unwrap()),
        f64::from(reference_striped(&narrow)).to_bits()
    );
    let widened = narrow.iter().map(|v| f64::from(*v)).collect::<Vec<_>>();
    assert_eq!(
        float_bits(&narrow_array.reduce(ArrayReduction::FastMean).unwrap()),
        (reference_striped(&widened) / 100.0).to_bits()
    );
    assert_eq!(
        float_bits(&narrow_array.dot(&narrow_array).unwrap()),
        f64::from(reference_striped(
            &narrow.iter().map(|v| v * v).collect::<Vec<_>>()
        ))
        .to_bits()
    );

    // NaN and infinities.
    let with_nan = float_array(&[3], vec![1.0, f64::NAN, 2.0]);
    for reduction in [
        ArrayReduction::FastSum,
        ArrayReduction::FastMean,
        ArrayReduction::Min,
        ArrayReduction::Max,
    ] {
        assert_eq!(
            float_bits(&with_nan.reduce(reduction).unwrap()),
            f64::NAN.to_bits()
        );
    }
    assert_eq!(
        float_bits(&with_nan.dot(&with_nan).unwrap()),
        f64::NAN.to_bits()
    );
    let opposite = float_array(&[2], vec![f64::INFINITY, f64::NEG_INFINITY]);
    assert_eq!(
        float_bits(&opposite.reduce(ArrayReduction::FastSum).unwrap()),
        f64::NAN.to_bits()
    );

    // Integer Arrays: the fast forms are the checked ones.
    let ints = array(&[4], ArrayStorage::Int64(vec![1, 2, 3, 4].into()));
    assert_eq!(
        ints.reduce(ArrayReduction::FastSum).unwrap(),
        Value::Int(IntegerValue::from_i64(10))
    );
    assert_eq!(
        ints.reduce(ArrayReduction::FastMean).unwrap(),
        Value::Float(2.5)
    );
    assert_eq!(
        ints.dot(&ints).unwrap(),
        Value::Int(IntegerValue::from_i64(30))
    );
    let ints32 = array(&[3], ArrayStorage::Int32(vec![5, -7, 9].into()));
    assert_eq!(
        ints32.reduce(ArrayReduction::FastSum).unwrap(),
        Value::Int(IntegerValue::from_i32(7))
    );
    assert_eq!(
        ints32.reduce(ArrayReduction::FastMean).unwrap(),
        Value::Float(7.0 / 3.0)
    );
    assert_eq!(
        ints32.dot(&ints32).unwrap(),
        Value::Int(IntegerValue::from_i32(155))
    );
    let huge = array(&[2], ArrayStorage::Int64(vec![i64::MAX, 2].into()));
    let error = huge.dot(&huge).unwrap_err();
    assert_eq!(error.code, "AU4002");
    assert!(error
        .message
        .contains("array dot overflowed at flat index 0"));
    let error = huge.reduce(ArrayReduction::FastSum).unwrap_err();
    assert_eq!(error.code, "AU4002");

    // Empty Arrays.
    let empty = float_array(&[0], vec![]);
    assert_eq!(
        empty.reduce(ArrayReduction::FastSum).unwrap(),
        Value::Float(0.0)
    );
    assert_eq!(empty.dot(&empty).unwrap(), Value::Float(0.0));
    let error = empty.reduce(ArrayReduction::FastMean).unwrap_err();
    assert_eq!(error.code, "AU4007");
    assert!(error.message.contains("fast_mean"));
    let empty_ints = array(&[0], ArrayStorage::Int32(Vec::new().into()));
    assert_eq!(
        empty_ints.reduce(ArrayReduction::FastSum).unwrap(),
        Value::Int(IntegerValue::from_i32(0))
    );
    let empty_ints = array(&[0], ArrayStorage::Int64(Vec::new().into()));
    assert_eq!(
        empty_ints.reduce(ArrayReduction::FastSum).unwrap(),
        Value::Int(IntegerValue::from_i64(0))
    );

    // Shape and dtype mismatches.
    let error = left.dot(&float_array(&[999], floats(999, 1))).unwrap_err();
    assert_eq!(error.code, "AU4007");
    assert!(error
        .message
        .contains("array dot requires the same shape and dtype"));
    assert_eq!(left.dot(&ints).unwrap_err().code, "AU4007");
    assert_eq!(ArrayReduction::from_code(4), Some(ArrayReduction::FastSum));
    assert_eq!(ArrayReduction::from_code(5), Some(ArrayReduction::FastMean));
    assert_eq!(ArrayReduction::from_code(6), None);
}

#[test]
fn array_matmul_checks_shapes_and_multiplies_each_dtype() {
    let a = float_array(&[2, 3], vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    let b = float_array(&[3, 2], vec![7.0, 8.0, 9.0, 10.0, 11.0, 12.0]);
    let product = a.matmul(&b).unwrap();
    assert_eq!(&*product.shape, &[2, 2]);
    assert_eq!(
        product.storage(),
        ArrayStorage::Float64(vec![58.0, 64.0, 139.0, 154.0].into())
    );
    let v = float_array(&[3], vec![1.0, 0.5, -1.0]);
    let by_vector = a.matmul(&v).unwrap();
    assert_eq!(&*by_vector.shape, &[2]);
    assert_eq!(
        by_vector.storage(),
        ArrayStorage::Float64(vec![-1.0, 0.5].into())
    );
    let w = float_array(&[2], vec![1.0, -1.0]);
    let from_vector = w.matmul(&a).unwrap();
    assert_eq!(&*from_vector.shape, &[3]);
    assert_eq!(
        from_vector.storage(),
        ArrayStorage::Float64(vec![-3.0, -3.0, -3.0].into())
    );

    let a32 = array(
        &[2, 2],
        ArrayStorage::Float32(vec![1.0, 2.0, 3.0, 4.0].into()),
    );
    assert_eq!(
        a32.matmul(&a32).unwrap().storage(),
        ArrayStorage::Float32(vec![7.0, 10.0, 15.0, 22.0].into())
    );
    let v32 = array(&[2], ArrayStorage::Float32(vec![1.0, 1.0].into()));
    assert_eq!(
        a32.matmul(&v32).unwrap().storage(),
        ArrayStorage::Float32(vec![3.0, 7.0].into())
    );
    let i64s = array(&[2, 2], ArrayStorage::Int64(vec![1, 2, 3, 4].into()));
    assert_eq!(
        i64s.matmul(&i64s).unwrap().storage(),
        ArrayStorage::Int64(vec![7, 10, 15, 22].into())
    );
    let i32s = array(&[2, 2], ArrayStorage::Int32(vec![1, 2, 3, 4].into()));
    assert_eq!(
        i32s.matmul(&i32s).unwrap().storage(),
        ArrayStorage::Int32(vec![7, 10, 15, 22].into())
    );
    let big = array(&[1, 1], ArrayStorage::Int32(vec![i32::MAX].into()));
    let error = big.matmul(&big).unwrap_err();
    assert_eq!(error.code, "AU4002");
    assert!(error
        .message
        .contains("array matmul overflowed at flat index 0"));
    let big = array(&[1, 1], ArrayStorage::Int64(vec![i64::MAX].into()));
    assert_eq!(big.matmul(&big).unwrap_err().code, "AU4002");

    // A zero-sized result.
    let none = float_array(&[0, 3], vec![]);
    let empty = none.matmul(&b).unwrap();
    assert_eq!(&*empty.shape, &[0, 2]);
    assert!(empty.is_empty());

    for (left, right) in [
        (&a, &a),
        (&v, &v),
        (&a, &w),
        (&v, &a),
        (&a, &i64s),
        (&float_array(&[1, 2, 3], vec![0.0; 6]), &b),
    ] {
        let error = left.matmul(right).unwrap_err();
        assert_eq!(error.code, "AU4007");
        assert!(error.message.contains("array matmul requires shapes"));
    }
}

#[test]
fn elementwise_kernels_agree_at_every_level() {
    use crate::runtime_value::{ArrayBinaryOp, IntegerArithmeticMode};
    let left = float_array(&[777], floats(777, 83));
    let right = float_array(&[777], floats(777, 89));
    let ints = array(
        &[777],
        ArrayStorage::Int64(integers(777, 97, 1 << 30).into()),
    );
    let results = LEVELS.map(|level| {
        with_kernel_level(level, || {
            (
                left.binary(&right, ArrayBinaryOp::Mul, IntegerArithmeticMode::Checked)
                    .unwrap(),
                left.scalar_binary(
                    &Value::Float(1.5),
                    true,
                    ArrayBinaryOp::Sub,
                    IntegerArithmeticMode::Checked,
                )
                .unwrap(),
                ints.binary(&ints, ArrayBinaryOp::Mul, IntegerArithmeticMode::Checked)
                    .unwrap(),
                ints.reduce(ArrayReduction::Sum).unwrap(),
                ints.reduce(ArrayReduction::Min).unwrap(),
                ints.reduce(ArrayReduction::Max).unwrap(),
            )
        })
    });
    assert_eq!(results[0], results[1]);
}
