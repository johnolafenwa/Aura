# Foreign Function Interface (FFI)

Aura FFI calls C functions through a fixed subset of the platform C ABI. A
package declares each function it calls and names the native libraries they
come from. It is an unsafe package capability for binding trusted native
code. It is not a general system for pointers.

## Package Opt-In

Every source file that declares an extern function or an opaque handle must
belong to an Aura package whose manifest opts in:

```toml
[package]
name = "native_binding"
version = "0.1.0"
edition = "2026"
allow_ffi = true
```

A standalone `.au` file outside a package cannot declare FFI. Compiler
embedders must use the public path-based checking, lowering, or execution APIs
for FFI source. Source-only APIs cannot establish manifest authorization.

If any dependency in the package graph enables FFI, the root package must also
set `allow_ffi = true`. It must list every reachable FFI-enabled dependency by
package name, including transitive dependencies:

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

The report must be exact. The resolver rejects duplicate, unknown, unreachable,
and non-FFI entries, and an entry for the root package. Each FFI-enabled
dependency must also opt itself in. The report grants visibility, not trust.
The root application is still responsible for reviewing the declarations and
the native code they call. [Packages](/manual/packages) documents the manifest
fields.

## Linking Libraries

A package names the native libraries its extern functions come from in the
`[ffi]` table of its manifest:

```toml
[package]
name = "store"
version = "0.1.0"
edition = "2026"
allow_ffi = true

[ffi]
libraries = ["sqlite3"]
library_paths = ["native/lib"]

[ffi.macos]
frameworks = ["Accelerate"]

[ffi.linux]
libraries = ["openblas"]
```

| Key | Meaning |
| --- | --- |
| `libraries` | Library names as the C linker's `-l` option takes them. `sqlite3` names `libsqlite3.so` on Linux and `libsqlite3.dylib` on macOS. |
| `library_paths` | Directories to search for those libraries before the system's own. A relative path starts at the manifest's directory. A dependency's entry must stay inside that dependency's directory, with no `..` segment. |
| `frameworks` | macOS frameworks. Other hosts ignore them. |

`[ffi.linux]` and `[ffi.macos]` take the same three keys and apply on that
host only. The settings of every package in the graph apply to the program:
each dependency's first, then the root package's.

The functions of the platform C library, such as `getpid` and `malloc`, need
no entry.

A package that names a library must set `allow_ffi = true`. A library or
framework name is a bare name of ASCII letters, digits, `_`, `+`, `.`, and
`-` that starts with a letter, digit, `_`, or `+`. The resolver rejects any
other entry with `AU2999`, so a manifest cannot pass options to the linker.

`aura build` and `aura run` pass the settings to the C linker. A program
that names a library the linker cannot find does not build, and the linker's
message names the library. The built program records each `library_paths`
directory as a run-time search path unless `aura build --no-rpath` omits it.
Pass `--no-rpath` when building a binary for another machine, and install the
library where that machine's loader looks.

`aura run --backend mir` loads the same libraries into the interpreter
before the program starts. A library it cannot load stops the run with
`AU4005`.

## Grammar

FFI accepts only bodyless C declarations:

```aura fragment
public extern "C" opaque class ProcessHandle
public extern "C" def getpid() -> int32
extern "C" def inspect(label: str, data: list[uint8]) -> uint64
extern "C" def update(data: mut list[uint8]) -> None
extern "C" def getenv(name: cstr) -> cstr | None
extern "C" def frexp(value: float64, exponent: mut int32) -> float64
extern "C" def getloadavg(averages: mut ptr[float64], count: int32) -> int32
extern "C" def close(handle: own ProcessHandle) -> None

extern "C" class Div:
    quot: int32
    rem: int32

extern "C" class Label:
    text: array[uint8, 16]
    corner: Div

extern "C" union Number:
    whole: int64
    real: float64
    bytes8: array[uint8, 8]

extern "C" def div(numerator: int32, denominator: int32) -> Div
extern "C" def total(parts: ptr[Div], count: int32) -> int64
extern "C" def qsort(
    base: mut ptr[int32], count: uint64, size: uint64,
    compare: def(a: mut int32, b: mut int32) -> int32
) -> None

def main() -> int32:
    print(getpid() > 0)
    return 0
```

The rules for these declarations:

- **ABI string.** It must be exactly `"C"`.
- **Symbol name.** The Aura declaration name is the C symbol name unless a
  `symbol` clause after the return type gives another:
  `extern "C" def c_abs(x: int32) -> int32 symbol "abs"`. Use the clause when
  the C name is long, or is already an Aura name. `symbol` is an ordinary
  name everywhere else. There is no source spelling for a calling
  convention, symbol version, or variadic tail.
- **Visibility.** `public` has its ordinary module meaning. Another module can
  import a public declaration. A private declaration is local to its module.
- **Return type.** Every extern function spells an explicit `-> Type`. Use
  `-> None` for a C function that returns no value.
- **No body.** An extern function has no Aura body, type parameters, receiver,
  or defaults. Its only block is an optional trusted contract.
