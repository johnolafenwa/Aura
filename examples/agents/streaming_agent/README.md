# Reference Streaming Agent

This package is an agent that reads a model's reply as it arrives. It sends
a conversation to the Claude Messages API, prints the reply's text piece by
piece, runs the tool the model calls, and sends the result back until the
model stops.

- `src/claude.au` is the client. It posts the request with the `http`
  module and reads the reply as server-sent events with
  `Response.read_event()`. It builds each content block from its deltas:
  text, thinking with its signature, and a tool call's JSON input.
- `src/main.au` holds the tool and the conversation loop.
- `src/mock.au` is a local stand-in for the API. It streams a scripted
  reply over a loopback socket, so the program runs the same way with no
  network and no key.

From the repository root:

```bash
cargo run -p aura -- run examples/agents/streaming_agent/src/main.au
```

With no `ANTHROPIC_API_KEY` in the environment, the program starts the
stand-in server and prints `stdout.txt`. The CLI smoke test checks both
backends against that file.

With `ANTHROPIC_API_KEY` set, the same loop talks to
`https://api.anthropic.com` and the model writes the reply:

```bash
ANTHROPIC_API_KEY=... cargo run -p aura -- run examples/agents/streaming_agent/src/main.au
```

## How It Behaves

- **Streaming.** A request sets `"stream": true`. The response returns when
  the headers arrive, and each event is handled as it is read. The first
  byte has a 60 second limit and each later read has a 120 second limit.
- **Tool calls.** The tool list carries the JSON Schema derived from
  `WordCountArgs`. A call's input arrives in fragments. `run_tool` decodes
  the whole input against the class before it runs the tool. An input that
  does not decode goes back to the model as an error result.
- **Turns.** The assistant turn goes back in the next request unchanged,
  thinking blocks and signatures included. All tool results of a turn go
  back in one user message.
- **Stopping.** The loop runs tools only when the stop reason is
  `tool_use`. It sends the conversation again on `pause_turn` and stops on
  anything else, such as `end_turn`, `max_tokens`, or `refusal`. It makes
  at most eight requests.
- **Fallbacks.** A request sets `"fallbacks": "default"`, so another model
  answers a request that the first model declines.
- **Errors.** `claude.Failure` separates a transport error, an API error
  with its status and body, and a stream that breaks the protocol.
- **Connections.** Both requests of the stand-in run use one connection,
  which the last line of the output reports.
