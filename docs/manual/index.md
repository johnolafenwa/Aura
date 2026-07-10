# Aurora Manual

The manual documents Aurora as an implemented language and runtime. It is written for precise answers: syntax, type rules, ownership behavior, runtime effects, module APIs, and CLI contracts.

The Learn track tells a story. The manual defines the rules behind that story.

## Language Reference

- [Lexical Structure](/manual/lexical-structure): files, indentation, comments, identifiers, keywords, literals, f-strings, and duration literals.
- [Types](/manual/types): primitive types, `None`, `Duration`, generic types, copy and move categories, and type annotations.
- [Expressions](/manual/expressions): operators, calls, indexing, member access, literals, `match` expressions, `try`, and f-strings.
- [Statements](/manual/statements): bindings, assignment, control flow, loops, imports, `with`, `pass`, and top-level execution.
- [Functions](/manual/functions): signatures, default arguments, named arguments, `main`, borrowed parameters, returns, and call binding.
- [Classes](/manual/classes): fields, constructors, methods, receivers, associated methods, resources, and mutation.
- [Enums And Pattern Matching](/manual/enums-and-match): variants, payloads, exhaustiveness, literal patterns, short-form variants, and match value flow.
- [Generics And Traits](/manual/generics-and-traits): type parameters, trait declarations, impls, bounds, dispatch, and current restrictions.
- [Ownership And Borrowing](/manual/ownership-and-borrowing): moves, copies, clones, shared borrows, mutable borrows, field moves, and task boundaries.

## Runtime And Library Reference

- [Collections](/manual/collections): `Vec[T]`, `Map[K, V]`, `Set[T]`, literals, iteration, mutation, and method contracts.
- [Concurrency](/manual/concurrency): `TaskGroup`, `Task[T]`, `Queue[T]`, cancellation, `wait_any`, `wait_all`, and scheduler-aware waits.
- [I/O Module](/manual/io): standard input/output and `io.Error`.
- [Filesystem Module](/manual/filesystem): one-shot helpers, `fs.File`, scoped file cleanup, byte and text limits.
- [Network Module](/manual/network): TCP, UDP, HTTP, WebSocket, Unix sockets, TLS, and HTTP client helpers.
- [Process Module](/manual/process): subprocess spawning, pipes, completed processes, process groups, supervisors, and restart policy.
- [Packages](/manual/packages): manifests, package roots, import resolution, lockfiles, and editor analysis behavior.
- [CLI And Tooling](/manual/cli-and-tooling): `aura` commands, diagnostics, analysis JSON, completions, and build modes.
- [API Index](/manual/api-index): every maintained builtin function, method, enum, and module type in one place.
- [Current Limits](/manual/current-limits): intentional current boundaries and practical workarounds.

## Conventions Used In This Manual

Code blocks marked `python` contain Aurora code using Python highlighting as a temporary stand-in until the book ships an Aurora grammar. Shell blocks contain repository commands.

Signatures use `Duration = ...` for optional timeout parameters whose default is documented in the relevant API section. In general:

- blocking APIs wait when a timeout is omitted
- convenience helpers ending in `_or_none` or `_or` may use immediate non-blocking checks when documented that way
- timeout results are explicit variants such as `TimedOut`, `None`, or `process.Error.TimedOut`

When a page says a value is returned "cloned", it means the caller receives a new owned value. When a page says a method "moves" an argument, the caller cannot use that argument after the call unless it is a copy type.
