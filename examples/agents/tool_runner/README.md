# Reference Tool Runner, Version 2

This deterministic package is the reference agent. It reads tool calls as
JSON text, checks each call's arguments against a typed class, runs the tool,
and returns a JSON result. It keeps the shape of the earlier versions:

- typed request, argument, and result classes
- a typed error enum returned through `Result`
- retry
- Queue streaming
- scoped cleanup

Version 2 replaces the handwritten JSON conversion with derived codecs and
adds decorators:

- `impl json.Codec for DoubleArgs` derives `encode`, `decode`, and a JSON
  Schema. The class docstring becomes the schema's `description`.
- `agent.tool[DoubleArgs]("double")` lists a tool with its schema. The
  program prints each schema, as a model's tool list would carry it.
- `@traced("double")` is a generic decorator factory. It infers its type
  parameters from each tool function and prints a line each time the tool
  runs.
- `agent.run(double, args)` decodes the arguments, calls the tool, and
  encodes the result. A decode error carries the path of the field that
  failed, such as `$.value`.
- `dispatch` lists the tools explicitly. Aura has no mutable module state,
  so a decorator cannot add a tool to a registry.

`src/agent.au` holds the reusable tool layer. `src/main.au` holds the tools
and the program. The Aura program has no network, filesystem, or process
access.

From the repository root:

```bash
cargo run -p aura -- run --backend mir examples/agents/tool_runner/src/main.au
cargo run -p aura -- run --backend direct examples/agents/tool_runner/src/main.au
```

## How It Behaves

- A call with a wrong argument type, an unknown key, or a missing key is
  rejected with the decoder's path and message.
- A call that names no tool is reported as unknown.
- The producer places events in one Queue. Only the parent prints them, so
  their order is preserved.
- The session closes even when the body returns.
- Retry readiness uses the process-local metrics counter and a zero-delay
  backoff, so it succeeds on attempt three.

The CLI smoke test checks both backends against `stdout.txt`. The package
stays below 400 Aura source lines.
