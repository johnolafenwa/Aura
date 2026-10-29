# Rust Speed Step 3e: JSON Without A Second Tree

Date: 2026-10-28

## Problem

`json.parse` built a `json_codec::JsonValue` tree, then converted it into
the runtime's `json.Value`, and a successful parse first scanned the text
twice more for syntax and number ranges. `json.dumps` converted the value
back into a `JsonValue` tree before writing it. Copying a `json.Value`
rebuilt the whole tree (`clone_json_runtime_tree`), a design from before
lists and dicts shared their storage on copy. Each field read that
`docdb` passes to a helper copies the document it reads from, so it paid
for a full tree copy each time.

## Changes

- `json_codec::JsonTree` lets the parser build any tree. `RuntimeJson`
  builds the runtime `json.Value`, with objects inserted into
  `DictEntries`, so a repeated key keeps its first place and takes the
  later value, as before. The codec service builds the value on its
  worker and hands it back beside the job's result.
- `parse_into` checks syntax and number ranges only when the parse fails,
  to rank the error as before.
- `json_dumps_runtime_direct` writes the runtime value as
  `json_codec::dumps` writes the tree. It returns `None` for anything the
  tree path would refuse (malformed values, non-finite floats, depth,
  node limit, a bad indent, output size) and while a test hook watches
  the conversion, and the caller then takes the tree path, which reports
  what it always reported.
- A variant whose payloads are leaves or collections that share their
  storage is copied by copying its payload list. `json.Value` copies
  share their lists and dicts until one side writes to them.
  `clone_json_runtime_tree` is gone, and the test that asserted rebuilt
  buffers now asserts shared buffers and copy on write.

## Measurements

Callgrind instructions at check size, Linux, base = step 3c part 2. All
30 programs print the same output, and no other program moves more than
0.5%.

| Program | Before | After | Change |
| --- | ---: | ---: | ---: |
| `json_roundtrip` | 491.8M | 365.0M | -25.8% |
| `pubsub_broker` | 67.3M | 58.5M | -13.0% |
| `docdb` | 35.6M | 32.1M | -10.0% |
| `http_api` | 28.1M | 26.3M | -6.3% |
| `task_pool` | 42.3M | 40.7M | -3.7% |

## Gate

3e's gate (json_roundtrip and docdb at most 2x Rust) is missed.
json_roundtrip at 5 rounds runs 785M instructions against Rust's 185M
(4.25x, was 5.85x), and docdb at 5,000 documents 1,168M against 137M
(8.5x, was 9.9x). What remains is allocation (about a fifth of each run:
a payload list per variant, a box per value read), field reads that copy
the value they reach before passing it to a function, and the walk in
`json_roundtrip`, which lists an object's keys and then looks each one up.
