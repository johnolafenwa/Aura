# JSON Module

The `json` module represents any JSON data as one recursive enum,
`json.Value`. Parsing returns malformed or unsupported input as typed error
data. Dumping produces one deterministic JSON string, or traps when the value
cannot be serialized.

The module also has helpers for flat `dict[str, str]` data. These are separate
from the dynamic tree. The module does not derive schemas from your classes or
enums.

| API | Signature | Contract |
| --- | --- | --- |
| `json.parse` | `parse(text: str) -> Result[json.Value, json.Error]` | Parses one strict JSON value from a shared string. |
| `json.dumps` | `dumps(value: json.Value, indent: int64 \| None = None) -> str` | Serializes a shared JSON tree deterministically. |
| `json.is_valid` | `is_valid(text: str) -> bool` | Reports whether the text is valid JSON. |
| `json.stringify_map` | `stringify_map(value: dict[str, str]) -> Result[str, str]` | Serializes a flat string dictionary in sorted-key order. |
| `json.parse_string_map` | `parse_string_map(text: str) -> Result[dict[str, str], str]` | Parses a JSON object whose values are all strings. |

## Example

This program parses a dynamic object, reads an exact integer variant, builds
a mixed nested tree, and prints compact and pretty output:

```aura
import json

def main():
    match json.parse("{\"workers\":3,\"tags\":[\"compiler\",\"service\"]}"):
        case Result.Ok(value):
            print(json.dumps(value))
        case Result.Err(error):
            print(error)

    integer = json.Value.Int(7)
    print(json.as_int(integer))

    payload = json.Value.Object({"workers": json.Value.Int(3), "ready": json.Value.Bool(true), "tags": json.Value.Array([json.Value.String("compiler"), json.Value.String("service")])})
    print(json.dumps(payload))
    print(json.dumps(payload, indent=2))
```

The same program is maintained as `examples/json/dynamic_values.au`.

## Value And Error Model

`json.Value` has exactly these variants:

| Variant | Payload | Meaning |
| --- | --- | --- |
| `Null` | none | JSON null. |
| `Bool` | `bool` | JSON true or false. |
| `Int` | `int64` | A mathematically integral JSON number in the `int64` range. |
| `Float` | `float64` | Any other finite JSON number representable by binary64. |
| `String` | `str` | An owned decoded JSON string. |
| `Array` | `list[json.Value]` | An owned ordered sequence of values. |
| `Object` | `dict[str, json.Value]` | An owned string-keyed object with insertion slots. |

Only `json.parse` returns `json.Error`, and only for problems with the input
data. Resource failures while parsing or building the runtime tree trap with
`AU4005`. There is no resource variant.

| Variant | Payload | Meaning |
| --- | --- | --- |
| `Syntax` | `message: str, line: int32, column: int32` | The input is not one strict JSON value. |
| `NumberOutOfRange` | `line: int32, column: int32` | A number fits neither the `Int` rule nor a finite `float64`. |
| `NestingTooDeep` | `limit: int32, line: int32, column: int32` | A container would exceed the depth limit. |
| `InputTooLarge` | `actual_bytes: int64, limit_bytes: int64` | The encoded input exceeds the parse cap. |

Lines and columns are one-based. A column counts Unicode scalar values from
the start of its line, not UTF-8 bytes. The position points at the offending
token or container:

- `NumberOutOfRange` points at the first scalar of the number token.
- `NestingTooDeep` points at the opening bracket or brace that would exceed
  the limit.
- `Syntax` points at the first unexpected scalar. At an unexpected end of
  input, it points right after the last scalar.

## Typed Accessors

Accessors never convert between variants:

| API | Signature | Contract |
| --- | --- | --- |
| `json.is_null` | `is_null(value: json.Value) -> bool` | `true` only for `Value.Null`. |
| `json.as_bool` | `as_bool(value: json.Value) -> bool \| None` | The Bool payload, or `None`. |
| `json.as_int` | `as_int(value: json.Value) -> int64 \| None` | The Int payload, or `None`. A Float is not converted. |
| `json.as_float` | `as_float(value: json.Value) -> float64 \| None` | The Float payload, or `None`. An Int is not converted. |
| `json.into_string` | `into_string(value: own json.Value) -> str \| None` | Consumes the value and returns its str payload, or `None`. |
| `json.into_array` | `into_array(value: own json.Value) -> list[json.Value] \| None` | Consumes the value and returns its Array payload, or `None`. An empty array is a present list. |
| `json.into_object` | `into_object(value: own json.Value) -> dict[str, json.Value] \| None` | Consumes the value and returns its Object payload, or `None`. An empty object is a present dictionary. |

