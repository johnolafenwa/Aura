# Status And Compatibility

Aurora 0.1 is an advanced technical preview. It is suitable for compiler and runtime evaluation, examples, and controlled experiments; it is not yet a production systems-language release or a security boundary for untrusted programs.

## Canonical Contract

The maintained language contract consists of:

1. the normative Language Specification and Manual
2. compiler fixtures and CLI/LSP regression tests as executable conformance evidence
3. the compiler, runtime, CLI, and language server as conforming implementations
4. categorized examples and Learn chapters as teaching material

The Manual and executable suite are expected to agree. A divergence is a project defect, not an alternate language rule. The historical proposal is design history. Features mentioned only there—including `Channel`, `select`, detached spawn, tuples, attributes, and registry publishing—are not part of Aurora 0.1.

See [Language Specification](/manual/language-specification) and [Conformance](/manual/conformance).

## Stability Policy

Syntax expansion is frozen for the 0.1 hardening cycle. Work in this cycle prioritizes distribution, correctness, native-runtime safety, editor responsiveness, and the control-plane standard library. Existing documented syntax may still receive correctness fixes, and APIs may change while 0.1 remains untagged.

Compiler coverage is held at the current non-regression floor rather than being pushed to 100%. New behavior still requires focused tests; the freeze only ends marginal coverage work that does not reduce product risk.

## Maintained Concurrency Surface

Aurora 0.1 uses structured concurrency:

- `TaskGroup()` owns child tasks inside `with`
- `TaskGroup.start(...)` returns a `Task[T]`
- `TaskGroup.start_soon(...)` starts a child whose result is not retained
- `Queue[T]` provides bounded or unbounded task-aware communication
- `wait_any(...)` and `wait_all(...)` coordinate task completion

There is no `Channel`, language-level `select`, bare `spawn`, or detached task.

## Platform And Distribution Support

Release archives target glibc Linux x86-64 and macOS x86-64/Apple silicon. Each archive includes the native runtime and linker manifest used by `aura build`; Cargo and the Aurora source checkout are not runtime dependencies of an installed archive. A host C compiler is still required.

See the repository `SUPPORTED_PLATFORMS.md` for the exact matrix and pinned toolchain.
