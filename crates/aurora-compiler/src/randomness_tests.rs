use std::cell::Cell;

use super::*;

const SEED_42_STATE: [u64; 4] = [
    0xbdd7_3226_2feb_6e95,
    0x28ef_e333_b266_f103,
    0x4752_6757_130f_9f52,
    0x581c_e1ff_0e4a_e394,
];

const SEED_42_OUTPUTS: [u64; 5] = [
    1_546_998_764_402_558_742,
    6_990_951_692_964_543_102,
    12_544_586_762_248_559_009,
    17_057_574_109_182_124_193,
    18_295_552_978_065_317_476,
];

#[test]
fn seed_expansion_uses_four_splitmix64_steps() {
    let rng = DeterministicRng::from_seed(42);

    assert_eq!(rng.state, SEED_42_STATE);
}

#[test]
fn signed_seed_is_reinterpreted_as_twos_complement_u64() {
    let rng = DeterministicRng::from_seed(-1);

    assert_eq!(
        rng.state,
        [
            0xe4d9_7177_1b65_2c20,
            0xe99f_f867_dbf6_82c9,
            0x382f_f84c_b272_81e9,
            0x6d1d_b36c_cba9_82d2,
        ]
    );
}

#[test]
fn xoshiro256_star_star_matches_seed_42_oracle() {
    let mut rng = DeterministicRng::from_seed(42);

    let actual = std::array::from_fn(|_| rng.next_u64());

    assert_eq!(actual, SEED_42_OUTPUTS);
}

#[test]
fn next_int_is_half_open_and_matches_seed_42_oracle_at_i64_boundaries() {
    let mut rng = DeterministicRng::from_seed(42);

    assert_eq!(rng.next_int(0, 10), Ok(2));
    assert_eq!(rng.next_int(-5, 6), Ok(2));
    assert_eq!(
        rng.next_int(i64::MIN, i64::MAX),
        Ok(3_321_214_725_393_783_201)
    );
    assert_eq!(rng.next_int(7, 8), Ok(7));
    assert_eq!(rng.next_u64(), SEED_42_OUTPUTS[4]);
}

#[test]
fn invalid_next_int_range_is_rejected_before_consuming_a_draw() {
    let mut rng = DeterministicRng::from_seed(42);

    assert_eq!(rng.next_int(4, 4), Err(InvalidRandomRange));
    assert_eq!(rng.next_int(5, 4), Err(InvalidRandomRange));
    assert_eq!(rng.next_u64(), SEED_42_OUTPUTS[0]);
}

#[test]
fn bounded_sampling_rejects_the_low_biased_tail() {
    let draws = [0_u64, 5, 6];
    let mut index = 0;

    let sampled = sample_bounded_with(10, || {
        let value = draws[index];
        index += 1;
        value
    });

    assert_eq!(sampled, 6);
    assert_eq!(index, 3);
}

#[test]
fn next_float_uses_the_high_53_bits_and_is_half_open() {
    let mut rng = DeterministicRng::from_seed(42);

    assert_eq!(rng.next_float(), 0.083_862_971_059_882_16);
    assert_eq!(rng.next_float(), 0.378_980_250_662_668_6);
    assert_eq!(rng.next_float(), 0.680_043_411_028_139_4);

    assert_eq!(unit_float_from_u64(0), 0.0);
    let value = unit_float_from_u64(u64::MAX);
    assert!(value < 1.0);
    assert_eq!(value, 1.0 - f64::EPSILON / 2.0);
}

#[test]
fn shuffle_is_descending_fisher_yates_with_seed_42() {
    let mut rng = DeterministicRng::from_seed(42);
    let mut values = [0, 1, 2, 3, 4, 5];

    rng.shuffle(&mut values);

    assert_eq!(values, [3, 5, 4, 1, 2, 0]);
}

#[test]
fn shuffle_of_zero_or_one_elements_consumes_no_draws() {
    for mut values in [vec![], vec![17]] {
        let mut rng = DeterministicRng::from_seed(42);
        let expected = values.clone();

        rng.shuffle(&mut values);

        assert_eq!(values, expected);
        assert_eq!(rng.next_u64(), SEED_42_OUTPUTS[0]);
    }
}

