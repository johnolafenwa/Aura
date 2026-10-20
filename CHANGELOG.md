# Changelog

All notable user-facing changes are recorded here. Aura follows semantic
versioning for release artifacts while it remains a technical preview; a minor
preview release may still contain source and API incompatibilities called out
in this file.

## Unreleased

### Added

- A generic function can call a trait's associated method through its
  bounded type parameter, as in `T.parse(text)` for `T: Parse`.
- Trait names can be module-qualified in implementations, bounds, and
  supertraits, as in `impl shapes.Make for Gadget`.
- The `json.Codec` trait, `json.DecodeError`, and the generic
  `json.encode`, `json.decode`, `json.decode_text`, and `json.schema`
  functions.
- `impl json.Codec for Name` with no body derives `encode`, `decode`, and a
  JSON Schema for a class or enum. An enum without payloads encodes as the
  variant name, and any other enum as an object tagged by `"type"`. A field
  can hold a union whose members have different JSON kinds, such as
  `int64 | str`. `AU2046` reports what cannot be derived.
- A class, enum, or field docstring becomes the `description` in its derived
  JSON Schema.
- Decorators on module-level functions. `@traced` above `def fact` binds
  `fact` to `traced(fact)`, a `Callable[def(...) -> R]` constant. Decorator
  expressions evaluate top to bottom and apply bottom to top, and a
  recursive call goes through the decorated binding. `AU2047` reports a
  decorator on a generic function, on `main`, on a function with a
  parameter default or view result, or a decorator line with no function
  after it.
- A module member that holds a shared callable can be called as
  `tools.run(3)`.
- Generic decorators. `def traced[A, R](f: own Fn[A, R]) -> Fn[A, R]`
  infers `A` and `R` from each function it decorates. A generic decorator
  factory such as `@retry(3)` takes the rest of its type parameters from
  the decorated function.
- A generic callable alias packs or adapts a value with its type arguments
  written out, as in `Fn[int64, str](lambda args: f"{args}")`.
- `factory(...)(args)` infers a generic factory's type parameters from
  `args` and the expected result when the factory's own arguments leave
  them open.

### Changed

- The reference tool runner in `examples/agents/tool_runner` is version 2.
  It uses derived codecs and schemas for tool calls and a generic
  `@traced` decorator on each tool. It links the JSON library, so its
  release executable is 1,886,456 bytes on Linux x86-64 and 1,789,712
  bytes on macOS arm64, and the size gate records those baselines.
- The semantic interface version is 20, and the native cache format is
  `v8`. Cached executables rebuild once.
- A native executable checks at startup that its runtime matches the value
  layout the compiler read lists with. A mismatch stops the program with
  "this executable was built for a different Aura runtime; rebuild it with
  `aura build`".

### Performance

- A native call enters its frame from one static record per function and
  finds its task state without a map search. Naive recursive `fib(30)` went
  from 129.3 ms to 34.7 ms on Linux x86-64, about 8 times the Rust
  reference. Frame records, the call depth limit, and traces are unchanged.
- A shared `for` loop over a local list of `str`, `list`, `dict`, or `set`
  values reads each element in place when the body only compares it with
  `==` or `!=`, calls `len()` on it, or calls `get(key)` on a `dict`
  element. Such a loop copies no element. On Linux x86-64,
  `string_list_scan` went from 115.0 ms to 32.8 ms and
  `record_field_reads` from 149.9 ms to 122.9 ms.
- Native code reads a list's length and its `int64`, `float64`, and `bool`
  elements inline, with one bounds check, and appends those scalars without
  a box. `int_list_sum` runs 106,060,271 instructions instead of
  444,958,513, and went from 47.3 ms to 23.9 ms on Linux x86-64.

### Fixed

- `aura fmt` wrote `@property` as `@ property` and could separate a marker
  from its method with blank lines.
- On the direct backend, a callable stored in a module constant that
  returned another callable could crash or leak when called.

- A `view` or a `for x in mut` element used inside a later `while` or `for`
  loop failed to compile with `AU4001`, because its loan ended at its last
  use inside the loop body.
- `case j.Value.Object(fields)` after `import json as j` failed with
  `AU2001`. Patterns now resolve a module alias.

## VS Code extension 0.5.0 — 2026-10-19

- Bundles language server 0.5.0. The semantic interface version stays 19.

## 0.5.0 — 2026-10-19 (technical preview)

Aura 0.5.0 makes native executables smaller and collection-heavy programs
faster, adds `fs.rename` and `fs.File.sync`, and fixes the defects a review
of the release found. Source written for 0.4.1 may need the changes listed
under "Breaking changes".

### Breaking changes

- The checker enforces the documented mutable receiver for
  every file, process, and network resource method. A `close()`, `flush()`,
  or write through a binding from a bare `match`, or through an immutable
  local or parameter, now fails with `AU3003` at check time. It used to
  pass the checker and fail later with `AU4001` "mutates through shared
  loan", or run on the MIR backend for an enum payload. Use `match own` or
  `match mut` for a match binding, and `mut` or `with` for a local.
- `list.get`, `dict.get`, `copy()`, and the other
  operations that duplicate a value reject a collection that holds a file,
  process, or network resource. They fail with `AU3007` "would duplicate
  resource". A resource has one owner, so borrow it with `view` or
  `match mut`, or take it out with `pop` or `remove`.
- `aura build` always uses the direct backend. `--backend auto` and
  `--backend direct` select the same code generator, and a file with no
  entry point fails with `AU2999`.
- The native cache format is `v7`. Executables cached by 0.4.1 rebuild on
  first use.

### Performance

- A native hello world on Linux x86-64 is 927,888 bytes, down from
  5,057,904. User programs link `libaura_runtime.a` in place of the
  compiler archive, and a program links only the host builtins it calls.
  Release archives ship `lib/aura/libaura_runtime.a`, and the installer
  removes the old `libaura_compiler.a`.
- `dict` and `set` are hash-indexed. Building and reading a large `dict`
  went from 18.47 s to 0.31 s in the runtime benchmark.
- `list`, `dict`, and `set` copy their storage on write, so passing or
  storing a collection no longer copies its elements. Summing a large
  `int64` list went from 1.96 s to 0.048 s.
- A value used by only one thread skips its lock.
- On the direct backend, a method call on a collection field, such as
  `self.docs.get(i)` or `for doc in self.docs`, no longer copies the whole
  field. Neither does passing a field such as `self.by_id` to a function
  parameter, or calling a method on an entry reached through `lookup`.
  Each of these used to deep-clone the field or entry on every call, so
  loops over a field were quadratic.

### Library

