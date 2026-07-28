# Changelog

All notable user-facing changes will be recorded here. Aurora has not made its first tagged release.

## Unreleased — 0.1.0 technical preview

### Breaking changes

- Replaced the source `borrow` capability syntax with three declaration-stable forms: bare parameters and receivers provide logical shared access for every type, including copy types; `mut` provides exclusive mutable access; and `own` transfers ownership. Code that requires the old bare-copy snapshot contract must spell it as `own CopyType`.
- Changed bare `match` to shared matching. Use `match mut value` for mutable matching and `match own value` to consume a scrutinee or its payloads.
- Retired the old spellings: `value: borrow T` becomes `value: T`, `value: borrow mut T` becomes `value: mut T`, `borrow self` becomes `self`, `borrow mut self` becomes `mut self`, and the same removal applies after `match` and `for value in`. During one compatibility release, `borrow` remains reserved solely so the compiler can report these exact replacements; it is not accepted as an alias for the new syntax.
- Migrate a checkout with `python3 scripts/capability_migrate.py apply`, then verify the recorded migration with `python3 scripts/capability_migrate.py check`.
- Removed borrowed-return labels and `borrow`/`borrow mut` return capabilities, superseding the borrowed-return contract. Copy-valued borrowed returns become ordinary owned returns; APIs returning access into non-copy owners must instead return an owned result, handle, index, or expose the operation on the owner.
- Bumped the native artifact-cache format to `aurora-native-cache-v4`, preventing native artifacts compiled with the old capability metadata from being reused.
- Changed lightweight task execution from one cooperative scheduler thread to
  pinned OS workers. The runtime uses the host's available parallelism by
  default; `AURORA_WORKERS=<positive integer>` selects an explicit worker
  count. Queue order remains FIFO per producer, but global sibling execution
  and output order are deliberately unspecified.
- Reserved the builtin names `select` and `SelectOutcome`. Existing user
  declarations with either name must be renamed. The new variadic
  `select(source, ...)` waits over Queue, Task, and relative-Duration sources
  and returns a typed `SelectOutcome[Q, T]`.
- Added `AURORA_BLOCKING_WORKERS` for an exact positive blocking-I/O worker
  count and `AURORA_BLOCKING_QUEUE_CAPACITY` for an optional positive bound on
  accepted pending jobs. Full queues now use FIFO scheduler-aware admission;
  timeout or cancellation before acceptance prevents submission, while
  accepted host work remains non-retractable and discards abandoned results.
  Invalid values fail with `AU4006` before user code under MIR, direct, and
  standalone execution.

- Built a typed bootstrap compiler, MIR runtime, direct native backend, package/workspace support, structured concurrency, file/network/process APIs, LSP, VS Code extension, and maintained book.
- Froze syntax expansion while the 0.1 distribution, safety validation, editor responsiveness, and control-plane standard library are hardened.
- Made release archives carry a relocatable native runtime and linker manifest.
- On maintained Unix hosts, serialized native cache establishment across processes by runtime identity and content key, so concurrent cold direct runs perform one build and the remaining runs consume the verified publication without blocking established warm hits.
- Made long direct-backend operations visible in human output with `aura: waiting for a concurrent build...` and `aura: rebuilding native runtime...`. JSON output provisionally buffers the same notices into its single structured stderr document rather than streaming them, including when `auto` records a direct-to-MIR fallback.
- Kept native caching optional for installed immutable runtime layouts: disabling or losing the cache no longer prevents an otherwise valid direct build.
