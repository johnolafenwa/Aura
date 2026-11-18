//! Text encodings other than UTF-8 for `bytes.encode` and `bytes.decode`:
//! UTF-16 and UTF-32 in both byte orders, Latin-1, ASCII, and Windows-1252.
//!
//! Every codec is strict. Decoding reports the byte offset of the first
//! sequence that is not valid in the encoding, and encoding reports the
//! index of the first character the encoding has no bytes for.

/// A supported encoding, after its name is normalized.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Encoding {
    Utf8,
    /// UTF-8 with a byte order mark: written on encode, dropped on decode.
    Utf8Sig,
    /// UTF-16 with a byte order mark: little-endian on encode, and the
    /// order the mark names on decode, little-endian when there is none.
    Utf16,
    Utf16Le,
    Utf16Be,
    /// UTF-32 with a byte order mark, with the rules of `Utf16`.
    Utf32,
    Utf32Le,
    Utf32Be,
    Latin1,
    Ascii,
    Windows1252,
}

/// Why text or bytes did not convert.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncodingError {
    /// Decoding: the byte offset where the first invalid sequence starts.
    Invalid { index: usize },
    /// Encoding: the index, in Unicode scalar values, of the first
    /// character the encoding cannot represent.
    Unencodable { index: usize },
}

/// The characters Windows-1252 assigns to bytes 0x80 to 0x9F. Zero marks
/// the five bytes it leaves undefined.
const WINDOWS_1252_HIGH: [u16; 32] = [
    0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
    0x0152, 0, 0x017D, 0, 0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC,
    0x2122, 0x0161, 0x203A, 0x0153, 0, 0x017E, 0x0178,
];

impl Encoding {
    /// Encoding codes shared by generated native calls and the runtime.
    pub const ALL: [Self; 11] = [
        Self::Utf8,
        Self::Utf8Sig,
        Self::Utf16,
        Self::Utf16Le,
        Self::Utf16Be,
        Self::Utf32,
        Self::Utf32Le,
        Self::Utf32Be,
        Self::Latin1,
        Self::Ascii,
        Self::Windows1252,
    ];

    /// The encoding a name spells. Case, `-`, `_`, and spaces do not
    /// matter, so `UTF-16LE`, `utf_16_le`, and `utf16le` are one name.
    pub fn from_name(name: &str) -> Option<Self> {
        let normalized = name
            .chars()
            .filter(|ch| !matches!(ch, '-' | '_' | ' '))
            .map(|ch| ch.to_ascii_lowercase())
            .collect::<String>();
        Some(match normalized.as_str() {
            "utf8" => Self::Utf8,
            "utf8sig" => Self::Utf8Sig,
            "utf16" => Self::Utf16,
            "utf16le" => Self::Utf16Le,
            "utf16be" => Self::Utf16Be,
            "utf32" => Self::Utf32,
            "utf32le" => Self::Utf32Le,
            "utf32be" => Self::Utf32Be,
            "latin1" | "iso88591" | "l1" => Self::Latin1,
            "ascii" | "usascii" => Self::Ascii,
            "windows1252" | "cp1252" => Self::Windows1252,
            _ => return None,
        })
    }
}

/// The bytes of `text` in `encoding`.
pub fn encode(text: &str, encoding: Encoding) -> Result<Vec<u8>, EncodingError> {
    let single_byte = |limit: u32| {
        text.chars()
            .enumerate()
            .map(|(index, ch)| {
                u8::try_from(u32::from(ch))
                    .ok()
                    .filter(|byte| u32::from(*byte) < limit)
                    .ok_or(EncodingError::Unencodable { index })
            })
            .collect::<Result<Vec<u8>, _>>()
    };
    match encoding {
        Encoding::Utf8 => Ok(text.as_bytes().to_vec()),
        Encoding::Utf8Sig => {
            let mut bytes = Vec::with_capacity(text.len() + 3);
            bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            bytes.extend_from_slice(text.as_bytes());
            Ok(bytes)
        }
        Encoding::Utf16 | Encoding::Utf16Le | Encoding::Utf16Be => {
            let big = encoding == Encoding::Utf16Be;
            let mut bytes = Vec::with_capacity(text.len() * 2 + 2);
            let marked = (encoding == Encoding::Utf16).then_some(0xFEFF_u16);
            for unit in marked.into_iter().chain(text.encode_utf16()) {
                bytes.extend_from_slice(&if big {
                    unit.to_be_bytes()
                } else {
                    unit.to_le_bytes()
                });
            }
            Ok(bytes)
        }
        Encoding::Utf32 | Encoding::Utf32Le | Encoding::Utf32Be => {
            let big = encoding == Encoding::Utf32Be;
            let mut bytes = Vec::with_capacity(text.len() * 4 + 4);
            let marked = (encoding == Encoding::Utf32).then_some(0xFEFF_u32);
            for scalar in marked.into_iter().chain(text.chars().map(u32::from)) {
                bytes.extend_from_slice(&if big {
                    scalar.to_be_bytes()
                } else {
                    scalar.to_le_bytes()
                });
            }
            Ok(bytes)
        }
        Encoding::Latin1 => single_byte(0x100),
        Encoding::Ascii => single_byte(0x80),
        Encoding::Windows1252 => text
            .chars()
            .enumerate()
            .map(|(index, ch)| {
                let scalar = u32::from(ch);
                if scalar < 0x80 || (0xA0..0x100).contains(&scalar) {
                    return Ok(scalar as u8);
                }
                WINDOWS_1252_HIGH
                    .iter()
                    .position(|high| *high != 0 && u32::from(*high) == scalar)
                    .map(|offset| 0x80 + offset as u8)
                    .ok_or(EncodingError::Unencodable { index })
            })
            .collect(),
    }
}

