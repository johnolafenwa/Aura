# Rust Speed Step 5b: Text From Bytes

Date: 2026-10-29

## Problem

`sql_engine` cuts every token out of the statement's bytes with

```aura
def text_of(data: list[uint8], start: int64, end: int64) -> str:
    match own str.from_bytes(data[start:end]):
        case Result.Ok(text):
            return text
        case Result.Err(_):
            return ""
```

After 5a this took about 4,100 instructions per token: the slice built a
new list in its own box, `str.from_bytes` went through the generic host
builtin path (argument buffer, validation, a boxed `Result`), the
`Result` was tagged, matched by name, and its payload taken out, and the
slice and the `Result` were released. `text_of` was 28% of the program
and the tokenizer 44%.

## Changes

- `lookups::text_byte_slices` finds a slice temporary whose only reader
  is the `str.from_bytes` call right after it, and which is otherwise
  only released. The emitter compiles the slice to its words alone and
  passes them, with the list, to `aura_direct_str_from_byte_slice`, which
  checks the bounds as the slice does (same trap and span) and reads the
  bytes in place.
- A new slot kind keeps the `Result` of `str.from_bytes` as the text's
  box, or null for bytes that are not UTF-8, when its matches test only
  `Ok` and `Err` and only `Ok`'s payload is read. The helper returns that
  form directly; a whole-list `str.from_bytes` in a slot calls the same
  helper with no bounds. `aura_direct_lookup_payload` converts a built
  `Result` for a slot too.
- A slot moved into another temporary, as `match own` does, makes that
  temporary a slot of the same kind. Both ends of the move are slots or
  neither is. This also applies to `match own table.get(key)`.

The plan named a two-word inline `str` for this step. The construction
path was the cost: a short `str` already keeps its bytes inside its box
(Phase 3), so the gate did not need a new representation, and it is not
done.

## Measurements

Instructions at check size (callgrind), 5a release prefix against this
step's:

| Program | 5a | 5b | Change |
| --- | ---: | ---: | ---: |
| sql_engine | 615,631,085 | 451,373,700 | -26.68% |
| mini_browser | 394,121,815 | 360,992,674 | -8.41% |
| interpreter | 26,511,943 | 25,806,705 | -2.66% |
| pubsub_broker | 63,382,908 | 62,415,830 | -1.53% |

The other 26 programs change by between -0.2% and +0.4%, and every
output matches.

The gate is the tokenizer alone. A probe that tokenizes 4,000 generated
`INSERT` statements, less a probe that only generates them, runs
304.6M instructions on 5a and 135.7M with this step: 55.4% fewer, against
a gate of 50%.
