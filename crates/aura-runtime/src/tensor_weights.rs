//! Reading and writing tensors in the safetensors file format: eight
//! bytes holding the length of a JSON header, the header, which names
//! each tensor's dtype, shape and byte range, and then the elements,
//! little-endian.

use super::*;
use std::io::{Read, Seek, SeekFrom, Write};

/// The largest header read. A header names tensors and holds no elements.
/// The reference implementation reads at most 100 MB.
const MAX_HEADER_BYTES: u64 = 100 << 20;

/// The most axes a tensor's shape may name.
const MAX_SHAPE_RANK: usize = 64;

/// One tensor's description in the header, read straight into its fields
/// so that a large header is not first built as a JSON tree.
#[derive(serde::Deserialize)]
struct RawEntry {
    dtype: String,
    shape: RawShape,
    data_offsets: (u64, u64),
}

struct RawShape(Vec<usize>);

impl<'de> serde::Deserialize<'de> for RawShape {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct Axes;
        impl<'de> serde::de::Visitor<'de> for Axes {
            type Value = RawShape;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "at most {MAX_SHAPE_RANK} axis lengths")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<RawShape, A::Error> {
                let mut axes = Vec::new();
                while let Some(axis) = seq.next_element::<u64>()? {
                    if axes.len() == MAX_SHAPE_RANK {
                        return Err(serde::de::Error::custom(format!(
                            "a shape has more than {MAX_SHAPE_RANK} axes"
                        )));
                    }
                    let axis = usize::try_from(axis)
                        .map_err(|_| serde::de::Error::custom("an axis is too long"))?;
                    axes.push(axis);
                }
                Ok(RawShape(axes))
            }
        }
        deserializer.deserialize_seq(Axes)
    }
}

/// Reads the tensors of a header, in the header's order, without
/// `__metadata__`. The name of a tensor whose description cannot be read
/// is left in the seed.
struct RawHeader<'a>(&'a mut Option<String>);

impl<'de> serde::de::DeserializeSeed<'de> for RawHeader<'_> {
    type Value = Vec<(String, RawEntry)>;

    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        struct Fields<'a>(&'a mut Option<String>);
        impl<'de> serde::de::Visitor<'de> for Fields<'_> {
            type Value = Vec<(String, RawEntry)>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a safetensors header object")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut entries = Vec::new();
                while let Some(name) = map.next_key::<String>()? {
                    if name == "__metadata__" {
                        map.next_value::<serde::de::IgnoredAny>()?;
                        continue;
                    }
                    *self.0 = Some(name);
                    let entry = map.next_value::<RawEntry>()?;
                    let name = self.0.take().unwrap_or_default();
                    entries.push((name, entry));
                }
                Ok(entries)
            }
        }
        deserializer.deserialize_map(Fields(self.0))
    }
}

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
    let failed =
        |what: &str, error: &dyn std::fmt::Display| format!("cannot {what} `{path}`: {error}");
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
    let mut header = Vec::new();
    header
        .try_reserve_exact(length as usize)
        .map_err(|error| failed("read", &error))?;
    header.resize(length as usize, 0);
    file.read_exact(&mut header)
        .map_err(|error| failed("read", &error))?;
    // Checking the syntax first builds nothing, so that a header that
    // is not JSON is told apart from one that describes tensors wrongly.
    serde_json::from_slice::<serde::de::IgnoredAny>(&header)
        .map_err(|error| format!("`{path}` has a header that is not JSON: {error}"))?;
    let mut failed_tensor = None;
    let mut reader = serde_json::Deserializer::from_slice(&header);
    let fields = serde::de::DeserializeSeed::deserialize(RawHeader(&mut failed_tensor), &mut reader)
        .map_err(|error| match &failed_tensor {
            Some(name) => format!("`{path}` describes tensor `{name}` wrongly: {error}"),
            None => format!("`{path}` is not a safetensors file"),
        })?;
    let data = 8 + length;
    let mut entries = Vec::new();
    for (name, field) in fields {
        let (start, end) = field.data_offsets;
        if start > end || end > size - data {
            return Err(format!("`{path}` describes tensor `{name}` wrongly"));
        }
        entries.push(Entry {
            name,
            dtype: field.dtype,
            shape: field.shape.0,
            start,
            end,
        });
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
        other => {
            return Err(format!(
                "tensor `{name}` has dtype {other}, which is not read"
            ))
        }
    };
    let wrongly = || format!("`{path}` describes tensor `{name}` wrongly");
    // The byte range lies inside the file, so a size that matches it is
    // one the file backs.
    let byte_count = entry
        .shape
        .iter()
        .try_fold(1usize, |count, &axis| count.checked_mul(axis))
        .and_then(|count| count.checked_mul(width))
        .ok_or_else(wrongly)?;
    if u64::try_from(byte_count).ok() != Some(entry.end - entry.start) {
        return Err(wrongly());
    }
    let count = byte_count / width;
    let out_of_memory = || format!("there is not enough memory to load tensor `{name}`");
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(byte_count)
        .map_err(|_| out_of_memory())?;
    bytes.resize(byte_count, 0);
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
    // The elements can take up to four times the file's bytes, so they
    // are reserved fallibly too.
    let storage = match dtype {
        ArrayDType::Float32 => ArrayStorage::Float32(
            gather(count, floats().map(|value| Ok(value as f32)), out_of_memory)?.into(),
        ),
        ArrayDType::Float64 => {
            ArrayStorage::Float64(gather(count, floats().map(Ok), out_of_memory)?.into())
        }
        ArrayDType::Int64 => {
            ArrayStorage::Int64(gather(count, integers().map(Ok), out_of_memory)?.into())
        }
        _ => ArrayStorage::Int32(
            gather(
                count,
                integers().map(|value| {
                    i32::try_from(value).map_err(|_| {
                        format!("tensor `{name}` holds {value}, which `int32` cannot hold")
                    })
                }),
                out_of_memory,
            )?
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

/// `count` values gathered into a slice reserved without aborting.
fn gather<T>(
    count: usize,
    values: impl Iterator<Item = std::result::Result<T, String>>,
    out_of_memory: impl Fn() -> String,
) -> std::result::Result<Box<[T]>, String> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|_| out_of_memory())?;
    for value in values {
        output.push(value?);
    }
    Ok(output.into_boxed_slice())
}

