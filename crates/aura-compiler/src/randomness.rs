use std::collections::TryReserveError;
use std::fmt;

const SPLITMIX64_INCREMENT: u64 = 0x9e37_79b9_7f4a_7c15;
const SPLITMIX64_MIX_1: u64 = 0xbf58_476d_1ce4_e5b9;
const SPLITMIX64_MIX_2: u64 = 0x94d0_49bb_1331_11eb;
const UNIT_FLOAT_SCALE: f64 = 1.0 / ((1_u64 << 53) as f64);
pub(crate) const MAX_SECURE_BYTES_LEN: usize = i32::MAX as usize;

#[derive(Clone, Debug)]
pub(crate) struct DeterministicRng {
    state: [u64; 4],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct InvalidRandomRange;

#[derive(Debug)]
pub(crate) enum SecureRandomError {
    InvalidRange,
    RequestExceedsCeiling { requested: usize, maximum: usize },
    Allocation(TryReserveError),
    Entropy(getrandom::Error),
}

impl fmt::Display for SecureRandomError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRange => {
                formatter.write_str("the lower bound must be below the upper bound")
            }
            Self::RequestExceedsCeiling { requested, maximum } => write!(
                formatter,
                "`random.secure_bytes(n)` count `{requested}` exceeds the secure-random request ceiling `{maximum}`"
            ),
            Self::Allocation(error) => {
                write!(formatter, "could not allocate secure bytes: {error}")
            }
            Self::Entropy(error) => write!(formatter, "OS entropy is unavailable: {error}"),
        }
    }
}

impl DeterministicRng {
    pub(crate) fn from_seed(seed: i64) -> Self {
        let mut splitmix_state = seed as u64;
        Self {
            state: std::array::from_fn(|_| splitmix64_next(&mut splitmix_state)),
        }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        let result = self.state[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let shifted = self.state[1] << 17;

        self.state[2] ^= self.state[0];
        self.state[3] ^= self.state[1];
        self.state[1] ^= self.state[2];
        self.state[0] ^= self.state[3];
        self.state[2] ^= shifted;
        self.state[3] = self.state[3].rotate_left(45);

        result
    }

    pub(crate) fn next_int(&mut self, lower: i64, upper: i64) -> Result<i64, InvalidRandomRange> {
        let span = checked_span(lower, upper).ok_or(InvalidRandomRange)?;
        let offset = sample_bounded_with(span, || self.next_u64());
        Ok(add_offset(lower, offset))
    }

    pub(crate) fn next_float(&mut self) -> f64 {
        unit_float_from_u64(self.next_u64())
    }

    pub(crate) fn shuffle<T>(&mut self, values: &mut [T]) {
        for upper in (1..values.len()).rev() {
            let selected = sample_bounded_with((upper as u64) + 1, || self.next_u64());
            values.swap(upper, selected as usize);
        }
    }
}

pub(crate) fn secure_int(lower: i64, upper: i64) -> Result<i64, SecureRandomError> {
    secure_int_with(lower, upper, getrandom::getrandom)
}

pub(crate) fn secure_bytes(length: usize) -> Result<Vec<u8>, SecureRandomError> {
    secure_bytes_with(length, getrandom::getrandom)
}

fn splitmix64_next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(SPLITMIX64_INCREMENT);
    let mut mixed = *state;
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(SPLITMIX64_MIX_1);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(SPLITMIX64_MIX_2);
    mixed ^ (mixed >> 31)
}

fn checked_span(lower: i64, upper: i64) -> Option<u64> {
    if lower < upper {
        Some(((upper as i128) - (lower as i128)) as u64)
    } else {
        None
    }
}

fn add_offset(lower: i64, offset: u64) -> i64 {
    ((lower as i128) + (offset as i128)) as i64
}

fn sample_bounded_with<F>(span: u64, mut next: F) -> u64
where
    F: FnMut() -> u64,
{
    let rejection_threshold = span.wrapping_neg() % span;
    loop {
        let value = next();
        if value >= rejection_threshold {
            return value % span;
        }
    }
}

fn unit_float_from_u64(value: u64) -> f64 {
    ((value >> 11) as f64) * UNIT_FLOAT_SCALE
}

fn secure_int_with<F>(lower: i64, upper: i64, mut fill: F) -> Result<i64, SecureRandomError>
where
    F: FnMut(&mut [u8]) -> Result<(), getrandom::Error>,
{
    let span = checked_span(lower, upper).ok_or(SecureRandomError::InvalidRange)?;
    let rejection_threshold = span.wrapping_neg() % span;
    loop {
        let mut bytes = [0_u8; size_of::<u64>()];
        fill(&mut bytes).map_err(SecureRandomError::Entropy)?;
        let value = u64::from_le_bytes(bytes);
        if value >= rejection_threshold {
            return Ok(add_offset(lower, value % span));
        }
    }
}

fn secure_bytes_with<F>(length: usize, mut fill: F) -> Result<Vec<u8>, SecureRandomError>
where
    F: FnMut(&mut [u8]) -> Result<(), getrandom::Error>,
{
    if length > MAX_SECURE_BYTES_LEN {
        return Err(SecureRandomError::RequestExceedsCeiling {
            requested: length,
            maximum: MAX_SECURE_BYTES_LEN,
        });
    }
    if length == 0 {
        return Ok(Vec::new());
    }

    let mut bytes = allocate_secure_bytes(length)?;
    fill(&mut bytes).map_err(SecureRandomError::Entropy)?;
    Ok(bytes)
}

fn allocate_secure_bytes(length: usize) -> Result<Vec<u8>, SecureRandomError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(SecureRandomError::Allocation)?;
    bytes.resize(length, 0);
    Ok(bytes)
}

#[cfg(test)]
#[path = "randomness_tests.rs"]
mod tests;
