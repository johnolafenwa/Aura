# Diagnostics

Aurora reports lexical, grammatical, static, package, build, and runtime failures through structured diagnostics. Diagnostics are intended to identify one primary failure precisely; the compiler generally stops the current operation after that failure instead of emitting a cascade of speculative follow-on messages.

## Diagnostic Categories

| Category | Detected during | Typical cause |
| --- | --- | --- |
| source decoding or lexing | tokenization | tab characters, invalid escapes, malformed numbers, unsupported characters, inconsistent indentation |
| parsing | AST construction | unexpected token, missing delimiter, invalid declaration or block shape, parser complexity limit |
| module/package loading | import graph construction | missing module, escaped package root, invalid manifest or lockfile, dependency failure |
| static checking | semantic analysis | unknown or duplicate name, type mismatch, invalid call, non-exhaustive match, move/borrow violation |
| lowering/building | MIR or native construction | maintained-backend limitation or failure to produce/link a native artifact |
| runtime | execution | checked arithmetic failure, recursion limit, bounds failure, resource misuse, or an explicitly trapping operation |
| typed library failure | normal program value | `Result.Err`, `Option.None`, timeout/cancellation outcome, process error, or `io.Error` variant |

Typed library failures are not compiler diagnostics. Programs handle them with `match`, helper methods, or `try` when the error type is compatible.

## Source Locations

Human-readable compiler spans use one-based line and column numbers. A source-backed diagnostic has this form:

```text
error: MESSAGE
 --> path/to/file.au:LINE:COLUMN
  |
LINE | source text
  |   ^
```

The caret identifies the primary token or source position, not necessarily the entire expression. If the implementation cannot attach a valid source line, it still reports the message and best available path/location.

Diagnostics raised while loading an imported module use that module's path and source context rather than incorrectly pointing at the importing file.

Editor-facing analysis uses zero-based line and character positions, following LSP conventions. Compiler analysis diagnostics currently have severity `1` (error), a one-character primary range, and the same semantic message as the underlying compiler diagnostic.

## Compile-Time Rejection

A program that violates a lexical, grammatical, name, type, ownership, or entrypoint rule MUST be rejected before execution. An implementation must not recover by silently changing the meaning of the source.

Examples of required rejection include:

- use of an unknown or out-of-scope name
- calling a function with missing, duplicate, unknown, or wrongly typed arguments
- using a move value after it was consumed
- overlapping a mutable borrow with another access at the same call boundary
- reading a private imported field or method
- omitting variants from a match that requires exhaustiveness
- mixing executable top-level statements with a local `main`

The exact wording is stable where it is captured by a diagnostic fixture. Otherwise the semantic category, source attribution, and actionable meaning are the contract; diagnostic text is not assigned a permanent numeric code in Aurora 0.1.

Integer `/` and `/=` are the deliberate exception whose teaching text is part of the maintained language contract:

```text
integer `/` is not supported; use `//` for floor division, or call `.to_float()` on both operands for true division
```

The typed-method-parameter trap likewise has maintained teaching text because
the invalid spelling would otherwise look like an instance receiver while
declaring an associated method:

```text
`self: Type` is not a method receiver; use `self` or `borrow self` for shared access, `own self` to consume, or `borrow mut self` to mutate
```

Consuming a defaulted borrowed parameter has stable guidance that names the
declaration fix and the local-copy alternative:

```text
parameter `x` is borrowed; declare it as `own String` to take ownership, or clone the value before consuming it
```

A mutable-borrow default is rejected at its signature:

```text
`borrow mut` parameter `x` cannot have a default: the default creates a caller-invisible temporary, so mutations through it would be silently lost; require the caller to pass a value, or take the parameter as `own T` and return the result
```

An ownership modifier on Queue iteration is rejected with teaching text: Queue
iteration receives values, each received item is already owned by the loop
binding, and the Queue handle is a copy value, so the modifier has nothing to
modify. The diagnostic suggests the bare form `for item in queue:`.

## Runtime Diagnostics

Runtime diagnostics use the source path and span embedded by MIR or native lowering whenever possible. Both maintained execution backends MUST preserve the same primary Aurora failure when cleanup also encounters an error.

Output produced before a runtime failure is not retroactively discarded. `aura run` streams standard output as execution proceeds, then renders the diagnostic to standard error and exits unsuccessfully.

Runtime failures are reserved for operations whose contracts trap rather than return a typed outcome. Current examples include:

- checked integer overflow and integer floor division or remainder by zero
- floating-point true division, floor division, or remainder by zero under Aurora's current non-IEEE failure rule
- invalid direct indexing or out-of-bounds collection mutation where the method contract specifies a runtime error
- recursion beyond the maintained Aurora call-depth limit
- invalid runtime type or resource state that should have been impossible in a checked program

I/O, process, timeout, and protocol APIs normally return typed errors. Their reference chapters identify the exceptions.

## CLI Exit Status

The `aura` process uses these command-level conventions:

| Status | Meaning |
| --- | --- |
| `0` | command succeeded, help/version was requested, or a `None`-returning program completed |
| `1` | compile, package, build, test, or runtime operation failed |
| `2` | command usage or option parsing was invalid |

For `aura run`, an `int32` result from the entry module's `main` becomes the requested process exit status; a `None` result completes successfully. Host operating systems may restrict how process exit values are represented after the value leaves Aurora.

`aura test` succeeds only when every selected `.au` program checks and runs within its timeout and every integer `main` result is zero. It exits `1` after printing the pass/fail summary if any selected program fails.

## Machine-Readable Diagnostics

`aura analyze` emits JSON containing diagnostics, symbols, occurrences, hover text, and definition ranges. `aura lsp` exposes the same compiler-owned semantics through a persistent JSON-lines service. The JavaScript language server may provide lexical declaration recovery if the compiler service is unavailable, but that fallback MUST NOT invent semantic success or member/type information.

`aura ast-json` and `aura mir` are inspection formats for the current toolchain. They are not stable serialization formats for third-party compiler implementations.

## Internal Errors

Messages prefixed with `internal error:` indicate an implementation invariant failure or a defensive check for malformed internal input. A valid, statically checked Aurora program should not produce one. Such a result is a compiler/runtime bug and should be reduced to a regression test.

Panics, host crashes, memory-safety failures, and hangs are never conforming diagnostic behavior. Fuzz targets, scheduler stress/model tests, sanitizer tests, and backend parity exist specifically to detect those failures.
