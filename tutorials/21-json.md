# Working With JSON Values

This chapter shows how to parse, inspect, build, and dump JSON. The `json`
module parses untrusted text into its own recursive value type and reports
failures as typed errors you can recover from. Its output is deterministic, so
you can use it for service messages, fixtures, and cache keys.

## Parse Into A Typed Tree

`json.parse` returns `Result[json.Value, json.Error]`:

```aura check-pass
import json

result = json.parse("{\"name\":\"aura\",\"workers\":3}")

match result:
    case Result.Ok(value):
        print(json.dumps(value))
    case Result.Err(error):
        print(error)
```

A successful parse gives a `json.Value`, not an untyped host object. It is one
of seven enum variants: `Null`, `Bool`, `Int`, `Float`, `String`, `Array`, or
`Object`. An ordinary exhaustive `match` tells them apart.

Parse errors are values too:

| `json.Error` variant | Meaning |
| --- | --- |
| `Syntax` | Holds a message and a location. |
| `NumberOutOfRange` | Names a number Aura cannot keep as an `int64` or a finite `float64`. |
| `NestingTooDeep` | Reports the nesting limit. |
| `InputTooLarge` | Reports the input-size limit. |

Lines and columns start at one. A column counts Unicode scalar values, not
UTF-8 byte offsets.

```aura fragment
match json.parse("{\"ready\":"):
    case Result.Ok(value):
        print(value)
    case Result.Err(json.Error.Syntax(message, line, column)):
        print(f"{line}:{column} {message}")
    case Result.Err(error):
        print(error)
```

## Numbers Keep Their JSON Meaning

The parser classifies the exact source number before any binary64 rounding.
Any mathematical integer in the `int64` range becomes `Value.Int`, even when
the source uses a decimal point or an exponent:

- `1`, `1.0`, and `1e0` become `Int(1)`
- `1.5e1` becomes `Int(15)`
- `-0.0` becomes `Int(0)`
- `1.5` becomes `Float(1.5)`
- `1e400` returns `NumberOutOfRange`

So a rounded float never poses as an exact integer. It also means the source
spelling alone does not pick the variant.

The scalar accessors are exact:

```aura check-pass
import json

integer = json.Value.Int(7)

match json.as_int(integer):
    case int64 as value:
        print(value)
    case None:
        print("not an integer")

print(json.as_float(integer) == None)
```

`as_float` does not convert an Int. If you need a numeric conversion, extract
the payload first and convert it yourself.

## Borrow To Inspect, Consume To Extract

`json.is_null`, `json.as_bool`, `json.as_int`, and `json.as_float` use the
ordinary bare parameter default. That default is shared access, so the JSON
value stays available after the call.

The owned `String`, `Array`, and Object payloads come out through consuming
functions: `json.into_string`, `json.into_array`, and `json.into_object`.

```aura check-pass
import json

def main():
    value = json.Value.Array([json.Value.Int(2), json.Value.Int(3)])

    match json.into_array(value):
        case list[json.Value] as items:
            print(items.len())
        case None:
            print("not an array")
```

An `into_*` call consumes its argument whether or not the variant matches.
Ownership transfer is explicit, and there is no hidden deep clone of a nested
tree.

## Build And Dump Deterministically

Build values with the ordinary qualified enum constructors. One Object can
hold different JSON kinds, because every dictionary value has the same type,
`json.Value`:

```aura check-pass
import json

payload = json.Value.Object({"workers": json.Value.Int(3), "ready": json.Value.Bool(true), "tags": json.Value.Array([json.Value.String("compiler"), json.Value.String("service")])})

print(json.dumps(payload))
print(json.dumps(payload, indent=2))
```

Compact output sorts object keys, so the first line is:

```text
{"ready":true,"tags":["compiler","service"],"workers":3}
```

Pretty output follows these rules:

- LF line endings
- two spaces for each nesting level
- one space after each colon
- no final newline
- empty arrays and objects stay `[]` and `{}`

Sorting happens only in the dump. It does not change the value: the Object's
underlying dict keeps its insertion order. When parsed input repeats an object
key, the key keeps its first insertion slot and takes the last value.

## Parse Errors And Dump Traps Are Different

Malformed input is normal at a service boundary, so `parse` returns a
`json.Error`. Match it and decide whether to reject, log, or retry.

`json.dumps` returns `str`, not `Result`. So its failures trap:

| Code | Cause |
| --- | --- |
| `AU4003` | An invalid indent, or depth greater than 128. |
| `AU4001` | NaN or infinity in a manually built Float. |
| `AU4005` | The output cap is exceeded, or an allocation fails. |

These limits apply:

- Indent must be `None` or an integer from 0 through 16.
- Parse input and dump output each have their own 67,108,864-byte cap. The
  exact boundary is accepted.
- Depth counts containers only. A root scalar is depth zero, and a root Object
  or Array is depth one. Depth 128 is accepted.
- Parse and dump share a 262,144-value structural budget. Every scalar, array,
  object, and object member value counts once. Object keys do not count. The
  exact boundary is accepted.

Going over the structural budget reports `AU4005`, as an output-cap overflow
or a controlled allocation failure does.

## Strict JSON, Not A Schema System

The parser accepts one strict JSON value with optional JSON whitespace around
it. It rejects comments, trailing commas, leading-zero integers, `NaN`, and
infinities.

Use `json.Value` when the shape is truly dynamic or your application code
checks it.

These are not implemented:

- derived class and enum schemas
- generated codecs
- a streaming JSON API
- an arbitrary-precision number type

For flat `dict[str, str]` data, `json.is_valid`, `json.stringify_map`, and
`json.parse_string_map` provide separate typed operations. They are not part
of the dynamic `json.Value` API.

## Full Contract

The normative [JSON Module](../docs/manual/json.md) chapter fixes the complete
variant shapes, numeric rules, error coordinates, ordering, escaping,
formatting, ownership, diagnostics, and limits.

Design record:
[ADR-0021](../architecture_docs/decisions/0021-json-value-model-and-codec-policy.md).
