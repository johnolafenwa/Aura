# SQLite through FFI

A real C library behind the FFI: the system SQLite, driven by the same
calls from Aura, from Rust, and from Python. It measures the boundary under
a workload that has work on both sides, where `benchmarks/ffi_calls`
measures one empty call.

Each side opens an in-memory database and runs three phases over 500,000
rows:

| Phase | What it does | Calls per row |
| --- | --- | --- |
| `insert` | binds an integer and a formatted string to a prepared insert, inside one transaction | bind, bind, step, reset |
| `read` | steps one query over every row and reads an integer and a text column | step, column, column |
| `lookup` | binds a key to a prepared query by primary key and reads the text | bind, step, column, reset |

- **Aura** (`src/main.au`) declares the functions with `extern "C" def`.
  The database and statements are opaque handles with `drop` functions,
  text arguments are `cstr`, and a text column comes back as a `str`.
- **Rust** (`ffi_sqlite.rs`) declares the same functions in an
  `extern "C"` block, with no binding crate. It makes a `CString` for each
  bound string and an owned `String` from each text column, which is the
  work the Aura side does.
- **Python** (`ffi_sqlite.py`) runs twice: with the standard library's
  `sqlite3` module, which is a C extension, and with the same C calls
  through ctypes.

## Running

```sh
cargo build --release -p aura -p aura-runtime
bash benchmarks/ffi_sqlite/run.sh
```

The host needs the SQLite library and its linker file, `libsqlite3-dev` on
Debian and Ubuntu.

## Results

Linux x86-64 cloud container, 2026-11-11, milliseconds for 500,000 rows:

| Phase | Aura | Rust | Python `sqlite3` | Python ctypes |
| --- | --- | --- | --- | --- |
| `insert` | 341 | 225 | 465 | 1082 |
| `read` | 70 | 45 | 204 | 736 |
| `lookup` | 501 | 468 | 1177 | 1481 |

All four print the same checksums. Aura, Rust, and ctypes call the system
SQLite 3.45. Python's `sqlite3` module carries its own copy, 3.53 on this
host, so its column compares two library versions as well as two
languages.

Aura's time over Rust in `insert` is the formatted string and its
NUL-terminated copy, and in `read` it is the new `str` for each text
column.
