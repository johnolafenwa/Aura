//! Reading and writing tensors in the safetensors file format: eight
//! bytes holding the length of a JSON header, the header, which names
//! each tensor's dtype, shape and byte range, and then the elements,
//! little-endian.

use super::*;
use std::io::{Read, Seek, SeekFrom, Write};

/// The largest header read. A header names tensors and holds no elements.
const MAX_HEADER_BYTES: u64 = 256 << 20;

struct Entry {
    name: String,
    dtype: String,
    shape: Vec<usize>,
    start: u64,
    end: u64,
}

/// The entries of the file at `path`, in the file's order, and the offset
/// of the first element byte.
fn read_header(path: &str) -> std::result::Result<(std::fs::File, Vec<Entry>, u64), String> {
    let failed = |what: &str, error: &dyn std::fmt::Display| format!("cannot {what} `{path}`: {error}");
    let mut file = std::fs::File::open(path).map_err(|error| failed("open", &error))?;
    let mut length = [0_u8; 8];
    file.read_exact(&mut length)
        .map_err(|error| failed("read", &error))?;
    let length = u64::from_le_bytes(length);
    let size = file
        .metadata()
        .map_err(|error| failed("read", &error))?
        .len();
    if length > MAX_HEADER_BYTES || length > size.saturating_sub(8) {
        return Err(format!("`{path}` is not a safetensors file"));
    }
    let mut header = vec![0_u8; length as usize];
    file.read_exact(&mut header)
        .map_err(|error| failed("read", &error))?;
    let header: serde_json::Value = serde_json::from_slice(&header)
        .map_err(|error| format!("`{path}` has a header that is not JSON: {error}"))?;
    let Some(fields) = header.as_object() else {
        return Err(format!("`{path}` is not a safetensors file"));
    };
    let data = 8 + length;
    let mut entries = Vec::new();
    for (name, field) in fields {
        if name == "__metadata__" {
            continue;
        }
        let entry = (|| {
            let offsets = field.get("data_offsets")?.as_array()?;
            Some(Entry {
                name: name.clone(),
                dtype: field.get("dtype")?.as_str()?.to_string(),
                shape: field
                    .get("shape")?
                    .as_array()?
                    .iter()
                    .map(|dimension| usize::try_from(dimension.as_u64()?).ok())
                    .collect::<Option<_>>()?,
                start: offsets.first()?.as_u64()?,
                end: offsets.get(1)?.as_u64()?,
            })
        })()
        .filter(|entry| entry.start <= entry.end && entry.end <= size - data)
        .ok_or_else(|| format!("`{path}` describes tensor `{name}` wrongly"))?;
        entries.push(entry);
    }
    entries.sort_by_key(|entry| entry.start);
    Ok((file, entries, data))
}

/// The names of the tensors in the file at `path`, in the file's order.
pub(super) fn names(path: &str) -> std::result::Result<Vec<String>, String> {
    read_header(path).map(|(_, entries, _)| entries.into_iter().map(|entry| entry.name).collect())
}

/// A half-precision float as a `float32`.
fn half(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) << 31;
    let exponent = u32::from(bits >> 10) & 0x1F;
    let fraction = u32::from(bits) & 0x3FF;
    let bits = match (exponent, fraction) {
        (0, 0) => sign,
        (0, _) => {
            // A subnormal half is a normal float32.
            let shift = fraction.leading_zeros() - 21;
            sign | ((113 - shift) << 23) | ((fraction << shift) & 0x3FF) << 13
        }
        (0x1F, _) => sign | 0x7F80_0000 | (fraction << 13),
        _ => sign | ((exponent + 112) << 23) | (fraction << 13),
    };
    f32::from_bits(bits)
}