- `fs.rename(source, target)` renames or moves a file and replaces an
  existing target. `fs.File.sync()` flushes a file and waits until its data
  reaches storage. Together they write a file atomically. See
  `examples/io/atomic_save.au`.

### Fixes

- A function in a dependency package can call builtin modules such as `fs`
  and `json`. It used to fail at run time with `AU4001` "unknown MIR
  function `pkg.fs::exists`".
- A module's private class works when another module calls that module's
  public functions or methods on the MIR backend. Constructing or calling a
  private class there used to fail with `AU4001` "unknown MIR class". The
  native backend was not affected.
- On the direct backend, passing two fields that may name the same object,
  such as `use_two(x.items, y.items)` or `b.overlap(b)`, works. It used to
  fail with `AU4001` "has no field". The same holds for a lent element
  receiver such as `x.order[0].size_with(y.order[0].items)`.
- A nested index assignment such as `d["a"][0] = 5` or `rows[i][0] += 1`
  writes the element. The direct backend used to drop the write, and the
  MIR backend failed at run time.
- An index assignment through a `view mut`, a `match mut` binding, or a
  `for ... in mut` element, such as `inner["q"] = 1`, compiles and writes
  through.
- A `lookup` or `view` loan over a `dict[str, ...]` that ends in both arms
  of an `if` no longer panics the direct backend.
- A function that returns a union parameter of Copy members, such as
  `def echo(v: int64 | None) -> int64 | None`, runs. It used to fail with
  "invalid MIR payload projection".
- `match mut b` works when `b` is a `view mut` alias, and an assignment to
  a view inside the arm writes through.
- `rows[i].sort()` sorts the element in place. It used to panic the
  compiler or fail at run time.
- A lambda can name a module member, such as `lambda x: math.cos(x)`,
  without moving the module into the closure.
- `x = math.nan` binds the constant. It used to fail with `AU3001` "cannot
  move".
- A default argument can name a module constant or an imported module. The
  default resolves names in the module that declares the function. A
  default that names another parameter reports `AU2004` "may not reference
  parameter" in place of `AU2001` "unknown name".
- On the direct backend, an imported module's private class can share a
  name with a root module class and appear in a union. The build used to
  fail with "unsupported value coercion".

## VS Code extension 0.4.1 — 2026-10-16

- Bundles language server 0.4.1. The semantic interface version stays 19.

## 0.4.1 — 2026-10-16 (technical preview)

Aura 0.4.1 fixes defects found after 0.4.0 and removes a shared git cache
location. It has no language or interface changes beyond these fixes.

- A cast of a call result, such as `box.size() as float64`, converts the
  value the call returns. It used to report `AU2002` "result type mismatch".
  A conditional operand, such as `(n if n > 0 else 1) as float64`, also
  works.
- A `match` over a union reports `AU2999` for an or-pattern whose class
  pattern repeats or is covered by an earlier alternative, such as
  `case Circle() | Circle(radius=2):`.
- A union written through a module alias, such as `geometry.Point | None`
  after `import shapes.geometry as geometry`, works in local annotations on
  the MIR backend. `is not None` tests and member access on it no longer
  fail with `AU4001`.
- Git dependencies are cached only under `$XDG_CACHE_HOME/aura/git` or
  `$HOME/.cache/aura/git`. The shared temporary directory fallback is
  removed, because another local user could plant a checkout there. With
  neither variable set, resolving a git dependency fails with a message that
  names them.
- The MIR validator rejects an enum construction whose variant is not
  declared or whose payload count differs from the declaration.

## VS Code extension 0.4.0 — 2026-10-15

- Bundles language server 0.4.0, which speaks semantic interface version 19.
- Format Document runs the compiler's formatter, the same one `aura fmt`
  uses. Set `"editor.formatOnSave": true` under `"[aura]"` to format on save.
  A buffer with a syntax error is left unchanged.
- Hover, completion, and signature help show declaration docstrings.
  Signature help also shows each parameter's `Args:` entry.
- Statement-position triple-quoted strings get the
  `string.quoted.docstring` scope, so themes can color docstrings apart from
  other strings.
- The language server runs the compiler named by `AURA_LSP_AURA_PATH`, or
  `aura` on `PATH`. It runs a compiler from the opened folder only when
  `AURA_LSP_USE_WORKSPACE_COMPILER=1` is set. The extension does not run in
  untrusted or virtual workspaces.
- Hover, go-to-definition, diagnostics, document symbols, and completion
  map compiler byte offsets to editor positions. Non-ASCII text earlier on a
  line no longer shifts their results.
- Saving, creating, or deleting an `.au` file refreshes the diagnostics and
  hover of every open file that imports it.
- An open file is checked against the unsaved text of the other open Aura
  files.
- Go-to-definition and the outline select the declared name, such as
  `helper` in `def helper()`, instead of text from the `def` keyword.
- Pressing Enter with several cursors places each cursor on its own new
  line.

## 0.4.0 — 2026-10-15 (technical preview)

Aura 0.4.0 adds class initializers, typed context managers, `Span[T]`
slice views, read-only properties, a `Display` trait, structural patterns,
and docstrings. `aura fmt` reflows code, and `aura doc` generates API pages.
Source written for 0.3.6 may need the changes listed under "Breaking
changes".

### Security

- The language server no longer runs `target/debug/aura`,
  `target/release/aura`, or `cargo run` from the opened folder unless
  `AURA_LSP_USE_WORKSPACE_COMPILER=1` is set. Before, opening an `.au` file
  in a cloned repository could run a program that repository supplied. Set
  `AURA_LSP_AURA_PATH` or put `aura` on `PATH` instead.
- The installer script runs only after the whole script has downloaded, so a
  cut-off `curl ... | sh` installs nothing.
- The HTTP runtime rejects a `Content-Length` or chunk size with a sign, such
  as `+5`. Only digits are valid, so Aura agrees with proxies on where a
  message ends.

### Breaking changes

- The semantic interface schema is version 19. Initializers and context
  managers change what a class contract means. Tools that pin the schema
  must update.
- `Span` is a reserved built-in type name. A class, enum, or alias named
  `Span` reports `AU2002`.
- A class that declares `def __init__(self, ...)` is constructed through
  that initializer. Its class call takes the initializer's parameters, and
  the field-based constructor is gone for that class. Calling `__init__` as
  a method or using it as a value reports `AU2005`. A malformed initializer
  or an `__init__` in a trait reports `AU2016`.
- `with` accepts a context manager by shape. A class with `__exit__` uses it
  and ignores `close`. A class with `__enter__` runs it before the body. A
  `__enter__` or `__exit__` of the wrong shape, or a `with` item that is not
  a manager, reports `AU2017`.
