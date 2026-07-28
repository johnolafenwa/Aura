# JSON Module

Aurora's `json` module represents arbitrary JSON data with one recursive enum.
Parsing reports malformed or unsupported input as typed data; dumping produces
one deterministic JSON string or traps when the supplied value cannot satisfy
the serializer contract.

The dynamic tree complements the older string-map helpers. It does not derive
schemas from user classes or enums.

| API | Signature | Contract |
| --- | --- | --- |
| `json.parse` | `parse(text: String) -> Result[json.Value, json.Error]` | Parses one strict JSON value from a shared string. |
| `json.dumps` | `dumps(value: json.Value, indent: Option[int64] = None) -> String` | Deterministically serializes a shared JSON tree. |
| `json.is_valid` | `is_valid(text: String) -> bool` | Existing validation helper; its compatibility contract is unchanged. |
| `json.stringify_map` | `stringify_map(value: Map[String, String]) -> Result[String, String]` | Existing sorted string-map serializer. |
| `json.parse_string_map` | `parse_string_map(text: String) -> Result[Map[String, String], String]` | Existing string-map-only parser. |

## Value And Error Model

`json.Value` has exactly these variants:

| Variant | Payload | Meaning |
| --- | --- | --- |
| `Null` | none | JSON null. |
| `Bool` | `bool` | JSON true or false. |
| `Int` | `int64` | A mathematically integral JSON number in the `int64` range. |
| `Float` | `float64` | Any other finite JSON number representable by binary64. |
| `String` | `String` | An owned decoded JSON string. |
| `Array` | `Vec[json.Value]` | An owned ordered sequence of values. |
| `Object` | `Map[String, json.Value]` | An owned string-keyed object with insertion slots. |

`json.Error` is returned only by `json.parse` and represents input-data
failures. Resource failures while parsing or materializing the runtime tree
trap with `AU4005` instead of adding a resource variant to this enum:

| Variant | Payload | Meaning |
| --- | --- | --- |
| `Syntax` | `message: String, line: int32, column: int32` | The input is not one strict JSON value. |
| `NumberOutOfRange` | `line: int32, column: int32` | A number fits neither the `Int` rule nor a finite `float64`. |
| `NestingTooDeep` | `limit: int32, line: int32, column: int32` | A container would exceed the depth limit. |
| `InputTooLarge` | `actual_bytes: int64, limit_bytes: int64` | The encoded input exceeds the parse cap. |

Lines and columns are one-based. Columns count Unicode scalar values from the
start of their line, not UTF-8 bytes. The position identifies the offending
token or container. `NumberOutOfRange` points at the first scalar of its number
token. `NestingTooDeep` points at the opening bracket or brace that would
exceed the limit. `Syntax` points at the first unexpected scalar or, for
unexpected end of input, the position immediately after the last scalar.

## Typed Accessors

Accessors never coerce between variants:

| API | Signature | Contract |
| --- | --- | --- |
| `json.is_null` | `is_null(value: json.Value) -> bool` | `true` only for `Value.Null`. |
| `json.as_bool` | `as_bool(value: json.Value) -> Option[bool]` | The Bool payload or `None`. |
| `json.as_int` | `as_int(value: json.Value) -> Option[int64]` | The Int payload or `None`; Float is not converted. |
| `json.as_float` | `as_float(value: json.Value) -> Option[float64]` | The Float payload or `None`; Int is not converted. |
| `json.into_string` | `into_string(value: own json.Value) -> Option[String]` | Consumes the value and returns its String payload or `None`. |
| `json.into_array` | `into_array(value: own json.Value) -> Option[Vec[json.Value]]` | Consumes the value and returns its Array payload or `None`. |
| `json.into_object` | `into_object(value: own json.Value) -> Option[Map[String, json.Value]]` | Consumes the value and returns its Object payload or `None`. |

The inspecting functions borrow `value`. The `into_*` functions take an
explicit owned value. A failed consuming accessor still consumes its argument
and returns `Option.None`.

## Parsing