- **Trusted contract.** An indented block of `requires` and `ensures` clauses
  states what the C function needs and promises. `aura verify` proves each
  `requires` at every call and assumes each `ensures` after it, without
  reading the C body. At run time, the program checks `requires` before the
  foreign call and `ensures` after it, stopping with `AU4009` when one fails.
  `aura build --remove-proved-checks` keeps these foreign boundary checks. See
  [Verification](/manual/verification#trust).
- **Opaque handles.** An opaque declaration is written
  `extern "C" opaque class Name`. It has no fields, methods, body, or type
  parameters.
- **C structs.** A struct declaration is written `extern "C" class Name:`
  with a block of fields. It has no methods, invariants, field defaults, or
  type parameters. A field of type `array[T, N]` is a C array of `N`
  elements, where `N` is a whole number written in the source. See
  [Structs](#structs).

- **C unions.** A union declaration is written `extern "C" union Name:`
  with a block of members, under the rules of a struct's fields. `union` is
  an ordinary name everywhere else. See [Unions](#unions).

To define a function that C code calls by name, see
[Exported Functions](/manual/freestanding#exported-functions).

Raw pointer syntax and `...` variadics are reserved. The
compiler rejects them with teaching diagnostics. Aura code cannot construct an
opaque handle. An extern declaration is direct-call-only, so it cannot be used
as a first-class function value.

## Typing Rules

### Scalars

The accepted scalar types are fixed:

| Aura type | C ABI value |
| --- | --- |
| `bool` | one-byte boolean; returns must be exactly `0` or `1` |
| `int8`, `int16`, `int32`, `int64` | signed 8-, 16-, 32-, or 64-bit integer |
| `uint8`, `uint16`, `uint32`, `uint64` | unsigned 8-, 16-, 32-, or 64-bit integer |
| `float32`, `float64` | IEEE-754 binary32 or binary64 |
| `int` | the exact `int64` alias; `int64` is preferred in ABI declarations |
| `None` | return-only void result |

A bare scalar parameter passes its bits by value. A `mut` scalar parameter
passes a pointer, as [Out-Parameters](#out-parameters) describes. A list of
scalars passes as a C array, as [Array Pointers](#array-pointers) describes. These
types have no FFI representation: `int128`, `uint128`, `intsize`,
`uintsize`, `Duration`, tuples, user classes, enums, generic types, and
arbitrary collection types.

### Text And Byte Views

Three parameter forms pass a pointer and a length:

| Aura parameter | C parameters in order | Contract |
| --- | --- | --- |
| `text: str` | `const uint8_t *`, `size_t` | UTF-8 bytes; not NUL-terminated |
| `data: list[uint8]` | `const uint8_t *`, `size_t` | read-only bytes |
| `data: mut list[uint8]` | `uint8_t *`, `size_t` | fixed-length writable bytes |

The pointer is valid only during the synchronous foreign call. The native
callee must not keep it.

- An empty `str` or byte view passes a null pointer and length zero.
- A non-empty view passes a valid pointer and its exact byte length.
- A mutable byte view lends exactly the list's bytes. What the foreign
  function writes is in the list when it returns, even if later result
  validation reports an Aura error. Foreign code cannot change the list's
  length or capacity.

The checker rejects `own str`, `mut str`, and `own list[uint8]`. An extern
function cannot return a text or byte view, because FFI has no foreign
allocator or lifetime contract. It can return [C text](#c-text), which is
copied.

### C Text

`cstr` is the C spelling of a `str`: UTF-8 bytes that end at a NUL byte,
`const char *` in C. The name is a type only in an extern declaration. The
Aura side of the call passes and receives a `str`.

```aura fragment
extern "C" def strlen(text: cstr) -> uint64
extern "C" def getenv(name: cstr) -> cstr | None

def main() -> int32:
    print(strlen("hello"))
    home = getenv("HOME")
    if home is None:
        return 1
    print(home)
    return 0
```

| Declaration | C type | Aura type | Contract |
| --- | --- | --- | --- |
| `text: cstr` | `const char *` | `str` | a NUL-terminated copy, valid during the call |
| `text: cstr \| None` | `const char *` | `str \| None` | the same, and `None` passes a null pointer |
| `-> cstr` | `const char *` | `str` | the text is copied into a new `str` |
| `-> cstr \| None` | `const char *` | `str \| None` | the same, and a null pointer is `None` |

- **Arguments.** Aura copies the text, adds the NUL, and frees the copy when
  the call returns. The callee must not keep the pointer. A `str` literal
  passes the program's constant bytes with no copy. Text that holds a NUL
  byte cannot be a C string, so the call fails with `AU4005` before the
  function runs.
- **Results.** Aura copies the bytes up to the NUL into a new `str` and never
  frees the C pointer. That fits a result the library owns, such as
  `sqlite3_errmsg` or `getenv`. A result the caller must free leaks. Declare
  such a function to return an opaque handle, and free it through the
  library.
- **Validation.** A null `-> cstr` result fails with `AU4005`, and so does a
  result that is not UTF-8.

A `cstr` parameter must be bare. The checker rejects `own cstr` and
`mut cstr` with `AU3004`. To let C fill a buffer, pass `mut list[uint8]`.

### Out-Parameters

A `mut` scalar parameter passes a pointer to the scalar, such as
`int32_t *`. The argument is a mutable variable or field. The function reads
the value it holds and may store another, which the variable holds when the
call returns.

```aura fragment
extern "C" def frexp(value: float64, exponent: mut int32) -> float64

def main() -> int32:
    mut exponent: int32 = 0
    fraction = frexp(48.0, exponent)
    print(fraction)
    print(exponent)
    return 0
```

This prints `0.75` and `6`. Every scalar type of the table above can be an
out-parameter. The pointer is valid only during the call. A `mut bool` that
comes back as a byte other than `0` or `1` fails with `AU4001`. The checker
rejects an `own` scalar with `AU3004`.

### Array Pointers

`ptr[T]` is a pointer to the first of a run of scalars, such as
`const double *` in C. The name is a type only in an extern parameter. The
argument is a `list[T]`, and its elements are laid out as a C array of `T`.

```aura fragment
extern "C" def getloadavg(averages: mut ptr[float64], count: int32) -> int32

def main() -> int32:
    mut averages: list[float64] = [0.0, 0.0, 0.0]
    if getloadavg(averages, 3) != 3:
        return 1
    print(averages)
    return 0
```

| Declaration | C type | Argument | Contract |
| --- | --- | --- | --- |
| `values: ptr[T]` | `const T *` | a `list[T]` | the function reads the elements |
| `values: mut ptr[T]` | `T *` | a mutable `list[T]` | the function reads and writes the elements |

`T` is any scalar type of the [Scalars](#scalars) table.

- **No length.** Only the pointer crosses. Pass the count as an argument of
  its own, the way the C function expects it. A function that reads or
  writes past the end of the list has undefined behavior.
- **Lifetime.** The pointer is valid only during the call. The callee must
  not keep it.
- **Empty lists.** An empty list passes a null pointer.
- **Reads.** A function must not write through a `ptr[T]`. Declare the
  parameter `mut ptr[T]` if it does.
- **Writes.** What the function writes through a `mut ptr[T]` is in the list
  when the call returns. The list keeps its length. A `mut ptr[bool]` list
  that comes back with a byte other than `0` or `1` fails with `AU4001`.

A built program passes the address of the list's own storage, with no copy,
when the list keeps its elements unboxed. Lists of scalars do. A `ptr[T]`
parameter accepts fixed-width scalars, `extern "C"` classes and `extern "C"`
unions as its element type. Other element types produce `AU2002`, and
`own ptr[T]` produces `AU3004`. `ptr[T]` cannot be a result, and
`list[uint8]` remains the form for bytes with a length.


`T` is also an `extern "C"` class. A `ptr[Name]` parameter takes a
`list[Name]` and passes a C array of `struct name`, as a vertex buffer or a
list of points is passed. [Arrays Of Structs](#arrays-of-structs) has the
rules.

### Arrays

A parameter of type `Array[T]` is also a pointer to the first of a run of
scalars. The argument is an Array, and its elements cross in row-major
order (ADR-0091).

```aura fragment
extern "C" def cblas_ddot(
    n: int32, x: Array[float64], incx: int32, y: Array[float64], incy: int32
) -> float64

def main() -> int32:
    x = Array[float64].full([4], 2.0)
    y = Array[float64].full([4], 0.5)
    print(cblas_ddot(4, x, 1, y, 1))
    return 0
```

| Declaration | C type | Contract |
| --- | --- | --- |
| `values: Array[T]` | `const T *` | the function reads the elements |
| `values: mut Array[T]` | `T *` | the function reads and writes the elements |

`T` is `int32`, `int64`, `float32`, or `float64`. The rules for a `ptr[T]`
apply: no length crosses, the pointer is valid only during the call, and an
empty Array passes a null pointer.

A built program passes the address of the Array's own elements when the
Array is one row-major block. A strided view, such as a transposed matrix
or a stepped slice, is packed into a temporary block for a read. A `mut
Array[T]` argument first becomes one row-major block that no other Array
shares, so the function writes that Array only and a clone keeps its
elements. `aura run` on the interpreter copies the elements in and out.

`Array[bool]` is `AU2002`: convert a mask with `as Array[int32]` first.
`own Array[T]` is `AU3004`, and an Array cannot be a result.

### Structs

`extern "C" class` declares a C struct. Its fields are laid out as a C
compiler lays out a struct with the same fields in the same order, with the
same padding.

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
    print(now.seconds > 0)
    return 0
```

| Declaration | C type | Argument | Contract |
| --- | --- | --- | --- |
| `value: Name` | `struct name` | a `Name` value | the struct crosses by value |
| `value: mut Name` | `struct name *` | a mutable `Name` place | the function reads and may write the fields |
| `-> Name` | `struct name` | | the struct comes back by value |

- **Fields.** A field is a [scalar](#scalars), a C struct or [union](#unions),
  or a fixed-size [array](#array-fields) of values with those layouts.
  Any other field type is `AU2002`.
  The class needs at least one field.
- **An ordinary value.** A struct is Copy when each field is a scalar or
  another Copy struct. A struct that contains an array or a union moves on
  assignment. Construct it with keyword arguments, read and assign its fields,
  and keep it in a list or another class. It has no C identity until it crosses.
- **By value.** A `Name` parameter or result is passed the way the C calling
  convention passes that struct: in registers when it is small, and in
  memory otherwise.
- **By pointer.** A `mut Name` parameter passes the address of a C copy of
  the argument. When the call returns, the argument holds the fields the
  function left there. Use it for `const struct name *` as well, since Aura
  has no read-only struct pointer. The argument must be a mutable place.
  The pointer is valid only during the call.
- **Booleans.** A `bool` field that comes back with a byte other than `0` or
  `1` fails with `AU4001`.

`own Name` is `AU3004`, and `Name | None` is `AU2010`. C
bit-fields, pointer fields, and packed or over-aligned structs have no
spelling. Declare a pointer field as `uint64` only when the code never
dereferences it.

Struct layout and passing follow the System V convention on x86-64 and the
standard procedure call convention on AArch64, for Linux and macOS. A
struct that does not fit the argument registers that are left is passed on
the stack, as each convention requires.

#### Array Fields

A field of type `array[T, N]` is laid out as `T name[N]`: `N` elements in a
row with the alignment of `T`. An element can be a scalar, a C struct or
union, or another array. `N` is a whole number from 1 to 1048576, written in the
source.

```aura fragment
extern "C" class Vertex:
    x: float32
    y: float32

extern "C" class Badge:
    name: array[uint8, 8]
    weights: array[float32, 4]
    corners: array[Vertex, 2]
    grid: array[array[int16, 3], 2]

extern "C" def badge_weight(badge: Badge) -> float32
extern "C" def badge_rename(badge: mut Badge) -> None

def main() -> int32:
    mut badge = Badge(
        name=[65, 117, 114, 97, 0, 0, 0, 0],
        weights=[0.5, 1.0, 1.5, 2.0],
        corners=[Vertex(x=0.0, y=0.0), Vertex(x=4.0, y=3.0)],
        grid=[[1, 2, 3], [4, 5, 6]],
    )
    badge.weights[0] = 2.5
    print(badge_weight(badge))
    badge_rename(badge)
    print(badge.name)
    return 0
```

- **The field is a list.** In Aura code the type of an `array[T, N]` field
  is `list[T]`. Index it, assign its elements, iterate over it, and pass it
  to any function that takes a list. `array` names a type only in a field
  of an `extern "C"` class.
- **Exactly `N` elements cross.** The list must hold `N` elements when the
  struct is passed to a foreign function. Any other length fails with
  `AU4005` before the call. A struct that comes back holds a list of `N`
  elements for each array field.
- **Not a `copy class`.** A class with an array field holds a list, so its
  values move, and so do the values of an `extern "C"` class that holds
  one. Write `.clone()` for a second value.
- **`char name[N]`.** C text in a fixed buffer is an `array[uint8, N]` or
  `array[int8, N]`. The bytes cross as they are, and the terminating NUL is
  the program's to write and to find.
- **Booleans.** An element of an `array[bool, N]` that comes back with a
  byte other than `0` or `1` fails with `AU4001`.

A class with an array field is passed and returned under the rules of the
table above. The native backend checks the length and can copy a small
scalar array directly from matching list storage. Nested and mixed
layouts use the runtime’s element conversion.

#### Arrays Of Structs

`ptr[Name]` is a pointer to the first of a run of structs, such as
`const Vector2 *` in C, where `Name` is an `extern "C"` class. The argument
is a `list[Name]`. No length crosses with it, so C functions of this kind
take the count as another parameter.

```aura fragment
extern "C" class Point:
    x: float32
    y: float32

extern "C" def draw_strip(points: ptr[Point], count: int32) -> None
extern "C" def move_points(points: mut ptr[Point], count: int32, by: float32) -> None

def main() -> int32:
    mut points = [Point(x=0.0, y=0.0), Point(x=4.0, y=3.0), Point(x=8.0, y=0.0)]
    count = len(points) as int32
    move_points(points, count, 1.5)
    draw_strip(points, count)
    return 0
```

| Declaration | C type | Argument | Contract |
| --- | --- | --- | --- |
| `values: ptr[Name]` | `const struct name *` | a `list[Name]` | the function reads the structs |
| `values: mut ptr[Name]` | `struct name *` | a mutable `list[Name]` | the function reads and writes the structs |

- **The call converts the elements.** Aura keeps a class value in its own
  form, so the call writes a C array before the foreign function runs.
  For `mut ptr[Name]` it reads the array back afterwards, and the list
  holds what the function left. The cost is linear in the list.
- **An empty list is a null pointer.**
- **The array lives for the call.** A C function that keeps the pointer
  after it returns reads freed memory.
- **Large numeric buffers.** A buffer of floats that must cross with no
  copy is an [`Array[float32]`](#arrays) or a `ptr[float32]` list, which
  lend their own storage.

`own ptr[Name]` is `AU3004`. Lists of C unions follow the same pointer
rules as lists of C structs.

#### Packing, Alignment And Bit-Fields

Use `packed` for one-byte member packing and `align(N)` to request a minimum alignment for the whole class. The final size includes the padding required for an array of the class.

```aura fragment
import mem

extern "C" packed align(8) class Header:
    kind: uint8
    serial: uint32
    count: uint16 bits(5)
    mode: uint16 bits(3)

def field_offset() -> uint64:
    return mem.offset_of(Header, serial)
```

| Form | Rule |
| --- | --- |
| `extern "C" packed class Header:` | Caps member alignment at one byte. A nested record retains its own internal layout. |
| `align(N)` before `class` | Takes a literal power of two from 1 to 8192. Natural alignment still applies unless the record is packed. |
| `field: uintN bits(W)` | Declares an unsigned bit-field. A positive literal width must fit the 8-, 16-, 32- or 64-bit base type. |
| `mem.size_of(T)`, `mem.align_of(T)` | Give the target size and alignment in bytes as `uint64` constants. Use a C scalar, class or union. |
| `mem.offset_of(T, field)` | Gives the byte offset of an addressable field as a `uint64` constant. Union members start at zero. |

A bit-field read returns its declared unsigned type. A write checks the value fits before updating the field. The platform C ABI determines bit allocation. In particular, Windows starts a new allocation unit when adjacent fields have different base widths. Use matching declarations in C and Aura.

`AU2085` reports an invalid alignment or field width. Use a supported power of two for alignment and an unsigned field type with sufficient bits. `AU2086` reports a layout query whose type or field lacks the requested layout. For a bit-field, query an enclosing record or an addressable sibling. A borrow of a bit-field reports `AU3033`. Copy it to a local and assign the result back. A value too large for a bit-field reports `AU4033`.

#### Unions

`extern "C" union Name:` declares a C union. Every member starts at offset
0, and the union is as large as its largest member, rounded up to the
strictest alignment among them. A member is a scalar, an `extern "C"`
class, another union, or an `array[T, N]` of those.

```aura fragment
extern "C" class KeyEvent:
    kind: uint32
    code: int32

extern "C" class MotionEvent:
    kind: uint32
    x: float32
    y: float32

extern "C" union Event:
    kind: uint32
    key: KeyEvent
    motion: MotionEvent
    padding: array[uint8, 56]

extern "C" def poll_event(event: mut Event) -> bool
extern "C" def push_event(event: Event) -> int32

KEY_DOWN: uint32 = 1

def main() -> int32:
    mut event = Event()
    while poll_event(event):
        if event.kind == KEY_DOWN:
            print(event.key.code)
    push_event(Event.from_key(KeyEvent(kind=KEY_DOWN, code=27)))
    return 0
```

In Aura a union is a class with one field, `bytes: list[uint8]`, which
holds the bytes of the C union. The declaration gives the class these
parts:

| Part | Meaning |
| --- | --- |
| `Name()` | a union of zero bytes |
| `value.member` | the leading bytes of the union, read as the member's type |
| `value.member = item` | stores the member in a mutable union, preserving bytes beyond that member |
| `Name.from_member(value)` | a union that holds `value` in that member, with every other byte zero |
| `value.bytes` | the bytes themselves, which the program can read and write |

- **A member read makes a new value.** `event.key` is a `KeyEvent` built
  from the bytes at that moment. Changing it does not change the union.
- **Assignment writes one member.** `event.kind = 2` sets the bytes of
  `kind`. To update a nested field, read the member into a mutable
  local, edit it and assign the member back.
- **No member is the active one.** A C union carries no tag, and neither
  does this one. Which member holds meaning is the library's convention,
  usually a first field every member shares. Reading another member
  reinterprets the bytes, as it does in C.
- **A `bool` member is checked.** Reading one from a byte other than `0`
  or `1` fails with `AU4001`.
- **The bytes keep their length.** A union whose `bytes` list is not
  exactly as long as the C union fails with `AU4005` when it crosses or
  when a member is read.
- **A union is not `Copy`.** It holds a list, so assignment moves it, as
  for a class with an array field. A class that holds a union is not
  `Copy` either.
- **A member cannot be named `bytes`.**

A union crosses wherever an `extern "C"` class does: by value, as `mut
Name` for a pointer the function writes through, as a field, as an array
element, and through `ptr[Name]`. By value it is passed the way the
platform's convention passes a C union of those members.

A native numeric member read checks the union’s byte storage and length
before loading the scalar. When C fills a list of unions and its only
later uses read numeric members, native code can keep the private C
array for those reads. A member with a nested layout uses the runtime
conversion.

### Callbacks

A parameter of function type is a C function pointer. The argument is a
named Aura function, and the C function calls it.

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

| Callback parameter | C type | Meaning |
| --- | --- | --- |
| `value: T` | `T` | a scalar by value |
| `value: mut T` | `T *` | a pointer to one scalar, read on entry and written back on return |
| `value: cstr` | `const char *` | NUL-terminated UTF-8 text, copied into a `str` before the Aura function runs |
| `-> T` | `T` | a scalar result |
| `-> None` | `void` | no result |

`T` is any scalar type of the [Scalars](#scalars) table.

Declare a text parameter as `cstr` in the foreign declaration and as
`str` in the Aura function. The C library owns the source bytes. The
callback can clone its string to keep it after the call. A pointer to a
single NUL byte gives the empty string. A null pointer or invalid UTF-8
raises `AU4005` when the enclosing foreign call returns. A callback
returns a scalar or `None`.

- **Named functions only.** The argument names a top-level function with
  exactly the declared parameter and result types. A lambda, a closure, a
  method, a generic function, and a function held in a variable are
  `AU2005`. C receives a plain code address, so the function has no
  captured values. Declare a handle such as GLFW's `GLFWwindow *` or a
  `void *` user-data parameter as `uint64` in both callback signatures.
  The callback receives the address as a number.
- **Pointer parameters.** `mut T` is also the spelling for `const T *`. A
  null pointer reads as zero and is not written.
- **Booleans.** A `bool` argument from C is `true` for any nonzero byte.
- **Lifetime.** The code address stays valid for the life of the program.
  The C library may keep it and call it during a later foreign call.
- **Threads.** C must call the function on the thread that made the foreign
  call, while a foreign call is running. A call from a thread the C library
  started has undefined behavior.
- **Suspension.** A callback should not wait on a task, a `Queue`, or a
  timer. Other tasks would run while the C function is still on the stack.
- **Failures.** A failure in the callback, such as an index out of range,
  does not unwind through the C function. The callback returns zero to C,
  later callbacks return zero without running, and the foreign call fails
  with the callback's diagnostic when the C function returns. Resource
  managers opened inside the callback exit before C resumes. Managers
  surrounding the foreign call exit after C finishes.

List, struct, and opaque-handle callback parameters produce `AU2002`.
The function-pointer parameter of the extern declaration uses the default
passing mode.

### Opaque Handles

An opaque handle is one non-null foreign pointer with no layout visible to
Aura.

- A bare handle parameter shares the pointer for that call. The caller keeps
  the Aura handle.
- An `own Handle` parameter consumes the handle. This is normally a foreign
  close or free operation.
- `mut Handle` is reserved. Use `mut Handle | None` for a pointer the
  function stores a handle through.
- Handles are not Copy, not cloneable, and never `Transfer`. They cannot cross
  a task or Queue boundary.
- A null `-> Handle` result is an Aura runtime failure, `AU4005`.

### Drop Functions

A `drop` clause names the C function that releases the object behind a
handle. Aura calls it when the program lets go of a handle it still owns.

```aura fragment
extern "C" opaque class Database drop sqlite3_close
extern "C" def sqlite3_open(path: cstr, out: mut Database | None) -> int32
extern "C" def sqlite3_close(db: own Database) -> int32

def opens() -> bool:
    mut found: Database | None = None
    sqlite3_open(":memory:", found)
    return found is not None
```

`opens` returns without a call of `sqlite3_close`, and the database is
closed when `found` goes out of scope.

- **The function.** It is an extern function of the same module. It takes
  the handle as its one parameter, declared `own`, and returns a scalar or
  nothing. Any other shape, or a name that is not such a function, is
  `AU2005`. Aura discards the result.
- **When it runs.** The function runs once, when the last owner of the
  handle goes away: a variable goes out of scope or is assigned another
  value, or a list, tuple, class instance, or enum payload that holds the
  handle is dropped. That covers a normal return and `try` propagation. A
  handle the program still holds when the process ends, after `main`
  returns or a runtime failure stops it, may not be released.
- **Giving a handle up.** A handle passed to an `own` parameter of any
  extern function belongs to that function. The `drop` function does not
  run for it. So an explicit close is still allowed, and it is the way to
  read the status the function returns.
- **Out-parameters.** After a call with a `mut Handle | None` argument, a
  pointer the function did not change keeps the handle the place held. A
  pointer it changed is a new handle, and the object the place held is no
  longer the program's: the `drop` function does not run for it.
- **Order.** The order in which handles that go out of scope together are
  released is not specified, and the two backends may differ. When one C
  object must outlive another, release the dependent one first with an
  explicit call, keep it in a narrower scope, or name a `drop` function
  that tolerates either order, such as `sqlite3_close_v2`.
- **Borrowed pointers.** Every handle of the type is released, however it
  was made. A C function that returns a pointer its library keeps must
  return a different opaque class, one with no `drop` clause.
- **The interpreter** looks the function up when a call first makes a
  handle of the type. A missing symbol is `AU4005` at that call.

### Nullable Handles

A type that is exactly a declared opaque handle plus `None` crosses C as one
pointer, where null is `None`. Write it as `Handle | None` or through an
alias of that shape. No union tag or aggregate crosses C.

| Declaration | C type | Meaning |
| --- | --- | --- |
| `-> Handle \| None` | `T *` | a null result is `None`, and any other is the owned handle |
| `handle: Handle \| None` | `T *` | `None` passes a null pointer |
| `out: mut Handle \| None` | `T **` | the function reads and stores a pointer through it |

The `mut` form is how C constructors that report a status return their
object:

```aura fragment
extern "C" opaque class Database
extern "C" def sqlite3_open(path: cstr, out: mut Database | None) -> int32
extern "C" def sqlite3_close(db: own Database) -> int32

def main() -> int32:
    mut found: Database | None = None
    status = sqlite3_open(":memory:", found)
    if found is None:
        return status
    return sqlite3_close(found)
```

The variable holds what the function stored when the call returns: a handle,
or `None` for a null pointer. Aura does not release the handle the variable
held before the call. The checker rejects `own Handle | None` with `AU3004`.

The checker rejects every other union shape with `AU2010`. That includes
optional scalars, byte views, and several handle alternatives. These need an
explicit C adapter. Every `mut` argument is stored back before the result is
validated. The resulting handle follows every opaque-handle rule.

### Handles Inside Other Values

Non-cloneability is structural. It carries through tuples, collections, user
classes, enum payloads, and generic specializations.

- `.clone()` is rejected when the duplicated value contains an opaque handle.
  So are clone-producing collection observations such as `get`, projected
  reads, and `filter`.
- Consuming operations such as `pop`, `remove`, and replacement are allowed.
- Equality and inequality are rejected for an opaque handle or any value that
  structurally contains one.
- Arithmetic and ordering operators on a handle are rejected with dedicated
  diagnostics. Raw pointer arithmetic and ordering by foreign address are not
  language capabilities.

FFI does not expose foreign addresses. It does not assume that address
identity is the native API's identity. When callers need to compare foreign
objects, a binding should expose reviewed extern operations or a stable scalar
or `str` key.

## Runtime Semantics

A built program calls a foreign function the way C code does. The call is
one machine call instruction, with each argument in the register or stack
slot the C ABI gives it. The linker resolves the symbol when the program is
built, from the platform C library and the libraries the manifest names. A
symbol that no library defines is a link error, not a run-time failure.

The interpreter, `aura run --backend mir`, looks the symbol up when the call
runs, among the libraries loaded into the process. A missing symbol there is
`AU4005`. `aura run` with the default backend falls back to the interpreter
when the program does not link, so it reports the same `AU4005`.

A call runs in this order:

1. Aura evaluates the arguments left to right under ordinary call rules.
2. Aura passes the arguments by the C ABI.
3. The foreign function runs.
4. Aura validates the result, including canonical booleans and non-null opaque
   handles.

A result validation failure cannot roll back foreign side effects or bytes
the function wrote.

Every foreign call is synchronous. It occupies the current Aura worker until
the native function returns. It does not move to the blocking I/O pool and
does not create an implicit scheduling point. A long or blocking native call
can delay other tasks pinned to that worker.

FFI declarations are unsafe contracts. Aura cannot verify that the real C
signature, pointer retention, allocation, thread safety, and mutation behavior
match the declaration.

## Ownership And Evaluation Order

| Argument | Ownership |
| --- | --- |
| Scalar | Copied into an ABI slot. |
| Bare `str`, `list[uint8]`, or opaque handle | Stays owned by the caller and is usable after the call. |
| `mut list[uint8]` | Needs an exclusive mutable place. In-place byte updates are visible after return. |
| `cstr` | Copied for the call. The caller keeps its `str`. |
| `mut` scalar or `mut Handle \| None` | Needs an exclusive mutable place, which holds what the function stored after return. |
| `ptr[T]` | The list stays owned by the caller and is usable after the call. |
| `mut ptr[T]` | Needs an exclusive mutable list. In-place element updates are visible after return. |
| `own` opaque handle | Moves before the call and cannot be used afterward. |

The declared capability is exact. Aura inserts no implicit clone, ownership
conversion, or pointer-lifetime extension. A foreign call may have
irreversible external effects. Evaluating a later argument or validating the
result does not undo earlier evaluation, the foreign call, or foreign writes.

An opaque class with no `drop` clause has no foreign destructor. A binding
package must declare and call the right consuming C function. Dropping an
unconsumed handle of such a class discards only Aura's wrapper. It may leak
the foreign resource if the native API needs explicit destruction.

Printing, f-string interpolation, and `str(...)` render a handle as
`<opaque TypeName>`, using its canonical Aura type name. The pointer address
never appears in source-visible output or diagnostics.

## Diagnostics

| Code | Cause |
| --- | --- |
| `AU1101` | Malformed extern or opaque syntax. The parser gives dedicated guidance for a foreign body, defaults, type parameters, variadics, and raw-pointer spelling. |
| `AU2002` | A type outside the fixed scalar, view, C text, struct, and opaque-handle tables, including a returned `str` or `list[uint8]` view, a `cstr` in a union other than `cstr \| None`, a `ptr[T]` whose element type is outside the supported fixed-width scalars, C structs, and C unions, and an `extern "C"` class field or union member with no C layout. |
| `AU2003` | Equality or inequality on an opaque handle or a value that structurally contains one. |
| `AU2005` | A reserved FFI form, construction of an opaque handle, a raw-pointer contract that reaches static checking, a callback argument that is not a named top-level function, a function as an extern result, or a `drop` clause that does not name an extern function taking the handle as its one `own` parameter. |
| `AU2010` | A union result or parameter other than `Handle \| None` and `cstr \| None`. |
| `AU2999` | Missing package opt-in, an inaccurate root dependency report, an invalid `[ffi]` library, framework, or path entry, standalone FFI source, an extern used as a value, or another FFI policy violation without a narrower code. |
| `AU3001` | Use of an opaque handle after an `own` extern call. |
| `AU3004` | An invalid scalar, view, C text, pointer, struct, or handle capability. |
| `AU3008` | An opaque handle at a task or Queue `Transfer` boundary. |
| `AU4001` | A non-canonical C boolean: the byte a function returned, stored through a `mut bool` or a `mut ptr[bool]`, or left in a `bool` field of a struct, was neither `0` nor `1`. |
| `AU4005` | A recoverable runtime boundary failure: a null opaque-handle result, a null or invalid UTF-8 `cstr` result or callback argument, an outgoing `cstr` with an embedded NUL, an incorrect array-field or union byte count, and interpreter library, symbol or marshalling failures. |

Aura panics and traps never unwind through a foreign frame. Pre-call failures
stop before entry, and post-call failures are raised after return.

FFI cannot catch or translate a native abort, signal, memory fault, or
foreign unwind. Foreign code must not unwind across the C ABI. Such a native
failure may terminate the process instead of producing an Aura diagnostic.
Out-of-bounds writes, a mismatched C signature, and keeping a temporary view
are outside Aura's memory-safety guarantees.

## Backend Support

The native backend and the interpreter share one validated ABI description.
The native backend emits the C call itself. The interpreter builds each call
at run time. They agree on argument layout, ownership, mutable views,
results, and Aura diagnostics. The maintained `examples/packages/sqlite_notes` package binds the system
SQLite library with handles, `drop` functions, C text, out-parameters, and
a callback. The maintained `examples/packages/raylib` package binds the
raylib graphics library: its programs open a window, draw in 2D and 3D,
read the keyboard and mouse, and write an image. The maintained
`examples/packages/ffi_getpid`
package and the FFI acceptance tests run the same declarations on both.

A foreign call from a built program costs about what the same call costs
from C. A scalar call such as `labs` takes under a nanosecond more than the
function itself. A `str` or `list[uint8]` argument adds a few nanoseconds to
find its bytes, and a `str` literal adds nothing. A `cstr` argument or
result costs one allocation and one copy, about 20 to 40 nanoseconds. An
out-parameter costs a store and a load. A `ptr[T]` argument costs a few
loads to find the list's elements. A struct argument or result costs a
store and a load for each field. A `ptr[Name]` list uses a private C
array. Native code can copy a matching record layout directly. The cost
of a union read depends on the member and its storage. A scalar-only
callback measured about 4 ns more than a C function pointer. A text
callback also validates UTF-8 and copies the string before the Aura body
runs. The benchmark source, reproduction commands and results are in
`benchmarks/ffi_calls/README.md`.

The interpreter loads libraries and looks up symbols on Linux, macOS, and
Windows. Aura does not silently select a different ABI. The declaration must
still name the host's actual C symbol.

## Limits And Implementation-Defined Behavior

FFI does not:

- define C bit-fields, or pass enums
- assign one member of a union in place
- pass a struct that holds a pointer Aura can follow
- allocate foreign memory or expose pointer arithmetic
- return views, or free a `cstr` result
- pass typed pointers to anything but one scalar, one struct, one handle,
  or the elements of a list of scalars or structs
- accept variadics
- pass a closure as a callback, pass a list or struct to one, or return
  text from one
- offer asynchronous foreign calls

Aura does not infer C ABI layout outside the tables above.

Symbol availability and behavior are host-defined. `size_t`, pointer layout,
and process symbol visibility follow the target platform. The fixed-width
integer and floating-point contracts are exact. A declaration that misstates
the real native signature has undefined foreign behavior. It may corrupt or
terminate the process, and no backend can make it safe.

## Status

Implemented:

- package opt-in and the root dependency report
- library, search path, and framework settings in `[ffi]`, with host tables
- bodyless `extern "C"` functions, the `symbol` clause, and opaque handles
- fixed-width scalars and pointer-length views
- `cstr` arguments and results, and `cstr | None`
- `mut` scalar out-parameters
- `ptr[T]` and `mut ptr[T]` pointers to the elements of a scalar list
- `extern "C"` classes passed and returned by value, and passed by pointer
- `array[T, N]` fields of an `extern "C"` class
- `ptr[Name]` and `mut ptr[Name]` pointers to the elements of a struct list
- `extern "C" union` declarations, passed wherever a struct is
- named functions passed as C callbacks, including callbacks taking `cstr`
- direct calls from built programs, and library loading in the interpreter
- `Handle | None` results and parameters, and `mut Handle | None`
- `drop` functions that release an opaque handle the program lets go of

Reserved or unavailable: closures as callbacks, raw-pointer arithmetic, variadics, returned views,
other nullable shapes, and Aura union types such as `int32 | float64`.
Aura does not infer them from current syntax.

Design record:
[anonymous closed union types](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0052-anonymous-closed-union-types.md)
