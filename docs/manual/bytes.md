# Bytes, Text Codecs, And SHA-256

Aurora represents an owned sequence of bytes as `Vec[uint8]`. There is no
separate `Bytes` nominal type and no implicit conversion between text and
bytes. UTF-8 conversion is available on `String`; hexadecimal, base64, and
SHA-256 operations live in the built-in `bytes` module.

## Public API

| API | Signature | Result |
| --- | --- | --- |
| `String.to_bytes` | `to_bytes() -> Vec[uint8]` | A fresh vector containing the receiver's exact UTF-8 encoding. |
| `String.from_bytes` | `from_bytes(bytes: Vec[uint8]) -> Result[String, bytes.Error]` | A fresh String when `bytes` is valid UTF-8, otherwise a typed error. |
| `bytes.hex_encode` | `hex_encode(value: Vec[uint8]) -> String` | Lowercase hexadecimal with two ASCII digits per byte. |
| `bytes.hex_decode` | `hex_decode(text: String) -> Result[Vec[uint8], bytes.Error]` | Strict hexadecimal decoding. |
| `bytes.base64_encode` | `base64_encode(value: Vec[uint8]) -> String` | RFC 4648 standard-alphabet base64 with canonical padding. |
| `bytes.base64_decode` | `base64_decode(text: String) -> Result[Vec[uint8], bytes.Error]` | Strict canonical RFC 4648 decoding. |
| `bytes.sha256` | `sha256(value: Vec[uint8]) -> Vec[uint8]` | The raw 32-byte SHA-256 digest of `value`. |
| `bytes.sha256_string` | `sha256_string(text: String) -> Vec[uint8]` | The raw SHA-256 digest of the text's exact UTF-8 bytes. |

The displayed parameter names are part of the callable contract and may be
used as named arguments. Bare `String` and `Vec[uint8]` parameters use the
ordinary shared-borrow default. None of these calls consumes or mutates an
input, and every returned collection or String is a fresh owned value.

`String.from_bytes` is an associated String method, so it is called on the
type, as in `String.from_bytes(payload)`. It is not a `String(...)`
constructor. The `encoding` parameter name is reserved for a possible future
extension; Aurora 0.1 accepts no encoding argument on either String conversion.

## Error Model

Malformed input is recoverable when its required offset or length fits the
retained `int32` error-payload domain, and returns one of these `bytes.Error`
variants:

| Variant | Payload meaning |
| --- | --- |
| `InvalidUtf8(index: int32)` | `index` is the zero-based byte offset at which the first invalid UTF-8 sequence begins. |
| `InvalidHexLength(length: int32)` | `length` is the odd UTF-8 byte length of the hexadecimal input. |
| `InvalidHexDigit(index: int32, byte: uint8)` | `index` identifies the first non-hex byte and `byte` is its exact value. |
| `InvalidBase64(index: int32)` | `index` identifies the first byte that violates canonical base64; a missing required byte is reported at the position immediately after the input. |

All positions and lengths are byte counts, not Unicode-scalar positions.
Hexadecimal length is validated before individual digits, so an odd input
returns `InvalidHexLength` even when it also contains a non-hex character.
For base64, an invalid alphabet byte reports that byte, a missing required
padding byte reports `text.byte_len()`, and nonzero discarded bits report the
last data symbol that contains them.

If the exact malformed-data offset or length exceeds `2147483647`, Aurora
cannot construct the retained `int32` payload without losing information. That
metadata overflow traps with `AU4005`; it is never truncated, clamped, or
wrapped into a `bytes.Error`. Resource or allocation failure likewise is not a
`bytes.Error` variant and traps with `AU4005` as described below.

## UTF-8 Conversion

`to_bytes` emits the standard UTF-8 encoding of every Unicode scalar in the
String. Embedded NUL bytes are preserved. A leading U+FEFF is encoded as the
ordinary bytes `ef bb bf`; it is not inserted, removed, or treated as a
byte-order marker. Conversion performs no normalization, case folding, or
newline replacement.

