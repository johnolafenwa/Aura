use std::fmt;
use std::str;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use sha2::{Digest, Sha256};

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
pub(crate) const MAX_BYTES_COLLECTION_LEN: usize = i32::MAX as usize;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BytesDataError {
    InvalidUtf8 { index: usize },
    InvalidHexLength { length: usize },
    InvalidHexDigit { index: usize, byte: u8 },
    InvalidBase64 { index: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BytesResourceError {
    OutputTooLarge { maximum: usize },
    AllocationFailed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BytesCodecError {
    Data(BytesDataError),
    Resource(BytesResourceError),
}

impl fmt::Display for BytesDataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUtf8 { index } => {
                write!(formatter, "invalid UTF-8 at byte {index}")
            }
            Self::InvalidHexLength { length } => {
                write!(
                    formatter,
                    "hexadecimal input must contain an even number of bytes, found {length}"
                )
            }
            Self::InvalidHexDigit { index, byte } => {
                write!(
                    formatter,
                    "invalid hexadecimal byte 0x{byte:02x} at byte {index}"
                )
            }
            Self::InvalidBase64 { index } => {
                write!(formatter, "invalid base64 encoding at byte {index}")
            }
        }
    }
}

impl fmt::Display for BytesResourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputTooLarge { maximum } => write!(
                formatter,
                "byte-codec output exceeds Aurora's maximum collection length of {maximum}"
            ),
            Self::AllocationFailed => {
                formatter.write_str("memory allocation failed while processing bytes")
            }
        }
    }
}