`json.parse` accepts exactly one RFC 8259 JSON value with optional JSON
whitespace before and after it. It rejects comments, trailing commas,
leading-zero integers, non-JSON string escapes, `NaN`, infinities, and any
non-whitespace after the first value.

Number classification uses the exact mathematical value of the source token
before any binary64 rounding:

- a mathematical integer in the `int64` range becomes `Value.Int`
- every other number whose IEEE-754 binary64 conversion is finite becomes
  `Value.Float`, with normal binary64 rounding and underflow
- a number whose conversion overflows returns `Error.NumberOutOfRange`

Consequently `1`, `1.0`, `1e0`, `1.5e1`, and `-0.0` all parse as Int values;
the last is integer zero. `1.5` parses as a Float. `1e400` returns
`NumberOutOfRange` rather than infinity.

Array elements retain source order. Object keys establish insertion slots on
their first occurrence. A later occurrence of the same key replaces its value
without moving that first slot. Duplicate comparison uses the decoded String,
so `"a"` and `"\u0061"` are the same key. This ordering remains observable
through the ordinary `Map` iteration APIs even though dumping applies its own
sorted-key order.

Depth counts arrays and objects, not scalar leaves. A root scalar has depth
zero and a root container has depth one. Depth 128 is accepted. The first
container that would have depth 129 returns
`Error.NestingTooDeep(limit=128, ...)`.

Input length is measured in UTF-8 bytes. At most 67,108,864 bytes are accepted,
including the exact boundary. A larger input returns
`Error.InputTooLarge(actual_bytes, limit_bytes=67108864)` before syntax,
number, or depth analysis.

Parsing and both runtime conversion directions share a structural
materialization limit of 262,144 JSON value nodes. The root counts as one node,
and every scalar, array, or object value counts as one more; object keys do not
count separately. Exactly 262,144 nodes are accepted. The next value traps with
`AU4005` rather than returning `json.Error`, because the input may be valid JSON
while the fixed runtime materialization budget has been exhausted.

## Deterministic Dumping

`json.dumps` emits arrays in element order and objects with keys sorted
lexicographically by their UTF-8 encodings. Valid UTF-8 preserves Unicode
scalar order under that comparison. The result therefore does not depend on
object insertion order.

`Value.Int` uses an ordinary base-ten integer spelling with no decimal point.
A finite `Value.Float` uses Aurora's maintained shortest binary64 spelling
that round-trips to the same binary64 value. An integral finite float retains
a decimal or exponent marker, and negative zero remains `-0.0`. Parsing that
text still applies the exact mathematical-integer rule, so
`parse(dumps(value))` is not promised to preserve an explicitly constructed
integral Float variant or negative floating zero. Sorted object keys likewise
need not preserve the original map's insertion slots.

Strings retain non-ASCII Unicode scalar values. Quotation mark and reverse
solidus are escaped. Backspace, tab, line feed, form feed, and carriage return
use `\b`, `\t`, `\n`, `\f`, and `\r`; other U+0000 through U+001F controls use
lowercase `\u00xx`. Solidus is not escaped.

With `indent=None`, the output contains no insignificant whitespace. With
`indent=Some(n)`, `n` must be from 0 through 16 inclusive. Pretty output uses:

- LF line endings
- `n` ASCII spaces for each container level
- one ASCII space after an object colon
- compact `[]` and `{}` for empty containers
- no final newline

Every nonempty container places each element or member on its own line, uses a
comma after every item except the last, and places its closing delimiter on a
separate line aligned with the container's opening level. `Some(0)` therefore
uses line breaks but no leading indentation.

Dump depth uses the same container-only definition and limit as parse depth.
The output has an independent 67,108,864-byte UTF-8 cap. The exact boundary is
accepted; a serializer never returns a partial result.

## Example

This executable example parses a dynamic object, inspects an exact integer
variant, constructs a mixed nested tree, and prints deterministic compact and
pretty output:

```aurora
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
    print(json.dumps(payload, indent=Option.Some(2)))
```