- A tuple, list, set, or dictionary literal moves its non-copy elements in
  every position. A later use of the moved name reports `AU3001`, as in
  `print((pair, 2))` followed by `print(pair)`. Such a program passed
  `aura check` and trapped at run time.
- Reading a list inside its own `for x in mut items:` loop reports `AU3002`.
  Read through the loop binding, or read the list before or after the loop.
- `aura fmt` reflows code to an 88-column layout with two blank lines
  between top-level declarations. Running it on existing files changes most
  of them, so `aura fmt --check` fails until the code is reformatted once.
- `aura complete` reads its character position as a UTF-8 byte offset, the
  same unit as the columns `aura analyze` reports.

### Language

- **Span views.** `view part = values[a:b]` lends a range of a list as a
  `Span[T]` without copying. `view mut part = values[a:b]` lends it for
  writing. A span supports indexing, iteration, `in`, `==`, `len`, `index`,
  `count`, `reverse`, `sort`, `swap`, `fill`, and `to_list`. An invalid
  range or position traps with `AU4003`.
- **Span evaluation.** A span evaluates its base, start, and end once each,
  in that order. `view alias = part` reborrows a span with the same range.
- **Span parameters.** A `Span[T]` or `mut Span[T]` parameter accepts a
  slice, a whole list, or another span, as in `total(values[1:3])`.
- **Returned range views.** A function can return a range of a list
  parameter with `-> view Span[T] from values`. Bind the result with `view`
  or `view mut`. Any other use reports `AU3010`. The call evaluates its
  origin argument once, so `middle(rows[i()])` covers the row the call used.
- **Union-parameter borrowing.** A bare union parameter such as
  `str | None` borrows a member argument for the call. A local, field,
  list element, dictionary entry, loop binding, or view works, and nothing
  is copied. A `mut` union parameter still needs the exact union type, and
  other arguments report `AU2010`.
- **Class initializers.** `def __init__(self, ...)` gives a class call its
  own parameters, with defaults and keyword-only parameters. Field defaults
  run before the body. The checker proves that every field is set before it
  is read, before `self` is used as a whole, and on every path out. A
  violation reports `AU3012`. A trap or cancellation during an initializer
  drops the fields already set, exactly once.
- **Typed context managers.** A class is a context manager when it declares
  `__exit__(mut self)` returning `None` or `Result[None, E]`. An optional
  `__enter__(mut self)` returns `None` or a view of `self`, and the `with`
  name then binds that view. A class with only `close(mut self) -> None` is
  a manager too, and generic classes qualify.
- **Multi-item `with`.** `with a = A(), b = B():` enters items left to
  right and exits them in reverse. Later items can use earlier bindings.
- **Result exits.** An `__exit__` that returns `Result` acts like `try` on
  the normal paths out of the body. Under a trap, its error becomes a note on
  the trap diagnostic. Under `try` propagation or cancellation, it is
  dropped. Exits run with cancellation still pending.
- **Trailing commas.** Every comma-separated list closed by `)`, `]`, or `}`
  accepts one trailing comma. A list without a closing delimiter still
  rejects one.
- **Parenthesized imports.** `from module import (a, b as c,)` can span
  several lines.
- **Text methods.** `str` gains `trim_start`, `trim_end`, `split_once`,
  `rsplit_once`, `lines`, `split_whitespace`, `find`, `count`, and
  `repeat`. An empty separator for `split_once` or `rsplit_once`, an empty
  `count` text, or a negative `repeat` count traps with `AU4003`. A `repeat`
  result above 64 MiB traps with `AU4005` before it allocates.
- **Byte search.** The `bytes` module gains `starts_with`, `ends_with`, and
  `find`. They compare raw bytes, and `find` runs in linear time.
- **Display.** Every module sees a prelude `Display` trait with
  `def display(self) -> str`. Implement it for a class or enum the module
  declares. `print`, `str(value)`, f-string fields, and nested rendering
  then call `display`. A module's own item named `Display` shadows the
  prelude trait. Invalid implementations report `AU2043`.
- **Display formatting.** An f-string field with a `Display` value accepts
  fill, alignment, width, precision, and `s`. Other format flags report
  `AU2002`.
- **Atomic `print`.** `print` renders its whole line before it writes. If a
  `display` call traps, that `print` writes nothing.
- **Read-only properties.** `@property` on the line before a shared method
  makes it a getter that reads like a field, as in `reply.total_tokens`. A
  getter has no parameters and returns an owned value other than `None`.
  Invalid getters report `AU2044`. Assignment, `mut` use, views, and call
  syntax on a property report `AU2045`. Any other decorator reports
  `AU2005`.
- **Tuple rest elements.** `*name` or `*_` in a tuple pattern or unpacking
  target covers the leftover positions, as in `head, *tail = triple`.
  Misplaced or extra rest elements report `AU2041`.
- **Sequence patterns.** A bare `match` on a list accepts patterns such as
  `[]`, `[only]`, and `[first, *rest]`. A named rest binds an owned list
  slice. `match mut` and `match own` reject sequence patterns with `AU2041`.
- **Class patterns.** `Point(x=0, y=y)` matches a class value or a union
  member class and tests the named fields. `match own` moves the fields out.
  Invalid class patterns report `AU2040`.
- **Generic class patterns.** Over a union, `Box()` matches every
  specialization of `Box` in the union. A tested field whose type differs
  between those specializations reports `AU2040`.
- **Docstrings.** A triple-quoted string as the first statement of a module,
  class, enum, trait, function, or method documents it. A triple-quoted
  string on the line after a field documents the field. Docstrings produce
  no code. A misplaced triple-quoted string in a class, enum, trait, or
  `impl` body, or documentation over a size limit, reports `AU2042`.

### Tooling

- **Formatter.** `aura fmt` is a token-based formatter. It applies
  four-space indentation, operator and comma spacing, blank-line rules,
  88-column bracket reflow, and magic trailing commas. Comments and literal
  text are kept exactly. The output must lex to the same tokens and parse,
  or the file is left unchanged. A parenthesized lambda keeps its
  parameters together and never gains a trailing comma.
- **Analysis dependencies.** `aura analyze` reports a `dependencies` array
  with every source file the check read. A failed check keeps the files it
  read before the failure.
- **Unsaved buffers.** `aura lsp` accepts an optional `overlays` array of
  `{"path", "source"}` objects. The compiler reads that text in place of the
  named files.
- **`aura fmt --stdin`.** Formats standard input to standard output. Add
  `--check` to report without writing.
- **Editor formatting.** The `aura lsp` compiler service has a `format`
  method. The language server offers whole-document formatting with the
  same rules as `aura fmt`.
