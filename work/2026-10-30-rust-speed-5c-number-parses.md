# Rust Speed Step 5c: Number Parses

Date: 2026-10-30

## Problem

The plan's step 5c returns runtime results (`Result`, `Lookup`,
optional values) as words instead of boxed variants. After 5b, the
runtime results left in the 30 programs are mostly number parses.
`csv_aggregate` parses three numbers per row with

```aura
def to_int(text: str) -> int64:
    match parse_int64(text):
        case Result.Ok(value):
            return value
        case Result.Err(_):
            return -1
```

and each call took about 1,300 instructions: the parse built a boxed
`Result` (and, for bad input, the error's text), the `Result` was
tagged, matched by name, its payload read, and the box released.
`dict` `get` already keeps a slot (Batch 7 step 10d), `str.from_bytes`
does since 5b, and text search returns an inline union.

## Changes

- The `Result` slot kind from 5b also covers `parse_int32`,
  `parse_int64`, and `parse_float64` (`lookups::parse_kind`).
- In a slot, the emitter calls `aura_direct_parse_number_ok(text, kind)`,
  which returns the number's box or null and builds no error text. The
  match tests the word for null, and the number is read out of the box
  where it lies.

## Measurements

Instructions at check size (callgrind), 5b release prefix against this
step's:

| Program | 5b | 5c | Change |
| --- | ---: | ---: | ---: |
| csv_aggregate | 32,493,166 | 30,340,971 | -6.62% |
| bytecode_vm | 5,963,948 | 5,904,019 | -1.00% |
| http_api | 19,499,470 | 19,357,583 | -0.73% |
| kv_server | 42,195,879 | 41,949,914 | -0.58% |
| sql_engine | 451,383,159 | 451,255,926 | -0.03% |

All outputs match. Both gates (`sql_engine` and `csv_aggregate` at least
15% fewer instructions) are missed. After 5b, neither program spends
much on runtime results:

- `csv_aggregate` spends its time splitting lines (`string_split`, 14%)
  and building f-strings (12%).
- In `sql_engine`, about 15% of the instructions are appends to lists
  held in instance fields (`parser.kinds.append(...)`). Every one of them
  takes the runtime path: generated code's in-place append never finds
  room in a list reached through a field. Step 5e (instances) takes this
  on.
