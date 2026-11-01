# Calling C

Sooner or later you will want a function that Aura's standard modules do not
have but the C library on your machine does. Most operating-system services
are C functions underneath, and so are a great many libraries. Aura can call
them through its foreign function interface, FFI v0. It is deliberately
small: it lets you bind a handful of trusted C functions with simple
signatures, and it does not let C code reach into Aura's memory in ways the
compiler cannot see. This chapter shows what it can do, what it refuses to
do, and why the line is where it is.

One thing is worth saying before any code. Everything else in this book has
been checked by the compiler: types, ownership, which values are still
alive. A C function is a black box. Aura checks that your *declaration* of it
uses only the supported forms, but it cannot check that the declaration
matches what the C code really does. If you declare a function with the wrong
signature, or the C code writes past the end of a buffer, the process can
crash or silently corrupt memory, and no Aura diagnostic will tell you why.
That is true of every language's FFI. Aura's contribution is to keep the
unsafe surface as small and as visible as it can.

## A first foreign function

Let's call `getpid`, the C function that returns the current process ID. A
foreign function is declared with `extern "C" def` and no body, and called
like any other function. Save this as *pid.au* and run it:

```aura check-fail:AU2999
extern "C" def getpid() -> int32

def main() -> int32:
    print(getpid() > 0)
    return 0
```

```console
$ aura run pid.au
error[AU2999]: FFI declarations in `pid.au` require an Aura package manifest; add `Aura.toml` with `[package] allow_ffi = true`
 --> pid.au
```

FFI is not available to a loose file. It needs a package, and the package
has to say in its manifest that it uses FFI. That is a review boundary: when
you read someone's package, `allow_ffi = true` in *Aura.toml* is the line that
tells you there is code in it whose correctness the compiler cannot vouch
for. Let's make a package and move the file in:

```console
$ aura new native_demo
created `native_demo`
$ cd native_demo
$ cp ../pid.au src/main.au
$ aura run src/main.au
error[AU2999]: package `native_demo` uses FFI but its manifest `/home/you/native_demo/Aura.toml` does not opt in; add `allow_ffi = true` to `[package]`
 --> src/main.au
```

Now add the opt-in to the `[package]` table:

*Filename: Aura.toml*

```toml
[package]
name = "native_demo"
version = "0.1.0"
edition = "2026"
allow_ffi = true
```

```console
$ aura run src/main.au
true
$ aura run --backend direct src/main.au
aura: building native program...
true
```

The call works on both backends. Notice what we did not do: we did not tell
Aura which library `getpid` lives in. FFI v0 looks the name up in the
symbols already loaded into the process at the moment of the call. The C
library is always loaded, so its functions are available; a function from a
library the process has not loaded is not. The Aura name of the declaration
is the C symbol name, and there is no way to give it a different link name.

Because the code examples in this chapter only check inside a package with
`allow_ffi = true`, the listings below are shown as they appear in
*src/main.au* of `native_demo`.

## Numbers, text, and bytes

The types a foreign function can take and return are fixed, and they are the
ones whose C representation is not open to interpretation. Scalars pass by
value: `bool`, the signed and unsigned integers from 8 to 64 bits, `float32`,
and `float64`. Write them with an explicit width, such as `int32` or `uint64`,
because the width is the whole contract; `int` works, as the alias of
`int64`, but it makes a reviewer look it up. A function that returns nothing
is declared `-> None`.

Strings and byte lists pass as temporary views, and here Aura does something
a little unusual. A `str` parameter becomes *two* C parameters: a pointer to
the UTF-8 bytes and a length. That happens to be exactly the shape of the C
function `write(int fd, const void *buf, size_t count)`, so we can declare it
with a file descriptor and a `str`:

*Filename: src/main.au*

```aura fragment
extern "C" def getpid() -> int32
extern "C" def write(fd: int32, text: str) -> int64
extern "C" def getentropy(buffer: mut list[uint8]) -> int32

def main() -> int32:
    print(getpid() > 0)

    written = write(1, "hello from C\n")
    print(written)

    zeros: list[uint8] = [0, 0, 0, 0, 0, 0, 0, 0]
    mut noise = zeros.copy()
    print(getentropy(noise))
    print(noise.len())
    print(noise == zeros)
    return 0
```

