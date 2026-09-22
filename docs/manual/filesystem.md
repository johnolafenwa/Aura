# Filesystem Module

The `fs` module provides one-shot helpers for common file operations. It also provides `fs.File`, an owned resource for handle-based work.

```aura
import fs
import io
```

Every filesystem API returns `Result[..., io.Error]`, except `fs.exists(...)`, which returns a plain `bool`.

## One-Shot Functions

| API | Signature | Contract |
| --- | --- | --- |
| `fs.exists` | `exists(path: str) -> bool` | Returns `true` when `path` exists. Any error becomes `false`. |
| `fs.read_to_string` | `read_to_string(path: str) -> Result[str, io.Error]` | Reads a UTF-8 file into a `str`. Reads are capped at 256 MiB. |
| `fs.read_bytes` | `read_bytes(path: str) -> Result[list[uint8], io.Error]` | Reads a file as raw bytes. Reads are capped at 256 MiB. |
| `fs.write_string` | `write_string(path: str, text: str) -> Result[None, io.Error]` | Creates or replaces `path` with `text`. |
| `fs.write_bytes` | `write_bytes(path: str, bytes: list[uint8]) -> Result[None, io.Error]` | Creates or replaces `path` with raw bytes. An empty byte vector is allowed. |
| `fs.append_string` | `append_string(path: str, text: str) -> Result[None, io.Error]` | Creates or opens `path` and appends `text`. |
| `fs.append_bytes` | `append_bytes(path: str, bytes: list[uint8]) -> Result[None, io.Error]` | Creates or opens `path` and appends bytes. |
| `fs.create_dir` | `create_dir(path: str) -> Result[None, io.Error]` | Creates one directory. The parent directories must already exist. |
| `fs.read_dir` | `read_dir(path: str) -> Result[list[str], io.Error]` | Returns the names of the directory's immediate entries, sorted. A name that is not valid UTF-8 is converted lossily. |
| `fs.remove_file` | `remove_file(path: str) -> Result[None, io.Error]` | Removes a file. |
| `fs.open` | `open(path: str) -> Result[fs.File, io.Error]` | Opens a file for reading. |
| `fs.create` | `create(path: str) -> Result[fs.File, io.Error]` | Creates or truncates a file for writing. |
| `fs.append` | `append(path: str) -> Result[fs.File, io.Error]` | Opens a file for appending. Creates it if needed. |

The 256 MiB read cap is part of the API contract. It also applies to `fs.File.read_all()` and `fs.File.read_bytes()`. Aura 0.3 has no chunked file-read API. To process a larger file, use a host-side helper or split the data before Aura reads it.

`fs.read_dir` has a known defect. It reports a failure to open the directory. After the directory is open, it silently skips any entry whose metadata or read operation fails. If you need a complete, audited directory listing, check the result with a host helper until the defect is fixed.

## fs.File

`fs.File` is an owned resource. Use `with` for deterministic cleanup:

```aura
def show_file() -> Result[None, io.Error]:
    with file = try fs.open("data.txt"):
        text = try file.read_all()
        print(text)
    return Result.Ok(None)
```

| API | Signature | Contract |
| --- | --- | --- |
| `read_all` | `read_all() -> Result[str, io.Error]` | Reads the remaining contents as strict UTF-8 text. Capped at 256 MiB. |
| `read_bytes` | `read_bytes() -> Result[list[uint8], io.Error]` | Reads the remaining contents as raw bytes. Capped at 256 MiB. |
| `write_all` | `write_all(text: str) -> Result[None, io.Error]` | Writes all of `text` to the file. |
| `write_bytes` | `write_bytes(bytes: list[uint8]) -> Result[None, io.Error]` | Writes all the raw bytes to the file. |
| `flush` | `flush() -> Result[None, io.Error]` | Flushes pending writes to the operating system. |
| `close` | `close() -> None` | Closes the handle. Any later use is invalid. |

## Text And Bytes

Use the text helpers when you know the file is UTF-8:

```aura
def read_config() -> Result[str, io.Error]:
    text = try fs.read_to_string("config.txt")
    return Result.Ok(text)
```

Use the byte helpers for binary data or an unknown encoding:

```aura
def read_image_size() -> Result[int64, io.Error]:
    bytes = try fs.read_bytes("image.bin")
    return Result.Ok(bytes.len())
```

`fs.File` has the same split between text and byte methods.

Every text read decodes UTF-8 strictly and returns `io.Error.InvalidData` for invalid input. A read over 256 MiB also returns `InvalidData`.

File writes are not transactional. After cancellation or a host failure, do not assume that no bytes were written.

To validate raw bytes as UTF-8, encode them as canonical hex or base64, or hash them, see [Bytes, Text Codecs, And SHA-256](/manual/bytes). Those conversions do not change the filesystem API's typed `io.Error` boundary.

## Example: Append A Line

