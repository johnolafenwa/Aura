# ADR-0075: FFI passes C text, out-parameters, and nullable handles

- Status: Accepted
- Date: 2026-11-10
- Builds on: ADR-0052 (the `Handle | None` result), ADR-0074 (direct calls)

## Context

After ADR-0074 a package can link a library, but most C functions are still
out of reach. They take NUL-terminated text, which a `str` view is not. They
return their result through a pointer and a status through the return value,
as `sqlite3_open(const char *, sqlite3 **)` does. They return text they own,
as `sqlite3_errmsg` does. Each of these needed a C adapter.

## Decision

- **`cstr` is the C spelling of `str`.** It is a type name only in an extern
  declaration, alone or as `cstr | None`. The parser rewrites it to `str`
  and records where it was, so every later stage sees an ordinary `str`,
  and a call takes and gives a `str`.
- **A `cstr` argument is a terminated copy.** The copy lives for the call. A
  literal passes constant bytes that already end in a NUL. Text that holds a
  NUL fails with `AU4005` before the call. `None` passes a null pointer.
- **A `cstr` result is copied and never freed.** Aura copies the bytes up to
  the NUL into a new `str`. A null result is `None` when the declaration says
  `cstr | None` and `AU4005` otherwise. Text that is not UTF-8 is `AU4005`.
- **A `mut` scalar is a pointer to the scalar.** The value is stored in a
  stack word, the word's address is passed, and the word is stored back to
  the argument's place when the call returns. A `mut bool` that comes back
  as another byte is `AU4001`.
- **`Handle | None` is a parameter type.** Bare, it passes a pointer that
  may be null. With `mut`, it passes a pointer to that pointer, and the
  place takes the handle or `None` the function left there.
- **Every `mut` argument is stored back before the result is validated**, as
  `mut list[uint8]` already was.

## Alternatives rejected

- **Make every `str` argument NUL-terminated.** It would hide a copy in every
  pointer-length call, and a `str` may hold a NUL.
- **A real `cstr` type usable anywhere.** Conversions to and from `str`
  would spread through user code for no gain. The contract belongs to the
  boundary.
- **A marker for results the caller must free.** The matching free function
  differs by library. An opaque handle and an `own` parameter already state
  that contract.
- **`mut Handle` as `T **`.** A handle is never null, and an out-pointer
  usually starts null, so the nullable form is the honest type.

## Consequences

- SQLite's open, error message, and column text calls can be declared
  without an adapter.
- A `cstr` argument or result costs one allocation and one copy, about 20 to
  40 ns, close to `CString::new` and `CStr::to_str().to_owned()` in Rust. An
  out-parameter costs a store and a load.
- A result the caller must free leaks when it is declared `-> cstr`. The
  manual says so.
- Typed pointers to more than one element are not covered. They come with
  pointer views over lists and arrays.