- **`aura doc`.** Checks each source and writes one API page per module.
  Pages go to standard output or to `--out DIR`, as Markdown or as JSON with
  `--json`. Private declarations are omitted.
- **Hover.** Hover shows declaration docstrings after the signature. A
  span binding hovers as `Span[T]` with its range source. A `with` name
  that binds an entry view hovers as that view. Class hover lists
  `__init__`, and property hover shows the property type and the getter's
  docstring.
- **Completion.** Items carry the first docstring line. Member completion
  lists span members, properties, and the new `str` methods, and omits
  `__init__`. Completion in the `bytes` namespace lists the new byte
  functions. Class completion detail shows the initializer's call shape.
- **Signature help.** A class call shows the initializer's parameters.
  Signature help shows the docstring and each parameter's `Args:` entry.
- **Navigation.** Hover and go-to-definition work on rest, sequence, and
  class pattern bindings, and on class and field names in class patterns.
  Properties appear in document symbols.

### Fixes

- The direct backend released a shared-loan lambda capture twice. Calling
  `lambda [values]: len(values)` and then dropping the closure trapped with
  `AU4001`.
- On the direct backend, passing a borrowed `str | None` to a generic
  `T | None` parameter emptied the caller's string. A later `print(name)`
  printed `None`.
- A `match` expression passed to a borrowed parameter moved a non-copy arm
  place. A later use failed at run time with "unknown MIR place".
- Assigning an element through a whole-list view, as in
  `view mut w = items` then `w[2] = 9`, failed MIR validation.
- An unknown `dict` member such as `get_optional` reports `AU2001` with help
  that points to `get` and `in`.
- TLS connections use rustls 0.23.46, which fixes advisories
  RUSTSEC-2026-0346 through RUSTSEC-2026-0349.
- Field reads, nested fields, method calls, and field writes such as
  `point.x += 10` work on a class value narrowed by `is not None`. This
  includes a module-qualified class such as `geometry.Point | None`. The
  checker reported `AU2999`.
- Moving a non-copy field out of a narrowed union reports `AU3004` with a
  fix. Passing a narrowed field to a `mut T | None` parameter passes MIR
  validation.
- A view of a list element of a returned view, such as
  `view first = rows_of(grid)[0]`, reports `AU3004` and asks for a `view`
  binding first. It failed at run time with `AU4001`.

### Documentation

- A new manual page, Docstrings And API Docs, covers docstring placement,
  `Args:` sections, presentation, `aura doc`, and limits.
- The Classes page covers initializers, definite initialization, properties,
  and context manager classes. The Statements page describes the manager
  protocol, entry views, and `Result` exits.
- Collections has a `Span[T]` section. Enums And Pattern Matching covers
  rest elements, sequence patterns, and class patterns. Generics And Traits
  covers `Display`.
- CLI And Tooling describes the formatter rules, `aura fmt --stdin`,
  `aura doc`, and editor formatting.
- New examples: `collections/span_views.au`, `classes/initializers.au`,
  `classes/factories.au`, `classes/properties.au`,
  `resources/context_managers.au`, `strings/text_parsing.au`,
  `traits/display.au`, `enums/structural_patterns.au`, and
  `modules/documented_api.au`. Tutorials 02, 05 through 11, 14, 16, 17, 22,
  24, and 25 cover the new features.

### Current limits

- `Span[T]` covers `list[T]` only. There are no spans of `str` or `Array`,
  and a method cannot return a span of its receiver.
- A function with a `Span[T]` parameter or result is not a function value,
  and a lambda cannot capture a span.
- An initializer cannot move a field out of `self` or return a `Result`. Use
  a named factory for fallible or alternate construction.
- `match mut` rejects class and sequence patterns, and `match own` rejects
  sequence patterns.
- `aura doc` writes Markdown and JSON, not HTML. Enum variants and `impl`
  blocks have no documentation.
- `aura fmt` cannot break a long expression at a binary operator. A
  statement without brackets may stay longer than 88 columns.
- A view local forwarded from a method's returned view can fail at run time
  with `AU4001` "uses ended loan".
- A union over a module-qualified type, such as `geometry.Point | None`, can
  fail at run time with `AU4001` "union tag test type identity mismatch".
- Duplicate or-alternatives of refutable class patterns over a union are not
  reported as unreachable.
- A cast of a method result, such as `x.m() as float64`, reports "result
  type mismatch".
- A comprehension over `obj.items()` can fail with "cannot move borrowed
  value" or "unknown name".

## VS Code extension 0.3.7 — 2026-10-11

- Bundles language server 0.3.6. The semantic interface version stays 17.

## 0.3.6 — 2026-10-11 (technical preview)

Aura 0.3.6 fixes defects in element and entry views reported against 0.3.5.
It has no breaking changes.

### Fixes

- The direct backend runs a returned view whose path crosses a class field
  between two collection selections, such as
  `return view mut boxes[0].values[-1]`. It trapped with `AU4001: invalid
  element path`.
- `list.set` with keyword arguments works beside a live view of another
  element, in any argument order. The MIR backend rejected the program and
  the direct backend reported `AU2999`.
- A disjoint `set` and the other mutating collection methods accept a
  grouped or field receiver, such as `(values).set(1, x)` or
  `box.values.set(1, x)`, beside a mutable element view. The checker
  reported `AU3002`.
- Dictionary entry views work for every dictionary key type on both
  backends. Float keys, integer keys outside the `int64` range, and enum
  keys failed on the MIR backend, and a returned view with a float key
  failed to compile on the direct backend.
- A view-returning call accepts an integer literal argument that only fits
  its parameter's wider type, such as an `int128` key. The checker reported
  that the literal does not fit in `int64`.

## VS Code extension 0.3.6 — 2026-10-10

- Bundles language server 0.3.5, which speaks semantic interface version 17.
- Completions list `lookup` for lists and dictionaries, and show `Lookup[T]`
  as the result of list `get`.

## 0.3.5 — 2026-10-10 (technical preview)

Aura 0.3.5 completes element and entry views. A list element or dictionary
entry works in place wherever a place is borrowed, a function can return a
view of one, and `for x in mut items:` writes through to the list.

### Breaking changes

- The semantic interface schema is version 17, because returned-view
  contracts can name an element step. Tools that pin the schema must update.
- In a script, a bare `name = value` after the first top-level statement
  declares an immutable top-level local instead of a module constant. It can
  read earlier locals, and closures can capture it. Bindings above the first
  statement are still module constants. A function cannot read a name bound
  after the first statement, and `public name = value` after a statement is a
  syntax error (`AU1101`).
