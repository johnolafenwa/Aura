# Diagnostics

Aurora diagnostics are part of the language and tooling contract. Lexing,
parsing, static checking, ownership checking, lowering, building, and runtime
traps all use the compiler-owned diagnostic structure described here. A typed
library failure such as `Result.Err`, `Option.None`, a timeout, cancellation, or
an `io.Error` value is ordinary program data, not a diagnostic.

## Stable Diagnostic Codes

Every diagnostic has a stable code of the form `AU####`. The first digits name
the phase that owns the failure:

| Band | Phase | Current codes |
| --- | --- | --- |
| `AU10xx` | lexical analysis | `AU1001` invalid lexical input; `AU1002` invalid f-string delimiter |
| `AU11xx` | parsing | `AU1101` invalid syntax |
| `AU20xx` | names and types | `AU2001` name resolution; `AU2002` type mismatch; `AU2003` unsupported operator; `AU2004` argument binding; `AU2005` migration guidance; `AU2006` builtin method collision; `AU2007` builtin function redefinition; `AU2999` general compile-time rejection |
| `AU30xx` | ownership and borrows | `AU3001` moved value; `AU3002` borrow violation; `AU3003` mutability violation; `AU3004` ownership mode; `AU3005` non-copy indexed read; `AU3006` non-copy indexed compound assignment; `AU3007` non-cloneable state duplication |
| `AU40xx` | runtime-checked traps | `AU4001` general runtime trap; `AU4002` arithmetic overflow or underflow; `AU4003` bounds or lookup violation; `AU4004` zero divisor; `AU4005` resource or I/O failure |

`AU1001` also owns source-delimiter pairing. An unexpected closer is primary at
that closer. A mismatched closer names the expected kind and labels its opener
as related information. EOF with an unclosed delimiter reports the expected
closer and labels the opener. These locations and labels are preserved by
analysis JSON and the LSP bridge.

The registry is append-only. Once published, a code MUST NOT be reused,
renumbered, or silently reassigned to a different diagnostic category. If a
diagnostic becomes obsolete, its number remains reserved. New categories
receive new numbers. Message wording and attached guidance may become more
specific without changing a code when the failure category is unchanged.

`AU2999` is the maintained catch-all for compile-time rejections that do not
yet have a narrower public category. It is a stable code, not permission for a
tool to omit the code.

`AU2006` identifies an explicit or inherited trait method whose name would
shadow a builtin member of the implementation's builtin target. The rule covers
every builtin target, from the runtime handles `Queue[T]`, `Task[T]`,
`TaskGroup`, `random.Rng`, `fs.File`, and the `net` and `process` handles, to
the builtin value types such as `String`, `Vec[T]`, `Map[K, V]`, `Set[T]`,
`Duration`, and the scalar types. Its guidance requires the trait method to be
renamed; backend dispatch is never selected by which implementation happens to
run first.

`AU2007` rejects a module-level function declaration whose name is already a
builtin function name, such as `len`, `str`, `abs`, or `print`. The builtin
surface is closed, so the declaration must be renamed. This rejection is
distinct from the `AU2006` method collision: it covers free functions rather
than trait methods on a builtin target.

`AU3005` rejects a direct `Vec` or `Map` indexed read that selects a non-copy
element or value, and constant tuple indexing that selects a non-copy element.
For collections its guidance is clone-safety aware, classified exactly as the
rejection is: a clone-safe type is directed to the explicit cloned `get`
surface; a type carrying non-cloneable `random.Rng` state is directed to
`remove` alone, because `get` on it would be rejected in turn by `AU3007`; and
an unresolved generic type is told that `get` requires a clone-safe type, with
`remove` offered unconditionally. For tuples, unpack the whole tuple to move
its non-copy elements. `AU3006` rejects the
corresponding `Vec` or `Map` indexed compound assignment because
read-modify-write would otherwise require a hidden clone or destructive move
of the stored value.

`AU3007` rejects an operation that would duplicate non-cloneable state. The
current protected state is `random.Rng`, and the check follows it through
collections, user classes, enum payloads, and other value wrappers. A generic
definition over unresolved types records an inferred clone-safety obligation;
`AU3007` is emitted at an unsafe concrete specialization, when a concrete
requirement cannot be proved, or when an implementation would strengthen its
trait method's contract. Copying `Task[random.Rng]` or
`Queue[random.Rng]` handles is valid because it copies only the handle;
transferring the generator with a move, collection removal, or queue receive is
also valid because it leaves one owner.

## Diagnostic Structure

A diagnostic contains all of the following fields:

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

The current compiler emits errors; the additional severity values are reserved
by the shared schema. A machine-applicable edit is safe for a tool to offer as
an automatic source replacement at the stated range. Tools MUST preserve edits
and MUST NOT infer an edit from prose alone.

Compiler and CLI spans use one-based line and column numbers. Each structured
span is a half-open range with `start` and `end`; current token diagnostics may
use a one-column primary range. The LSP bridge converts those ranges to the
zero-based line and character coordinates required by the Language Server
Protocol.

