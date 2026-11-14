extern crate std;

use super::{compare, copy, copy_overlapping};
use std::vec::Vec;

/// Bytes that differ from their neighbors and from the fill byte.
fn pattern(length: usize, seed: u8) -> Vec<u8> {
    (0..length)
        .map(|index| (index as u8).wrapping_mul(31).wrapping_add(seed) | 1)
        .collect()
}

const SIZES: [usize; 12] = [
    300, 511, 512, 513, 1000, 2047, 2048, 2049, 4096, 5000, 9999, 70_000,
];

fn sizes() -> impl Iterator<Item = usize> {
    (0..=270).chain(SIZES)
}

#[test]
fn copy_writes_exactly_the_bytes_asked_for() {
    for count in sizes() {
        for (source_offset, destination_offset) in [(0, 0), (1, 0), (0, 3), (5, 7)] {
            let source = pattern(count + 16, 7);
            let mut destination = std::vec![0u8; count + 32];
            // SAFETY: both runs lie inside their buffers, which are
            // separate allocations.
            unsafe {
                copy(
                    destination.as_mut_ptr().add(8 + destination_offset),
                    source.as_ptr().add(source_offset),
                    count,
                );
            }
            let start = 8 + destination_offset;
            assert_eq!(
                &destination[start..start + count],
                &source[source_offset..source_offset + count],
                "count {count}"
            );
            assert!(
                destination[..start].iter().all(|byte| *byte == 0),
                "count {count}"
            );
            assert!(
                destination[start + count..].iter().all(|byte| *byte == 0),
                "count {count}"
            );
        }
    }
}

#[test]
fn overlapping_copy_moves_bytes_in_either_direction() {
    let distances: [usize; 9] = [0, 1, 2, 7, 8, 15, 16, 31, 33];
    for count in sizes().filter(|count| *count <= 5000) {
        for distance in distances.into_iter().chain([count / 2, count, count + 5]) {
            for down in [false, true] {
                let mut buffer = pattern(count + distance + 64, 3);
                let expected = buffer.clone();
                let (source, destination) = if down {
                    (16 + distance, 16)
                } else {
                    (16, 16 + distance)
                };
                // SAFETY: both runs lie inside `buffer`.
                unsafe {
                    let base = buffer.as_mut_ptr();
                    copy_overlapping(base.add(destination), base.add(source), count);
                }
                assert_eq!(
                    &buffer[destination..destination + count],
                    &expected[source..source + count],
                    "count {count} distance {distance} down {down}"
                );
                // Bytes outside the destination run are untouched.
                for (index, byte) in buffer.iter().enumerate() {
                    if index < destination || index >= destination + count {
                        assert_eq!(
                            *byte, expected[index],
                            "count {count} distance {distance} down {down} index {index}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn compare_orders_runs_by_their_first_different_byte() {
    for count in sizes().filter(|count| *count <= 600) {
        let left = pattern(count + 3, 11);
        for offset in [0, 1, 3] {
            let left = &left[offset..offset + count];
            let mut right = left.to_vec();
            // SAFETY: both runs are `count` bytes long.
            assert_eq!(unsafe { compare(left.as_ptr(), right.as_ptr(), count) }, 0);
            for position in (0..count)
                .filter(|position| *position < 20 || *position + 20 >= count || position % 37 == 0)
            {
                let original = right[position];
                // A later difference in the other direction must not win.
                let later = (position + 1 < count).then(|| position + 1);
                for changed in [original.wrapping_add(2), original - 1] {
                    // The left run sorts first when the right byte grew.
                    let sign = if changed > original { -1 } else { 1 };
                    right[position] = changed;
                    if let Some(later) = later {
                        right[later] = if sign < 0 { 0 } else { 255 };
                    }
                    // SAFETY: both runs are `count` bytes long.
                    let found = unsafe { compare(left.as_ptr(), right.as_ptr(), count) };
                    let expected = left.cmp(&right[..]) as i32;
                    assert_eq!(
                        found.signum(),
                        expected,
                        "count {count} position {position}"
                    );
                    assert_eq!(expected, sign, "the test's own expectation");
                    if let Some(later) = later {
                        right[later] = left[later];
                    }
                }
                right[position] = original;
            }
        }
    }
}

#[test]
fn compare_treats_bytes_as_unsigned() {
    let low = [0x01u8; 24];
    let mut high = low;
    for position in [0, 5, 8, 17, 23] {
        high[position] = 0xfe;
        // SAFETY: both runs are 24 bytes long.
        unsafe {
            assert!(compare(low.as_ptr(), high.as_ptr(), 24) < 0, "{position}");
            assert!(compare(high.as_ptr(), low.as_ptr(), 24) > 0, "{position}");
            assert_eq!(
                compare(high.as_ptr(), low.as_ptr(), position),
                0,
                "{position}"
            );
        }
        high[position] = 0x01;
    }
}