- Mutating a collection while an element or entry view of it is live reports
  `AU3011`, which names the view and the operation. A structural change such
  as `append` or any dictionary assignment is refused, and so is a write to
  the viewed element or through a computed index. A write to a different
  literal position, such as `values[1] = x`, `values[1] += x`, or
  `values.set(1, x)` for a Copy element, is allowed.

### Language

- **Elements in place.** A list element or dictionary entry can be a shared
  or `mut` argument, a method receiver, an operand, a `match` or `match mut`
  scrutinee, or part of a nested selection such as `bump(grid[i][j])`. Writes
  through a `mut` argument or mutating receiver land in the collection.
  Moving a non-copy element out is refused with `AU3005`, and the message
  suggests a `view` first.
- **Returned element views.** A function can return a view of one element or
  entry of its origin: `return view items[i]`, `return view mut table[key]`,
  or a field inside the element. The index or key is evaluated once in the
  callee, and an invalid one traps there with `AU4003`. The caller's view
  holds the whole collection while it is live. A returned view of an element
  argument, such as `view name = label(users[0])`, reaches the element.
- **`lookup`.** Lists and dictionaries gain `lookup`, which views an element
  in place when it may be missing. Write `match items.lookup(i):` or
  `match mut table.lookup(key):` with `case Lookup.Found(item):` and
  `case Lookup.Missing:` arms. Nothing is cloned, so it works for any element
  type. `lookup` is valid only as the subject of a `match` statement.
- **Mutable iteration.** `for x in mut items:` binds each element as a
  mutable element view, so a write lands in the list immediately. Non-Copy
  elements are not cloned and written back. A nested element such as
  `for x in mut grid[1]:` can be iterated.
- **Owned bindings.** A binding introduced by a consuming form is a mutable
  place: the element of `for value in own values:`, every leaf of its tuple
  target, and a `match own` payload or type-pattern binding.
- Reading `users[1]` while a mutable view of `users[0]` is live is allowed.
  Literal positions and keys are disjoint.
- Narrowing applies to top-level script bindings on both backends, including
  module constants such as `value: int64 | None = 5` followed by
  `if value is not None:`.

### Fixes

- Iterating a nested element with `for x in mut grid[1]:` failed with an
  internal MIR error.
- A view whose last use is inside an `if`, `else`, or `match` branch that
  then leaves the loop with `break`, `continue`, or `return` failed with
  "ends inactive loan".
- The direct backend stopped with "does not know dynamic method" on a method
  call on a field reached through an element view, such as
  `team.members.append(name)` after `view mut team = teams[0]`.
- The direct backend refused a narrowed read of a top-level script binding.
- A `match` with type patterns over a union-valued element view, such as
  `view v = labels["name"]` then `match v: case str as text:`, failed MIR
  validation.
- Every `aura` code block in the documentation compiles. Seventeen broken
  examples were corrected.

### Current limits

- Slice views and `Span[T]` are not implemented. A slice in value position
  is a fresh owned list.
- A non-copy value cannot be borrowed into a union parameter (`AU2010`).
  Pass an owned value or a clone.

## VS Code extension 0.3.5 — 2026-09-22

- Bundles language server 0.3.4, which speaks semantic interface version 16.
- Adds signature help, find references, and rename with a prepare step. Rename
  refuses an edit that would change which binding a name refers to.
- Highlights union types, `type` aliases, `|`, the contextual `is`, `Callable`,
  and stored view contracts in type positions.

## 0.3.4 — 2026-09-22 (technical preview)

Aura 0.3.4 adds union types, complete callable contracts, element and entry
views, and a faster direct backend. It removes `Option[T]` in favour of
`T | None`. Source written for 0.3.3 that uses `Option` needs the changes
listed under "Breaking changes".

### Breaking changes

- `Option[T]`, `Option.Some`, `Option.None`, and the `T?` suffix are gone.
  Write `T | None` instead. `Option[int64]` is now an unknown type and `T?` is
  a parse error. You may declare your own enum named `Option`.
- `list.get`, `dict.get`, and `dict.remove` return the new `Lookup[T]` enum,
  with `Found(value)` and `Missing`.
- `Queue.get_or_none` and `Task.result_or_none` are renamed `poll`. They
  return the new `Poll[T]` enum, with `Ready(value)` and `Unavailable`.
- Every other optional library result or parameter is now `T | None`. This
  covers `str.strip_prefix` and `strip_suffix`, `Array.get` and `set`,
  `io.read_line`, `sys.env`, `path.parent`, `file_name`, and `extension`,
  the `json.as_*` and `into_*` accessors, the `json.dumps` `indent`
  parameter, the network `read_*` and `recv*` results, the `cwd` parameter of
  `process.start`, `run`, and `Supervisor.start`, `process.Child.stdin`,
  `stdout`, and `stderr`, and the `wait_or_none`, `read_line`, and
  `read_bytes` results.
- A recursive optional class field is written `next: indirect Node | None`.
- A callable value must match its destination's complete contract, including
  parameter names, keyword-only markers, and defaults (`AU2015`). A narrower
  contract needs an explicit alias or callable constructor. The rule also
  applies when the destination is a union such as
  `(def(value: int64) -> int64) | None`.
- `None` prints as `None` in `print`, `str`, f-strings, and containers. It
  used to print as empty text, so `[, 1]` is now `[None, 1]`.
- The semantic interface schema is version 16. Editor caches from older
  versions are rebuilt.

### Language

- **Union types.** `A | B` annotations, `type` aliases, type patterns in
  `match` with exhaustiveness checking, and narrowing after a conditional
  test. Generic code accepts union type arguments. A union is Copy,
  clone-safe, or Transfer exactly when every member is. Equality, hashing,
  and trait calls go to the active member. One handle type plus `None` can be
  an extern C result, passed as a nullable pointer.
- **Complete callable contracts.** Parameter names, keyword-only markers, and
  defaults are part of every function, lambda, and callable type.
  `Callable[...]` and `TaskCallable[...]` store owned callables, and every
  `TaskGroup` start method accepts a stored `TaskCallable`.
- **Method values.** `receiver.method` is a closure over the receiver,
  `Class.method` is a function value, and `method[T]` fixes a generic
  method's type arguments. A stored callable may return a view of one named
  parameter, such as `def(pair: Pair) -> view str from pair`.
- **Element and entry views.** `view name = items[i]`,
  `view mut name = table[key]`, and projections such as
  `view mut visits = users[i].visits` bind one list element or dictionary
  entry in place. The index is evaluated once, and a missing position or key
  fails there with `AU4003`. Two views of different literal positions or keys
  can coexist. A computed index, or a structural change such as `append`,
  conflicts with every element view (`AU3002`).