```aura
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

Filesystem errors are `io.Error` values. Match the variants when the program has a different policy for each case:

```aura
match fs.read_to_string("config.txt"):
    case Result.Ok(text):
        print(text)
    case Result.Err(io.Error.NotFound):
        print("using defaults")
    case Result.Err(error):
        print(error)
```

## Grammar

The filesystem module adds no grammar. Programs use ordinary imports, calls, member calls, `Result`, `try`, `match`, and `with`. A `with name = expression:` binding follows the general resource-scope grammar. It calls the resource's `close()` operation on every scope exit.

Paths are `str` values. There is no path literal and no separate path type. Text and byte operations have different function names. No encoding annotation turns a byte operation into a text operation.

## Typing Rules

The signatures in the one-shot and `fs.File` tables are normative.

- Every operation except `fs.exists` returns `Result`. The failure type is `io.Error`.
- Text reads produce `str`. Binary reads produce `list[uint8]`.
- `fs.open`, `fs.create`, and `fs.append` produce `fs.File`, a non-copy resource type.

`fs.File.write_all`, `write_bytes`, `flush`, and `close` need a mutable receiver place. `read_all` and `read_bytes` work through a shared receiver, even though the host file cursor advances.

The ordinary static rules check a method called on the wrong type, a wrong argument type, and an ignored `Result` where a `try` expression needs one.

## Runtime Semantics

Each one-shot operation performs the host filesystem action in the table:

- `write_string` and `write_bytes` create or replace a file.
- The append operations create the file when it is absent and otherwise append to it.
- `create_dir` creates only one directory.
- `read_dir` returns the sorted names of immediate entries.
- `fs.exists` turns any metadata error into `false` by design.

Text is strict UTF-8. Invalid text and reads over 256 MiB return `io.Error.InvalidData`. Byte reads keep the bytes as they are.

A file handle keeps an operating-system cursor. Each read sees and advances the same position. Writes and appends take effect as they happen and are not transactional. Ordinary host failures return the closest `io.Error` variant.

## Ownership And Evaluation Order

Call arguments are evaluated left to right. The filesystem API shares path, text, and byte-list arguments for the duration of the operation and does not keep them. A successful read returns a fresh owned value.

`fs.File` is non-copy. Assigning it or passing it by ownership moves the handle. The checker rejects any later use of the moved binding.

`with` owns the bound resource for its lexical scope. It closes the resource exactly once, whether the scope exits normally or by early return, loop transfer, or error propagation. Cleanup runs after the body and does not undo host I/O that already finished.

The shared read methods use interior host state for the file cursor. The write, flush, and close methods mutate, so they need a mutable receiver binding.

## Diagnostics

| Code | Cause |
| --- | --- |
| `AU2001` | Unknown filesystem member. |
| `AU2002` | Wrong type. |
| `AU2004` | Invalid argument binding. |
| `AU3001` | Use of a file handle after it moved. |
| `AU3002` | Conflicting borrows. |
| `AU3003` | A mutating file method called through an immutable place. |
| `AU2999` | Any other static rejection. |

Filesystem failures on this page are typed outcomes, not language traps. They return `Result.Err(io.Error)`. Handle these through `Result`: missing files, permission failures, invalid UTF-8, closed handles, and the 256 MiB cap.

A compiler or runtime invariant failure outside that typed boundary uses the general categories in [Diagnostics](/manual/diagnostics). These include `AU4005` for an uncaught resource or I/O trap.

## Backend Support

The MIR runtime and the direct native backend implement the whole API on this page. MIR is the compiler's mid-level intermediate representation. Both backends must match on strict UTF-8 decoding, the read cap, sorted directory results, error variants, owned-resource behavior, and cleanup.

Host filesystem results can differ by operating system and environment. Those differences never let a backend change the Aura return type, drop a successful byte value, or replace a typed `io.Error` with a backend-specific value.

## Limits And Implementation-Defined Behavior

Each one-shot read, and each whole-file read on `fs.File`, is capped at 256 MiB of remaining content.

Aura 0.3 has none of these:

- chunked file reading
- recursive directory operations
- transactional writes
- an atomic replace helper
- memory mapping
- filesystem watchers
- a permission API
- a symlink-specific API

The host decides paths, permissions, case sensitivity, separators, and symlink traversal.

After a directory is open, an entry that fails during enumeration is skipped. Only a failure to open the directory is returned. Non-Unicode entry names are converted lossily. Partial writes and other visible side effects can remain after a host failure or task cancellation.

## Status

Aura 0.3 implements the one-shot functions, the `fs.File` methods, typed errors, deterministic cleanup, the strict split between text and bytes, and the limits on this page. The fixed 256 MiB whole-read limit is an accepted design decision.

The skipped-entry behavior in `fs.read_dir` is a known defect. Do not rely on it. Aura 0.3 has no chunked or asynchronous file access, transactional operations, richer metadata, or cross-platform path abstraction API.

Design record: [ADR-0018: Fixed resource read limits](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0018-fixed-resource-read-limits.md).
