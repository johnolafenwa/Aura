# Typed Tool Agents

This chapter builds the core of a tool-calling agent. A model sends a tool
call as JSON text. The program checks the arguments against a typed class,
runs the tool, and sends back a JSON result. Three features do the work:

- derived `json.Codec` implementations, which decode and encode the classes
- derived JSON Schemas, which describe each tool's arguments to the model
- decorators, which wrap every tool in the same tracing code

The finished program is the reference agent in
[examples/agents/tool_runner](../examples/agents/tool_runner/README.md).

## Describe The Arguments

A tool's arguments are a class. An empty `impl json.Codec` derives `encode`,
`decode`, and `schema` for it. The class docstring becomes the schema's
`description`, which is the text a model reads:

```aura
import json


class SearchArgs:
    """Searches the notes for a phrase."""

    query: str
    limit: int32 = 5


impl json.Codec for SearchArgs


def main():
    print(json.dumps(json.schema[SearchArgs]()))
```

This prints one schema object. Its `required` list holds only `query`,
because `limit` has a default. Its `additionalProperties` is `false`, so a
model that invents an argument gets an error instead of a silent default.

## Decode A Call

The model's call names the tool and carries its arguments. A field of type
`json.Value` passes its value through, so `ToolCall` decodes the outer object
and leaves the arguments for the tool:

```aura
import json


class ToolCall:
    tool: str
    args: json.Value


impl json.Codec for ToolCall


class SearchArgs:
    query: str
    limit: int32 = 5


impl json.Codec for SearchArgs


def main():
    match own json.decode_text[ToolCall]('{"tool": "search", "args": {"query": 3}}'):
        case Ok(call):
            match own json.decode[SearchArgs](call.args):
                case Ok(args):
                    print(args.query)
                case Err(error):
                    print(error)
        case Err(error):
            print(error)
```

This prints `$.query: expected string, found integer`. The decoder
stops at the first error and names the field it was reading, so the agent can
send that text back to the model.

## Trace Every Tool

A decorator wraps a function once, when the module starts. A generic
decorator infers its type parameters from each function it wraps, so one
`traced` serves every tool:

```aura
type Fn[A, R] = Callable[def(args: A) -> R]
type Decorator[A, R] = Callable[def(f: own Fn[A, R]) -> Fn[A, R]]


def traced[A, R](name: own str) -> Decorator[A, R]:
    return Decorator[A, R](lambda [own name] own f: trace(name.clone(), f))


def trace[A, R](name: own str, f: own Fn[A, R]) -> Fn[A, R]:
    return Fn[A, R](lambda [own name, own f] args: trace_call(name, f, args))


def trace_call[A, R](name: str, f: Fn[A, R], args: A) -> R:
    print(f"call {name}")
    return f(args)


@traced("double")
def double(args: int64) -> int64:
    return args * 2


def main():
    print(double(21))
```

This prints `call double`, then `42`. `traced("double")` cannot infer `A` and
`R` from its own argument. The function it decorates fixes them. Each tool
takes one parameter named `args`, because a decorator's callable type names
its parameters.

## Dispatch By Name

Aura has no mutable module state, so a decorator cannot add a tool to a
global registry. The program lists its tools explicitly. One generic helper
decodes the arguments, calls the tool, and encodes the result:

```aura fragment
def run[A: json.Codec, R: json.Codec](
    f: Fn[A, Result[R, ToolError]], args: own json.Value
) -> Result[json.Value, ToolError]:
    match own json.decode[A](args):
        case Ok(decoded):
            result = try f(decoded)
            return Ok(json.encode(result))
        case Err(error):
            return Err(ToolError.InvalidRequest(reason=f"args {error}"))


def dispatch(call: own ToolCall) -> Result[json.Value, ToolError]:
    if call.tool == "double":
        return agent.run(double, call.args)
    if call.tool == "increment":
        return agent.run(increment, call.args)
    return Err(ToolError.UnknownTool(name=call.tool))
```

`f` is a shared parameter, so `run` borrows the decorated binding and never
moves it. A tool returns `Result[R, ToolError]`, and `try` passes a tool's
error straight back to the caller.

## The Reference Agent

[examples/agents/tool_runner](../examples/agents/tool_runner/README.md) puts
these pieces together. `src/agent.au` holds `ToolCall`, `ToolError`,
`traced`, the tool specs, and `run`. `src/main.au` defines two tools, prints
their schemas, and answers seven calls, including a wrong argument type, an
unknown key, and an unknown tool. It also retries a connection, streams
events through a Queue, and closes its session on exit.

See [JSON Module](../docs/manual/json.md#derived-codecs) and
[Decorators](../docs/manual/functions.md#decorators) in the Manual.