impl fmt::Display for BytesCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Data(error) => error.fmt(formatter),
            Self::Resource(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for BytesDataError {}
impl std::error::Error for BytesResourceError {}
impl std::error::Error for BytesCodecError {}

impl From<BytesDataError> for BytesCodecError {
    fn from(error: BytesDataError) -> Self {
        Self::Data(error)
    }
}

impl From<BytesResourceError> for BytesCodecError {
    fn from(error: BytesResourceError) -> Self {
        Self::Resource(error)
    }
}

pub(crate) fn hex_encoded_len(input_len: usize) -> Result<usize, BytesResourceError> {
    representable_output_len(input_len.checked_mul(2))
}

pub(crate) fn base64_encoded_len(input_len: usize) -> Result<usize, BytesResourceError> {
    let encoded_len = input_len
        .checked_add(2)
        .map(|length| length / 3)
        .and_then(|groups| groups.checked_mul(4));
    representable_output_len(encoded_len)
}

pub(crate) fn string_to_bytes(text: &str) -> Result<Vec<u8>, BytesCodecError> {
    let output_len = representable_output_len(Some(text.len()))?;
    let mut bytes = try_byte_buffer(output_len)?;
    bytes.copy_from_slice(text.as_bytes());
    Ok(bytes)
}

pub(crate) fn string_from_bytes(bytes: &[u8]) -> Result<String, BytesCodecError> {
    let text = str::from_utf8(bytes).map_err(|error| BytesDataError::InvalidUtf8 {
        index: error.valid_up_to(),
    })?;
    representable_output_len(Some(text.len()))?;
    try_owned_string(text).map_err(Into::into)
}

pub(crate) fn hex_encode(bytes: &[u8]) -> Result<String, BytesCodecError> {
    let output_len = hex_encoded_len(bytes.len())?;
    let mut output = try_string_with_capacity(output_len)?;
    for byte in bytes {
        output.push(HEX_DIGITS[(byte >> 4) as usize] as char);
        output.push(HEX_DIGITS[(byte & 0x0f) as usize] as char);
    }
    Ok(output)
}

pub(crate) fn hex_decode(text: &str) -> Result<Vec<u8>, BytesCodecError> {
    let source = text.as_bytes();
    if !source.len().is_multiple_of(2) {
        return Err(BytesDataError::InvalidHexLength {
            length: source.len(),
        }
        .into());
    }

    for (index, byte) in source.iter().copied().enumerate() {
        decode_hex_digit(byte, index)?;
    }

    let output_len = representable_output_len(Some(source.len() / 2))?;
    let mut output = try_byte_buffer(output_len)?;
    for (pair_index, pair) in source.chunks_exact(2).enumerate() {
        let first_index = pair_index * 2;
        let high = decode_hex_digit(pair[0], first_index)?;
        let low = decode_hex_digit(pair[1], first_index + 1)?;
        output[pair_index] = (high << 4) | low;
    }
    Ok(output)
}

pub(crate) fn base64_encode(bytes: &[u8]) -> Result<String, BytesCodecError> {
    let output_len = base64_encoded_len(bytes.len())?;
    let mut output = try_byte_buffer(output_len)?;
    let written = STANDARD
        .encode_slice(bytes, &mut output)
        .map_err(|_| output_too_large())?;
    output.truncate(written);

    // The standard base64 alphabet and padding are ASCII, so the engine can
    // only write valid UTF-8 into this freshly allocated buffer.
    Ok(unsafe { String::from_utf8_unchecked(output) })
}

pub(crate) fn base64_decode(text: &str) -> Result<Vec<u8>, BytesCodecError> {
    let layout = validate_base64(text)?;
    let mut output = try_byte_buffer(layout.buffer_len)?;
    let mut output_index = 0;
    for quartet in text.as_bytes().chunks_exact(4) {
        let first = decode_base64_digit(quartet[0])
            .expect("validated base64 quartets start with an alphabet digit");
        let second = decode_base64_digit(quartet[1])
            .expect("validated base64 quartets have a second alphabet digit");
        let third = if quartet[2] == b'=' {
            0
        } else {
            decode_base64_digit(quartet[2])
                .expect("validated base64 third bytes are alphabet digits or padding")
        };
        let fourth = if quartet[3] == b'=' {
            0
        } else {
            decode_base64_digit(quartet[3])
                .expect("validated base64 fourth bytes are alphabet digits or padding")
        };

        if output_index < output.len() {
            output[output_index] = (first << 2) | (second >> 4);
            output_index += 1;
        }
        if output_index < output.len() {
            output[output_index] = (second << 4) | (third >> 2);
            output_index += 1;
        }
        if output_index < output.len() {
            output[output_index] = (third << 6) | fourth;
            output_index += 1;
        }
    }
    debug_assert_eq!(output_index, output.len());
    Ok(output)
}

pub(crate) fn sha256_bytes(bytes: &[u8]) -> Result<Vec<u8>, BytesCodecError> {
    let digest = Sha256::digest(bytes);
    let mut output = try_byte_buffer(digest.len())?;
    output.copy_from_slice(&digest);
    Ok(output)
}

pub(crate) fn sha256_string(text: &str) -> Result<Vec<u8>, BytesCodecError> {
    sha256_bytes(text.as_bytes())
}

fn representable_output_len(output_len: Option<usize>) -> Result<usize, BytesResourceError> {
    match output_len {
        Some(output_len) if output_len <= MAX_BYTES_COLLECTION_LEN => Ok(output_len),
        _ => Err(output_too_large()),
    }
}

fn output_too_large() -> BytesResourceError {
    BytesResourceError::OutputTooLarge {
        maximum: MAX_BYTES_COLLECTION_LEN,
    }
}

fn decode_hex_digit(byte: u8, index: usize) -> Result<u8, BytesDataError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(BytesDataError::InvalidHexDigit { index, byte }),
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct Base64Layout {
    buffer_len: usize,
}

fn validate_base64(text: &str) -> Result<Base64Layout, BytesCodecError> {
    let source = text.as_bytes();
    if source.is_empty() {
        return Ok(Base64Layout { buffer_len: 0 });
    }

    let mut first_padding = None;
    for (index, byte) in source.iter().copied().enumerate() {
        if decode_base64_digit(byte).is_some() {
            if let Some(padding_index) = first_padding {
                let error_index = if padding_index % 4 < 2 {
                    padding_index
                } else {
                    index
                };
                return Err(BytesDataError::InvalidBase64 { index: error_index }.into());
            }
        } else if byte == b'=' {
            first_padding.get_or_insert(index);
        } else {
            return Err(BytesDataError::InvalidBase64 { index }.into());
        }
    }

    let padding = match first_padding {
        Some(index) => {
            let padding = source.len() - index;
            let quartet_offset = index % 4;
            if quartet_offset < 2 {
                return Err(BytesDataError::InvalidBase64 { index }.into());
            }
            let expected_padding = 4 - quartet_offset;
            if padding > expected_padding {
                return Err(BytesDataError::InvalidBase64 {
                    index: index + expected_padding,
                }
                .into());
            }
            if padding < expected_padding {
                return Err(BytesDataError::InvalidBase64 {
                    index: source.len(),
                }
                .into());
            }

            let final_digit_index = index - 1;
            let final_digit = decode_base64_digit(source[final_digit_index])
                .expect("base64 padding validation retains a preceding alphabet digit");
            let discarded_bits_mask = if padding == 1 { 0x03 } else { 0x0f };
            if final_digit & discarded_bits_mask != 0 {
                return Err(BytesDataError::InvalidBase64 {
                    index: final_digit_index,
                }
                .into());
            }
            padding
        }
        None => {
            if !source.len().is_multiple_of(4) {
                return Err(BytesDataError::InvalidBase64 {
                    index: source.len(),
                }
                .into());
            }
            0
        }
    };

    let buffer_len = source
        .len()
        .checked_div(4)
        .and_then(|groups| groups.checked_mul(3))
        .ok_or_else(output_too_large)?;
    let decoded_len = buffer_len - padding;
    representable_output_len(Some(decoded_len))?;
    Ok(Base64Layout {
        buffer_len: decoded_len,
    })
}

fn decode_base64_digit(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn try_byte_buffer(length: usize) -> Result<Vec<u8>, BytesResourceError> {
    if length == 0 {
        return Ok(Vec::new());
    }
    allocation_checkpoint()?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(length)
        .map_err(|_| BytesResourceError::AllocationFailed)?;
    output.resize(length, 0);
    Ok(output)
}

fn try_string_with_capacity(length: usize) -> Result<String, BytesResourceError> {
    if length == 0 {
        return Ok(String::new());
    }
    allocation_checkpoint()?;
    let mut output = String::new();
    output
        .try_reserve_exact(length)
        .map_err(|_| BytesResourceError::AllocationFailed)?;
    Ok(output)
}

fn try_owned_string(text: &str) -> Result<String, BytesResourceError> {
    let mut output = try_string_with_capacity(text.len())?;
    output.push_str(text);
    Ok(output)
}

#[cfg(test)]
thread_local! {
    static ALLOCATION_BUDGET: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
struct AllocationBudgetGuard(Option<usize>);

#[cfg(test)]
impl Drop for AllocationBudgetGuard {
    fn drop(&mut self) {
        ALLOCATION_BUDGET.with(|budget| budget.set(self.0));
    }
}

#[cfg(test)]
fn with_allocation_budget<R>(budget: usize, operation: impl FnOnce() -> R) -> R {
    let previous = ALLOCATION_BUDGET.with(|current| current.replace(Some(budget)));
    let _guard = AllocationBudgetGuard(previous);
    operation()
}

fn allocation_checkpoint() -> Result<(), BytesResourceError> {
    #[cfg(test)]
    {
        let injected_failure = ALLOCATION_BUDGET.with(|budget| match budget.get() {
            Some(0) => true,
            Some(remaining) => {
                budget.set(Some(remaining - 1));
                false
            }
            None => false,
        });
        if injected_failure {
            return Err(BytesResourceError::AllocationFailed);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "bytes_codec_tests.rs"]
mod tests;
