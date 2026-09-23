# Diagnostics

A diagnostic is the compiler's report of a rejected program or a runtime trap.
This page lists every diagnostic code, the fields a diagnostic carries, and how
the CLI and the editor present it. Diagnostics are part of the language and
tooling contract.

Lexing, parsing, static checking, ownership checking, lowering, building, and
runtime traps all report through the same compiler-owned diagnostic structure.

A typed library failure is ordinary program data, not a diagnostic. This
covers `Result.Err`, a `None` member of a `T | None` result, `Lookup.Missing`,
`Poll.Unavailable`, a timeout, cancellation, and an `io.Error` value.

Aura has no builtin `Option` type and no `T?` type suffix. Unless the program
declares its own `Option`, these spellings get only the ordinary diagnostics
below. Nothing else recognizes them.

- `Option[T]` in a type position reports `AU2001` ``unknown type `Option` ``.
- `Option.Some` and `Option.None` report the ordinary `AU2001` unknown name.
- A `?` after a type reports the ordinary `AU1101` unexpected-token error.

## Stable Diagnostic Codes

Every diagnostic has a stable code of the form `AU####`. The first digits name
the phase that owns the failure.

| Band | Phase |
| --- | --- |
| `AU10xx` | lexical analysis |
| `AU11xx` | parsing |
| `AU20xx` | names and types |
| `AU30xx` | ownership, loans, and transfer |
| `AU40xx` | runtime-checked traps |

The registry is append-only:

- A published code MUST NOT be reused, renumbered, or silently reassigned to a
  different diagnostic category.
- An obsolete code keeps its number as a reserved number.
- A new category receives a new number.
- Message wording and attached guidance may become more specific under the
  same code, as long as the failure category is unchanged.

The tables below give each code's meaning, its usual causes, and the usual
fix. The paragraphs after each table give the precise rules.

### Lexical And Syntax Codes

| Code | Meaning | Common causes | How to fix |
| --- | --- | --- | --- |
| `AU1001` | invalid lexical input | An unexpected, mismatched, or unclosed delimiter. A malformed or unterminated string. | Close or remove the delimiter the diagnostic labels. Correct the string literal. |
| `AU1002` | invalid f-string delimiter | A raw or triple-quoted f-string prefix. A single-quoted f-string. | Use the supported f-string form the guidance names. |
| `AU1101` | invalid syntax | Malformed syntax, including a `?` after a type, `mut` or `own` in a comprehension clause, a malformed format specification, or malformed extern syntax. | Rewrite the source in the form the message describes. |

`AU1001` owns source-delimiter pairing:

- An unexpected closer is the primary location.
- A mismatched closer names the expected kind and labels its opener as related
  information.
- End of file with an unclosed delimiter reports the expected closer and labels
  the opener.

Analysis JSON and the Language Server Protocol (LSP) bridge preserve these
locations and labels.