The inspecting functions borrow `value`. The `into_*` functions take an
explicitly owned value. A consuming accessor that does not match still
consumes its argument, and returns `None`.

## Parsing

`json.parse` accepts exactly one RFC 8259 JSON value, with optional JSON
whitespace before and after it. It rejects:

- comments
- trailing commas
- integers with leading zeros
- string escapes that JSON does not define
- `NaN` and infinities
- anything other than whitespace after the first value

### Number Classification

The parser classifies a number by the exact mathematical value of its source
token, before any binary64 rounding:

- A mathematical integer in the `int64` range becomes `Value.Int`.
- Any other number whose IEEE-754 binary64 conversion is finite becomes
  `Value.Float`, with normal binary64 rounding and underflow.
- A number whose conversion overflows returns `Error.NumberOutOfRange`.

So `1`, `1.0`, `1e0`, `1.5e1`, and `-0.0` all parse as Int values, and
`-0.0` becomes integer zero. `1.5` parses as a Float. `1e400` returns
`NumberOutOfRange`, not infinity.

### Arrays And Objects

Array elements keep their source order. An object key gets its insertion slot
the first time it appears. A later occurrence of the same key replaces the
value and keeps that first slot. Keys are compared after decoding, so `"a"`
and `"\u0061"` are the same key. The ordinary `dict` iteration APIs show this
insertion order, even though dumping sorts keys.

### Parse Limits

Depth counts arrays and objects, not scalar leaves. A root scalar has depth
zero, and a root container has depth one. Depth 128 is accepted. The first
container that would reach depth 129 returns
`Error.NestingTooDeep(limit=128, ...)`.

Input length is measured in UTF-8 bytes. Up to 67,108,864 bytes are accepted,
including exactly that many. A larger input returns
`Error.InputTooLarge(actual_bytes, limit_bytes=67108864)` before any syntax,
number, or depth check.

Parsing and both runtime conversion directions share a limit of 262,144 JSON
value nodes. The root counts as one node, and each scalar, array, or object
value counts as one more. Object keys do not count. Exactly 262,144 nodes are
accepted. The next value traps with `AU4005` instead of returning a
`json.Error`, because the input can be valid JSON that exceeds this fixed
runtime budget.

## Deterministic Dumping

`json.dumps` emits array elements in order. It sorts object keys
lexicographically by their UTF-8 encodings, which for valid UTF-8 matches
Unicode scalar order. The output does not depend on object insertion order.

### Number Spelling

- `Value.Int` uses plain base-ten digits with no decimal point.
- A finite `Value.Float` uses Aura's shortest binary64 spelling that
  round-trips to the same value.
- An integral finite float keeps a decimal point or exponent marker.
- Negative zero stays `-0.0`.

Parsing that text still applies the exact mathematical-integer rule. So
`parse(dumps(value))` does not promise to keep an integral Float variant you
built explicitly, or a negative floating zero. Sorted keys also mean the
result need not keep the original dictionary's insertion slots.

### Strings

Strings keep non-ASCII Unicode scalar values as they are.

- Quotation mark and reverse solidus are escaped.
- Backspace, tab, line feed, form feed, and carriage return use `\b`, `\t`,
  `\n`, `\f`, and `\r`.
- Other controls from U+0000 through U+001F use lowercase `\u00xx`.
- Solidus is not escaped.

### Indentation

With `indent=None`, the output has no insignificant whitespace. With
`indent=n`, `n` must be from 0 through 16 inclusive. Pretty output uses:

- LF line endings
- `n` ASCII spaces for each container level
- one ASCII space after an object colon
- compact `[]` and `{}` for empty containers
- no final newline

A non-empty container puts each element or member on its own line, with a
comma after every item except the last. Its closing delimiter goes on its own
line at the container's opening level. So `indent=0` uses line breaks but no
leading indentation.