#[test]
fn secure_int_uses_injected_entropy_and_rejection_sampling() {
    let draws = Cell::new(0);

    let sampled = secure_int_with(0, 10, |bytes| {
        let draw = match draws.get() {
            0 => 0_u64,
            1 => 5_u64,
            _ => 16_u64,
        };
        draws.set(draws.get() + 1);
        bytes.copy_from_slice(&draw.to_le_bytes());
        Ok(())
    });

    assert_eq!(sampled.unwrap(), 6);
    assert_eq!(draws.get(), 3);
}

#[test]
fn secure_int_rejects_invalid_range_without_requesting_entropy() {
    let called = Cell::new(false);

    let result = secure_int_with(9, 9, |_| {
        called.set(true);
        Ok(())
    });

    assert!(matches!(result, Err(SecureRandomError::InvalidRange)));
    assert!(!called.get());
}

#[test]
fn os_entropy_entry_points_handle_prevalidated_no_entropy_cases() {
    assert!(matches!(
        secure_int(1, 1),
        Err(SecureRandomError::InvalidRange)
    ));
    assert_eq!(secure_bytes(0).unwrap(), Vec::<u8>::new());
}

#[test]
fn secure_int_propagates_entropy_failure() {
    let result = secure_int_with(0, 10, |_| Err(getrandom::Error::UNSUPPORTED));

    assert!(matches!(
        result,
        Err(SecureRandomError::Entropy(error))
            if error == getrandom::Error::UNSUPPORTED
    ));
}

#[test]
fn secure_bytes_returns_injected_entropy() {
    let result = secure_bytes_with(4, |bytes| {
        bytes.copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        Ok(())
    });

    assert_eq!(result.unwrap(), [0xde, 0xad, 0xbe, 0xef]);
}

#[test]
fn secure_bytes_zero_length_skips_allocation_and_entropy() {
    let called = Cell::new(false);

    let result = secure_bytes_with(0, |_| {
        called.set(true);
        Ok(())
    });

    assert_eq!(result.unwrap(), Vec::<u8>::new());
    assert!(!called.get());
}

#[test]
fn secure_bytes_rejects_requests_above_its_safety_ceiling_before_allocation_or_entropy() {
    let called = Cell::new(false);
    let requested = i32::MAX as usize + 1;

    let result = secure_bytes_with(requested, |_| {
        called.set(true);
        Ok(())
    });

    let error = result.expect_err("the secure-byte request ceiling must be enforced");
    assert_eq!(
        error.to_string(),
        "`random.secure_bytes(n)` count `2147483648` exceeds the secure-random request ceiling `2147483647`"
    );
    assert!(!called.get());
}

#[test]
fn secure_bytes_reports_entropy_and_allocation_failures() {
    let entropy_failure = secure_bytes_with(8, |_| Err(getrandom::Error::UNSUPPORTED));
    assert!(matches!(
        entropy_failure,
        Err(SecureRandomError::Entropy(error))
            if error == getrandom::Error::UNSUPPORTED
    ));

    let allocation_failure = allocate_secure_bytes(usize::MAX);
    assert!(matches!(
        allocation_failure,
        Err(SecureRandomError::Allocation(_))
    ));
}

#[test]
fn secure_random_errors_explain_invalid_input_and_host_resource_failures() {
    assert_eq!(
        SecureRandomError::InvalidRange.to_string(),
        "the lower bound must be below the upper bound"
    );

    let entropy = getrandom::Error::UNSUPPORTED;
    assert_eq!(
        SecureRandomError::Entropy(entropy).to_string(),
        format!("OS entropy is unavailable: {entropy}")
    );
    let request_ceiling = secure_bytes_with(i32::MAX as usize + 1, |_| {
        unreachable!("an over-ceiling request must fail before requesting entropy")
    })
    .expect_err("the secure-byte request ceiling must be enforced");
    assert_eq!(
        request_ceiling.to_string(),
        "`random.secure_bytes(n)` count `2147483648` exceeds the secure-random request ceiling `2147483647`"
    );

    let allocation = allocate_secure_bytes(usize::MAX)
        .expect_err("an impossible secure byte allocation should fail");
    assert!(allocation
        .to_string()
        .starts_with("could not allocate secure bytes: "));
}