<span class="caption">Listing 18-1: Passing text and a writable byte buffer to C</span>

```console
$ aura run src/main.au
true
hello from C
13
0
8
false
```

`write` put thirteen bytes on file descriptor 1, standard output, and
returned the count. The string view is *not* terminated with a NUL byte, which
is why it comes with its length. So you cannot pass a `str` to a C function
that expects a NUL-terminated `char *`, such as `strlen` or `fopen`; those
need a small C adapter that takes a pointer and a length.

`getentropy(void *buf, size_t len)` fills a buffer with random bytes, which
is what a `mut list[uint8]` parameter is for. Aura copies the list into a
scratch buffer of the same length, passes the pointer and length, and copies
the bytes back after the call returns. The C function can change the bytes
but never the length; `noise` still has eight elements, and they are no
longer all zero. A bare `list[uint8]` passes a read-only view the same way,
and an empty `str` or list passes a null pointer with length zero.

Every view is valid only for the duration of the call. The C side must not
keep the pointer, because Aura may free or move the bytes as soon as the
call returns. For the same reason a foreign function cannot *return* a `str`
or a byte list:

```aura fragment
extern "C" def getenv(name: str) -> str
```

```console
$ aura check src/ret.au
error[AU2002]: FFI v0 cannot return a str view because no foreign lifetime or allocator contract exists
 --> src/ret.au:1:37
  |
1 | extern "C" def getenv(name: str) -> str
  |                                     ^
```

The message says exactly what is missing. A returned pointer could point at
static memory, at a buffer the caller must free, or at something that will
change on the next call, and the declaration has no way to say which. Rather
than guess, FFI v0 does not offer the form. (For environment variables,
`sys.env` already exists.) Raw pointers are refused for the same reason, with
a parser error that suggests the alternatives:

```aura fragment
extern "C" def strlen(text: *uint8) -> uint64
```

```console
$ aura check src/bad.au
error[AU1101]: FFI v0 does not expose raw pointer syntax; use a supported byte/string view or opaque handle
 --> src/bad.au:1:29
  |
1 | extern "C" def strlen(text: *uint8) -> uint64
  |                             ^
```

## Opaque handles

Many C APIs hand you a pointer to an object you are not supposed to look
inside, and later want it back: a `FILE *` from `fopen`, a database
connection, a compression stream. For those, Aura has opaque handles. An
`extern "C" opaque class` is a type that holds one non-null foreign pointer
and nothing else that Aura code can see.

Here is the C library's temporary-file API, which uses a `FILE *`. `tmpfile()`
returns one, `fwrite(ptr, size, count, file)` writes to it (our `str` supplies
the pointer and the size, and we pass a count of one), `ftell` reports the
position, and `fclose` closes it:

*Filename: src/cfile.au*

```aura fragment
extern "C" opaque class CFile
extern "C" def tmpfile() -> CFile
extern "C" def fwrite(text: str, count: uint64, file: CFile) -> uint64
extern "C" def ftell(file: CFile) -> int64
extern "C" def fclose(file: own CFile) -> int32

def main() -> int32:
    file = tmpfile()
    print(file)
    print(fwrite("twelve bytes", 1, file))
    print(ftell(file))
    print(fclose(file))
    return 0
```

<span class="caption">Listing 18-2: Wrapping a C `FILE *` in an opaque handle</span>

```console
$ aura run src/cfile.au
<opaque CFile>
1
12
0
```

Printing a handle shows only its type, never the address. `fwrite` wrote one
item of twelve bytes, `ftell` says we are at position 12, and `fclose`
returned `0` for success.

The interesting part is the ownership. `fwrite` and `ftell` take the handle
as a bare parameter, which shares it for the call, so `main` still has it
afterwards. `fclose` takes it as `own`, which consumes it. That is the
ownership system you learned in [Chapter 4](/book/ch04-00-understanding-ownership)
describing a C API: after `fclose`, the `FILE *` is dangling, and in C nothing
stops you from using it. In Aura, the checker does:

```aura fragment
extern "C" opaque class CFile
extern "C" def tmpfile() -> CFile
extern "C" def fclose(file: own CFile) -> int32

def main() -> int32:
    file = tmpfile()
    print(fclose(file))
    print(fclose(file))
    return 0
```

