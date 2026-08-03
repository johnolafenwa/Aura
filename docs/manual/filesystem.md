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
| `fs.exists` | `exists(path: str) -> bool` | Returns `true` when `path` exists. Errors are collapsed to `false`. |
| `fs.read_to_string` | `read_to_string(path: str) -> Result[str, io.Error]` | Reads a UTF-8 file into a `str`. Reads are capped at 256 MiB. |
| `fs.read_bytes` | `read_bytes(path: str) -> Result[list[uint8], io.Error]` | Reads a file into raw bytes. Reads are capped at 256 MiB. |
| `fs.write_string` | `write_string(path: str, text: str) -> Result[None, io.Error]` | Creates or replaces `path` with `text`. |
| `fs.write_bytes` | `write_bytes(path: str, bytes: list[uint8]) -> Result[None, io.Error]` | Creates or replaces `path` with raw bytes. Empty byte vectors are allowed. |
| `fs.append_string` | `append_string(path: str, text: str) -> Result[None, io.Error]` | Creates or opens `path` and appends `text`. |
| `fs.append_bytes` | `append_bytes(path: str, bytes: list[uint8]) -> Result[None, io.Error]` | Creates or opens `path` and appends bytes. |
| `fs.create_dir` | `create_dir(path: str) -> Result[None, io.Error]` | Creates one directory. Parent directories must already exist. |
| `fs.read_dir` | `read_dir(path: str) -> Result[list[str], io.Error]` | Returns the directory's immediate entry names in sorted order. Names that are not valid UTF-8 are converted lossily. |
| `fs.remove_file` | `remove_file(path: str) -> Result[None, io.Error]` | Removes a file. |
| `fs.open` | `open(path: str) -> Result[fs.File, io.Error]` | Opens a file for reading. |
| `fs.create` | `create(path: str) -> Result[fs.File, io.Error]` | Creates or truncates a file for writing. |
| `fs.append` | `append(path: str) -> Result[fs.File, io.Error]` | Opens a file for appending, creating it if needed. |

The read cap is part of the API contract and also applies to `fs.File.read_all()` and `fs.File.read_bytes()`. Aura 0.3 has no chunked file-read API, so a program that must process a larger file needs a host-side helper or must split the data before reading it through Aura.

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
| `read_all` | `read_all() -> Result[str, io.Error]` | Reads remaining file contents as strict UTF-8 text, capped at 256 MiB. |
| `read_bytes` | `read_bytes() -> Result[list[uint8], io.Error]` | Reads remaining file contents as raw bytes, capped at 256 MiB. |
| `write_all` | `write_all(text: str) -> Result[None, io.Error]` | Writes all of `text` to the file. |
| `write_bytes` | `write_bytes(bytes: list[uint8]) -> Result[None, io.Error]` | Writes all raw bytes to the file. |
| `flush` | `flush() -> Result[None, io.Error]` | Flushes pending writes to the operating system. |
| `close` | `close() -> None` | Closes the handle. Further use is invalid. |

## Text And Bytes

Use text helpers when the file is known to be UTF-8:

```python
def read_config() -> Result[str, io.Error]:
    text = try fs.read_to_string("config.txt")
    return Result.Ok(text)
```

Use byte helpers for binary data or unknown encodings:

```python
def read_image_size() -> Result[int64, io.Error]:
    bytes = try fs.read_bytes("image.bin")
    return Result.Ok(bytes.len())
```

The same distinction exists on `fs.File`.

Raw file bytes can be validated as UTF-8, encoded as canonical hex/base64, or
hashed through the separate [Bytes, Text Codecs, And SHA-256](/manual/bytes)
surface. Those conversions do not change the filesystem API's typed
`io.Error` boundary.

All text reads decode UTF-8 strictly and return `io.Error.InvalidData` for invalid input. A read that exceeds 256 MiB also returns `InvalidData`. File writes are not transactional: after cancellation or a host failure, the caller must not assume that no bytes were written.

## Example: Append A Line

