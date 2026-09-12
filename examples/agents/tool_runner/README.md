# Reference tool runner, version 1

This deterministic package is the Batch 1 phase 1 reference agent. It keeps
version 0's shape (typed request/result classes with handwritten JSON
conversion, a typed error enum returned through Result, retry, Queue
streaming, and scoped cleanup) and adds the owned-callable surface that phase
1 delivered:

- `type Tool = Callable[def(value: int64) -> Result[ToolResult, ToolError]]`
  names the registry contract once instead of repeating a long function type.
- `make_double(factor)` packs a lambda whose owned capture outlives the
  factory scope.
- `Increment(delta=1).run` is a bound method packed into the same `Tool`
  contract; its receiver moved into the closure.
- `dispatch` takes the registry as `mut`, removes the tool, calls it with the
  preserved `value=` name, and reinserts it, because `dict.get` requires a
  clone-safe value and a packed tool is not one. Removal and reinsertion may
  change insertion order; this program never iterates the registry.

The JSON codecs still use the Option library patterns of this language
version; the phase 2 diff replaces them. There is no network, filesystem, or
process access in the Aura program.

From the repository root:

```bash
cargo run -p aura -- run --backend mir examples/agents/tool_runner/src/main.au
cargo run -p aura -- run --backend direct examples/agents/tool_runner/src/main.au
```

The CLI smoke test checks both backends against `stdout.txt`, whose only
change from version 0 is the version header. The producer places events in
one Queue; only the parent prints them, preserving their order. The session
closes even when the body returns. Retry readiness uses the existing
process-local metrics counter and zero-delay backoff to succeed on attempt
three. Version 1 stays below 400 Aura source lines.