- **Field access through an element.** `users[i].visits` reads the field in
  place, and `users[i].visits = 5` or `+= 1` writes it in place.

### Performance

- Unions and callables are inline values on the direct backend. A union is
  one tagged value, and a callable is four words with up to three words of
  captures inline, so calls, moves, and copies allocate nothing. A larger
  closure environment takes one checked block, and failure to allocate it is
  `AU4005`. A callable is boxed only where it leaves generated code: a
  container, a task start, or a runtime helper.
- Cranelift now optimizes for speed.
- Release builds use fat LTO, one codegen unit, and stripped symbols. On the
  measured arm64 macOS host the compiler is 29.35% smaller than
  `v0.3.3-preview`, and the hello and worker executables are 93.29% and 84.54%
  smaller. The Performance chapter has the byte counts.
- Shared `float32` and `float64` Array elementwise kernels and scalar
  broadcasts are vectorized, with exact reductions, division traps, and
  integer modes unchanged. On a quiet host, clean detached addition falls from
  1.244818 ms to 0.249473 ms, within 1.0065x of Rust.
- The `fib`, `int32`, and `int64` benchmarks improve by 19.06%, 20.56%, and
  15.69% at the median.
- The program entry runs on a 16 MiB stack. A task whose stack cannot hold
  another call fails with `AU4005` instead of crashing. Child tasks default to
  768 KiB.
- `AURA_RUNTIME_STATS=1` reports union boxes, closure environments, runtime
  boxes, and callable overflow allocations on standard error.
  `AURA_NATIVE_KEEP_SYMBOLS=1` keeps symbols in native binaries for profiling.

### Tooling

- The CLI and language server add signature help, references, and rename.
- Editor analysis resolves type aliases and packed `Callable` calls.
- `examples/agents/tool_runner/` is a maintained reference package: a tool
  registry with typed JSON requests and results, typed errors, retry, Queue
  streaming, and resource cleanup.
- The benchmark lanes gain pinned Rust baselines, and every Aura source under
  `benchmarks/` is checked by the CLI tests.

### Fixes

- A `match` on a trait method call through a union receiver no longer panics
  the compiler.
- Reading a trait bound's method as a field of a type-parameter value is
  refused with `AU2005` instead of trapping at run time.
- A stored `TaskCallable` target must be Transfer in every argument slot, and
  a target that returns a view of its arguments is refused (`AU3008`).
- Generic enum inference, imported type arguments, parenthesized specialized
  constructors, loop-guard narrowing, union values meeting trait bounds,
  method binding through a Copy view, and `control.retry` with a packed
  worker all work as documented.
- A bare tuple is refused as a trait implementation target.
- In a top-level script, assigning through a view (`view mut second =
  pair[1]` then `second = 7`) writes through the view. It used to be read as
  a new module constant and refused with `AU3004`.
- A view of an immutable top-level binding is refused with a clear `AU3004`
  message. The binding is a module constant, which has no place to lend; bind
  it with `mut` or inside a function. It used to pass the checker and then
  fail as an internal MIR error.
- `rustls` is 0.23.45, which addresses RUSTSEC-2026-0285.

### Current limits

- A list element or dictionary entry cannot yet be a `mut` argument, the
  receiver of a mutating method, or the result of `return view`. Bind it with
  `view mut` first.
- A nested index in one expression, such as `grid[i][j]` on a non-Copy
  element, is refused. Take the inner element with a view first.

## VS Code extension 0.3.4 — 2026-09-06

- Added syntax highlighting and snippets for shared and mutable views,
  returned-view contracts, and explicit shared/mutable/owned closure captures.
- Updated the bundled language server to semantic interface version 6 with
  view-aware diagnostics, completion, hover, go-to-definition, and capture
  provenance.

## 0.3.3 — 2026-09-06 (technical preview)

Aura 0.3.3 is a technical preview feature release of the compiler,
command-line tools, language server, VS Code extension, Manual, Learn track,
tutorials, examples, and installation tooling.

- Implemented ADR-0038 place-based loans and views. `view name = place` and
  `view mut name = place` grant shared or exclusive non-owning access to fixed local,
  parameter, receiver, class-field, and tuple places without copying or moving
  the owner.
- Added inferred last-use lifetimes, reborrowing with parent suspension and
  resumption, immediate mutable write-through, and deterministic cleanup on
  ordinary, early-return, branch, match, and managed-resource exits.
- Added returned-view contracts with `view ... from source`, including methods,
  generic trait dispatch, forwarded calls, and exact runtime handoff of the
  selected fixed projection from one declared origin.
- Added explicit closure loan captures (`[value]`, `[mut value]`, and
  `[own value]`) with repeatability, escape, storage, ownership, and Transfer
  checks that preserve the source place and release each loan exactly once.
- Added stable `AU3010` provenance, overlap, escape, and lifetime diagnostics;
  compiler analysis and editor tooling expose the same place and capture
  information.
- Added explicit MIR loan operations, public serialized-MIR validation, and
  direct-native lowering with forced MIR/direct parity across local, returned,
  reborrowed, and closure-held views. Validation bounds adversarial projection
  expansion before allocation.
- Ratified ADR-0045's binding-test and assertion-introspection checkpoint, and
  formally recorded ADR-0049's positional and named class-pattern deferment.
- Published VS Code extension 0.3.4 with the matching syntax, snippets, and
  compiler-backed semantic behavior.

## VS Code extension 0.3.3 — 2026-08-04

- Added builtin-function highlighting for calls such as `print(...)`,
  `range(...)`, and `len(...)`.
- Preserved member completion inside nested calls and in buffers containing an
  unrelated compiler diagnostic.
- Fixed extension shutdown by registering the language client itself as the
  disposable after asynchronous startup.

## 0.3.2 — 2026-08-04 (technical preview)

Aura 0.3.2 is a technical preview patch release of the compiler, command-line
tools, language server, VS Code extension, Manual, Learn track, tutorials, and
installation tooling.

- Fixed MIR and direct execution of module constants that call list
  algorithms such as `map`, `filter`, and `copy`.
- Fixed module-constant list, set, and dictionary comprehensions by retaining
  their checked closure and comprehension metadata through lowering.
- Added `aura upgrade`, which downloads the maintained installer and upgrades
  the compiler and bundled native runtime in the active install prefix.
- Corrected tutorial ownership, equality, error-conversion, queue-send,
  receiver, networking, numeric-cast, and current-surface guidance.
- Classified and compiler-checked every Aura tutorial fence through the new
  tutorial integrity gate.
- Published VS Code extension 0.3.3 with builtin highlighting, resilient list
  member completion, and correct language-client disposal.