`from_bytes` validates strictly. It never inserts U+FFFD and never decodes a
prefix while discarding a malformed suffix. On success it preserves the byte
sequence exactly, including embedded NUL and a leading UTF-8 encoding of
U+FEFF. After matching `String.from_bytes(text.to_bytes())`, a successful
`case Result.Ok(decoded):` branch therefore satisfies `decoded == text`.

## Hexadecimal

Hex encoding emits exactly two lowercase ASCII digits for each input byte,
using `0` through `9` and `a` through `f`. Empty input produces the empty
String.

Hex decoding accepts either lowercase or uppercase ASCII digits. It does not
accept a `0x` prefix, signs, separators, whitespace, or non-ASCII digits.
Empty text produces an empty vector. An even-length input is processed from
left to right, and the first invalid byte determines `InvalidHexDigit`.

## Base64

Base64 uses the RFC 4648 standard alphabet `A-Z`, `a-z`, `0-9`, `+`, and `/`.
Encoding always emits the canonical number of trailing `=` bytes. Empty input
produces empty text.

Decoding accepts only that standard alphabet and canonical padding. It rejects
the URL-safe `-` and `_` characters, whitespace, separators, omitted padding,
excess padding, padding in a non-final quartet, data after padding, and
nonzero discarded bits. It does not ignore malformed bytes and does not
silently repair input. Successfully decoded output may contain arbitrary
bytes and is not required to be UTF-8.

## SHA-256

`bytes.sha256` is the FIPS 180-4 SHA-256 function. It returns a fresh vector of
exactly 32 digest bytes. `bytes.sha256_string(text)` is exactly equivalent to
hashing `text.to_bytes()`; it adds no terminator and performs no text
normalization or newline conversion.

The digest is raw bytes, not hexadecimal text. Compose the operations when a
text digest is needed:
`bytes.hex_encode(bytes.sha256(payload))`.

SHA-256 is a general-purpose digest. It is not encryption, a message
authentication code, a signature, a password hash, a random generator, or a
constant-time equality operation. This module does not imply suitability for
any of those uses.

## Example

```aurora
import bytes

def main():
    text = "Aurora 🌌"
    encoded = text.to_bytes()
    print(bytes.hex_encode(encoded))

    match String.from_bytes(encoded):
        case Result.Ok(decoded):
            print(decoded)
        case Result.Err(error):
            print(error)

    payload: Vec[uint8] = [0, 1, 254, 255]
    print(bytes.base64_encode(payload))
    print(bytes.hex_encode(bytes.sha256_string("abc")))
    print(bytes.hex_encode(encoded))
```

The program prints the UTF-8 bytes as lowercase hex, the original text,
`AAH+/w==`, the standard SHA-256 digest of `abc`, and the same UTF-8 hex again.
The final line demonstrates that conversion did not consume `encoded`.

## Grammar

The Bytes surface adds no source-language grammar. `Vec[uint8]`, `import
bytes`, associated calls, method calls, module calls, named arguments, and
`Result` patterns use the ordinary forms defined elsewhere in this Manual.
Aurora 0.1 has no byte-string literal.

## Typing Rules

`Vec[uint8]` is the sole built-in bytes representation. The signatures in the
Public API table are normative. There is no implicit `String`/byte-vector
coercion and no overload that accepts another integer element type.

`bytes.Error` is a copy-valued enum because all of its payloads are copy
types. Its offsets and lengths remain `int32` as the current error-payload
compatibility contract; that fixed payload type is independent of the public
String and `Vec` length domains. The invalid hexadecimal byte payload is
`uint8`. Required malformed-data metadata above the `int32` maximum traps with
`AU4005` instead of constructing a lossy payload. Match handling follows the
ordinary exhaustive enum rules.

All successful functions return owned values. Ordinary bare inputs grant
shared access for the call, so a caller may reuse the input after `to_bytes`,
`from_bytes`, encode, decode, or hash. Explicit `own` is neither required nor
implied by these signatures.

