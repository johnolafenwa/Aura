# Calling C

Sooner or later you will want a function that Aura's standard modules do not
have but the C library on your machine does. Most operating-system services
are C functions underneath, and so are a great many libraries. Aura can call
them through its foreign function interface, FFI. It is deliberately
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
Aura which library `getpid` lives in. Every program links the C library, so
its functions are always available. A function from any other library needs
one more line in the manifest, which we get to in
[Linking a library](#linking-a-library).

The Aura name of the declaration is the C symbol name. When that name is
awkward, or is already taken in Aura the way `abs` is, give the C name in a
`symbol` clause and pick your own name for the declaration:

```aura fragment
extern "C" def c_abs(x: int32) -> int32 symbol "abs"
```

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
is why it comes with its length. A C function that expects a NUL-terminated
`char *`, such as `strlen` or `fopen`, takes a different parameter type,
`cstr`, which we come to in a moment.

`getentropy(void *buf, size_t len)` fills a buffer with random bytes, which
is what a `mut list[uint8]` parameter is for. Aura passes a pointer to the
list's own bytes and their length. The C function can change the bytes
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
error[AU2002]: an extern function cannot return a str view because no foreign lifetime or allocator contract exists
 --> src/ret.au:1:37
  |
1 | extern "C" def getenv(name: str) -> str
  |                                     ^
  = help: return `cstr` to copy NUL-terminated text that the C side keeps
```

The message says exactly what is missing. A returned pointer and length
could describe static memory, a buffer the caller must free, or something
that will change on the next call, and the declaration has no way to say
which. Write a typed pointer as `ptr[T]`. Using C’s `*T` spelling produces a parser diagnostic.

```aura fragment
extern "C" def strlen(text: *uint8) -> uint64
```

```console
$ aura check src/bad.au
error[AU1101]: use `ptr[T]` for a typed pointer in an extern declaration
 --> src/bad.au:1:29
  |
1 | extern "C" def strlen(text: *uint8) -> uint64
  |                             ^
```

## C strings and out-parameters

Most C functions that take text want a `char *` that ends at a NUL byte, with
no length. The type `cstr` is that contract. It exists only in extern
declarations: on the Aura side the value is an ordinary `str`. Two more forms
cover the other everyday C habits. A `cstr | None` is a pointer that may be
null, and a `mut` scalar is a pointer the function writes its answer through:

*Filename: src/main.au*

```aura fragment
extern "C" def strlen(text: cstr) -> uint64
extern "C" def getenv(name: cstr) -> cstr | None
extern "C" def frexp(value: float64, exponent: mut int32) -> float64

def main() -> int32:
    print(strlen("hello from C"))

    home = getenv("HOME")
    if home is None:
        print("HOME is not set")
    else:
        print(strlen(home) > 0)
    print(getenv("AURA_NO_SUCH_VARIABLE") is None)

    mut exponent: int32 = 0
    fraction = frexp(48.0, exponent)
    print(fraction)
    print(exponent)
    return 0
```

<span class="caption">Listing 18-2: NUL-terminated text and a scalar out-parameter</span>

```console
$ aura run src/main.au
12
true
true
0.75
6
```

For a `cstr` argument Aura makes a copy of the text with a NUL on the end,
passes its address, and frees the copy when the call returns. A string
literal skips the copy, because the compiler stores it with its NUL already
in place. An Aura string can hold a NUL byte and a C string cannot, so
passing one traps with `AU4005` before the C function runs.

A `cstr` result goes the other way: Aura reads bytes up to the NUL, checks
that they are UTF-8, and copies them into a new `str`. It never frees the C
pointer. That is the right contract for `getenv`, which returns a pointer
into memory the C library owns, and for most "get the error message" calls.
It is the wrong one for a function such as `strdup`, whose result the caller
must free: declared `-> cstr`, that memory leaks. Declare such a function to
return an opaque handle, the subject of the next section, and free it with
the library's own function.

`getenv` returns a null pointer for a variable that is not set, so its result
is `cstr | None`, and the checker makes us test for `None` before we use the
text. Had we declared it `-> cstr`, a null result would trap with `AU4005`.

`frexp(double, int *)` splits a number into a fraction and a power of two.
It returns the fraction and stores the exponent through its second
parameter. The `mut int32` parameter passes the address of a temporary that
holds `exponent`, and after the call `exponent` holds what C left there:
`48.0` is `0.75 * 2^6`. Any scalar type can be an out-parameter.

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

<span class="caption">Listing 18-3: Wrapping a C `FILE *` in an opaque handle</span>

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

A handle is never null. `tmpfile` can fail, though, and then it returns a
null pointer, which would trap with `AU4005`. A C function that reports
failure that way should say so in its declaration, with `| None`:

*Filename: src/open.au*

```aura fragment
extern "C" opaque class CFile
extern "C" def fopen(path: cstr, mode: cstr) -> CFile | None
extern "C" def fclose(file: own CFile) -> int32

def main() -> int32:
    missing = fopen("/no/such/file", "r")
    print(missing is None)

    found = fopen("/dev/null", "r")
    if found is None:
        print("could not open /dev/null")
        return 1
    print(fclose(found))
    return 0
```

<span class="caption">Listing 18-4: A handle result that may be null</span>

```console
$ aura run src/open.au
true
0
```

A null pointer becomes `None`, and anything else becomes the handle, so the
checker makes us deal with the failure before we can use the file. The same
type works as a parameter, where `None` passes a null pointer. And a
`mut CFile | None` parameter is C's pointer to a pointer: the function stores
the object it creates through it, which is how `sqlite3_open` and many other
constructors that return a status code hand back their result. The
[Manual's FFI page](/manual/ffi#nullable-handles) shows that form.

So far a `CFile` we forget to close leaks: Aura drops its own wrapper and
the `FILE *` stays open. A `drop` clause on the opaque class fixes that. It
names the C function that releases the object, and Aura calls it when the
program lets go of a handle it still owns:

*Filename: src/note.au*

```aura fragment
extern "C" opaque class CFile drop fclose
extern "C" def tmpfile() -> CFile
extern "C" def fwrite(text: str, count: uint64, file: CFile) -> uint64
extern "C" def ftell(file: CFile) -> int64
extern "C" def fclose(file: own CFile) -> int32

def write_note(text: str) -> int64:
    file = tmpfile()
    fwrite(text, 1, file)
    return ftell(file)

def main() -> int32:
    print(write_note("twelve bytes"))
    return 0
```

```console
$ aura run src/note.au
12
```

`write_note` never calls `fclose`, and the file is closed all the same,
when `file` goes out of scope at the end of the function. The same happens
when a handle is replaced by assignment, and when a list or a class
instance that holds one is dropped. The `drop` function must be an extern
function of the same module that takes the handle as its one `own`
parameter.

A handle is released exactly once. Calling `fclose(file)` ourselves still
works, and it is the way to see the status the function returns. The handle
moves into the call, so there is nothing left for Aura to release. Declare
`drop` on a type only when every handle of that type is ours to release. A
C function that returns a pointer its library keeps, such as the `stdout`
stream, needs a second opaque class without the clause.

## Arrays of numbers

Numeric libraries take their data as a pointer to the first element of an
array, with the count in a separate argument. The type `ptr[T]` is that
pointer. Like `cstr`, it exists only in extern declarations: on the Aura
side the argument is a `list[T]`, where `T` is one of the scalar types.
`ptr[T]` is a `const T *` the function reads through, and `mut ptr[T]` is a
`T *` it may also write through:

*Filename: src/arrays.au*

```aura fragment
extern "C" def wcslen(text: ptr[int32]) -> uint64
extern "C" def getloadavg(averages: mut ptr[float64], count: int32) -> int32

def main() -> int32:
    wide: list[int32] = [72, 105, 0]
    print(wcslen(wide))

    mut averages: list[float64] = [-1.0, -1.0, -1.0]
    print(getloadavg(averages, 3))
    print(averages[0] >= 0.0)
    return 0
```

<span class="caption">Listing 18-5: Passing lists of numbers to C as arrays</span>

```console
$ aura run src/arrays.au
2
3
true
```

`wcslen` counts wide characters up to a zero, and on Linux and macOS a wide
character is 32 bits, so a `list[int32]` that ends in `0` is a wide string.
`getloadavg(double[], int)` fills an array with the system's load averages
and returns how many it wrote. After the call, `averages` holds what C
stored.

No length crosses with the pointer, so nothing stops a C function from
reading or writing past the end of the list. That is why `getloadavg` gets
`3` as its own argument, and it is on us to pass a count that is no larger
than the list. A list of scalars keeps its elements side by side in memory,
exactly as a C array does, so a built program passes the address of the
list's own storage and copies nothing. A million-element list costs the same
to pass as a three-element one. That is the form to reach for when you wrap
BLAS, an image codec, or a model runtime.

## Structs

Many C functions take or return a small struct: a point, a color, a pair of
results. An `extern "C" class` declares one. It lists the fields in the
order the C header does, and Aura lays them out exactly as the C compiler
does, padding included. The C library has two functions we can try it on.
`div` returns a quotient and a remainder together, and `clock_gettime`
fills in a struct we hand it by pointer:

*Filename: src/structs.au*

```aura fragment
extern "C" class Div:
    quot: int32
    rem: int32

extern "C" class Timespec:
    seconds: int64
    nanoseconds: int64

extern "C" def div(numerator: int32, denominator: int32) -> Div
extern "C" def clock_gettime(clock: int32, time: mut Timespec) -> int32

def main() -> int32:
    parts = div(17, 5)
    print(parts.quot)
    print(parts.rem)

    mut now = Timespec(seconds=0, nanoseconds=0)
    if clock_gettime(0, now) != 0:
        return 1
    print(now.seconds > 1700000000)
    return 0
```

<span class="caption">Listing 18-6: A struct result and a struct passed by pointer</span>

```console
$ aura run src/structs.au
3
2
true
```

A parameter or result of the class type crosses by value, so `-> Div` is a
C function that returns `div_t` itself, not a pointer to one. A `mut`
parameter is the pointer form: `time: mut Timespec` is `struct timespec *`.
Aura passes the address of a C copy of `now`, and when the call returns,
`now` holds whatever the function stored. C headers also write
`const struct timespec *` for a struct the function only reads. Declare that
as `mut` too, and pass a `mut` variable.

Our `Timespec` has only scalar fields, so assignment copies it. We can
build one with keyword arguments, read its fields, and keep it in a list.
C lays out nested structs inline. A `fixed[T, N]` field copies when `T` copies. A hosted C union or a list-backed `array[T, N]` field moves with its containing struct. An
`extern "C"` class has no methods, field defaults, or type parameters.

Small structs travel in registers and larger ones in memory, by the same
rules the C compiler follows on the machine we build for, so a call with a
struct costs a few nanoseconds more than one with plain numbers. Declare an unsigned C bit-field with its width, as in `code: uint16 bits(3)`.

### Arrays of structs

C functions often take a pointer to the first of a run of structs, with a
count beside it. A graphics library takes its points that way, and so does
`poll`, the system call that asks which of a set of files are ready. Each
entry is a `struct pollfd`: a file descriptor, the events we ask about, and
the events that happened, which `poll` fills in. We met `ptr[T]` for lists
of numbers, and it takes an `extern "C"` class as well:

*Filename: src/ready.au*

```aura fragment
extern "C" class PollFd:
    fd: int32
    events: int16
    revents: int16

extern "C" def poll(fds: mut ptr[PollFd], count: uint64, timeout_ms: int32) -> int32

POLLOUT: int16 = 4

def main() -> int32:
    mut watched = [
        PollFd(fd=1, events=POLLOUT, revents=0),
        PollFd(fd=2, events=POLLOUT, revents=0),
    ]
    ready = poll(watched, 2, 0)
    print(ready)
    for entry in watched:
        print(entry.revents & POLLOUT != 0)
    return 0
```

<span class="caption">Listing 18-7: A list of structs passed as a C array</span>

```console
$ aura run src/ready.au
2
true
true
```

The argument is an ordinary `list[PollFd]`. For the call, Aura writes its
elements into a C array, one `struct pollfd` after another, and passes the
address of the first. Because the parameter is `mut`, it reads the array
back when `poll` returns, and that is how `revents` reached our list. Both
standard output and standard error were ready for writing. A parameter
without `mut` is `const struct pollfd *`, and skips the reading back.

The copy costs time for every element on every call. That is fine for a
few dozen points or file descriptors. A buffer of a million floats should
be an `Array[float32]`, which lends its own memory instead.

### Arrays inside structs

The other place C puts an array is inside the struct itself: `char
name[32]`, `float params[4]`. The field type `array[T, N]` declares one. This is raylib's `Material`,
whose two pointers we carry as addresses:

```aura fragment
extern "C" class Shader:
    id: uint32
    locs: uint64

extern "C" class Material:
    shader: Shader
    maps: uint64
    params: array[float32, 4]

extern "C" def LoadMaterialDefault() -> Material
extern "C" def UnloadMaterial(material: Material) -> None
```

`array` is a word only here, in a field of an `extern "C"` class. To the
rest of the program, `params` is a `list[float32]`: we can index it, loop
over it, and assign `material.params[0] = 2.5`. The `4` matters at the
boundary. A `Material` that comes back from C always has four elements in
`params`, and one we pass must have exactly four, or the call fails with
`AU4005` before C sees anything. One more thing changes: a class that
holds a list cannot be a `copy class`, so a `Material` moves when we assign
it, like any other class with a list inside.

### Matching a packed C header

A packet header often puts a four-byte field immediately after a byte. In
this declaration the serial number starts at byte 1. The fields `urgent`
and `channel` share the next byte.

```aura fragment
import mem

extern "C" packed class Header:
    kind: uint8
    serial: uint32
    urgent: uint8 bits(1)
    channel: uint8 bits(7)

def serial_offset() -> uint64:
    return mem.offset_of(Header, serial)
```

Assigning `header.urgent = 1` sets a single bit and preserves all seven
channel bits. A C declaration on the other side must specify the same
packing and bit widths. Use `mem.size_of(Header)` and `mem.align_of(Header)`
to compare with the C compiler's `sizeof` and `_Alignof` for the target.

### Unions

A C union is several types laid over the same bytes. SDL, the library
behind many games, reports everything that happens through one: a key
press, a mouse move, and a window resize are all an `SDL_Event`, and its
first four bytes say which. We declare a union with `extern "C" union`,
listing the members we care about:

*Filename: src/events.au*

```aura fragment
extern "C" class Keysym:
    scancode: int32
    sym: int32
    modifiers: uint16
    unused: uint32

extern "C" class KeyboardEvent:
    kind: uint32
    timestamp: uint32
    window: uint32
    state: uint8
    repeat: uint8
    padding2: uint8
    padding3: uint8
    keysym: Keysym

extern "C" union Event:
    kind: uint32
    key: KeyboardEvent
    padding: array[uint8, 56]

extern "C" def SDL_Init(flags: uint32) -> int32
extern "C" def SDL_PushEvent(event: mut Event) -> int32
extern "C" def SDL_PollEvent(event: mut Event) -> int32
extern "C" def SDL_Quit() -> None

SDL_INIT_EVENTS: uint32 = 16384
SDL_KEYDOWN: uint32 = 768

def main() -> int32:
    SDL_Init(SDL_INIT_EVENTS)
    mut escape = Event.from_key(KeyboardEvent(
        kind=SDL_KEYDOWN, timestamp=0, window=0, state=1, repeat=0, padding2=0, padding3=0,
        keysym=Keysym(scancode=41, sym=27, modifiers=0, unused=0),
    ))
    SDL_PushEvent(escape)

    mut event = Event()
    while SDL_PollEvent(event) != 0:
        if event.kind == SDL_KEYDOWN:
            pressed = event.key
            print(f"key {pressed.keysym.sym}, state {pressed.state}")
    SDL_Quit()
    return 0
```

<span class="caption">Listing 18-8: Reading SDL events through a union</span>

With `libraries = ["SDL2"]` in the manifest, the program prints the key we
pushed:

```console
$ aura run
key 27, state 1
```

`SDL_Event` has about thirty members in C. We listed three, and that is
fine as long as one of them is as large as the whole union. SDL pads its
union to 56 bytes for that reason, and our `padding` member does the same.

Three things in `main` are new. `Event()` makes a union of zero bytes for
SDL to fill. `event.kind` and `event.key` read the same bytes two ways: the
first as a `uint32`, the second as a whole `KeyboardEvent`. And
`Event.from_key(...)` builds a union from one member, with every byte the
member does not cover set to zero. Each member we declare gets a read like
`event.key` and a `from_` function like `Event.from_key`.

A C union does not remember which member was stored, and neither does
ours. Nothing stops us from reading `event.key` when the event is a mouse
move. We would get a `KeyboardEvent` made of the mouse event's bytes, as a
C program would. The `kind` field is SDL's convention for telling us which
member to read, and checking it first is our job.

Reading a member gives us a copy. `event.key` builds a new `KeyboardEvent`
each time, which is why the loop binds it to `pressed` and reads two
fields from that. Assign the whole member to change a mutable union.
For example, `event.kind = 1` writes the tag and preserves the rest of
the bytes. To update the key state, read `event.key` into a mutable
local, update its `state`, then assign the local back to `event.key`.

## Callbacks

Sometimes the C function wants to call us. `qsort` sorts an array of
anything, so it asks for a function that compares two elements. In an
extern declaration a parameter with a function type is a C function
pointer, and the argument is the name of an Aura function:

*Filename: src/sorting.au*

```aura fragment
extern "C" def qsort(
    base: mut ptr[int32], count: uint64, size: uint64,
    compare: def(a: mut int32, b: mut int32) -> int32
) -> None

def descending(a: mut int32, b: mut int32) -> int32:
    if a < b:
        return 1
    if a > b:
        return -1
    return 0

def main() -> int32:
    mut values: list[int32] = [3, 9, -4, 12, 0]
    qsort(values, 5, 4, descending)
    print(values)
    return 0
```

<span class="caption">Listing 18-7: Handing an Aura function to C's `qsort`</span>

```console
$ aura run src/sorting.au
[12, 9, 3, 0, -4]
```

`qsort` hands its comparison function two `const void *` pointers. We know
they point at `int32` elements, so the callback type says `mut int32`: a
pointer to one number, which Aura reads on the way in. A plain `int32`
parameter would be a number passed by value.

GLFW gives its error callback a code and a message. We declare the
message as `cstr` where C calls us, then use an ordinary `str` in the
function that prints it.

```aura fragment
extern "C" def glfwSetErrorCallback(
    callback: def(code: int32, description: cstr) -> None
) -> uint64

def report_error(code: int32, description: str) -> None:
    print(f"GLFW error {code}: {description}")
```

Pass `report_error` to `glfwSetErrorCallback`. Aura copies the bytes up
to the first NUL and checks their UTF-8 encoding before calling
`report_error`. GLFW can reuse its buffer as soon as the callback
returns. A null pointer or invalid UTF-8 causes the foreign call that
invoked the callback to fail with `AU4005`.

The argument must be a function we name, defined at the top level of a
module. C receives a bare code address with nowhere to keep captured
variables, so a lambda or a closure is a compile error. C libraries usually
make up for that with a `void *user_data` parameter that they pass back to
the callback untouched. Declare it `uint64`, and use it to carry an index
or a key. A handle passed into a callback, such as the window in a GLFW
key callback, is also declared `uint64`.

A callback can fail like any other Aura code, with an index out of range
or an overflow. Aura does not unwind through the C function, which would
not expect it. The callback returns zero to C, any later callbacks return
zero without running, and the `qsort` call itself fails with the
callback's error as soon as the C function returns. If the callback has a
`with` block, its manager exits before C resumes. A manager around
`qsort` exits after C returns. The C library must call the function on the
thread that made the foreign call, and the callback should not wait on a
task or a queue.

## When things go wrong at run time

Some foreign failures Aura can catch. If no library defines the symbol, the
program cannot be linked, so `aura build` stops with the linker's "undefined
reference" message. `aura run` then runs the program in its interpreter,
which looks each symbol up at the call, and the call fails with a trap before
anything is called:

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

The interpreter got as far as the call, because its lookup happens there. (The path in the
message is the program the symbol was looked up in, here the `aura` command
itself, so yours will differ.) A handle-returning
function that returns a null pointer traps with `AU4005` too, as does a
`cstr` result that is null or not UTF-8, and a `bool`
result that is anything other than `0` or `1` traps with `AU4001`.

Everything else is outside Aura's reach. A segmentation fault, an abort, an
out-of-bounds write, or a C++ exception unwinding through the call is not
turned into an Aura error; it may end the process on the spot. And every
foreign call is synchronous: it runs on the current scheduler worker until it
returns, without a scheduling point, so a C function that blocks for a second
stalls every task on that worker for a second. Keep foreign calls short, or
keep blocking work in a process you start with the `process` module.

## Linking a library

Most C code worth calling lives in a library of its own. To use one, name it
in the `[ffi]` table of the manifest. This package calls zlib, the
compression library, to checksum some text:

*Filename: Aura.toml*

```toml
[package]
name = "checksum"
version = "0.1.0"
edition = "2026"
allow_ffi = true

[ffi]
libraries = ["z"]
```

*Filename: src/main.au*

```aura fragment
extern "C" def crc32_z(seed: uint64, text: str) -> uint64

def main():
    print(crc32_z(0, "hello"))
```

```console
$ aura run src/main.au
907060870
```

`crc32_z` takes a starting value, a pointer, and a length, so the `str`
parameter supplies its last two arguments.

The name is the one a C programmer gives the linker: `z` for *libz.so* on
Linux and *libz.dylib* on macOS. `aura build` passes it to the linker, and
the built program loads the library when it starts, as a C program would.
The call itself is an ordinary machine call, so calling `crc32_z` from Aura
costs what it costs from C.

Three more settings cover the cases that come up in practice. If the library
is not in a directory the system searches, list the directory in
`library_paths`. A relative path starts at the package. A macOS framework
goes in `frameworks`. And when the name differs between systems, put the
setting in a table for that system:

```toml
[ffi]
library_paths = ["vendor/lib"]

[ffi.linux]
libraries = ["openblas"]

[ffi.macos]
frameworks = ["Accelerate"]
```

A library that is not installed is a link error from `aura build`, and the
message names it. The library needs its development files for linking. On
Debian and Ubuntu that is the package whose name ends in `-dev`, such as
`zlib1g-dev`.

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

## What FFI leaves out

The list of missing features is long, and every item on it is missing for the
same reason: it would let C code and Aura code disagree about memory in a way
the compiler cannot check. There are no closures as callbacks, no
variadic functions such as `printf`, no
returned views, no freeing of
returned C strings, and no asynchronous foreign calls. When you need one of
those, the practical answer today is a thin C adapter that presents a simpler
signature, built as a library the package links, or a separate helper program driven
through `process`. The [Manual's FFI page](/manual/ffi) has the full ABI
table, the diagnostics, and the backend contract.

## Summary

You can now call a C function from Aura: declare it with `extern "C" def`
inside a package that opts in with `allow_ffi = true`, name its library
under `[ffi]`, pass scalars by value,
text and bytes as temporary views, C strings as `cstr`, lists of numbers as
`ptr[T]` arrays, C structs as `extern "C"` classes, Aura functions as
callbacks, and results through
`mut` out-parameters, and wrap C objects in opaque handles
whose ownership the checker tracks, so a use after close or a double close is
a compile error, and whose `drop` function closes the ones you forget. You have also seen where the compiler's help ends, at the
boundary of the C code itself, and why FFI keeps that boundary small.

The next section goes one step further down:
[a program with no operating system under it](/book/ch18-01-without-an-operating-system).

That completes the tour of the language and its tools. In
[Chapter 19](/book/ch19-00-final-project) we put the pieces together in one
last project: a supervised job runner that runs jobs in parallel with a
worker pool, restarts the ones that fail, and shuts down gracefully.
