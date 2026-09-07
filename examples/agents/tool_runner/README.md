# Reference tool runner, version 0

This deterministic package is the pre-Batch-1 reference agent. It uses the
implemented language surface: a dictionary of capture-free named functions,
typed request/result classes with handwritten JSON conversion, a typed error
enum returned through Result, retry, Queue streaming, and scoped cleanup.
There is no network, filesystem, or process access in the Aura program.

From the repository root:

```bash
cargo run -p aura -- run --backend mir examples/agents/tool_runner/src/main.au
cargo run -p aura -- run --backend direct examples/agents/tool_runner/src/main.au
```

The CLI smoke test checks both backends against `stdout.txt`. The producer
places events in one Queue; only the parent prints them, preserving their order.
The session closes even when the body returns. Retry readiness uses the existing
process-local metrics counter and zero-delay backoff to succeed on attempt three.

`ToolRequest.from_json` validates the two explicit fields; `ToolResult.from_json`
reuses that validation for the same wire shape. These methods demonstrate manual
codecs, not generated schemas. Version 0 stays below 400 Aura source lines and is
the comparison point for later usability batches.