String and format-specification diagnostics are described under
[String And Format Diagnostics](#string-and-format-diagnostics).

### Name And Type Codes

| Code | Meaning | Common causes | How to fix |
| --- | --- | --- | --- |
| `AU2001` | name resolution | An unknown name or an unknown type. | Declare or import the name, or correct its spelling. |
| `AU2002` | type mismatch | A value of the wrong type, a callback with the wrong contract, a type without the ordering an operation needs, or a format specification that does not fit its value. | Make the value's type match the type the position requires. |
| `AU2003` | unsupported operator | An operator the operand types do not define, including ordering or arithmetic on a union. | Use an operator the operand types define. |
| `AU2004` | argument binding | Arguments that do not bind to the callee's parameters. | Match the call to the callee's parameter list. |
| `AU2005` | unsupported syntax or feature | Python-shaped source, a generator expression, a slice step, slice assignment, or a reserved FFI contract. | Use the Aura form the message names. |
| `AU2006` | builtin method collision | A trait method whose name shadows a builtin member of its target. | Rename the trait method. |
| `AU2007` | builtin function redefinition | A module-level function named after a builtin function. | Rename the function. |
| `AU2008` | equality unavailable | `==`, `!=`, or an equality-based collection operation on a type without equality. | Use a type that defines equality, or avoid the equality-based operation. |
| `AU2010` | union member or context | A value that is not a direct member of the expected union, or a union without enough context. | Supply a direct member or the missing type context. |
| `AU2011` | ambiguous union injection | A literal that more than one union member accepts. | Make the intended member explicit. |
| `AU2012` | cyclic type alias | A transparent alias that refers back to itself. | Break the cycle. |
| `AU2013` | union pattern coverage | Missing, duplicate, unreachable, or nonmember union type arms. | Add the missing arms and remove the others. |
| `AU2014` | invalidated narrowing | A member use after the `None` test that narrowed the place stopped applying. | Test the current value again. |
| `AU2015` | callable contract mismatch | A callable whose complete contract differs from the one its destination requires. | Match the contract, or restrict it with an explicit adapter or constructor. |
| `AU2999` | general compile-time rejection | A rejection that has no narrower public category. | Follow the message. |

**`AU2002` type mismatch.** List `map`, `filter`, and keyed `sort` require the
documented shared `def(T) -> ...` parameter capability. The checker does not
silently adapt a `mut` or `own` callback. The same code reports:

- a `control.retry` worker that is not exactly a zero-parameter
  `def() -> Result[T, E]`
- a `sort` element type or keyed `sort` key type without the required natural
  ordering
- a union type argument that fails a trait bound because a member lacks the
  implementation

**`AU2003` unsupported operator.** An incomparable union pair reports
``cannot compare `U` and `T`: expected the same union type or an eligible member``.
Any ordering or arithmetic operator on a union also reports `AU2003`.

**`AU2004` argument binding.** A positional argument that reaches a
keyword-only parameter reports ``parameter `x` of function `f` is keyword-only``.
A named argument aimed at a positional-only slot of a written callable type
also reports `AU2004`.

**`AU2005` unsupported syntax or feature.** Binding a trait method whose
implementation names a keyword-only parameter differently from the trait
reports `AU2005`. The bound closure exposes the trait's names and cannot yet
forward such a slot. For Python-shaped source and slices, see
[Python-Shaped Source Guidance](#python-shaped-source-guidance).

**`AU2006` builtin method collision.** The checker rejects an explicit or
inherited trait method whose name would shadow a builtin member of the
implementation's builtin target. The rule covers every builtin target:

- the runtime handles `Queue[T]`, `Task[T]`, `TaskGroup`, `random.Rng`,
  `fs.File`, and the `net` and `process` handles
- the builtin value types, such as `str`, `list[T]`, `dict[K, V]`, `set[T]`,
  `Duration`, and the scalar types

The guidance requires renaming the trait method. Backend dispatch is never
chosen by whichever implementation happens to run first.

**`AU2007` builtin function redefinition.** The checker rejects a module-level
function declaration whose name is already a builtin function name, such as
`len`, `str`, `abs`, `print`, or `select`. The builtin surface is closed, so
rename the declaration. `AU2006` covers trait methods on a builtin target.
`AU2007` covers free functions.

**`AU2008` equality unavailable.** This code reports an unmet equality
obligation. It covers direct `==` and `!=` and every collection operation that
depends on equality:

- membership
- `list.remove`, `list.index`, and `list.count`
- set element insertion
- dictionary-key use

Named function values, closures, `random.Rng`, opaque FFI handles, and values
containing any of those types do not define equality. The diagnostic names the
missing relation before execution can reach a backend identity comparison.

**`AU2010` union member or context.** This code reports:

- a value that is not a direct member of the expected union
- a union spelling without enough context
- a type parameter that a union member argument cannot determine
- an extern signature whose union is not the one admitted `Handle | None`
  result, such as a union parameter or any other union result

**`AU2011` ambiguous union injection.** A literal that more than one member
could accept reports `AU2011`.

**`AU2012` cyclic type alias.** A cyclic transparent alias reports `AU2012`.

**`AU2013` union pattern coverage.** Missing, duplicate, unreachable, or
nonmember union type arms report `AU2013`.

**`AU2014` invalidated narrowing.** A member use through a place whose
`None`-test narrowing was invalidated reports exactly this message:

``narrowing of `PLACE` no longer applies after OPERATION; test the current value again``

`OPERATION` is `assignment`, `a mutable match`, `a call with mutable access`,
or the mutating operation that exposed the place. The diagnostic carries two
related labels: `narrowed by this test` at the test and `invalidated here` at
the invalidation.

**`AU2015` callable contract mismatch.** A callable's complete contract is its
exposed names, keyword-only boundary, default availability, call kind, and
obligations. `AU2015` reports a value whose complete contract differs from the
one required, in these cases:

- at a bare written destination, a rebinding, a branch join, a container
  literal, or a generic observation that would need an invented common contract
- a lambda that does not meet its expected contract
- an invalid thin-alias adaptation
- implicit erased storage into `Callable[...]`
- a packed call kind that would strengthen

The message names the difference. A safe restriction requires an explicit
`Unary(function)` adapter or a `Callable[...]`/`TaskCallable[...]` constructor.
A bare written contract never performs that restriction.

**`AU2999` general compile-time rejection.** This is the maintained catch-all
for compile-time rejections that do not yet have a narrower public category. It
is a stable code. It does not permit a tool to omit the code. `AU2999` also
reports a union type argument whose members' contracts are not coherent.

### FFI Boundary Codes

Foreign function interface (FFI) failures use these codes:

| Code | FFI failure |
| --- | --- |
| `AU1101` | Malformed extern bodies, defaults, type parameters, callbacks, variadics, and raw-pointer syntax, each with dedicated parser guidance. |
| `AU2002` | A type outside the FFI v0 scalar/view/opaque table. |
| `AU2005` | A reserved raw-pointer or callback contract, or opaque construction. |
| `AU2999` | Package authorization, root dependency reports, and the direct-call-only policy. |
| `AU3004` | An invalid FFI capability. |
| `AU3001`, `AU3008` | Opaque handle moves and task/Queue boundaries keep their usual codes. |
| `AU4001` | A non-canonical C boolean result, meaning a returned byte other than `0` or `1`. |
| `AU4005` | A missing process-global symbol, a null opaque-handle result, or a runtime marshalling failure. |

Native aborts, signals, memory faults, and foreign unwinds may terminate the
process. They are not Aura diagnostics. See [FFI v0](/manual/ffi).

### Ownership Codes

| Code | Meaning | Common causes | How to fix |
| --- | --- | --- | --- |
| `AU3001` | moved value | A use of a value after it moved. | Clone at a deliberate ownership boundary, or change a parameter to `own`. |
| `AU3002` | borrow/loan conflict | An access that overlaps a live borrow or view. | Remove the later use, shorten the scope, select a proven-disjoint place, or create an owned clone. |
| `AU3003` | mutability violation | Mutation through a shared place or view. | Add `mut`, or declare a mutating receiver as `mut self`. |
| `AU3004` | ownership or place mode | A source or target in the wrong ownership or place mode. | Use the mode the operation requires, on an addressable place. |
| `AU3005` | non-copy indexed read | Moving a non-copy value out of a `list`, `dict`, or constant tuple index, or a shared read of an element that cannot be cloned. | Use a `view`, `.clone()`, `pop`, `remove`, or tuple unpacking. |
| `AU3006` | non-copy indexed compound assignment | A compound assignment such as `+=` on a non-copy `list` or `dict` element. | Rewrite the update so it needs no hidden clone or move of the stored value. |
| `AU3007` | non-cloneable state duplication | Copying a value that holds `random.Rng`, an opaque FFI handle, or a capturing closure environment. | Move or remove the existing value, or construct a new generator from an explicit seed. |
| `AU3008` | non-transferable task/Queue boundary | A task capture, task result, or Queue payload that cannot cross to another task worker. | Pass owned transferable data, and keep host authority on its owning task. |
| `AU3009` | single-consumer task-result duplication | Copying the right to observe a task's result. | Keep one owner of the task handle and observe its result once. |
| `AU3010` | view escape or returned provenance | A view that outlives or does not come from its declared origin. | Return an owned clone, index, or handle, or keep the loan inside the owner's region. |

The guidance these diagnostics carry is described under
[Ownership Diagnostics](#ownership-diagnostics).

A **view** is a borrowed access to a place, written `view` or `view mut`. A
**Transfer** type is one whose values can cross a task-worker boundary.
Transfer is compiler-derived and has no builtin source-level trait surface.

**`AU3002`, `AU3003`, and `AU3004` with views.** For an explicit view,
`AU3002` labels both the view creation and the final use that keeps its
inferred region live. Removing a later use can shorten the loan. Otherwise,
shorten the lexical scope, select a proven-disjoint place, or create an owned
clone. `AU3003` covers mutation through a shared view and calling a
mutable-repeatable closure through an immutable place. `AU3004` covers
non-place sources, immutable mutable-view targets, and unsupported projections
such as collection indexes.

**Bound methods.** `AU3002` rejects binding a non-Copy shared or mutable
parameter. `AU3004` rejects binding a view of a non-Copy value. A Copy receiver
reached either way is snapshotted and reports nothing.

**`AU3005` and `AU3006` indexed access.** `AU3005` rejects a direct `list` or
`dict` indexed read that selects a non-copy element or value. It also rejects
constant tuple indexing that selects a non-copy element. A `view` of the
element or entry is a place read and is accepted. So is a field access through
it, such as `users[i].visits`.

For collections, the `AU3005` guidance follows the same clone-safety
classification as the rejection:

- A clone-safe type is directed to the explicit cloned `get` surface.
- A type carrying non-cloneable `random.Rng` state is directed to `remove`
  alone, because `AU3007` would reject `get` on it.
- An unresolved generic type is told that `get` requires a clone-safe type,
  and `remove` is offered unconditionally.

For tuples, unpack the whole tuple to move its non-copy elements.

`AU3006` rejects the matching `list` or `dict` indexed compound assignment. The
read-modify-write would otherwise need a hidden clone or a destructive move of
the stored value.

**`AU3007` non-cloneable state duplication.** Protected values include
`random.Rng`, opaque FFI handles, and capturing closure environments. The check
follows them through collections, user classes, enum payloads, and other value
wrappers.

A generic definition over unresolved types records an inferred clone-safety
obligation. `AU3007` is emitted in three cases:

- at an unsafe concrete specialization
- when a concrete requirement cannot be proved
- when an implementation would strengthen its trait method's contract

`list.filter` clones accepted source elements into a fresh result. It
establishes the same obligation, so it rejects `list[random.Rng]` or a
transitive wrapper.

`random.Rng` is not Transfer. A task that returns it and a Queue that carries
it are rejected with `AU3008`, and the task handle is not copyable. Moving or
removing a generator within one owning task stays valid, because it leaves one
owner.

**`AU3008` non-transferable task/Queue boundary.** This code reports a captured
argument, task result, or Queue payload that cannot cross a task-worker
boundary. The diagnostic names the failed boundary. It follows the specialized
type to the first non-transferable leaf, including its field, tuple element,
collection component, or enum payload path. For example, it explains that a
`Job` cannot cross because `Job.source` contains `fs.File`. It does not stop at
"`Job` is not Transfer."

The `AU3008` guidance follows these rules:

- It recommends passing owned transferable input and output data instead of a
  non-copy shared or mutable capability.
- It recommends keeping live host authority or `random.Rng` on its owning task.
- It may explain that reading Copy data materializes an owned snapshot. It must
  not claim that all borrowed Copy captures fail.
- It never proposes an `impl Transfer`. An ordinary user trait also named
  `Transfer`, and its implementations, do not alter the structural property.

A task callable contract that returns a view also reports `AU3008`.

**`AU3009` single-consumer task-result duplication.** This code rejects an
operation that would duplicate an existing single-consumer task-result
observation right. It covers explicit clone, clone-producing `get`, and
implicit collection or aggregate copy. It is not a Transfer-boundary failure.
The contained task handle is Transfer, but it is non-copy because its result is
non-repeatable.

Two related cases use other codes:

- A later use of the same binding after a consuming result observation is
  `AU3001`.
- Trying to consume the right through shared access is `AU3002`.

**`AU3010` view escape or returned provenance.** This code rejects:

- a view that escapes into ordinary storage
- a returned view whose expression does not derive from its declared receiver
  or parameter origin
- an invalid returned kind
- a call whose origin is not an addressable place
- a function type whose `view` result names an origin that is not one of its
  named parameters
- a function type with a `view mut` result over a non-`mut` origin

The diagnostic identifies the declared origin and the incompatible expression
or destination. Return an owned clone, index, or handle when the access must
escape. Otherwise, keep a local or closure loan synchronous and inside the
owner's region.

**`select(...)`.** These codes apply to a `select` call:

| Code | Cause |
| --- | --- |
| `AU3009` | The same statically visible non-repeatable Task source appears twice in one call. |
| `AU3002` | A non-repeatable Task does not arrive through owned access. `select` consumes all such observation rights at entry and abandons the losers. |
| `AU2004` | A call-shape error, such as an empty call or a named source. |
| `AU2002` | An invalid source kind, or an inconsistent Queue/Task category type. |

### Runtime Codes

| Code | Meaning | Common causes | How to fix |
| --- | --- | --- | --- |
| `AU4001` | general runtime trap | A failed `assert`, a NaN or infinite float in `json.dumps`, an invalid timer `Duration`, or an invalid runtime state. | Fix the condition or input the message names. |
| `AU4002` | arithmetic overflow or underflow | Checked arithmetic that leaves its type's range. | Keep values within the range of their type. |
| `AU4003` | bounds or lookup violation | An index, slice endpoint, or interval outside its valid range. | Check the value against its range before the operation. |
| `AU4004` | zero divisor | Division by zero. | Check the divisor before dividing. |
| `AU4005` | resource, allocation, or I/O failure | An exceeded size limit, allocation failure, or stack exhaustion. | Stay within the documented limit, or request more stack. |
| `AU4006` | invalid runtime configuration | An invalid `AURA_WORKERS`, `AURA_BLOCKING_WORKERS`, or `AURA_BLOCKING_QUEUE_CAPACITY` value. | Set the variable to a positive decimal integer, or unset it. |
| `AU4007` | numeric Array shape or reduction violation | Mismatched Array shapes, or `min`, `max`, or `mean` of an empty Array. | Match the shapes, or check that the Array is not empty. |

Each runtime code's precise causes are listed under
[Runtime Traps And Backtraces](#runtime-traps-and-backtraces).

**`AU4007` numeric Array structural failures.** `AU4007` reports:

- rank-zero or negative-dimension construction
- a `from_list` count mismatch
- an exact-shape operator mismatch
- a direct coordinate-count or runtime-rank mismatch
- empty `min`, `max`, or `mean`

Shape-product or element-count overflow and allocation failure stay `AU4005`.
Out-of-range coordinates and invalid first-axis slice bounds stay `AU4003`.
Absence from the optional `get` is ordinary `None`. Method `set` traps on an
invalid coordinate or rank.

**Repeated task-result claims.** Runtime containment for non-repeatable
results is separate from the static errors above. If a backend defect or a
foreign handle reaches a second runtime claim, Aura traps with `AU4001` and
this message:

``task result has already been observed; non-repeatable task results allow exactly one observing attempt``

It never returns or clones the stored value. The same defense applies when
malformed backend state reaches `select`. An already-claimed or duplicated
non-repeatable Task traps with `AU4001` before any result is delivered.

## Diagnostic Structure

Every diagnostic contains all of these fields:

| Field | Meaning |
| --- | --- |
| `code` | stable `AU####` identifier |
| `severity` | `error`, `warning`, `information`, or `hint` |
| `message` | concise primary explanation |
| `primary_span` | optional path and source range for the failed operation |
| `secondary_spans` | related source ranges, each with a label |
| `notes` | contextual facts that do not prescribe a change |
| `help` | actionable human guidance |
| `edits` | source replacements with an applicability classification |
| `call_frames` | Aura call frames, ordered innermost first |
| `task_ancestry` | structured task parentage, ordered youngest first |

The current compiler emits only errors. The shared schema reserves the other
severity values.

A machine-applicable edit is safe for a tool to offer as an automatic source
replacement at the stated range. Tools MUST preserve edits and MUST NOT infer
an edit from prose alone.

Compiler and CLI spans use one-based line and column numbers. Each structured
span is a half-open range with `start` and `end`. Current token diagnostics may
use a one-column primary range. The LSP bridge converts these ranges to the
zero-based line and character coordinates that the Language Server Protocol
requires.

## Human-Readable Form

The default CLI form begins with the stable code:

```text
error[AU2001]: unknown name `missing`
 --> path/to/file.au:2:11
  |
2 |     print(missing)
  |           ^
```

The rest of the diagnostic follows in labeled records:

- related spans as `related`
- context as `note`
- proposed actions as `help`
- source replacements as `fix`

A source-backed diagnostic uses the path and source context where it was
detected. For a failure in an imported module, that is the imported module, not
its importer. If no valid source line is available, the renderer still emits
the code, the message, and the best available location.

The compiler normally reports one primary failure for an operation. It does not
invent speculative follow-on errors. A conforming implementation MUST reject
invalid source rather than silently reinterpret it.

## JSON Form

`aura check --format json` writes one JSON document. `aura run --format json`
and `aura build --format json` use the same document for compile failures. The
top-level `schema_version` is currently `1`, and `diagnostics` is an array.

For `check`, `run`, and `build`, the current compiler emits at most one
diagnostic per invocation, because the pipeline stops at the first failure. On
failure, the schema-version-1 `diagnostics` array contains exactly one entry.
On a successful `check`, it is empty. The array exists for schema compatibility
and future recovery. Tools must not infer that the source contains no other
errors.

```json
{
  "schema_version": 1,
  "diagnostics": [
    {
      "code": "AU2001",
      "severity": "error",
      "message": "unknown name `missing`",
      "primary_span": {
        "path": "path/to/file.au",
        "start": { "line": 2, "column": 11 },
        "end": { "line": 2, "column": 12 }
      },
      "secondary_spans": [],
      "notes": [],
      "help": [],
      "edits": [],
      "call_frames": [],
      "task_ancestry": []
    }
  ]
}
```

Field shapes:

- `primary_span` is `null` when no source location exists.
- A secondary span has the same `path`, `start`, and `end` fields, plus a
  string `label`.
- Each edit has `path`, `start`, `end`, `replacement`, and `applicability`.

Successful `aura check --format json` emits schema version 1 with an empty
diagnostics array. Successful `run` and `build` keep their ordinary
program-output and artifact contracts. For them, `--format` selects the
diagnostic representation, not the program's data format.

A direct run that performs long native work may also write one
schema-version-1 status document on standard error. Its `progress` array
contains the exact wait and rebuild notices. If `auto` falls back
successfully, the same document contains
`"fallback":{"from":"direct","to":"mir","reason":"..."}`. If the fallback then
fails, its progress and the direct failure are kept as notes in the one
diagnostic document.

Every diagnostic entry contains both frame arrays. They are present even for
compile-time and pre-user-code failures, where they are empty.

- A call-frame record contains `function` and a `span`.
- A task-ancestry record contains `task_function`, `task_entry_span`,
  `parent_function`, and `spawn_span`.
- Each frame span carries its own `path`, `start`, and `end`. A frame defined
  or spawned in an imported module is never labeled with the entry module's
  path.

The frame arrays are an additive schema-version-1 extension. Schema-version-1
readers MUST ignore unrecognized object members while still validating the
fields they use. The compiler-service semantic-interface version is `16`.

The process exits unsuccessfully after it emits a JSON error report. In JSON
mode, tools MUST parse standard error as one JSON document and MUST NOT scrape
the human renderer.

## LSP Contract

The compiler service owns editor diagnostics. Its analysis record carries the
same code, severity, message, secondary spans, notes, help, edits, call frames,
and task ancestry. In this editor shape, frame spans use zero-based
`file_path`, `line`, `start_character`, and `end_character` coordinates.

The JavaScript language-server bridge maps each part to the LSP:

- the primary span to the LSP range
- secondary spans to `relatedInformation`
- the code to `Diagnostic.code`
- the remaining metadata to `Diagnostic.data`

The language server has no semantic-diagnostic implementation of its own. If
the compiler service is unavailable, lexical recovery may keep basic editor
navigation usable. It MUST NOT invent semantic success or fabricate compiler
diagnostics.

## Ownership Diagnostics

Ownership diagnostics use the `AU30xx` band. When the checker has both sites,
the primary span identifies the invalid later operation. A labeled secondary
span identifies the earlier move or borrow that made it invalid.

Where it applies, the guidance names the smallest explicit repair:

- change a parameter to `own`
- clone at a deliberate ownership boundary
- use the appropriate borrow loop form
- add `mut`
- declare a mutating receiver as `mut self`

When a repair is a local, unambiguous source replacement, the diagnostic also
carries a machine-applicable edit.

Guidance does not relax the ownership rules. Aura never inserts a hidden clone
or converts a borrow into ownership to recover from an error.

For example, consuming a bare shared parameter reports that parameter `x` is
borrowed. It recommends declaring it as `own str` to take ownership, or cloning
the value before consuming it. The parameter name and concrete type in that
message come from the rejected declaration.

**`AU3007` guidance.** It offers the two explicit single-owner exits: move or
remove the existing value, or construct an independent generator from an
explicit seed. It does not offer `.clone()` on any type whose value contains or
may contain `random.Rng`. Clone-producing aliases, including collection reads
and task-result observations, follow the same rule as a direct clone.

**Retained borrows.** A binary left operand, index base, method receiver, or
indexed-assignment target may keep a non-copy borrow alive through later
inputs. An overlapping later mutable borrow or consumption is then `AU3002`.
The conflicting later access is the primary span. The retained selection is a
labeled borrow-origin secondary span. Guidance may suggest an explicit clone
when the type supports it, or a separate earlier mutation. The compiler does
not deep-clone implicitly.

## Python-Shaped Source Guidance

`AU2005` gives focused guidance where Python-looking source has an Aura
spelling. Maintained hints cover:

- `True`/`False`
- `.append(...)`
- `is` and `is None`
- `try`/`except`
- `str(...)` constructor-shaped source, which is directed to Aura string
  literals

Related diagnostics cover missing `mut`, consuming calls, integer `/`, typed
`self: Type`, tab indentation, and single-quoted f-strings.

Hints MUST name an available spelling when one exists. For an unavailable
form, they MUST name a working expression or statement form. The complete hint
family is pinned under `crates/aura-compiler/tests/fixtures/python-hints/`.

These forms are accepted, and their fixtures assert the spellings: `in`,
`not in`, chained comparisons, `len(...)`, `str(...)`, and contextually typed
expression lambdas.

### Comprehensions

Eager list, set, and dictionary comprehensions are accepted. A generator
expression, parenthesized or used as a call argument, receives this exact
`AU2005`:

    generator expressions are unavailable; use an eager owned list comprehension or an explicit loop

`mut` or `own` in a comprehension clause is malformed syntax. It receives
`AU1101` with this exact message:

    comprehensions use bare iteration; remove `mut` or `own` and write `for name in values`

The bare form keeps the iterable's ordinary contract, including owned receive
items for Queue.

### Slices

Owned list and str slicing is implemented. Step syntax and slice assignment
are reserved. They report `AU2005` with these exact messages:

    slice steps are unavailable; use an explicit loop to select a stride

    slice assignment is unavailable because slices are owned copies; mutate the source by index or build a new value

Other slice rules:

- Written slice endpoints use the `int64` index domain. A mismatched bound
  reports `AU2002`. Fixed-width narrower integers widen losslessly at that
  position.
- A list slice that would duplicate `random.Rng`, an opaque FFI handle, or a
  capturing closure environment reports `AU3007`.
- A list slice that would duplicate a non-repeatable Task result right reports
  `AU3009`.
- An endpoint outside `0..=len` after one negative normalization, or a start
  greater than its end, traps with `AU4003`.

Unlike Python, Aura never clamps a slice endpoint.

### String And Format Diagnostics

String literal and format diagnostics point at the smallest location that
proves the error.

| Code | Cause |
| --- | --- |
| `AU1001` | A malformed or unterminated ordinary, triple-quoted, or raw string, including a later physical line that contains an invalid escape. |
| `AU1002` | A raw or triple-quoted f-string prefix. The guidance names the supported form. |
| `AU1101` | Malformed static format grammar, nested fields, unsupported codes, or a width or precision above `1_000_000`. |
| `AU2002` | A valid specification that does not fit the interpolation's static type. |
| `AU4005` | Constructed string output above the 64 MiB limit. The trap happens before the oversized append changes the partial result. |

Python allows decimal grouping without an explicit type code, as in
`f"{n:,}"`. Aura requires the numeric code: use `f"{n:,d}"`, `f"{n:,f}"`, or
`f"{n:,%}"`. This ties grouping validation to a statically selected numeric
rendering contract.

## Runtime Traps And Backtraces

Runtime diagnostics use `AU40xx` and keep the source span embedded during
lowering. `aura run` keeps any output produced before a trap. It leaves program
standard output intact, renders the diagnostic on standard error, and exits
unsuccessfully.

### Assertions

A failed assertion is `AU4001` at the `assert` keyword location. The message
is exactly `assertion failed` when the assertion has no message. Otherwise it
is exactly the evaluated str, including an empty or whitespace-only value. A
failure while evaluating the condition or the message stays primary. Active
cleanup still runs, but a cleanup failure cannot replace an assertion
diagnostic that is already established.

### Call Frames And Task Ancestry

The MIR runtime and the direct runtime attach the same typed Aura frames to
every trap. MIR is Aura's mid-level intermediate representation.

- Call frames name the Aura function and its defining source span, ordered
  innermost first.
- If the trap occurs in a task, task-ancestry records identify that task's
  entry, its parent function, and the exact source location that started each
  task. They are ordered youngest first.

These are Aura frames, not host Rust, Cranelift, scheduler, or service-worker
frames.

Frame records are captured once, when the primary trap is established, before
cleanup or task-state reset. Propagation through callers, Task results, task
groups, or workers does not append observer frames. A child task starts a new
call chain. Task ancestry records its relationship to the parent.

Human rendering builds the `Aura call chain`, `Aura task entry`, and
`Aura task ancestry` note lines from the typed records, after the ordinary
notes. These generated strings are not stored in the structured `notes`. JSON
and LSP clients read the frame arrays without parsing or deduplicating prose.

### Direct Runs In JSON Mode

A JSON-mode direct run carries a native trap to the `aura` parent through a
private fixed-marker pipe and a separate bounded JSON-data pipe. Native
initialization hides both descriptors and marks them close-on-exec before user
code runs. The parent emits one schema-version-1 document. Any buffered
native-build progress appears in its ordinary `notes`.

An Aura trap is distinct from a successful `main() -> int32` that returns a
nonzero status. A signalled trap with a missing or malformed record is a host
failure. `auto` never falls back to MIR after launch. Human-mode direct runs
and standalone direct binaries render the complete diagnostic themselves.

### Which Failures Trap

Checked overflow, zero division, bounds failure, recursion-depth failure, and
an explicitly trapping invalid runtime state are diagnostics. File, process,
network, timeout, cancellation, and protocol operations normally return typed
values instead. The feature page for an API states any exception that traps.

**Duration inputs.** A negative, host-unrepresentable, or deadline-overflowing
`Duration` returns the documented `InvalidInput` or process error when the API
has a compatible typed carrier. A timer API without one traps with `AU4001`. A
deadline overflow never means an unlimited wait.

**`control.retry`.** It doubles a `Duration` backoff only when a later attempt
can use the doubled value. If that doubling exceeds the exact signed `Duration`
range, it traps with `AU4002`. It does not wrap, clamp, return the most recent
`Err`, or compute an unused delay after the final attempt. Worker traps and
current-task cancellation also propagate instead of becoming the worker's `E`.
These inputs are validated before the worker runs:

- `max_attempts < 1` traps with `AU4003`.
- A negative or host-unrepresentable `initial_backoff` traps with `AU4001`.

**Random numbers.** The random module returns plain values, not a
`random.Error` enum.

- `AU4003` reports an empty or reversed `next_int`/`secure_int` interval and a
  negative `secure_bytes` count.
- `AU4005` reports a `secure_bytes` count above the fixed per-request ceiling
  of `2147483647`, before any allocation or entropy request. It also reports a
  secure operating-system entropy failure and an allocation failure.

A secure operation never recovers by substituting bytes from the deterministic
generator.

**Task stacks.** An explicit task-stack request has exact type `int64` and an
inclusive 262,144..67,108,864-byte range.

- `AU2002` rejects an out-of-range literal during checking.
- A dynamic value outside that range traps with `AU4005`.
- A stack-allocation or platform-size failure traps with `AU4005`.

Aura never clamps the request or silently substitutes the default.

On the MIR backend, an Aura call traps with `AU4005` when it would leave less
than the interpreter's headroom reserve on the running task's writable
coroutine stack. The message reads "task stack exhausted while calling ...". It
names the callee and the writable bytes that remain. The guard page below the
requested capacity does not count toward that headroom. The reserve is 128 KiB
in an optimized interpreter build and 224 KiB in a debug-assertion build. The
direct backend performs no headroom probe and has only its 256-call depth
limit.

**Runtime configuration.** `AU4006` reports invalid process runtime
configuration. `AURA_WORKERS`, `AURA_BLOCKING_WORKERS`, and
`AURA_BLOCKING_QUEUE_CAPACITY` each require a positive decimal integer. Aura
rejects empty, zero, signed, whitespace-padded, non-decimal, non-Unicode, and
overflowing values before user code runs. The diagnostic names the setting and
renders the supplied value, using a lossy display for a non-Unicode value.
Failure to create the configured blocking-I/O worker set also reports
`AU4006`. The runtime does not fall back to fewer workers or to synchronous
execution.

**JSON.** JSON input-data failures are typed `json.Error` values, not
diagnostics. Parse allocation failure, or exceeding the shared 262,144-value
materialization limit, reports `AU4005` instead. `json.dumps` reports:

- `AU4003` for an indent outside `0..=16` or a value deeper than 128
  containers
- `AU4001` for a NaN or infinite `json.Value.Float`
- `AU4005` when conversion exceeds the same node limit, when encoded output
  would exceed 67,108,864 bytes, or when a controlled conversion or output
  allocation fails

No failed dump returns a partial str.

**Bytes.** Malformed UTF-8, hexadecimal, and base64 input returns
`bytes.Error`. The error includes the relevant zero-based byte offset or odd
input length when that value fits the retained `int32` payload. A required
offset or length above `2147483647` reports `AU4005` rather than truncating or
wrapping the typed error. These also report `AU4005`:

- a fresh bytes conversion or codec destination above the fixed
  2,147,483,647-byte safety ceiling
- destination-size arithmetic overflow
- allocation failure

The ceiling is independent of the public str and `list` length domains. No
failed operation returns a partial str or byte list.

Unrecoverable host or dependency-internal out-of-memory termination is outside
the catchable diagnostic contract.

## CLI Exit Status

| Status | Meaning |
| --- | --- |
| `0` | command succeeded, help/version was requested, or a `None`-returning program completed |
| `1` | compile, package, build, test, or runtime operation failed |
| `2` | command usage or option parsing was invalid |

For `aura run`, an `int32` result from the entry module's `main` becomes the
requested process exit status. A `None` result completes successfully. The host
operating system may restrict how exit values are represented once they leave
Aura.

`aura test` succeeds only when every selected `.au` program checks and runs
within its timeout and every integer `main` result is zero.

## Internal Errors

An `internal error` message means an implementation invariant failed, or a
defensive check caught malformed internal input. Valid, statically checked Aura
source must not produce one. Panics, host crashes, memory-safety failures, and
hangs are never conforming diagnostic behavior. Treat them as compiler or
runtime bugs.

Design records:
[ADR-0019: Duration conversion and timer policy](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0019-duration-conversion-and-timer-policy.md)
and
[ADR-0033: Structural Transfer and task-result consumption](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0033-structural-transfer-and-task-results.md).
