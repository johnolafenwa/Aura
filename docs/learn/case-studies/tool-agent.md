# Case Study: A Typed Tool Agent

This case study walks through the reference tool runner, a deterministic
agent that answers tool calls. A model sends each call as JSON text. The
program checks the arguments against a typed class, runs the tool, and
returns a JSON result. Bad calls come back as errors that name the field
that failed.

The full package is
[examples/agents/tool_runner](https://github.com/johnolafenwa/Aura/tree/main/examples/agents/tool_runner).
It has two files:

- `src/agent.au` is the reusable tool layer.
- `src/main.au` defines the tools and runs the program.

## The Tool Layer

<<< ../../../examples/agents/tool_runner/src/agent.au{aura}

- **`ToolCall`** decodes the outer call. Its `args` field is a `json.Value`,
  so the arguments pass through untouched until the tool decodes them.
- **`traced`** is a generic decorator factory. `@traced("double")` takes the
  tool's name, and the decorated function fixes `A` and `R`.
- **`tool`** pairs a name with `json.schema[A]()`. The schema's
  `description` is the argument class's docstring.
- **`run`** decodes the arguments as `A`, calls the tool, and encodes its
  result as JSON. It borrows the tool, so the decorated binding stays in
  place.

## The Tools

Each tool takes one argument class and returns `Result[Answer, ToolError]`:

```aura fragment
class IncrementArgs:
    """Adds a step to a number."""

    value: int64
    step: int64 = 1


impl json.Codec for IncrementArgs


@traced("increment")
def increment(args: IncrementArgs) -> Result[Answer, ToolError]:
    return Ok(Answer(tool="increment", value=args.value + args.step))
```

`step` has a default, so the schema lists only `value` as required, and a
call may leave `step` out.

## Dispatch

Aura has no mutable module state, so a decorator cannot register a tool.
`dispatch` lists the tools by name and hands each call to `agent.run`:

```aura fragment
def dispatch(call: own ToolCall) -> Result[json.Value, ToolError]:
    if call.tool == "double":
        return agent.run(double, call.args)
    if call.tool == "increment":
        return agent.run(increment, call.args)
    return Err(ToolError.UnknownTool(name=call.tool))
```

## What It Prints

The program prints each tool's schema, then answers seven calls:

| Call | Output |
| --- | --- |
| `double` with `{"value": 21}` | `call double`, then `result {"tool":"double","value":42}` |
| `increment` with `{"value": 42}` | `call increment`, then the result with `43` |
| `increment` with `{"value": 42, "step": 8}` | the result with `50` |
| `double` with `{"value": "x"}` | `rejected args $.value: expected integer, found string` |
| `double` with an extra key | ``rejected args $: unknown field `extra` `` |
| an unknown tool | `unknown tool missing` |
| a call with no `tool` key | ``rejected $: missing field `tool` `` |

After the calls it retries a connection with `control.retry`, streams events
from a child task through a `Queue`, and closes its session on exit. Both
backends print the same output, which the CLI tests pin in `stdout.txt`.

## Where To Go Next

- [Typed Codecs](/manual/json#typed-codecs) covers derived codecs, decode
  errors, and schemas.
- [Decorators](/manual/functions#decorators) covers decorator order and
  generic decorators.