/// Writes `tensors` to the file at `path` and gives the file's length.
/// The file appears whole or not at all.
pub(super) fn save(
    path: &str,
    tensors: &[(String, ArrayValue)],
) -> std::result::Result<u64, String> {
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
    let (partial, mut file) = create_unique_partial_file(path).map_err(failed)?;
    let write_outcome = file
        .write_all(&(header.len() as u64).to_le_bytes())
        .and_then(|()| file.write_all(&header))
        .and_then(|()| file.write_all(&data))
        .and_then(|()| file.sync_all());
    drop(file);
    write_outcome
        .and_then(|()| std::fs::rename(&partial, path))
        .map_err(|error| {
            let _ = std::fs::remove_file(&partial);
            failed(error)
        })?;
    Ok(8 + header.len() as u64 + data.len() as u64)
}

fn create_unique_partial_file(path: &str) -> std::io::Result<(std::path::PathBuf, std::fs::File)> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;

    for _ in 0..16 {
        let mut random = [0_u8; 16];
        getrandom::getrandom(&mut random).map_err(std::io::Error::other)?;
        let mut suffix = String::with_capacity(random.len() * 2);
        for byte in random {
            use std::fmt::Write as _;
            let _ = write!(suffix, "{byte:02x}");
        }
        let partial = std::path::PathBuf::from(format!("{path}.{suffix}.partial"));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        match options.open(&partial) {
            Ok(file) => return Ok((partial, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a unique temporary file name",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safetensors_save_does_not_clobber_partial_symlink() {
        let root =
            std::env::temp_dir().join(format!("aura-weights-partial-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let sentinel = root.join("sentinel.txt");
        std::fs::write(&sentinel, b"original-sentinel").unwrap();

        let target = root.join("weights.safetensors");
        let predictable = format!("{}.partial", target.display());
        #[cfg(unix)]
        std::os::unix::fs::symlink(&sentinel, &predictable).unwrap();
        #[cfg(not(unix))]
        std::fs::write(&predictable, b"existing").unwrap();

        let sample = ArrayValue::new(
            Box::new([2]),
            ArrayStorage::Float32(vec![1.0, 2.0].into_boxed_slice().into()),
        )
        .unwrap();
        let written = save(target.to_str().unwrap(), &[("w".to_string(), sample)]).unwrap();
        assert!(written > 0);
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"original-sentinel");
        let loaded = load(target.to_str().unwrap(), "w", ArrayDType::Float32).unwrap();
        assert_eq!(loaded.shape.as_ref(), &[2]);

        let _ = std::fs::remove_dir_all(&root);
    }

    fn write_raw(path: &std::path::Path, header: &str, data: &[u8]) {
        let mut file = (header.len() as u64).to_le_bytes().to_vec();
        file.extend_from_slice(header.as_bytes());
        file.extend_from_slice(data);
        std::fs::write(path, file).unwrap();
    }

    #[test]
    fn a_shape_whose_product_overflows_is_refused() {
        let root =
            std::env::temp_dir().join(format!("aura-weights-overflow-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("w.safetensors");
        let text = path.to_str().unwrap();
        // 2^62 * 4 * 4 bytes wraps to 0 in 64 bits, which matched the
        // empty byte range.
        write_raw(
            &path,
            r#"{"w":{"dtype":"F32","shape":[4611686018427387904,4],"data_offsets":[0,0]}}"#,
            &[],
        );
        let error = load(text, "w", ArrayDType::Float32).err().unwrap();
        assert!(error.contains("describes tensor `w` wrongly"), "{error}");
        // A shape of more than 64 axes is refused while reading.
        let axes = vec!["1"; 65].join(",");
        write_raw(
            &path,
            &format!(r#"{{"w":{{"dtype":"F32","shape":[{axes}],"data_offsets":[0,4]}}}}"#),
            &[0; 4],
        );
        let error = names(text).unwrap_err();
        assert!(error.contains("describes tensor `w` wrongly"), "{error}");
        assert!(error.contains("more than 64 axes"), "{error}");
        // Metadata is skipped, and a header that is not an object is not a
        // safetensors file.
        write_raw(
            &path,
            r#"{"__metadata__":{"k":"v"},"w":{"dtype":"F32","shape":[1],"data_offsets":[0,4]}}"#,
            &[0; 4],
        );
        assert_eq!(names(text).unwrap(), ["w"]);
        assert_eq!(
            load(text, "w", ArrayDType::Float32).unwrap().shape.as_ref(),
            &[1]
        );
        write_raw(&path, "[1, 2]", &[]);
        assert!(names(text).unwrap_err().contains("is not a safetensors file"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
