# Batch 6 Step 8: The Reference Agent, Version 2

## Goal

Rewrite the reference tool runner with derived codecs, schemas, and
`@traced`, and add a tutorial chapter and Manual pages, as the Batch 6
checkpoint's delivery plan describes. This is the last Batch 6 step.

## Work Completed

- `examples/agents/tool_runner` is version 2. `src/agent.au` holds
  `ToolCall`, `ToolError`, the generic `traced` decorator factory, tool
  specs that carry `json.schema[A]()`, and `run`, which decodes arguments,
  calls a tool, and encodes its result. `src/main.au` defines two tools,
  prints their schemas, and answers seven calls, three of them rejected
  with the decoder's path. Retry, Queue streaming, and session cleanup stay.
- Tutorial chapter 27, "Typed Tool Agents", teaches the pieces in order.
- The Learn section has a "Typed Tool Agent" case study that embeds
  `src/agent.au`.
- The JSON Manual page says that codecs can be derived and links the
  reference agent.
- The binary-size bench labels the subject as reference agent version 2.

## Deviation

Section E4 sketches `tools = [agent.tool("search", search)]`, a list that
holds each tool function. A decorated binding is a module constant, which
cannot move into an owned list, so the tool specs carry only the name and
schema, and `dispatch` borrows each decorated binding by name.

## Verification

- `reference_tool_runner_package_has_pinned_output_on_both_backends`.
- `python3 -m unittest scripts/test_bench_binary_size.py`.
- `npm run check:doc-examples`, which checks the tutorial and the case study.

## Follow-Up

- Batch 6 is complete once this merges. The Batch 7 decisions come next:
  the native backend, inlining, bounds checks, and SIMD.
