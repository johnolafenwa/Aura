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
| `fs.read_dir` | `read_dir(path: String) -> Result[Vec[String], io.Error]` | Returns directory entry paths as strings. |
| `fs.remove_file` | `remove_file(path: String) -> Result[None, io.Error]` | Removes a file. |
| `fs.open` | `open(path: String) -> Result[fs.File, io.Error]` | Opens a file for reading. |
| `fs.create` | `create(path: String) -> Result[fs.File, io.Error]` | Creates or truncates a file for writing. |
| `fs.append` | `append(path: String) -> Result[fs.File, io.Error]` | Opens a file for appending, creating it if needed. |

The one-shot read cap is part of the API contract. Use file handles when a program needs an incremental or larger workflow.

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
| `read_all` | `read_all() -> Result[String, io.Error]` | Reads remaining file contents as UTF-8 text. |
| `read_bytes` | `read_bytes() -> Result[Vec[uint8], io.Error]` | Reads remaining file contents as raw bytes. |
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