The same program is maintained as
`examples/json/dynamic_values.au`.

## Grammar

The module adds no source-language grammar. Imports, qualified enum variants,
variant construction, method calls, `Result` and `Option` matching, maps,
vectors, and named/default arguments use the ordinary grammar defined
elsewhere in this Manual. JSON text is runtime `String` data; JSON object,
array, string, number, Boolean, and null syntax is not Aurora source syntax.

## Typing Rules

The signatures and variant tables above are normative. `json.Value` and
`json.Error` are module-qualified builtin enums. `json.Value` is a move type
because its declaration contains owned String, Vec, Map, and recursive Value
payloads. `json.Error` is also a move type because `Syntax` contains a String.

Every variant payload uses the normal owned enum-construction rule.
`json.parse` and `json.dumps` use ordinary bare parameters, which are
shared borrows under Aurora's declaration-stable parameter policy.
`indent=None` is an `Option[int64]` default evaluated at the call boundary.

An accessor's `value` parameter mode is part of its type. Inspecting accessors
do not change ownership. Each `into_*` call consumes its argument even when
the runtime variant does not match. No accessor converts Int to Float, Float
to Int, or a scalar to text.

The legacy string-map helpers keep their existing types. They are not aliases
for `parse` and `dumps`, and the dynamic API does not broaden
`parse_string_map` to accept nested or non-string values. The legacy
`json.is_valid` and `json.parse_string_map` parsers remain bounded caller-side
compatibility operations; neither is submitted to the dynamic-parse codec
service. `json.stringify_map` likewise remains caller-side.

## Runtime Semantics

Parse first enforces the UTF-8 byte cap, then validates and constructs one
owned tree under the numeric, duplicate-key, position, depth, and node-budget
rules above. It returns `Result.Ok(value)` on success and one exact `json.Error`
variant for data failure. A codec or runtime-tree allocation failure, or
exhaustion of the 262,144-node materialization budget, traps with `AU4005`; it
is not malformed-input data and does not become a `json.Error` variant.

The dependency-owned recursive parse used by dynamic `json.parse` runs on
Aurora's dedicated JSON codec service rather than on a lightweight task's
coroutine stack. The service is process-global and independent of the protocol
and generic blocking-I/O pools. It has two workers with 2 MiB native stacks
and a total in-flight capacity of two operations, including work that has
reserved capacity but has not yet entered a worker. Capacity is reserved
before the fallible owned copy of the source is made, so saturation cannot
accumulate unbounded waiting source copies. A lightweight task waiting to
enter the service parks on a scheduler notification rather than spinning.

After parsing, codec-to-runtime materialization uses an iterative traversal.
JSON-aware runtime cloning and rendering are iterative as well. These
traversals preserve the exact tree, ownership, diagnostic, and resource rules
in this chapter without making host call depth proportional to JSON nesting.

Dump validates indent, depth, and finite floating values while emitting into a
capped destination. It applies the exact sorted-key, number, escape, and
whitespace rules above. A successful call returns one fresh owned String.
Validation or resource failure produces the diagnostic described below rather
than a `json.Error`, because the public return type is not a `Result`.
Before emission, runtime-to-codec conversion applies the same root-inclusive,
key-exclusive 262,144-node materialization limit. Runtime-to-codec conversion
and deterministic emission are iterative; dumping does not use the recursive
parser service.

Equality and pattern matching follow the ordinary enum and collection rules.
Float equality remains IEEE equality. A program can explicitly construct a
non-finite `Value.Float`; that value can be inspected and matched but cannot be
dumped as JSON.

## Ownership And Evaluation Order

`parse` shares its input only for the call and does not retain it. Every String
key, String value, array, object, and enum payload in the returned tree is
fresh owned data. `dumps` shares its tree only for the call, does not reorder
or mutate object maps, and leaves the caller's value available afterward.

Enum constructors evaluate payload expressions in source order and consume
non-copy payloads. Array and object construction therefore uses the existing
Vec, Map, and enum ownership rules. Inspecting accessors borrow their
argument; consuming accessors transfer one payload out of the supplied value
or consume the unmatched value.

