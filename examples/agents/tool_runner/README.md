# Reference tool runner, version 1

This deterministic package is the reference agent for owned callables. It
keeps the shape of version 0:

- typed request and result classes with handwritten JSON conversion
- a typed error enum returned through `Result`
- retry
- Queue streaming
- scoped cleanup

Version 1 adds owned callables:

- `type Tool = Callable[def(value: int64) -> Result[ToolResult, ToolError]]`
  names the registry contract once, instead of repeating a long function type.
- `make_double(factor)` packs a lambda whose owned capture outlives the
  factory scope.
- `Increment(delta=1).run` is a bound method packed into the same `Tool`
  contract. Its receiver moves into the closure.
- `dispatch` takes the registry as `mut`. It removes the tool, calls it with
  the preserved `value=` name, and reinserts it. It works this way because
  `dict.get` requires a clone-safe value, and a packed tool is not one.
  Removal and reinsertion may change insertion order, but this program never
  iterates the registry.

The JSON codecs use optional `T | None` unions and `Lookup` results. Aura has
no builtin `Option` type. The Aura program has no network, filesystem, or
process access.

From the repository root:

```bash
cargo run -p aura -- run --backend mir examples/agents/tool_runner/src/main.au
cargo run -p aura -- run --backend direct examples/agents/tool_runner/src/main.au
```

## How It Behaves

- The producer places events in one Queue. Only the parent prints them, so
  their order is preserved.
- The session closes even when the body returns.
- Retry readiness uses the existing process-local metrics counter and a
  zero-delay backoff, so it succeeds on attempt three.

The CLI smoke test checks both backends against `stdout.txt`. The only change
from version 0's output is the version header. Version 1 stays below 400 Aura
source lines.