## Human-Readable Form

The default CLI form begins with the stable code:

```text
error[AU2001]: unknown name `missing`
 --> path/to/file.au:2:11
  |
2 |     print(missing)
  |           ^
```

Related spans follow as `related` records. Context appears as `note`, proposed
actions as `help`, and source replacements as `fix`. A source-backed operation
uses the path and source context where the diagnostic was detected, including
an imported module rather than its importer. If no valid source line is
available, the renderer still emits the code, message, and best available
location.

The compiler normally reports one primary failure for an operation instead of
inventing speculative follow-on errors. A conforming implementation MUST reject
invalid source rather than silently reinterpret it.

## JSON Form

`aura check --format json` writes one JSON document. `aura run --format json`
and `aura build --format json` use the same document for compile failures. The
top-level `schema_version` is currently `1`, and `diagnostics` is an array.

For `check`, `run`, and `build`, the current compiler emits at most one
diagnostic per invocation: the pipeline stops at the first failure. On failure
the schema-version-1 `diagnostics` array therefore contains exactly one entry;
on successful `check` it is empty. The array is retained for schema
compatibility and future recovery, and tools must not infer that the source
contains no additional errors.

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
      "edits": []
    }
  ]
}
```

`primary_span` is `null` when no source location exists. A secondary span has
the same `path`, `start`, and `end` fields plus a string `label`. Each edit has
`path`, `start`, `end`, `replacement`, and `applicability`. Successful
`aura check --format json` emits schema version 1 with an empty diagnostics
array. Successful `run` and `build` retain their ordinary program-output and
artifact contracts; `--format` selects their diagnostic representation, not
the program's data format. A direct run that performs long native work may also
write one schema-version-1 status document on standard error. Its `progress`
array contains the exact wait/rebuild notices. If `auto` falls back
successfully, the same document contains
`"fallback":{"from":"direct","to":"mir","reason":"..."}`. If the fallback
then fails, its progress and direct failure are retained as notes in the one
diagnostic document.

The process exits unsuccessfully after emitting a JSON error report. Tools MUST
parse standard error as one JSON document in JSON mode and MUST NOT scrape the
human renderer.

## LSP Contract

The compiler service owns editor diagnostics. Its analysis record carries the
same code, severity, message, secondary spans, notes, help, and edits. The
JavaScript language-server bridge maps the primary span to the LSP range, maps
secondary spans to `relatedInformation`, places the code in `Diagnostic.code`,
and preserves notes, help, and edits in `Diagnostic.data`.

There is no independent semantic-diagnostic implementation in the language
server. If the compiler service is unavailable, lexical recovery may keep basic
editor navigation usable, but it MUST NOT invent semantic success or fabricate
compiler diagnostics.

## Ownership Diagnostics

Ownership diagnostics use the `AU30xx` band. When the checker has both sites,
the primary span identifies the invalid later operation and a labeled secondary
span identifies the earlier move or borrow that made it invalid. Applicable
guidance names the smallest explicit repair: change a parameter to `own`, clone
at a deliberate ownership boundary, use the appropriate borrow loop form, add
`mut`, or declare a mutating receiver as `mut self`. When a repair is a
local, unambiguous source replacement, the diagnostic also carries a
machine-applicable edit.

Guidance is not a relaxation of ownership rules. In particular, Aurora never
inserts a hidden clone or converts a borrow into ownership to recover from an
error.

For `AU3007`, guidance offers the two explicit single-owner exits: move or
remove the existing value, or construct an independent generator from an
explicit seed. It does not offer `.clone()` on any type whose value contains or
may contain `random.Rng`. Clone-producing aliases—including collection reads
and task-result observations—are subject to the same rule as a direct clone.

When a binary left operand, index base, method receiver, or indexed-assignment
target retains a non-copy borrow through later inputs, an overlapping later
mutable borrow or consumption is `AU3002`. The conflicting later access is the
primary span and the retained selection is a labeled borrow-origin secondary
span. Guidance may suggest an explicit clone when the type supports it or a
separate earlier mutation, but the compiler does not deep-clone implicitly.

For example, consuming a bare shared parameter reports that parameter `x` is
borrowed and recommends declaring it as `own String` to take ownership or
cloning the value before consuming it. The parameter name and concrete type in
that message come from the rejected declaration.

## Python-Migration Guidance

`AU2005` identifies focused migration guidance where Python-looking source has
an Aurora spelling or an explicitly later language surface. Maintained hints
cover `True`/`False`, `.append(...)`, `is` and `is None`, `try`/`except`,
lambdas, and comprehensions. Related diagnostics
cover missing `mut`, consuming calls, integer `/`, typed `self: Type`, tab
indentation, and single-quoted f-strings.

A hint is retired when Aurora implements the form it pointed at. `in`,
`not in`, chained comparisons, `len(...)`, and `str(...)` are implemented and no
longer produce a hint; their fixtures remain in the family and now assert that
the Python spelling is accepted.

Hints MUST name an available spelling when one exists. For a reserved future
feature, they MUST say that it arrives in a later Aurora release and name a
working expression or statement form for today. The complete hint family is
pinned under `crates/aurora-compiler/tests/fixtures/python-hints/`.
`AU2005` also identifies `String(...)` constructor-shaped source and directs
the caller to Aurora string literals.

## Runtime Traps And Backtraces

Runtime diagnostics use `AU40xx` and preserve the source span embedded during
lowering. Output produced before a trap is not discarded: `aura run` leaves
program standard output intact, renders the diagnostic on standard error, and
exits unsuccessfully.

A failed assertion is `AU4001` at the `assert` keyword location. The
message is exactly `assertion failed` when omitted and otherwise exactly the
evaluated String, including an empty or whitespace-only value. A failure while
evaluating the condition or message remains primary. Active cleanup still
runs, but a cleanup failure cannot replace an already established assertion
diagnostic.

The MIR runtime attaches the Aurora call chain to every trap. Frames name the
Aurora function and its source span, ordered innermost first. If the trap occurs
in a task, notes also identify that task's entry and its ancestry, including the
source location from which each task was started. These are Aurora frames, not
host Rust frames.

Structured output currently transports the Aurora call chain, task entry, and
task ancestry as flat prose strings in `notes`. Dedicated structured frame
lists are deferred to the later native-frames stage of the Batch 4 runtime work; tools MUST treat these
notes as prose rather than parse them.

Native-backend Aurora backtraces are deferred to that Batch 4 native-frames stage. Until
then, native execution preserves the same primary trap code, message, and source
location but may omit the supplemental Aurora frame and task-ancestry notes;
this temporary difference is recorded in Current Limits.

Checked overflow, zero division, bounds failure, recursion-depth failure, and
an explicitly trapping invalid runtime state are diagnostics. File, process,
network, timeout, cancellation, and protocol operations normally return typed
values instead; the feature page for an API states any trapping exception. A
negative, host-unrepresentable, or deadline-overflowing Duration returns the
documented `InvalidInput`/process error when that API has a compatible typed
carrier. A timer API without one traps with `AU4001`; deadline overflow never
means an unlimited wait. This classification is accepted under ADR-0019.

The random module returns plain values rather than a `random.Error` enum.
`AU4003` reports an empty or reversed `next_int`/`secure_int` interval and a
negative `secure_bytes` count. `AU4005` reports a `secure_bytes` count above the
fixed per-request ceiling of `2147483647` before allocation or entropy is
requested, secure operating-system entropy failure, or allocation failure. A
secure operation never recovers by substituting bytes from the deterministic
generator.

An explicit task-stack request has exact type `int64` and an inclusive
262,144..67,108,864-byte range. `AU2002` rejects an out-of-range literal during
checking. A dynamic value outside that range and a stack-allocation or
platform-size failure trap with `AU4005`; Aurora never clamps the request or
silently substitutes the default.

JSON input-data failures are typed `json.Error` values rather than diagnostics.
Parse allocation failure or exceeding the shared 262,144-value
materialization limit uses `AU4005` instead. `json.dumps` uses `AU4003` for an
indent outside `0..=16` or a value deeper than 128 containers, `AU4001` for a
NaN or infinite `json.Value.Float`, and `AU4005` when conversion exceeds the
same node limit, encoded output would exceed 67,108,864 bytes, or a controlled
conversion/output allocation fails. No failed dump returns a partial String.

Malformed UTF-8, hexadecimal, and base64 input returns `bytes.Error`, including
the relevant zero-based byte offset or odd input length, when that metadata fits
the retained `int32` payload. A required offset or length above `2147483647`
uses `AU4005` rather than truncating or wrapping the typed error. A fresh bytes
conversion or codec destination above the fixed 2,147,483,647-byte safety
ceiling, destination-size arithmetic overflow, or allocation failure also uses
`AU4005`; the ceiling is independent of the public String and `Vec` length
domains, and no failed operation returns a partial String or byte vector.

Unrecoverable host or dependency-internal out-of-memory termination remains
outside the catchable diagnostic contract.

## CLI Exit Status

| Status | Meaning |
| --- | --- |
| `0` | command succeeded, help/version was requested, or a `None`-returning program completed |
| `1` | compile, package, build, test, or runtime operation failed |
| `2` | command usage or option parsing was invalid |

For `aura run`, an `int32` result from the entry module's `main` becomes the
requested process exit status; a `None` result completes successfully. Host
operating systems may restrict how exit values are represented after the value
leaves Aurora. `aura test` succeeds only when every selected `.au` program
checks and runs within its timeout and every integer `main` result is zero.

## Internal Errors

An `internal error` message indicates an implementation invariant failure or a
defensive check for malformed internal input. Valid, statically checked Aurora
source must not produce one. Panics, host crashes, memory-safety failures, and
hangs are never conforming diagnostic behavior and must be treated as compiler
or runtime bugs.