## VS Code extension 0.3.2 — 2026-08-04

- Fixed VS Code and documentation highlighting for f-string interpolations.
  Interpolation braces now receive the standard format-placeholder scope, and
  embedded identifiers and operators are highlighted as Aura expressions
  instead of inheriting the surrounding string color.

## 0.3.1 — 2026-08-04 (technical preview)

Aura 0.3.1 is a technical preview patch release of the compiler, command-line
tools, language server, VS Code extension, Manual, Learn track, tutorials, and
installation tooling.

- Fixed plain and compound reassignment of mutable top-level script locals.
  Module constants that read a script local now receive a focused diagnostic
  explaining initialization order and the valid `mut` or `main` repairs.
- Fixed direct-backend integer `==` and `!=` when a function call supplies an
  `int32` or `uint64` operand. Function-result temporaries retain their declared
  type across ordinary comparisons, reversed operands, comparison chains, and
  assertion introspection, with MIR/direct parity coverage.
- Documented the required language-server request field
  `semantic_interface_version: 5` in the Manual and server package.
- Added Aura file icons to the VS Code language contribution using the
  maintained Aura mark.
- Expanded platform installation guidance, native Aura syntax highlighting,
  AI-agent documentation entry points, the Python fast track, conversions,
  testing guidance, and the systems-language positioning for agents and ML
  infrastructure.

## 0.3.0 — 2026-08-03 (technical preview)

Aura 0.3.0 is a technical preview of the current Python-shaped language,
compiler, runtime, command-line tools, language server, VS Code extension,
Manual, Learn track, tutorials, and examples. It combines static typing,
deterministic ownership, native compilation, structured concurrency, and typed
failure for systems software, ML infrastructure, and agent runtimes.

- Unified collection positions on `int64`: ranges, list indices, slice
  endpoints, enumeration positions, Array coordinates, and concurrent
  source-result indices now compose directly with `len(...)`. Fixed-width
  `int8`, `int16`, `int32`, `uint8`, `uint16`, and `uint32` values widen
  losslessly only at these position sites.
- Standardized owned text and collections as `str`, `list[T]`, `dict[K, V]`,
  and `set[T]`. Collection literals are homogeneous and methods use the
  canonical Python-shaped names with typed ownership and failures.
- Expanded `aura test` with function-level discovery, literal case-sensitive
  `-k` filtering, schema-versioned JSON results, per-case setup and teardown,
  parametrized function-value registration, and deterministic source-order
  reporting. Supported comparison and membership assertions now report the
  two typed operand values on failure while preserving once-only left-to-right
  evaluation and lazy assertion messages.
- Added module aliases with `import ... as ...` and per-name aliases in
  `from ... import ...` declarations. Aliases preserve the target module,
  declaration, type, visibility, and constant-storage identity.
- Added decimal separators, hexadecimal, binary, and octal integer literals;
  fixed-width bitwise operators; checked shifts; and explicit wrapping and
  saturating shift methods. Added right-associative power, exact-type
  `round` and `divmod`, and the scalar `math` functions `floor`, `ceil`,
  `trunc`, `pow`, `exp`, `log`, `log2`, `log10`, `sin`, `cos`, and `tan`.
- Added exact-content triple-quoted strings, single-line raw strings, and a
  closed f-string format grammar covering alignment, fill, signs, width,
  precision, grouping, integer bases, fixed-point, scientific, percentage,
  and text formatting.
- Added Boolean match guards and recursive or-patterns with left-to-right
  probing, one guard evaluation, exhaustive binding checks, delayed owned
  extraction, and mutable writeback on every guard exit path.
- Added eager immutable module constants with dependency-first, source-ordered,
  once-only initialization. Copy values read by value, while non-Copy values
  provide shared access to their defining module's stored value.
- Added root binding patterns, sign-aware zero padding, equality-obligation
  checking across collections, and precise diagnostics for unsupported string
  prefixes. Values without defined equality cannot reach hidden identity
  comparison through membership, list search, sets, or dictionary keys.
- Published coordinated `aura-v0.3.0-preview-*` archives, the 0.3.0 VS Code
  extension, static documentation, and a verified `SHA256SUMS` manifest.

## 0.2.0 — 2026-07-31 (technical preview)

Aura 0.2.0 is the first complete distribution of the implemented
language reference: a typed Python-shaped source language, deterministic
ownership model, structured-concurrency runtime, MIR and direct-native
backends, package tooling, language server, VS Code extension, and maintained
Learn and Manual tracks. It is intended for evaluation and controlled
experiments, not production deployment or execution of untrusted code. It was
developed under the working name Aurora before its first publication.

### Ownership surface

- Bare parameters and receivers grant logical shared access for every type,
  including copy types. `mut` grants exclusive mutable access and `own`
  transfers ownership.
- Bare `match` is shared matching. `match mut value` enables mutable matching
  with writeback, and `match own value` consumes the scrutinee and owned
  payloads.
- Return annotations carry types only. Every return is an ordinary owned
  return.
- `str.len()`, `str.byte_len()`, `list.len()`, `dict.len()`, and
  `set.len()` return `int64`. `str.byte_len()` is the UTF-8 byte count;
  `str.len()` is the Unicode-scalar count.
- The maintained builtin surface now reserves `len`, `str`, `select`,
  `SelectOutcome`, and the `control` module namespace. Rename conflicting user
  declarations.
- Lightweight tasks run on pinned cooperative OS workers. Per-producer Queue
  order stays FIFO, but global sibling
  scheduling and output order are unspecified. Programs must synchronize any
  order they observe.
- The native artifact-cache format is `aura-native-cache-v5`.

### Language

- Added fixed structural tuples, recursive unpacking and patterns, recursive
  equality, and whole-source copy/move behavior.
- Added conditional expressions, `in`/`not in`, chained comparisons,
  `enumerate` and `zip` loop forms, and maintained `len` and `str` builtins.
- Added eager owned list, set, and dictionary comprehensions, including nested `for`
  clauses and left-to-right filters. Generator expressions remain rejected
  with guidance to use a comprehension or explicit loop.
- Added owned `list[T]` and Unicode-scalar `str` slicing with omitted
  endpoints and one-time negative normalization. Aura traps invalid or
  reversed ranges with `AU4003`; slice bounds are not clamped. Steps, slice
  assignment, str integer indexing, and views
  remain unavailable.
- Added checked contextual narrow integers, wrapping and saturating integer
  methods, multiline delimiter continuation, assertions, bytes/codecs,
  deterministic seeded randomness, JSON, and the maintained typed
  control-plane modules.