### Dump Limits

Dump depth uses the same container-only definition and limit as parse depth.
The output has its own cap of 67,108,864 UTF-8 bytes, and output of exactly
that size is accepted. The serializer never returns a partial result.

## Grammar

The module adds no grammar. Imports, qualified enum variants, variant
construction, method calls, `Result` matching, `T | None` unions,
dictionaries, lists, and named and default arguments use the ordinary grammar
defined elsewhere in this Manual. JSON text is runtime `str` data. JSON
object, array, string, number, Boolean, and null syntax is not Aura source
syntax.

## Typing Rules

The signatures and variant tables above are normative. `json.Value` and
`json.Error` are builtin enums qualified by the module name. Both are move
types:

- `json.Value`, because its variants hold owned str, list, dict, and recursive
  Value payloads.
- `json.Error`, because `Syntax` holds a str.

Every variant payload follows the normal owned enum-construction rule.
`json.parse` and `json.dumps` take ordinary bare parameters, which are shared
borrows under Aura's parameter rules. The default `indent=None` is an
`int64 | None` evaluated at the call. A bare `int64` argument is converted
into that union.

An accessor's `value` parameter mode is part of its type. Inspecting
accessors do not change ownership. Each `into_*` call consumes its argument
even when the variant does not match. No accessor converts Int to Float, Float
to Int, or a scalar to text.

The flat-dictionary helpers have their own exact types. They are not aliases
for `parse` and `dumps`. `parse_string_map` does not accept nested or
non-string values. `json.is_valid`, `json.parse_string_map`, and
`json.stringify_map` are bounded operations that run on the caller. None of
them uses the JSON codec service described below.

## Runtime Semantics

### Parse

`json.parse` runs in this order:

1. It checks the UTF-8 byte cap.
2. It validates the input and builds one owned tree, following the number,
   duplicate-key, position, depth, and node-budget rules above.
3. It returns `Result.Ok(value)` on success, or one exact `json.Error` variant
   for bad data.

A codec or runtime-tree allocation failure, or exceeding the 262,144-node
budget, traps with `AU4005`. These are not input errors and never become a
`json.Error` variant.

### The JSON Codec Service

`json.parse` uses a recursive parser from a dependency. That parser runs on
Aura's dedicated JSON codec service, not on a lightweight task's coroutine
stack.

- The service is global to the process. It is separate from the protocol pool
  and the general blocking-I/O pool.
- It has two workers with 2 MiB native stacks.
- It admits at most two operations in flight. That count includes work that
  has reserved capacity but has not yet reached a worker.
- Capacity is reserved before the fallible owned copy of the source is made.
  So a busy service cannot pile up waiting copies of the source.
- A lightweight task waiting to enter the service parks on a scheduler
  notification instead of spinning.

After parsing, the conversion from codec output to the runtime tree is
iterative. JSON-aware runtime cloning and rendering are iterative too. These
traversals keep the tree, ownership, diagnostic, and resource rules on this
page, and host call depth does not grow with JSON nesting.

### Dump

`json.dumps` checks indent, depth, and that floats are finite while it writes
into a capped buffer. It applies the sorted-key, number, escape, and
whitespace rules above. A successful call returns one fresh owned str.

