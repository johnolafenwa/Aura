# Working With JSON Values

Aura's JSON surface gives untrusted JSON its own recursive value and
typed parse-error enums. It keeps parsing failures recoverable while making
serialization deterministic enough for service messages, fixtures, and cache
keys.

The observable gap-fill policy is Accepted under ADR-0021, and the API and
examples described here are implemented.

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

The successful value is not an untyped host object. It is one of seven enum
variants: `Null`, `Bool`, `Int`, `Float`, `String`, `Array`, or `Object`.
Ordinary exhaustive `match` can distinguish them.

Parse errors are values too. `Syntax` contains a message and location;
`NumberOutOfRange` identifies a number Aura cannot preserve as `int64` or a
finite `float64`; `NestingTooDeep` and `InputTooLarge` report their limits.
Lines and columns start at one, and a column counts Unicode scalar values.
UTF-8 byte offsets are not used for this field.

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

Parsing classifies the exact source number before binary64 rounding. Any
mathematical integer in the `int64` range becomes `Value.Int`, even when its
source uses a decimal point or exponent:

- `1`, `1.0`, and `1e0` become `Int(1)`
- `1.5e1` becomes `Int(15)`
- `-0.0` becomes `Int(0)`
- `1.5` becomes `Float(1.5)`
- `1e400` returns `NumberOutOfRange`

This keeps a rounded float from masquerading as an exact integer. It also means
source spelling alone does not select the variant.

The scalar accessors are intentionally exact:

```aura check-pass
import json

integer = json.Value.Int(7)

match json.as_int(integer):
    case Option.Some(value):
        print(value)
    case Option.None:
        print("not an integer")

print(json.as_float(integer) == Option.None)
```

`as_float` does not convert an Int. Perform any numeric conversion explicitly
after extracting the payload.

## Borrow To Inspect, Consume To Extract

`json.is_null`, `json.as_bool`, `json.as_int`, and `json.as_float` use the
ordinary bare parameter default: shared access, so the JSON value
remains available. Owned `String`, `Array`, and
Object payloads use the consuming module functions `json.into_string`,
`json.into_array`, and `json.into_object`:

```aura check-pass
import json

def main():
    value = json.Value.Array([json.Value.Int(2), json.Value.Int(3)])

    match json.into_array(value):
        case Option.Some(items):
            print(items.len())
        case Option.None:
            print("not an array")
```

An `into_*` call consumes its argument whether or not the variant matches.
That makes ownership transfer explicit and avoids a hidden deep clone of a
nested tree.

## Build And Dump Deterministically

Construct Values with ordinary qualified enum constructors. One Object can
contain different JSON kinds because every dictionary value has the same
`json.Value` type:

```aura check-pass
import json

payload = json.Value.Object({"workers": json.Value.Int(3), "ready": json.Value.Bool(true), "tags": json.Value.Array([json.Value.String("compiler"), json.Value.String("service")])})

print(json.dumps(payload))
print(json.dumps(payload, indent=Option.Some(2)))
```

Compact output sorts object keys, so the first line is:

```text
{"ready":true,"tags":["compiler","service"],"workers":3}
```

Pretty output uses LF line endings, two spaces for each nesting level, one
space after each colon, and no final newline. Empty arrays and objects remain
`[]` and `{}`.

Sorting is a dump rule, not a mutation. The Object's underlying dict keeps its
insertion order. Parsing duplicate object keys keeps the key's first insertion
slot but replaces it with the last value.

## Parse Errors And Dump Traps Are Different

Malformed input is normal at a service boundary, so parse returns
`json.Error`. Match it and decide whether to reject, log, or retry.

`json.dumps` has the roadmap-mandated return type `str`, not `Result`.
Failures therefore trap:

- invalid indent or depth greater than 128 uses `AU4003`
- NaN or infinity in a manually constructed Float uses `AU4001`
- output-cap or allocation failure uses `AU4005`

Indent must be `None` or `Some(0)` through `Some(16)`. Both parse input and dump
output have independent 67,108,864-byte caps. The exact boundary is accepted.
Depth counts containers only: a root scalar is depth zero, a root Object or
Array is depth one, and depth 128 is accepted.

Parse and dump also share a 262,144-value structural budget. Every scalar,
array, object, and object member value counts once; object keys do not count.
The exact boundary is accepted. Exceeding this budget, like exceeding an output
cap or encountering a controlled allocation failure, reports `AU4005`.

## Strict JSON, Not A Schema System

The parser accepts one strict JSON value plus surrounding JSON whitespace. It
does not accept comments, trailing commas, leading-zero integers, `NaN`, or
infinities.

`json.Value` is useful when the shape is genuinely dynamic or checked by
application code.

Derived class/enum schemas and generated codecs remain deferred beyond Phase 6.
Aura also has no streaming JSON API or arbitrary-precision number type.

`json.is_valid`, `json.stringify_map`, and `json.parse_string_map` provide
typed operations for flat `dict[str, str]` data. They are distinct from the
dynamic `json.Value` API.

## Full Contract

The normative [JSON Module](../docs/manual/json.md) chapter fixes the complete
variant shapes, numeric rules, error coordinates, ordering, escaping,
formatting, ownership, diagnostics, and limits. ADR-0021 records those
observable policies and their rationale.