```python
import fs
import io

def append_line(path: str, line: str) -> Result[None, io.Error]:
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

## Grammar

The filesystem module adds no source-language grammar. Programs use ordinary imports, calls, member calls, `Result`, `try`, `match`, and `with`. A `with name = expression:` binding follows the general resource-scope grammar and invokes the resource's `close()` operation on every scope exit.

Paths are `str` values, not path literals or a distinct path type. Text and byte operations are selected by different function names; no encoding annotation changes a byte operation into a text operation.

## Typing Rules

The signatures in the one-shot and `fs.File` tables are normative. All operations except `fs.exists` return `Result`; failure values are `io.Error`. Text reads produce `str`, binary reads produce `list[uint8]`, and open/create/append produce the non-copy resource type `fs.File`.

`fs.File.write_all`, `write_bytes`, `flush`, and `close` require a mutable receiver place. `read_all` and `read_bytes` are callable through a shared receiver even though the host file cursor advances. Calling a method on the wrong type, supplying a wrong argument type, or ignoring the `Result` where a `try` expression requires it is checked by the ordinary static rules.

## Runtime Semantics

One-shot operations perform the host filesystem action named in the table. `write_string` and `write_bytes` create or replace a file; append operations create when absent and otherwise append. `create_dir` creates only one directory. `read_dir` returns sorted immediate entry names. `fs.exists` deliberately collapses metadata errors to `false`.

Text is strict UTF-8. Invalid text and reads over 256 MiB return `io.Error.InvalidData`; byte reads preserve bytes. A file handle maintains an operating-system cursor, so successive reads observe and advance the same underlying position. Writes and appends are observable as they occur and are not transactional. Normal host failures return the closest documented `io.Error` variant.

## Ownership And Evaluation Order

Call arguments are evaluated left to right. Path, text, and byte-list arguments are shared for the duration of the operation and are not retained by the filesystem API. Successful reads return fresh owned values. `fs.File` is non-copy: assigning or passing it by ownership moves the handle, and later use of the moved binding is rejected.

`with` owns the bound resource for the lexical scope and closes it exactly once on normal exit, early return, loop transfer, or error propagation. Cleanup runs after the body and does not undo completed host I/O. Shared read methods use interior host state for the file cursor; mutating write, flush, and close methods require a mutable receiver binding.

## Diagnostics

Unknown filesystem members use `AU2001`, wrong types use `AU2002`, and invalid argument binding uses `AU2004`. Use after moving a file handle uses `AU3001`; conflicting borrows use `AU3002`; invoking a mutating file method through an immutable place uses `AU3003`; remaining static rejections use `AU2999`.

Documented filesystem failures are typed outcomes, not language traps: they return `Result.Err(io.Error)`. In particular, missing files, permission failures, invalid UTF-8, closed handles, and the 256 MiB cap must be handled through `Result`. A compiler or runtime invariant failure outside that typed boundary uses the general diagnostic categories in [Diagnostics](/manual/diagnostics), including `AU4005` for an uncaught resource/I/O trap.

## Backend Support

The complete API on this page is implemented by the MIR runtime and direct native backend. Strict UTF-8 decoding, the read cap, sorted directory results, error variants, owned-resource behavior, and cleanup are backend-parity requirements.

Host filesystem results can differ by operating system and environment. Such differences do not permit a backend to change the Aura return type, discard a successful byte value, or replace a documented typed `io.Error` with a backend-specific value.

## Limits And Implementation-Defined Behavior

Each one-shot read and each `fs.File` whole-file read is capped at 256 MiB of remaining content. Aura 0.3 has no chunked file-reading API, recursive directory operation, transactional write, atomic replace helper, memory mapping, filesystem watcher, permission API, or symlink-specific API. Host paths, permissions, case sensitivity, separators, and symlink traversal follow the host.

After opening a directory, an individual entry that fails during enumeration is currently skipped; only failure to open the directory is returned. Non-Unicode entry names are converted lossily. Partial writes and externally visible side effects may remain after a host failure or task cancellation.

## Status

The one-shot functions, `fs.File` methods, typed errors, deterministic cleanup, strict text/byte distinction, and limits documented here are implemented and maintained in Aura 0.3. The fixed 256 MiB whole-read policy is accepted under ADR-0018.

The skipped-entry behavior is a documented current defect, not a guarantee that callers should rely on. Aura 0.3 has no chunked or asynchronous file access, transactional operations, richer metadata, or cross-platform path abstraction API.