- Kept the Manual reference-frozen: every accepted semantic addition has an
  ADR, executable conformance coverage, examples or tutorials where relevant,
  and MIR/direct parity evidence.

### Runtime and structured concurrency

- Fixed a queue-iteration livelock under oversubscription. Receive iteration
  now subscribes only to producers that are still running; a producer that
  has already completed can no longer keep every consumer in an immediate
  scheduler-ready loop while CPU burners occupy the worker pool. The reported
  iteration-consumer shape is pinned on MIR and direct at the default worker
  count.
- Replaced the periodic scheduler tick with persistent readiness registration,
  heap deadlines, cross-worker notifications, and loop-backedge fairness
  checks. Idle workers block until runnable work, I/O readiness, or a deadline.
- Added guarded 512 KiB default task stacks and explicit bounded stack-size
  overrides, compiler-derived structural `Transfer`, conditional task-handle
  Copy, and statically single-consumer non-repeatable results.
- Added typed heterogeneous `select(...)` across Queue, Task, and Duration
  sources, plus `wait_any`, `wait_all`, cancellation, scheduler-aware Queue
  operations, and structured child cleanup.
- Added a lazily created blocking-I/O pool. `AURA_BLOCKING_WORKERS` selects an
  exact positive worker count;
  `AURA_BLOCKING_QUEUE_CAPACITY` optionally bounds accepted pending work.
  Invalid settings fail before user code with `AU4006` on both backends.
- Expanded the maintained filesystem, process, TCP/UDP, HTTP, WebSocket, Unix
  socket, TLS, JSON, and control-plane surfaces with typed errors, explicit
  limits, timeout policy, cancellation behavior, and deterministic cleanup.
- Made the direct-native artifact cache content-addressed, integrity checked,
  concurrency safe on maintained Unix hosts, and optional for installed
  immutable runtimes. Long cold operations report whether Aura is waiting for
  a concurrent build or building the native program.

### Callables and closures

- Added structural `def(...) -> ...` types and Copy/Transfer named-function
  values, with inference, generic specialization, cross-module use, and both
  backend implementations.
- Added eager callable-powered natural and keyed `list.sort`, plus `map` and `filter`, and
  let `control.retry` accept repeatable capturing closures as well as named
  functions.
- Added contextually typed expression lambdas. Captures are by value: Copy
  values are snapshotted, non-Copy values move at creation, read-only closures
  are repeatable, and a consuming capture makes the closure single-use. A
  closure is Transfer only when each capture is Transfer.
- Callable equality is uniformly rejected for named function values and
  closures. Compare call results or carry an explicit discriminant instead.
- Mutable captured state, capability capture, nested capture of an enclosing
  lambda's bare parameter, and arbitrary persistence of capturing-closure
  metadata remain outside the 0.2 surface. ADR-0038 defines the planned 0.3
  loan/view foundation.

### Foreign function interface

- Added explicitly authorized FFI v0 packages and direct synchronous
  `extern "C"` calls to process-global symbols.
- The supported ABI includes fixed-width scalars, pointer-length str and
  byte views, and non-null opaque handles. Package manifests must opt in and
  dependency authorization is reported exactly from the root package.
- FFI declarations are a native trust boundary. False declarations or
  misbehaving C code can violate Aura's invariants; callbacks, variadics,
  raw pointer values, nullable handles, returned views, retained views, and
  dynamic-library selection remain unsupported. The MIR and direct backends
  use the same validated ABI description and host-call engine.

### Numeric arrays

- Added owned contiguous row-major `Array[T]` for `int32`, `int64`, `float32`,
  and `float64`, with shape metadata and rank of at least one.
- Added `zeros`, `full`, and `from_list`; multidimensional get/set; `fill`;
  first-axis owned copying slices; map; sum/min/max/mean; exact-shape
  elementwise arithmetic; and scalar arithmetic.
- Elementwise and reduction work runs in dtype-specialized contiguous native
  runtime kernels. Release disassembly recorded scalar floating-point kernels.
  The Manual records the baseline-host one-million-element measurements and
  their provenance.
- Arrays intentionally have no views, array-shape broadcasting, mixed-dtype
  promotion, equality, shape transformations, autograd, accelerators, or
  integer division. Use explicit casts and owned copies.

### Tooling and diagnostics

- `aura --version` now prints the preview channel and 12-hex-digit source
  commit (`aura 0.2.0-preview (<commit>)`), so preview executables are
  distinguishable from a future final 0.2.0. Release publication is marked as
  a GitHub prerelease and includes a generated, verified `SHA256SUMS` asset.
- AU3002 diagnostics use the concise wording “shared values” and retain the
  explicit stable code.
- Added `aura run --backend auto|mir|direct`, native builds with relocatable
  runtime/link manifests, a content-addressed native cache, function-level
  `aura test` discovery, recursive formatting/testing, package/workspace
  resolution and lockfiles, and compiler inspection/analysis commands.
- Added complete typed Aura call frames and child-task ancestry to MIR and
  direct runtime failures. Human diagnostics, schema-version-1 JSON, analysis,
  the language server, and the VS Code extension preserve the structured
  records. A private direct-runtime trap channel keeps ordinary process exit
  status distinct from `AU####` runtime failures.
- Added stable dedicated codes for semantic defect classes, exact
  UTF-8/source spans, completion recovery on incomplete programs, hover and
  go-to-definition, and maintained example-file regression coverage.
- Release archives carry the compiler, native runtime, and linker manifest.
  Installed archives can check, run, and build Aura without Cargo or a source
  checkout; a host C compiler is still required for native output.

### Current limits

- Aura 0.2.0 is a technical preview whose compatibility may change. Production
  systems, sandboxing, and untrusted-source security are outside this release's
  scope.
- Release archives support glibc Linux x86-64 and macOS x86-64/Apple silicon.
  Windows, musl Linux, other architectures, and cross-compilation are
  experimental source-build territory.
- Structured concurrency is cooperative and pinned. Preemption, migration,
  work stealing, and detached tasks are unavailable; sibling order is
  unspecified, and speedup depends on the workload.
- Owned copies are the only 0.2 slicing model. ADR-0038's place loans, returned
  views, mutable closure capture, and view-aware concurrency checks target 0.3.
- Arrays are CPU-only and intentionally narrower than NumPy; externs are
  direct-call-only values on both maintained backends; imported modules have no
  runtime initialization side effects; package registries and publishing are
  not implemented.
- Resource caps, protocol boundaries, backend-specific FFI availability, and
  failure guidance are normative in the Manual's
  [Current Limits](docs/manual/current-limits.md) and
  [Status And Compatibility](docs/manual/status-and-compatibility.md) pages.
