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
| `fs.rename` | `rename(source: str, target: str) -> Result[None, io.Error]` | Renames or moves `source` to `target`. An existing file at `target` is replaced. |
| `fs.open` | `open(path: str) -> Result[fs.File, io.Error]` | Opens a file for reading. |
| `fs.create` | `create(path: str) -> Result[fs.File, io.Error]` | Creates or truncates a file for writing. |
| `fs.append` | `append(path: str) -> Result[fs.File, io.Error]` | Opens a file for appending. Creates it if needed. |

The 256 MiB read cap is part of the API contract. It also applies to `fs.File.read_all()` and `fs.File.read_bytes()`. To process a larger file, or to keep memory flat on any file, open it and read it in pieces with `read_lines`, `read_line`, or `read_chunk` (see [Reading A File In Pieces](#reading-a-file-in-pieces)).

`fs.read_dir` has a known defect. It reports a failure to open the directory. After the directory is open, it silently skips any entry whose metadata or read operation fails. If you need a complete, audited directory listing, check the result with a host helper until the defect is fixed.

## fs.File

`fs.File` is an owned resource. Use `with` for deterministic cleanup:

```aura fragment
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
| `read_line` | `read_line() -> Result[str \| None, io.Error]` | Reads the next line as strict UTF-8 text, without its LF or CRLF. `None` at the end of the file. |
| `read_lines` | `read_lines(max_lines: int64) -> Result[list[str], io.Error]` | Reads the next `max_lines` lines, or the fewer that remain. An empty list at the end of the file. |
| `read_chunk` | `read_chunk(max_bytes: int64) -> Result[list[uint8] \| None, io.Error]` | Reads the next `max_bytes` bytes, or the fewer that remain. `None` at the end of the file. |
| `write_all` | `write_all(text: str) -> Result[None, io.Error]` | Writes all of `text` to the file. The bytes may wait in the handle's write buffer. |
| `write_bytes` | `write_bytes(bytes: list[uint8]) -> Result[None, io.Error]` | Writes all the raw bytes to the file. The bytes may wait in the handle's write buffer. |
| `flush` | `flush() -> Result[None, io.Error]` | Hands buffered writes to the operating system. |
| `sync` | `sync() -> Result[None, io.Error]` | Flushes buffered writes, then waits until the file's data and metadata reach storage. This is `fsync`. |
| `close` | `close() -> None` | Flushes buffered writes and closes the handle. Any later use is invalid. |

### Reading A File In Pieces

`read_all` and `read_bytes` return everything that is left in one value. The three methods below return a piece at a time, so a program can work through a file of any size in constant memory:

```aura
import fs
import io

def count(path: str) -> Result[(int64, int64), io.Error]:
    mut lines = 0
    mut words = 0
    with file = try fs.open(path):
        while true:
            batch = try file.read_lines(1024)
            if batch.is_empty():
                break
            for line in batch:
                lines += 1
                words += line.split_whitespace().len()
    return Result.Ok((lines, words))

def main():
    match fs.write_string("notes.txt", "one two\nthree\n"):
        case Result.Ok(_):
            pass
        case Result.Err(error):
            print(error)
    match count("notes.txt"):
        case Result.Ok((lines, words)):
            print(f"{lines} lines, {words} words")
        case Result.Err(error):
            print(error)
```

The program prints `2 lines, 3 words`.

- **Lines.** A line ends at a line feed. The returned text has neither the line feed nor a carriage return right before it, so files with LF and with CRLF endings read the same. The last line needs no line feed. A file that ends with a line feed has no empty line after it, and an empty file has no lines. Each line must be UTF-8. A line that is not returns `io.Error.InvalidData`, the line is skipped, and the next read continues after it. A line longer than 64 MiB returns `io.Error.InvalidData` as well.
- **`read_lines`** returns exactly `max_lines` lines until the file runs out, then the remaining lines, then an empty list. When a line in the middle of a batch is not UTF-8, the call returns the lines before it, and the next call reports the bad line on its own. `max_lines` must be from 1 to 1,048,576. Any other count returns `io.Error.InvalidInput`.
- **`read_line`** returns one line per call. In a compiled program, `try file.read_line()` hands the line to the program with no `Result` built around it, so a loop that reads a file one line at a time costs no more per line than a `read_lines` loop.
- **`read_chunk`** returns exactly `max_bytes` bytes until the file runs out, then the bytes that remain, then `None`. It does no decoding. `max_bytes` must be from 1 to 268,435,456 (256 MiB). Any other count returns `io.Error.InvalidInput`.

The handle reads ahead and serves lines and chunks from that buffer. On Linux, a line read takes bytes the operating system already holds in memory on the calling thread, 64 KiB at a time. A read that would have to wait for a disk goes to the blocking pool, as it does on every other host, and asks for 64 KiB at first and up to 1 MiB at a time once a file is being read from start to end. All five read methods share one position, so a program can read a header with `read_line` and the rest with `read_chunk` or `read_all`. The end of the file is not remembered. After `None` or an empty list, a later call returns data that another program has appended since.

### Write Buffering

A handle from `fs.create` or `fs.append` buffers its writes, as Python's files and Rust's `BufWriter` do. Writes collect in a 64 KiB buffer, and the handle hands them to the operating system in one call when the buffer would overflow. A single write of 64 KiB or more goes to the operating system at once, after the bytes buffered before it. Writing a file in many small pieces therefore costs about as much as writing it in one piece.

The program itself never sees a buffered write go missing. Before any other filesystem operation, the program hands every handle's buffered writes to the operating system. That covers each one-shot function, `fs.open`, `fs.create`, `fs.append`, and a read through any handle. It does the same before it starts a process, so a child process sees the file as written. When a second handle opens a file that another handle still writes, neither handle buffers from then on, so writes through the two reach the file in the order the program made them.

A handle's buffer is also flushed when:

- the program calls `flush()`, `sync()`, or `close()` on it,
- a `with` block that owns it ends, in any way, since its exit calls `close()`,
- the last reference to it goes away, for example when the task that owns it ends,
- the program exits, including after a trap.

Until then, another program reading the file may not see the buffered bytes. Call `flush()` when one needs them.

A failed write to the operating system is reported by the call that flushed it: a later `write_all` or `write_bytes`, or `flush()` or `sync()`. The bytes in that flush are dropped and are not written again. `close()` and the automatic flushes have no result to report a failure in, so a program that must know a write succeeded calls `flush()` or `sync()` before it closes the handle. A handle from `fs.open` is read-only and does not buffer: a write to it fails at once.

## Text And Bytes

Use the text helpers when you know the file is UTF-8:

```aura fragment
def read_config() -> Result[str, io.Error]:
    text = try fs.read_to_string("config.txt")
    return Result.Ok(text)
```

Use the byte helpers for binary data or an unknown encoding:

```aura fragment
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

## Example: Read A File In Pieces

This program counts lines in batches, reads a header line and then the rest of the file, and copies a file in 16-byte chunks:

<<< ../../examples/io/read_lines.au{aura}

It prints `4 lines, 9 words`, `the quick brown fox`, `24`, `3`, and `true`.

## Example: Replace A File Atomically

Write the new contents to a temporary file, call `sync()`, and rename it over the target. A reader then sees either the old file or the new one, never a partial write.

<<< ../../examples/io/atomic_save.au{aura}

`fs.rename` is atomic when `source` and `target` are on the same filesystem. Across filesystems it fails with an `io.Error`. To make the rename itself survive a power loss, the host must also sync the parent directory. Aura has no directory sync call.

## Error Handling

Filesystem errors are `io.Error` values. Match the variants when the program has a different policy for each case:

```aura fragment
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

`fs.File.write_all`, `write_bytes`, `flush`, `sync`, and `close` need a mutable receiver place. `read_all`, `read_bytes`, `read_line`, `read_lines`, and `read_chunk` work through a shared receiver, even though the host file cursor advances.

The ordinary static rules check a method called on the wrong type, a wrong argument type, and an ignored `Result` where a `try` expression needs one.

## Runtime Semantics

Each one-shot operation performs the host filesystem action in the table:

- `write_string` and `write_bytes` create or replace a file.
- The append operations create the file when it is absent and otherwise append to it.
- `create_dir` creates only one directory.
- `read_dir` returns the sorted names of immediate entries.
- `rename` replaces an existing target file in one host operation.
- `fs.exists` turns any metadata error into `false` by design.

Text is strict UTF-8. Invalid text and reads over 256 MiB return `io.Error.InvalidData`. Byte reads keep the bytes as they are.

A file handle keeps an operating-system cursor. Each read sees and advances the same position. Writes and appends reach the operating system when the handle flushes its write buffer (see [Write Buffering](#write-buffering)), in the order they were made, and are not transactional. Ordinary host failures return the closest `io.Error` variant.

## Ownership And Evaluation Order

Call arguments are evaluated left to right. The filesystem API shares path, text, and byte-list arguments for the duration of the operation and does not keep them. A successful read returns a fresh owned value.

`fs.File` is non-copy. Assigning it or passing it by ownership moves the handle. The checker rejects any later use of the moved binding.

`with` owns the bound resource for its lexical scope. It closes the resource exactly once, whether the scope exits normally or by early return, loop transfer, or error propagation. Cleanup runs after the body and does not undo host I/O that already finished.

The shared read methods use interior host state for the file cursor. The write, flush, sync, and close methods mutate, so they need a mutable receiver binding. A binding from a bare `match` is shared, so write `match own` or `match mut` before calling them.

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

Each one-shot read, and each whole-file read on `fs.File`, is capped at 256 MiB of remaining content. One `read_chunk` call returns at most 256 MiB, one `read_lines` call at most 1,048,576 lines, and one line is at most 64 MiB.

Aura 0.15 has none of these:

- seeking to a position in a file
- recursive directory operations
- transactional writes
- a one-call atomic replace helper. Use `sync()` and `fs.rename` as shown above.
- a directory sync call
- memory mapping
- filesystem watchers
- a permission API
- a symlink-specific API

The host decides paths, permissions, case sensitivity, separators, and symlink traversal.

After a directory is open, an entry that fails during enumeration is skipped. Only a failure to open the directory is returned. Non-Unicode entry names are converted lossily. Partial writes and other visible side effects can remain after a host failure or task cancellation.

## Status

Aura 0.15 implements the one-shot functions, the `fs.File` methods, typed errors, deterministic cleanup, the strict split between text and bytes, and the limits on this page. The fixed 256 MiB whole-read limit is an accepted design decision.

The skipped-entry behavior in `fs.read_dir` is a known defect. Do not rely on it. Aura 0.15 has no seeking, transactional operations, richer metadata, or cross-platform path abstraction API.

Design records: [ADR-0018: Fixed resource read limits](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0018-fixed-resource-read-limits.md) and [ADR-0073: Streaming reads, gzip, and HMAC](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0073-streaming-reads-gzip-and-hmac.md).