/// The text `bytes` spell in `encoding`.
pub fn decode(bytes: &[u8], encoding: Encoding) -> Result<String, EncodingError> {
    match encoding {
        Encoding::Utf8 => decode_utf8(bytes, 0),
        Encoding::Utf8Sig => match bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
            Some(rest) => decode_utf8(rest, 3),
            None => decode_utf8(bytes, 0),
        },
        Encoding::Utf16 => match bytes {
            [0xFF, 0xFE, rest @ ..] => decode_utf16::<false>(rest, 2),
            [0xFE, 0xFF, rest @ ..] => decode_utf16::<true>(rest, 2),
            _ => decode_utf16::<false>(bytes, 0),
        },
        Encoding::Utf16Le => decode_utf16::<false>(bytes, 0),
        Encoding::Utf16Be => decode_utf16::<true>(bytes, 0),
        Encoding::Utf32 => match bytes {
            [0xFF, 0xFE, 0, 0, rest @ ..] => decode_utf32(rest, false, 4),
            [0, 0, 0xFE, 0xFF, rest @ ..] => decode_utf32(rest, true, 4),
            _ => decode_utf32(bytes, false, 0),
        },
        Encoding::Utf32Le => decode_utf32(bytes, false, 0),
        Encoding::Utf32Be => decode_utf32(bytes, true, 0),
        Encoding::Latin1 => Ok(bytes.iter().map(|byte| char::from(*byte)).collect()),
        Encoding::Ascii => match bytes.iter().position(|byte| !byte.is_ascii()) {
            Some(index) => Err(EncodingError::Invalid { index }),
            None => Ok(bytes.iter().map(|byte| char::from(*byte)).collect()),
        },
        Encoding::Windows1252 => bytes
            .iter()
            .enumerate()
            .map(|(index, byte)| match byte {
                0x80..=0x9F => {
                    char::from_u32(u32::from(WINDOWS_1252_HIGH[usize::from(byte - 0x80)]))
                        .filter(|ch| *ch != '\0')
                        .ok_or(EncodingError::Invalid { index })
                }
                _ => Ok(char::from(*byte)),
            })
            .collect(),
    }
}

fn decode_utf8(bytes: &[u8], skipped: usize) -> Result<String, EncodingError> {
    match std::str::from_utf8(bytes) {
        Ok(text) => Ok(text.to_string()),
        Err(error) => Err(EncodingError::Invalid {
            index: skipped + error.valid_up_to(),
        }),
    }
}

fn decode_utf16<const BIG_ENDIAN: bool>(
    bytes: &[u8],
    skipped: usize,
) -> Result<String, EncodingError> {
    let (pairs, trailing) = bytes.as_chunks::<2>();
    let mut text = String::with_capacity(pairs.len());
    let mut units = pairs
        .iter()
        .map(|&pair| {
            if BIG_ENDIAN {
                u16::from_be_bytes(pair)
            } else {
                u16::from_le_bytes(pair)
            }
        })
        .enumerate();
    while let Some((index, unit)) = units.next() {
        let invalid = EncodingError::Invalid {
            index: skipped + index * 2,
        };
        let ch = match unit {
            0..=0x7F => {
                text.push(char::from(unit as u8));
                continue;
            }
            0xD800..=0xDBFF => {
                let Some((_, low @ 0xDC00..=0xDFFF)) = units.next() else {
                    return Err(invalid);
                };
                let scalar =
                    0x10000 + ((u32::from(unit) - 0xD800) << 10) + (u32::from(low) - 0xDC00);
                char::from_u32(scalar).ok_or(invalid)?
            }
            0xDC00..=0xDFFF => return Err(invalid),
            _ => char::from_u32(u32::from(unit)).ok_or(invalid)?,
        };
        text.push(ch);
    }
    if !trailing.is_empty() {
        return Err(EncodingError::Invalid {
            index: skipped + pairs.len() * 2,
        });
    }
    Ok(text)
}

fn decode_utf32(bytes: &[u8], big: bool, skipped: usize) -> Result<String, EncodingError> {
    let mut text = String::with_capacity(bytes.len() / 4);
    let mut chunks = bytes.chunks_exact(4);
    for (position, chunk) in chunks.by_ref().enumerate() {
        let word = [chunk[0], chunk[1], chunk[2], chunk[3]];
        let scalar = if big {
            u32::from_be_bytes(word)
        } else {
            u32::from_le_bytes(word)
        };
        text.push(char::from_u32(scalar).ok_or(EncodingError::Invalid {
            index: skipped + position * 4,
        })?);
    }
    if !chunks.remainder().is_empty() {
        return Err(EncodingError::Invalid {
            index: skipped + bytes.len() - chunks.remainder().len(),
        });
    }
    Ok(text)
}

#[cfg(test)]
#[path = "text_encodings_tests.rs"]
mod tests;