```console
$ aura check src/twice.au
error[AU3001]: use of moved value `file`
 --> src/twice.au:8:18
  |
8 |     print(fclose(file))
  |                  ^
  = related src/twice.au:7:18: value moved here
  = help: pass shared access when ownership is not needed, or transfer this non-cloneable value only once
```

A double close is one of the classic C bugs, and here it is a compile error.
Handles have more rules in the same spirit. They cannot be cloned
(`file.clone()` is rejected), because two Aura values owning one `FILE *`
would bring the double close right back. They cannot be compared with `==`,
because two pointers being equal says nothing reliable about the objects.
And they cannot cross into another task: passing a `CFile` to `group.start`
fails with `AU3008`, because C libraries differ wildly in what they allow
across threads, and Aura cannot know.

What Aura does *not* do is close a handle for you. A dropped `CFile` only
discards Aura's wrapper, and the `FILE *` leaks. When you write a binding,
declare the C function that releases the object with an `own` parameter,
and give your callers a way to call it, typically through a class with a
`close()` method that they can put in a `with` block, as
[Chapter 14](/book/ch14-00-resources-and-cleanup) showed.

## When things go wrong at run time

Some foreign failures Aura can catch. If the symbol does not exist in the
process, the call fails with a trap before anything is called:

```aura fragment
extern "C" def no_such_function_anywhere() -> int32

def main():
    print("before")
    print(no_such_function_anywhere())
```

```console
$ aura run src/missing.au
before
error[AU4005]: FFI call to `no_such_function_anywhere` failed: FFI symbol `no_such_function_anywhere` was not found: /home/you/.local/bin/aura: undefined symbol: no_such_function_anywhere
 --> src/missing.au
  = note: Aura call chain (innermost first): main at 3:1
```

The program compiled, because lookup happens at the call. (The path in the
message is the program the symbol was looked up in, here the `aura` command
itself, so yours will differ.) A handle-returning
function that returns a null pointer traps with `AU4005` too, and a `bool`
result that is anything other than `0` or `1` traps with `AU4001`.

Everything else is outside Aura's reach. A segmentation fault, an abort, an
out-of-bounds write, or a C++ exception unwinding through the call is not
turned into an Aura error; it may end the process on the spot. And every
foreign call is synchronous: it runs on the current scheduler worker until it
returns, without a scheduling point, so a C function that blocks for a second
stalls every task on that worker for a second. Keep foreign calls short, or
keep blocking work in a process you start with the `process` module.

## Depending on an FFI package

When your application depends on a package that uses FFI, the opt-in has to
travel up. The root package must also set `allow_ffi = true`, and it must
list every FFI-enabled package it reaches, directly or through other
dependencies:

```toml
[package]
name = "app"
version = "0.1.0"
edition = "2026"
allow_ffi = true

[dependencies]
native_binding = { path = "../native_binding" }

[ffi]
dependencies = ["native_binding"]
```

The list must be exact: a missing, unknown, or duplicate entry is an error.
That way, adding a dependency that quietly pulls in native code cannot happen
without a change to your own manifest that a reviewer will see.

## What FFI v0 leaves out

The list of missing features is long, and every item on it is missing for the
same reason: it would let C code and Aura code disagree about memory in a way
the compiler cannot check. There are no callbacks from C into Aura, no
variadic functions such as `printf`, no structs or unions passed by value, no
raw pointers or pointer arithmetic, no returned views, no loading of
libraries by path, and no asynchronous foreign calls. When you need one of
those, the practical answer today is a thin C adapter that presents a simpler
signature, linked into the process, or a separate helper program driven
through `process`. The [Manual's FFI page](/manual/ffi) has the full ABI
table, the diagnostics, and the backend contract.

## Summary

You can now call a C function from Aura: declare it with `extern "C" def`
inside a package that opts in with `allow_ffi = true`, pass scalars by value
and text and bytes as temporary views, and wrap C objects in opaque handles
whose ownership the checker tracks, so a use after close or a double close is
a compile error. You have also seen where the compiler's help ends, at the
boundary of the C code itself, and why FFI v0 keeps that boundary small.

That completes the tour of the language and its tools. In
[Chapter 19](/book/ch19-00-final-project) we put the pieces together in one
last project: a supervised job runner that runs jobs in parallel with a
worker pool, restarts the ones that fail, and shuts down gracefully.
