//! gzip compression and decompression for the `gzip` module.

use std::fmt;
use std::io::{self, Read, Write};

use flate2::read::MultiGzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;

/// The compression level `gzip.compress` uses when the caller names none.
pub const DEFAULT_GZIP_LEVEL: i64 = 6;

/// The most bytes `gzip.decompress` returns when the caller names no limit.
pub const DEFAULT_GZIP_MAX_BYTES: i64 = 256 * 1024 * 1024;

/// Why gzip data could not be decompressed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GzipError {
    /// The input is not a complete, well-formed gzip stream.
    InvalidData(String),
    /// The decompressed data is longer than the caller's limit.
    OutputTooLarge { limit_bytes: usize },
}

impl fmt::Display for GzipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidData(message) => write!(f, "invalid gzip data: {message}"),
            Self::OutputTooLarge { limit_bytes } => {
                write!(f, "gzip output exceeds the {limit_bytes}-byte limit")
            }
        }
    }
}

/// `data` as one gzip member at `level`, from 0 (stored) to 9 (smallest).
pub fn gzip_compress(data: &[u8], level: u32) -> io::Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(
        Vec::with_capacity(data.len() / 3 + 64),
        Compression::new(level),
    );
    encoder.write_all(data)?;
    encoder.finish()
}

/// The data of every gzip member in `data`, one after another, as `gzip`
/// the tool reads a file. At most `limit_bytes` bytes come back.
pub fn gzip_decompress(data: &[u8], limit_bytes: usize) -> Result<Vec<u8>, GzipError> {
    if data.is_empty() {
        return Err(GzipError::InvalidData("the input is empty".to_string()));
    }
    let mut output = Vec::new();
    // One byte past the limit tells a stream that fits from one that does
    // not, without decompressing the rest of it.
    let mut decoder = MultiGzDecoder::new(data).take(limit_bytes as u64 + 1);
    decoder
        .read_to_end(&mut output)
        .map_err(|error| GzipError::InvalidData(error.to_string()))?;
    if output.len() > limit_bytes {
        return Err(GzipError::OutputTooLarge { limit_bytes });
    }
    Ok(output)
}

#[cfg(test)]
#[path = "gzip_codec_tests.rs"]
mod tests;