Parsing and dumping are synchronous observable calls. Argument and receiver
expressions are evaluated in ordinary call-site source order. Once a
`json.parse` call has been admitted, cancellation does not abandon the codec
job: the call waits for its result, and the task observes cancellation at its
next ordinary cancellation boundary. There is no process-global mutable
parser configuration, serializer setting, or key-order configuration; the
process-global codec service carries work, not language-visible parse state.

## Diagnostics

`AU2001` reports an unavailable `json` name, enum variant, function, or
accessor. `AU2002` reports argument, constructor payload, return, or annotation
type mismatches. `AU2004` reports invalid arity, argument names, or
positional/named binding. Ordinary ownership diagnostics apply to moved
Values, Errors, strings, arrays, and objects.

Malformed syntax, an out-of-range number, excessive parse depth, and oversized
parse input return typed `json.Error` values and are not runtime diagnostics.
Parse allocation failure or a value beyond the shared 262,144-node
materialization limit traps with `AU4005`.

`json.dumps` traps with `AU4003` when indent is outside `0..=16` or a value
exceeds depth 128. It traps with `AU4001` for a NaN or infinite Float payload.
It traps with `AU4005` when the shared node budget is exceeded, output would
exceed 67,108,864 bytes, or a controlled conversion/output allocation fails.
These failures return no partial string.

## Backend Support

The MIR runtime and direct native backend use the same recursive enum
identity, numeric classification, error positions, duplicate-key behavior,
depth, node, and byte limits, key order, number spelling, escaping, indentation,
and diagnostic categories. For one input or value, both backends MUST produce
the same Aurora result and exact dump bytes.

Both backends use the same bounded codec service for `json.parse`. The direct
backend holds value-table read access only long enough to validate and copy
the shared source String; it does not hold that access while waiting for
service admission or completion.

Runtime, direct-codegen, analysis, language-server, fixture, and
executable-reference coverage maintain this surface across the two backends.

## Limits And Implementation-Defined Behavior

JSON numbers have only the specified `int64` and finite `float64`
representations. There is no arbitrary-precision integer, decimal, lossless
source-number token, or non-finite JSON encoding. Object keys are Strings.
The human-readable message carried by `Error.Syntax` may evolve; the error
variant, coordinate convention, and location are normative.

Parse and dump are whole-value operations. There is no incremental parser,
streaming encoder, caller-provided writer, configurable key order, alternate
escape mode, comments mode, trailing-comma mode, or configurable depth or byte
cap. The parser accepts at most 67,108,864 bytes and 128 container levels; dump
independently accepts the same maximum output size and value depth.
Parse/runtime materialization and dump/runtime conversion additionally accept
at most 262,144 JSON value nodes, counting the root and values but not object
keys.

The dynamic-`json.parse` codec service admits two operations process-wide. Its
two 2 MiB-stack workers are initialized lazily and intentionally live until
process exit; Aurora 0.1 has no codec-service shutdown, join, sizing, or
capacity configuration API. The service capacity does not govern
`json.is_valid`, `json.parse_string_map`, or `json.stringify_map`.

Derived class/enum schemas and generated codecs remain deferred beyond Phase 6.
Schema validation, MessagePack, CBOR, Protobuf, and other binary formats are
also unavailable.
Codec-controlled collection/string growth and both runtime conversion trees use
fallible allocation and map failure to `AU4005`. An unrecoverable host allocator
failure inside Rust, the operating system, or dependency-owned scratch work
remains external and can still terminate the process; the language does not
claim that every possible host out-of-memory condition is catchable.

## Status

The recursive value/error model, parse/dump surface, accessors, ordering,
formatting, and resource boundary are implemented Aurora 0.1 behavior. Their
exact gap-fill semantics are accepted under ADR-0021.

The older `is_valid`, `stringify_map`, and `parse_string_map` helpers remain
implemented compatibility surface. Streaming codecs remain future work.
