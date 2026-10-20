# Batch 6 design checkpoint: typed agent APIs and serialization

Status: **Ratified 2026-10-19 by the owner**

Date: 2026-10-19. Source baseline: `8c31f3f` (main after the
[0.5.0 preview release](../work/2026-10-19-v0.5.0-preview-release.md)).

This checkpoint plans
[roadmap Batch 6](14-priority-roadmap.md#priority-batches). It has two halves:

- typed JSON codecs, validation, and schemas for classes and enums, under
  [ADR-0062](decisions/0062-typed-serialization-validation-and-schemas.md)
- function decorators, under
  [ADR-0053](decisions/0053-function-decorators.md)

Each section states the rule the checkpoint proposes, then the evidence the
implementation must pin. [Owner Questions](#owner-questions) lists the choices
that change user-facing behavior. Every other rule is the proposed default.

## Starting Point

A program that moves a class through JSON today writes both directions by
hand. `ToolRequest` in `examples/agents/tool_runner/src/main.au` spends 19
lines on two fields, and its decoder reports only `InvalidRequest`, with no
field name. Nothing produces a JSON Schema, so an agent tool must write its
input schema as a string that can drift from the decoder.

The compiler already has the pieces a generator needs.

| Piece | Where | Use in Batch 6 |
| --- | --- | --- |
| Traits with default and associated methods | `sema/traits.rs`, `sema/display.rs` | `json.Codec` is a prelude trait like `Display` |
| Generated MIR functions | default-argument helpers and `render` wrappers in `mir.rs` | derived `encode`, `decode`, and `schema` bodies |
| Field and class docstrings | `FieldDecl.doc`, `ClassDecl.doc` in `ast.rs` | schema `description` values |
| Eagerly initialized module constants | `build_constant_init_plan` in `lib.rs`, `aura_direct_module_constant` | decorated function bindings |
| `@property` marker | `parse_property_marker` in `parser.rs` | the `@` line grammar that decorators extend |
| Owned callables and closures | `Type::Callable`, `Type::Closure` in `aura-core/src/types.rs` | decorator results |
| `json.Value`, `json.Error`, codec limits | `builtin_modules.rs`, `aura-runtime/src/json_codec.rs` | the wire tree and the size and depth limits |

Two gaps matter. A generic function cannot call an associated method through
a type parameter, such as `T.decode(...)`. ADR-0053 also rejects a generic
decorator, so a library cannot write one `@retry` for every signature. Both
are addressed below.

## Goals And Non-Goals

Goals:

- one opt-in line gives a class or enum a JSON encoder, decoder, and schema
- the schema accepts exactly what the decoder accepts
- a decode failure names the field path, such as `$.items[2].name`
- a failed decode cleans up what it built exactly once
- decorators wrap free functions with an exact signature, in a defined order
- a library can define `@trace` and `@retry` once for many signatures

Non-goals:

- formats other than JSON, such as TOML or MessagePack
- runtime reflection, attribute lookup by string, or a global registry
- value constraints such as `min_length` or regular expressions in metadata
- decorators on classes, methods, fields, or parameters
- macros or source rewriting

## A. Opting In

A class or enum opts in with an empty implementation of the prelude trait
`json.Codec`:

```aura fragment
import json

class SearchArgs:
    """Arguments for the search tool."""

    query: str
    """Text to search for."""

    limit: int64 = 10

impl json.Codec for SearchArgs
```

The trait has three members:

```aura fragment
trait Codec:
    def encode(self) -> json.Value
    def decode(value: own json.Value) -> Result[Self, json.DecodeError]
    def schema() -> json.Value
```

An implementation with no body asks the compiler to derive all three. An
implementation with a body must write all three, and gets no derived member.
The derived form is allowed only in the module that declares the type, so a
derived decoder can set private fields only where that module already can.

Generic entry points dispatch on the concrete type:

| Function | Signature |
| --- | --- |
| `json.encode` | `encode[T: json.Codec](value: T) -> json.Value` |
| `json.decode` | `decode[T: json.Codec](value: own json.Value) -> Result[T, json.DecodeError]` |
| `json.decode_text` | `decode_text[T: json.Codec](text: str) -> Result[T, json.DecodeError]` |
| `json.schema` | `schema[T: json.Codec]() -> json.Value` |

`json.decode[SearchArgs](value)` and `SearchArgs.decode(value)` mean the same
call. Supporting the generic form needs associated-method calls through a
bounded type parameter, which the checker lacks today. Step 1 adds them for
every trait, not only `json.Codec`.

**Evidence.** Opt-in and opt-out fixtures, a derived implementation outside
the declaring module rejected, a partial explicit body rejected, and
`T.decode` through a bound on both backends.

## B. What Can Be Encoded

A derived codec accepts these field types. Any other field type rejects the
`impl` with a diagnostic that names the field path and the type.

| Aura type | JSON |
| --- | --- |
| `bool` | `true` or `false` |
| signed and unsigned integers | number with no fraction, range-checked on decode |
| `float32`, `float64` | number. NaN and infinities fail to encode |
| `str` | string |
| `list[T]` | array |
| `dict[str, T]` | object. Other key types are rejected |
| tuples | fixed-length array |
| `T \| None` | the value of `T`, or `null` |
| a union of members with different JSON kinds, such as `int64 \| str` | the member's value |
| a class or enum that implements `json.Codec` | its encoding |
| `json.Value` | passed through unchanged |

Rejected: sets, `Array`, `bytes`, callables, tasks, queues, files, processes,
sockets, and unions whose members share a JSON kind, such as two classes.
Recursive types are allowed through `list`, `dict`, or `| None`. The existing
depth limit of 128 bounds them.

### Encoding

`encode(self)` reads its receiver and builds a new `json.Value` tree. It
copies strings and collections into the tree. It never moves or clones a
field in place, so the value stays usable. A class encodes as an object with
one key per field, in declaration order. `None` encodes as `null`; a field is
never omitted.

### Enums

- An enum whose variants all lack payloads encodes as the variant name, such
  as `"Ready"`.
- Any other enum encodes as an object with a `"type"` key naming the variant
  and one key per named payload, such as `{"type": "Failed", "reason": "x"}`.
  A unit variant in such an enum encodes as `{"type": "Empty"}`.
- A positional payload, such as `Failed(str)`, has no key name, so a derived
  codec rejects it. The diagnostic suggests `Failed(reason: str)`.

**Evidence.** Round trips for every row of the table, rejection fixtures for
each excluded type, and identical bytes from `json.dumps` on both backends.

## C. Decoding Rules

The decoder is strict. It takes the `json.Value` by ownership, so it moves
strings and collections into the result instead of copying them. Each rule
below is an observable contract.

| Input | Result |
| --- | --- |
| a key for every field | the value, built with automatic field construction |
| a missing key for a field with a declaration default | the default, evaluated in declaration order |
| a missing key for a field without a default | `MissingField` |
| `null` for a `T \| None` field | `None` |
| a missing key for a `T \| None` field without a default | `MissingField`. Write `= None` to make it optional |
| a key that names no field | `UnknownField` |
| the wrong JSON kind | `WrongType` with the expected and found kinds |
| an integer outside the field type's range | `OutOfRange` |
| an integer for a float field | accepted when it converts exactly |
| a float for an integer field, including `1.0` | `WrongType` |
| an unknown enum tag or variant name | `UnknownVariant` |

There is no other coercion. A string never becomes a number, and a number
never becomes a string.

### Errors

```aura fragment
enum DecodeErrorKind:
    Parse(error: json.Error)
    MissingField(name: str)
    UnknownField(name: str)
    WrongType(expected: str, found: str)
    OutOfRange(type_name: str)
    UnknownVariant(name: str)
    Invalid(message: str)

class DecodeError:
    path: str
    kind: DecodeErrorKind
```

`path` uses `$` for the root, `.name` for a key, and `[3]` for an array
position. `decode_text` reports a parse failure as `Parse` with path `$`. An
explicit decoder reports its own checks as `Invalid`.

The decoder stops at the first error in document order. This keeps the
result deterministic and the cost bounded.

### Classes With An Initializer

A class with `__init__` can compute fields, and stored fields cannot be
turned back into initializer arguments in general. A derived codec on such a
class is rejected. The class writes an explicit implementation instead. The
usual shape decodes a plain wire class and calls the initializer:

```aura fragment
class UserWire:
    name: str
    age: int64

impl json.Codec for UserWire

class User:
    name: str
    age: int64

    def __init__(self, name: str, age: int64):
        self.name = name.strip()
        self.age = age

impl json.Codec for User:
    def encode(self) -> json.Value:
        return json.encode(UserWire(name=self.name.clone(), age=self.age))

    def decode(value: own json.Value) -> Result[User, json.DecodeError]:
        match own json.decode[UserWire](value):
            case Result.Ok(wire):
                return Result.Ok(User(wire.name, wire.age))
            case Result.Err(error):
                return Result.Err(error)

    def schema() -> json.Value:
        return json.schema[UserWire]()
```

### Cleanup

A derived decoder decodes each field into a local, then constructs the value
once every field is present. A failure returns early, and the locals already
built drop as ordinary owned values, exactly once. This is the same rule the
Batch 3 initializer uses, so no new exit action is needed.

**Evidence.** One fixture per table row, field paths through nested lists,
dicts, enums, and recursive classes, a decoder that fails after building a
`list` field, and both backends.

## D. Schemas

`schema()` returns a JSON Schema 2020-12 document as a `json.Value`. A derived
schema follows the decoder rules exactly:

- an object has `"additionalProperties": false`
- `required` lists the fields without defaults
- a `T | None` field is `{"anyOf": [<T>, {"type": "null"}]}`
- integer fields carry `minimum` and `maximum` for their range
- a unit enum is `{"enum": [...]}`, and a payload enum is a `oneOf` keyed by
  `"type"` with a `const`
- a field or class docstring becomes `description`
- each named class or enum appears once under `$defs` and is referenced
  with `$ref`, which also covers recursion

The schema is the same for every call, and the compiler may build it once.

A test generates the schema for every codec fixture and checks that a
JSON Schema validator accepts exactly the inputs the decoder accepts.

**Evidence.** Golden schemas for each fixture class, the agreement test, and
descriptions taken from docstrings.

## E. Decorators

This section resolves the items ADR-0053 left open. Its other rules stand.

### E1. Order

Decorator expressions evaluate once, top to bottom. Their results apply
bottom to top, so the decorator nearest the definition wraps first. This is
the ADR-0053 baseline and matches Python.

### E2. Initialization And Recursion

A decorated function is a module binding initialized with the module's
constants, in declaration order. The binding's slot exists before its
decorators run. A recursive call inside the body goes through the slot, so
it calls the decorated function. A decorator that reads the slot during its
own initialization gets `AU2001`, the error for reading a module constant
before it is initialized.

### E3. Generic Decorators

ADR-0053 rejects a generic decorator. Without one, `@retry` must be written
again for each signature, which defeats the feature. This checkpoint allows
a generic decorator whose type parameters are inferred from the decorated
function at each use:

```aura fragment
def traced[A, R](f: def(args: A) -> R) -> def(args: A) -> R:
    return lambda args: trace_call(f, args)

@traced
def search(args: SearchArgs) -> Result[SearchHits, ToolError]:
    ...
```

Inference runs once per use, against the concrete function type. The result
must still be exactly that type. This adds no higher-kinded types. A type
parameter stands for a value type, never for a whole signature. The decorated
definition itself stays non-generic.

### E4. Registration

Aura has no mutable module state, and Batch 6 does not add any. A decorator
cannot add a function to a global table. A program lists its tools or routes
explicitly, and the list carries each tool's schema:

```aura fragment
tools = [agent.tool("search", search), agent.tool("fetch", fetch)]
```

`agent.tool` is ordinary library code in the reference agent example. Given
a function whose one argument implements `json.Codec`, it can store
`json.schema[A]()` beside the function.

### E5. Retry

`control.retry` already retries a shared worker. A `@retry` decorator built
on it accepts only functions whose parameters are shared or Copy. A function
with an `own` parameter fails at the decorator with a diagnostic that points
to an explicit loop, because a consumed argument cannot be reused. This keeps
the ADR-0053 rule that retry ownership is explicit.

### E6. Tooling

Hover on a decorated function shows its decorators above the signature.
Go-to-definition works on each decorator expression. Docs from `aura doc`
keep the function's docstring. The semantic interface schema moves to 20,
and the native cache format to `v8`.

**Evidence.** The ADR-0053 completion matrix, plus generic inference per use,
a mismatched inferred result, `@retry` on an `own` parameter, and recursion
through the decorated slot on both backends.

## F. Delivery Plan

Each step is one pull request with its tests, docs, and examples.

| Step | Work |
| --- | --- |
| 1 | Associated-method calls through a bounded type parameter, for every trait |
| 2 | `json.Codec` prelude trait, explicit implementations, `json.encode`, `json.decode`, `json.schema` dispatch, and `json.DecodeError` |
| 3 | Derived `encode` and `decode` for classes, with field paths and cleanup |
| 4 | Derived codecs for enums and unions, and `json.decode_text` |
| 5 | Derived schemas with docstring descriptions, and the schema agreement test |
| 6 | Decorator syntax, concrete decorators, order, and recursion through the slot |
| 7 | Generic decorators inferred per use, and the `@retry` ownership rule |
| 8 | Reference agent example rewritten with codecs, schemas, and `@traced`. Tutorial chapter and Manual pages |

Steps 1 to 5 do not depend on steps 6 and 7. The two halves can run in
parallel after step 1.

## Owner Questions

None of these block the start. Each names the recommended default first.

1. **Opt-in spelling.** An empty `impl json.Codec for T`, as in
   [section A](#a-opting-in). Recommended, because it reuses traits and
   needs no new syntax. The alternative is a class decorator such as
   `@json.codec`, which ADR-0053 excludes today.
2. **Unknown keys.** Reject them with `UnknownField`. Recommended, because
   a misspelled or invented argument from a model fails loudly. The
   alternative ignores unknown keys.
3. **Absent optional fields.** A `T | None` field without a default is
   required, and the key may hold `null`. Recommended, because one rule
   decides what is required. The alternative treats an absent key as `None`.
4. **Error reporting.** Stop at the first error. Recommended. The
   alternative collects every error, which costs more and makes the order
   part of the contract.
5. **Enum tag key.** `"type"`. Recommended, because tool-calling APIs use it.
   The alternative is `"kind"` or a per-enum choice.
6. **Generic decorators.** Allow them, inferred per use, as in
   [E3](#e3-generic-decorators). Recommended. The alternative keeps ADR-0053's
   concrete-only rule and drops `@retry` and `@traced` as library decorators.
7. **Registration.** Explicit lists, as in [E4](#e4-registration).
   Recommended. The alternative is a compiler-collected registry, which needs
   a separate design for mutable module state.

## Ratification Record

The owner ratified this checkpoint on 2026-10-19 and approved all seven
recommendations:

1. An empty `impl json.Codec for T` opts a type in.
2. Decoding rejects unknown keys.
3. A `T | None` field without a default is required, and the key may hold
   `null`.
4. Decoding stops at the first error, which carries its `$.path`.
5. The enum tag key is `"type"`.
6. Generic decorators are allowed, inferred per use.
7. Registration uses explicit lists, with no compiler-collected registry.

Every other rule above is the ratified default.

## Implementation Notes

Step 6 types the decorated binding as `Callable[def(...) -> R]`, not the thin
`def` type the E3 example shows. A thin `def` value cannot carry a closure, so
a wrapper could not capture the function it wraps. A decorator takes
`own Callable[...]` and returns the same type. A closure cannot promise
parameter defaults, so a decorated function cannot declare one.