A check or resource failure raises the diagnostic listed under
[Diagnostics](#diagnostics), not a `json.Error`, because the return type is
not a `Result`. Before writing, the conversion from runtime tree to codec
value applies the same 262,144-node limit, counting the root and excluding
keys. That conversion and the output writer are iterative. Dumping does not
use the codec service.

### Equality And Non-Finite Floats

Equality and pattern matching follow the ordinary enum and collection rules.
Float equality is IEEE equality. A program can build a non-finite
`Value.Float` explicitly. You can inspect and match that value, but you
cannot dump it as JSON.

## Ownership And Evaluation Order

`parse` shares its input for the call only and does not keep it. Every str
key, str value, array, object, and enum payload in the returned tree is fresh
owned data. `dumps` shares its tree for the call only. It does not reorder or
change object maps, and the caller's value stays usable afterward.

Enum constructors evaluate payload expressions in source order and consume
non-copy payloads. Building arrays and objects therefore follows the existing
list, dictionary, and enum ownership rules. Inspecting accessors share their
argument. Consuming accessors move one payload out of the value, or consume
the value when it does not match.

Parsing and dumping are synchronous calls with visible effects. Argument and
receiver expressions evaluate in source order, as at any call site. Once a
`json.parse` call is admitted to the codec service, cancellation does not
abandon the job. The call waits for its result, and the task sees the
cancellation at its next ordinary cancellation point.

There is no global mutable parser configuration, serializer setting, or
key-order setting. The global codec service carries work, not parse state
that the language can see.

## Diagnostics

Compile-time codes:

| Code | Cause |
| --- | --- |
| `AU2001` | Unknown `json` name, enum variant, function, or accessor. |
| `AU2002` | Type mismatch in an argument, constructor payload, return, or annotation. |
| `AU2004` | Wrong arity, argument name, or positional/named binding. |

The ordinary ownership diagnostics apply to moved Values, Errors, strings,
arrays, and objects.

Malformed syntax, an out-of-range number, excessive depth, and oversized input
during parsing return typed `json.Error` values. They are not runtime
diagnostics. Parsing traps with `AU4005` on allocation failure or when a value
goes past the 262,144-node limit.

`json.dumps` traps as follows, and never returns a partial string:

| Code | Cause |
| --- | --- |
| `AU4003` | `indent` is outside `0..=16`, or a value is deeper than 128. |
| `AU4001` | A Float payload is NaN or infinite. |
| `AU4005` | The node budget is exceeded, output would exceed 67,108,864 bytes, or a conversion or output allocation fails. |

## Backend Support

The MIR runtime and the direct native backend share the same behavior for:

- the recursive enum identity
- number classification and error positions
- duplicate-key handling
- depth, node, and byte limits
- key order, number spelling, escaping, and indentation
- diagnostic categories

For the same input or value, both backends must produce the same Aura result
and the same dump bytes.

Both backends use the same bounded codec service for `json.parse`. The direct
backend holds read access to the value table only while it validates and
copies the shared source str. It releases that access before it waits for the
service to admit or finish the job.

Runtime, direct-codegen, analysis, language-server, fixture, and executable
reference tests cover this surface on both backends.

## Limits And Implementation-Defined Behavior

JSON numbers have only the `int64` and finite `float64` representations. There
is no arbitrary-precision integer, decimal, lossless source-number token, or
non-finite JSON encoding. Object keys are strings.

The human-readable message in `Error.Syntax` may change. The variant, the
coordinate convention, and the reported location are normative.

Parse and dump work on whole values. These are not available:

- an incremental parser or streaming encoder
- a caller-provided writer
- configurable key order
- alternate escape modes
- comment or trailing-comma modes
- configurable depth or byte caps

The fixed limits are:

| Limit | Value |
| --- | --- |
| Parse input size | 67,108,864 bytes |
| Parse depth | 128 container levels |
| Dump output size | 67,108,864 bytes |
| Dump depth | 128 container levels |
| Nodes when parsing or dumping | 262,144 JSON value nodes, counting the root and values but not object keys |

The codec service for `json.parse` admits two operations across the process.
Its two 2 MiB-stack workers start lazily and live until the process exits.
Aura 0.3 has no API to shut down, join, size, or configure the service. The
service capacity does not apply to `json.is_valid`, `json.parse_string_map`,
or `json.stringify_map`.

Aura 0.3 does not derive schemas or generate codecs for classes and enums.
Schema validation, MessagePack, CBOR, Protobuf, and other binary formats are
also not available.

Collection and string growth controlled by the codec, and both runtime
conversion trees, use fallible allocation and report failure as `AU4005`. An
allocator failure inside Rust, the operating system, or scratch work owned by
a dependency is outside Aura's control and can still end the process. The
language does not claim that every host out-of-memory condition can be caught.

## Status

The value and error model, parse and dump, accessors, ordering, formatting,
and resource limits are implemented Aura 0.3 behavior.

`is_valid`, `stringify_map`, and `parse_string_map` are bounded
flat-dictionary operations. Aura 0.3 has no streaming JSON codec.

Design record: [ADR-0021: JSON value model and codec policy](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0021-json-value-model-and-codec-policy.md).
