# Filesystem Module

The `fs` module provides one-shot helpers for common file operations and an owned `fs.File` resource for handle-based workflows.

```python
import fs
import io
```

Filesystem APIs return `Result[..., io.Error]` except `fs.exists(...)`, which returns a plain `bool`.

## One-Shot Functions

| API | Signature | Contract |
| --- | --- | --- |
| `fs.exists` | `exists(path: String) -> bool` | Returns `true` when `path` exists. Errors are collapsed to `false`. |
| `fs.read_to_string` | `read_to_string(path: String) -> Result[String, io.Error]` | Reads a UTF-8 file into a `String`. Reads are capped at 64 MiB. |
| `fs.read_bytes` | `read_bytes(path: String) -> Result[Vec[uint8], io.Error]` | Reads a file into raw bytes. Reads are capped at 64 MiB. |
| `fs.write_string` | `write_string(path: String, text: String) -> Result[None, io.Error]` | Creates or replaces `path` with `text`. |
| `fs.write_bytes` | `write_bytes(path: String, bytes: Vec[uint8]) -> Result[None, io.Error]` | Creates or replaces `path` with raw bytes. Empty byte vectors are allowed. |
| `fs.append_string` | `append_string(path: String, text: String) -> Result[None, io.Error]` | Creates or opens `path` and appends `text`. |
| `fs.append_bytes` | `append_bytes(path: String, bytes: Vec[uint8]) -> Result[None, io.Error]` | Creates or opens `path` and appends bytes. |
| `fs.create_dir` | `create_dir(path: String) -> Result[None, io.Error]` | Creates one directory. Parent directories must already exist. |
| `fs.read_dir` | `read_dir(path: String) -> Result[Vec[String], io.Error]` | Returns the directory's immediate entry names in sorted order. Names that are not valid UTF-8 are converted lossily. |
| `fs.remove_file` | `remove_file(path: String) -> Result[None, io.Error]` | Removes a file. |
| `fs.open` | `open(path: String) -> Result[fs.File, io.Error]` | Opens a file for reading. |
| `fs.create` | `create(path: String) -> Result[fs.File, io.Error]` | Creates or truncates a file for writing. |
| `fs.append` | `append(path: String) -> Result[fs.File, io.Error]` | Opens a file for appending, creating it if needed. |

The read cap is part of the API contract and also applies to `fs.File.read_all()` and `fs.File.read_bytes()`. Aurora 0.1 has no chunked file-read API, so a program that must process a larger file needs a host-side helper or must split the data before reading it through Aurora.

`fs.read_dir` reports failure to open the directory, but the current implementation silently skips an individual entry whose metadata/read operation fails after opening. Code that requires a complete audited directory snapshot must validate results through a host helper until that defect is fixed.

## fs.File

`fs.File` is an owned resource. Use `with` for deterministic cleanup:

```python
def show_file() -> Result[None, io.Error]:
    with file = try fs.open("data.txt"):
        text = try file.read_all()
        print(text)
    return Result.Ok(None)
```

| API | Signature | Contract |
| --- | --- | --- |
| `read_all` | `read_all() -> Result[String, io.Error]` | Reads remaining file contents as strict UTF-8 text, capped at 64 MiB. |
| `read_bytes` | `read_bytes() -> Result[Vec[uint8], io.Error]` | Reads remaining file contents as raw bytes, capped at 64 MiB. |
| `write_all` | `write_all(text: String) -> Result[None, io.Error]` | Writes all of `text` to the file. |
| `write_bytes` | `write_bytes(bytes: Vec[uint8]) -> Result[None, io.Error]` | Writes all raw bytes to the file. |
| `flush` | `flush() -> Result[None, io.Error]` | Flushes pending writes to the operating system. |
| `close` | `close() -> None` | Closes the handle. Further use is invalid. |

## Text And Bytes

Use text helpers when the file is known to be UTF-8:

```python
def read_config() -> Result[String, io.Error]:
    text = try fs.read_to_string("config.txt")
    return Result.Ok(text)
```

Use byte helpers for binary data or unknown encodings:

```python
def read_image_size() -> Result[int32, io.Error]:
    bytes = try fs.read_bytes("image.bin")
    return Result.Ok(bytes.len())
```

The same distinction exists on `fs.File`.

All text reads decode UTF-8 strictly and return `io.Error.InvalidData` for invalid input. A read that exceeds 64 MiB also returns `InvalidData`. File writes are not transactional: after cancellation or a host failure, the caller must not assume that no bytes were written.

## Example: Append A Line

```python
import fs
import io

def append_line(path: String, line: String) -> Result[None, io.Error]:
    with file = try fs.append(path):
        try file.write_all(line)
        try file.write_all("\n")
        try file.flush()
    return Result.Ok(None)
```

## Error Handling

Filesystem errors use `io.Error`. Match variants when the program has different policy for different cases:

```python
match fs.read_to_string("config.txt"):
    case Result.Ok(text):
        print(text)
    case Result.Err(io.Error.NotFound):
        print("using defaults")
    case Result.Err(error):
        print(error)
```