An `encoding` positional or named argument is not part of the 0.1 signature
and is rejected by ordinary argument checking. A user-defined source module
whose final component is named `bytes` does not acquire this built-in API:
built-in behavior belongs only to the compiler-synthesized `bytes` module.

## Runtime Semantics

All operations first evaluate the receiver, then supplied arguments in source
order. They observe the input value produced at that point and allocate a
fresh result. No operation changes an input vector or String.

UTF-8 validation returns the first invalid sequence start. Hex decoding first
checks even byte length, then decodes pairs from left to right. Base64
decoding validates the canonical standard-alphabet representation rather than
using a whitespace-tolerant or unpadded mode.

SHA-256 follows FIPS 180-4 over the exact input byte sequence. In particular,
the digest of empty input is
`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`
when rendered through `hex_encode`, and the digest of `abc` is
`ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.

## Ownership And Evaluation Order

Every public input uses shared access. A String receiver remains usable after
`to_bytes`, and a byte vector remains usable after `from_bytes`, an encoder,
or `sha256`. A text argument remains usable after a decoder or
`sha256_string`. Returned Strings and vectors do not alias mutable storage in
the input.

Nested calls use ordinary inside-out evaluation. For example,
`bytes.hex_encode(bytes.sha256(payload))` first hashes a shared observation of
`payload`, then passes the fresh digest to `hex_encode`; `payload` remains
owned by the caller. When arguments have other observable effects, their
source order remains the language-wide call order.

## Diagnostics

Malformed UTF-8, hex, and base64 return `bytes.Error` when the exact error
offset or length fits its retained `int32` payload. Required metadata above
`2147483647` traps with `AU4005` rather than emitting a truncated or wrapped
typed error. Static misuse uses the ordinary name/type/argument codes,
including `AU2001`, `AU2002`, and `AU2004`.

`AU4005` reports a fresh codec destination above the fixed
2,147,483,647-byte safety ceiling, arithmetic overflow while computing the
expanded destination size, error metadata outside the retained `int32` payload
domain, or allocation failure. This codec output/resource boundary is
independent of the public String and `Vec` length domains. The operation
produces no partial successful value.

## Backend Support

The MIR and direct backends implement the same UTF-8, hex, base64, and SHA-256
contract and must return identical bytes, text, variants, offsets, and runtime
diagnostics. Both backends use the same strict codec policy. The maintained
backend-parity fixture matrix covers successful and malformed inputs.

Compiler analysis and the language server expose the same canonical `bytes`
module, `bytes.Error` variants, String methods, named parameters, and return
types as the runtime surface.

## Limits And Implementation-Defined Behavior

Codec inputs have no separate byte-count cap. Each fresh String or
`Vec[uint8]` destination produced by a byte conversion, encoder, or decoder has
a fixed safety ceiling of 2,147,483,647 bytes. This is a codec output/resource
boundary independent of the public String and `Vec` length domains. Hex output
requires `2 * input_length` bytes. Padded base64 output requires
`4 * ceil(input_length / 3)` bytes. Operations preflight the destination size
before allocating; a destination exactly at the ceiling is accepted when
allocation succeeds, and the first larger destination traps with `AU4005`.
Because the input domain is wider than a `bytes.Error` payload, malformed input
whose exact reported offset or length exceeds `2147483647` also traps with
`AU4005`.

Actual allocation success within the codec destination ceiling is
host-dependent. SHA-256 output is always 32 bytes. Codec output, errors, and
offsets are not host-dependent.

Aurora 0.1 does not provide alternate text encodings, URL-safe or unpadded
base64, streaming codecs, incremental hashing, HMAC, password hashing,
constant-time digest comparison, a distinct mutable byte buffer, or implicit
String conversion. The reserved `encoding` parameter is not implemented.

## Status

`Vec[uint8]` is the implemented Aurora 0.1 bytes type. The conversion, codec,
error, and hash policy on this page is implemented as the Phase 3 control-plane
surface and is accepted under ADR-0023. Derived class/enum codecs and schemas
remain deferred beyond Phase 6.