/// The tensor `name` of the file at `path`, as elements of `dtype`. A
/// float tensor of any width loads as either float dtype, and an integer
/// tensor as either integer dtype.
pub(super) fn load(
    path: &str,
    name: &str,
    dtype: ArrayDType,
) -> std::result::Result<ArrayValue, String> {
    let (mut file, entries, data) = read_header(path)?;
    let entry = entries
        .iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| format!("`{path}` has no tensor `{name}`"))?;
    let width = match entry.dtype.as_str() {
        "F64" | "I64" => 8,
        "F32" | "I32" => 4,
        "F16" | "BF16" | "I16" => 2,
        "I8" | "U8" | "BOOL" => 1,
        other => return Err(format!("tensor `{name}` has dtype {other}, which is not read")),
    };
    let count: usize = entry.shape.iter().product();
    if (entry.end - entry.start) as usize != count * width {
        return Err(format!("`{path}` describes tensor `{name}` wrongly"));
    }
    let mut bytes = vec![0_u8; count * width];
    file.seek(SeekFrom::Start(data + entry.start))
        .and_then(|_| file.read_exact(&mut bytes))
        .map_err(|error| format!("cannot read `{path}`: {error}"))?;
    let float_source = matches!(entry.dtype.as_str(), "F64" | "F32" | "F16" | "BF16");
    let float_target = matches!(dtype, ArrayDType::Float32 | ArrayDType::Float64);
    if float_source != float_target || dtype == ArrayDType::Bool || entry.dtype == "BOOL" {
        return Err(format!(
            "tensor `{name}` has dtype {}, which does not load as `{}`",
            entry.dtype,
            dtype.runtime_type_name()
        ));
    }
    let chunks = bytes.chunks_exact(width);
    let floats = || {
        chunks.clone().map(|chunk| match entry.dtype.as_str() {
            "F64" => f64::from_le_bytes(chunk.try_into().expect("eight bytes")),
            "F32" => f32::from_le_bytes(chunk.try_into().expect("four bytes")).into(),
            "F16" => half(u16::from_le_bytes([chunk[0], chunk[1]])).into(),
            // The top half of a float32.
            _ => f32::from_bits(u32::from(u16::from_le_bytes([chunk[0], chunk[1]])) << 16).into(),
        })
    };
    let integers = || {
        chunks.clone().map(|chunk| match entry.dtype.as_str() {
            "I64" => i64::from_le_bytes(chunk.try_into().expect("eight bytes")),
            "I32" => i32::from_le_bytes(chunk.try_into().expect("four bytes")).into(),
            "I16" => i16::from_le_bytes([chunk[0], chunk[1]]).into(),
            "I8" => (chunk[0] as i8).into(),
            _ => chunk[0].into(),
        })
    };
    let storage = match dtype {
        ArrayDType::Float32 => ArrayStorage::Float32(
            floats()
                .map(|value| value as f32)
                .collect::<Box<[f32]>>()
                .into(),
        ),
        ArrayDType::Float64 => ArrayStorage::Float64(floats().collect::<Box<[f64]>>().into()),
        ArrayDType::Int64 => ArrayStorage::Int64(integers().collect::<Box<[i64]>>().into()),
        _ => ArrayStorage::Int32(
            integers()
                .map(|value| {
                    i32::try_from(value)
                        .map_err(|_| format!("tensor `{name}` holds {value}, which `int32` cannot hold"))
                })
                .collect::<std::result::Result<Box<[i32]>, String>>()?
                .into(),
        ),
    };
    // A tensor has at least one axis. The format's rank-zero tensor is
    // one element.
    let shape: Box<[usize]> = if entry.shape.is_empty() {
        Box::new([1])
    } else {
        entry.shape.clone().into()
    };
    ArrayValue::new(shape, storage).map_err(|error| error.message)
}

/// Writes `tensors` to the file at `path` and gives the file's length.
/// The file appears whole or not at all.
pub(super) fn save(path: &str, tensors: &[(String, ArrayValue)]) -> std::result::Result<u64, String> {
    let mut header = serde_json::Map::new();
    let mut data: Vec<u8> = Vec::new();
    for (name, elements) in tensors {
        let start = data.len();
        let storage = elements.elements().map_err(|error| error.message)?;
        let dtype = match &*storage {
            ArrayStorage::Int32(values) => {
                data.extend(values.iter().flat_map(|value| value.to_le_bytes()));
                "I32"
            }
            ArrayStorage::Int64(values) => {
                data.extend(values.iter().flat_map(|value| value.to_le_bytes()));
                "I64"
            }
            ArrayStorage::Float32(values) => {
                data.extend(values.iter().flat_map(|value| value.to_le_bytes()));
                "F32"
            }
            ArrayStorage::Float64(values) => {
                data.extend(values.iter().flat_map(|value| value.to_le_bytes()));
                "F64"
            }
            ArrayStorage::Bool(values) => {
                data.extend(values.iter().map(|&value| u8::from(value)));
                "BOOL"
            }
        };
        if header.contains_key(name) || name == "__metadata__" {
            return Err(format!("cannot save two tensors named `{name}`"));
        }
        header.insert(
            name.clone(),
            serde_json::json!({
                "dtype": dtype,
                "shape": &*elements.shape,
                "data_offsets": [start, data.len()],
            }),
        );
    }
    let mut header = serde_json::to_vec(&header).map_err(|error| error.to_string())?;
    // The format asks for elements that begin on an eight-byte boundary.
    header.resize(header.len().next_multiple_of(8), b' ');
    let failed = |error: std::io::Error| format!("cannot write `{path}`: {error}");
    let partial = format!("{path}.partial");
    let mut file = std::fs::File::create(&partial).map_err(failed)?;
    file.write_all(&(header.len() as u64).to_le_bytes())
        .and_then(|()| file.write_all(&header))
        .and_then(|()| file.write_all(&data))
        .and_then(|()| file.sync_all())
        .and_then(|()| std::fs::rename(&partial, path))
        .map_err(|error| {
            let _ = std::fs::remove_file(&partial);
            failed(error)
        })?;
    Ok(8 + header.len() as u64 + data.len() as u64)
}
