# Bytes, Text Codecs, And SHA-256

This page covers byte data, UTF-8 conversion, hexadecimal and base64 codecs,
and SHA-256 hashing.

Aura represents an owned sequence of bytes as `list[uint8]`. There is no
separate `Bytes` type and no implicit conversion between text and bytes.
UTF-8 conversion is a pair of `str` methods. Hexadecimal, base64, and SHA-256
live in the built-in `bytes` module.

## Public API

| API | Signature | Result |
| --- | --- | --- |
| `str.to_bytes` | `to_bytes() -> list[uint8]` | A fresh list holding the exact UTF-8 encoding of the receiver. |
| `str.from_bytes` | `from_bytes(bytes: list[uint8]) -> Result[str, bytes.Error]` | A fresh str when `bytes` is valid UTF-8, otherwise a typed error. |
| `bytes.hex_encode` | `hex_encode(value: list[uint8]) -> str` | Lowercase hexadecimal, two ASCII digits per byte. |
| `bytes.hex_decode` | `hex_decode(text: str) -> Result[list[uint8], bytes.Error]` | Strict hexadecimal decoding. |
| `bytes.base64_encode` | `base64_encode(value: list[uint8]) -> str` | RFC 4648 standard-alphabet base64 with canonical padding. |
| `bytes.base64_decode` | `base64_decode(text: str) -> Result[list[uint8], bytes.Error]` | Strict canonical RFC 4648 decoding. |
| `bytes.sha256` | `sha256(value: list[uint8]) -> list[uint8]` | The raw 32-byte SHA-256 digest of `value`. |
| `bytes.sha256_string` | `sha256_string(text: str) -> list[uint8]` | The raw SHA-256 digest of the exact UTF-8 bytes of the text. |

The parameter names shown are part of each signature, so you can pass them as
named arguments. Bare `str` and `list[uint8]` parameters are shared borrows,
the ordinary default. No call consumes or changes an input. Every returned
list or str is a fresh owned value.

`str.from_bytes` is an associated method, so you call it on the type:
`str.from_bytes(payload)`. It is not a `str(...)` constructor.

The parameter name `encoding` is reserved. Aura 0.3 accepts no encoding
argument on either conversion.

## Example

```aura
import bytes

def main():
    text = "Aura 🌌"
    encoded = text.to_bytes()
    print(bytes.hex_encode(encoded))

    match str.from_bytes(encoded):
        case Result.Ok(decoded):
            print(decoded)
        case Result.Err(error):
            print(error)

    payload: list[uint8] = [0, 1, 254, 255]
    print(bytes.base64_encode(payload))
    print(bytes.hex_encode(bytes.sha256_string("abc")))
    print(bytes.hex_encode(encoded))
```

The program prints, in order:

1. the UTF-8 bytes of `text` as lowercase hex
2. the original text
3. `AAH+/w==`
4. the standard SHA-256 digest of `abc`
5. the same UTF-8 hex again, which shows that the conversion did not consume
   `encoded`

## Error Model

Decoders return a `bytes.Error` for malformed input:

| Variant | Payload meaning |
| --- | --- |
| `InvalidUtf8(index: int32)` | `index` is the zero-based byte offset where the first invalid UTF-8 sequence starts. |
| `InvalidHexLength(length: int32)` | `length` is the odd UTF-8 byte length of the hexadecimal input. |
| `InvalidHexDigit(index: int32, byte: uint8)` | `index` is the position of the first non-hex byte, and `byte` is its exact value. |
| `InvalidBase64(index: int32)` | `index` is the first byte that breaks canonical base64. A missing required byte is reported at the position right after the input. |

All positions and lengths count bytes, not Unicode scalar values.

- Hex length is checked before the digits. An odd-length input returns
  `InvalidHexLength` even if it also contains a non-hex character.
- For base64, an invalid alphabet byte reports that byte. A missing required
  padding byte reports `text.byte_len()`. Nonzero discarded bits report the
  last data symbol that contains them.

The payload offsets and lengths are `int32`. When the exact offset or length
exceeds `2147483647`, Aura cannot build the payload without losing
information. That case traps with `AU4005`. Aura never truncates, clamps, or
wraps it into a `bytes.Error`.

Resource and allocation failures are not `bytes.Error` variants either. They
trap with `AU4005`, as described under [Diagnostics](#diagnostics).

## UTF-8 Conversion

`to_bytes` emits the standard UTF-8 encoding of every Unicode scalar in the
`str`.

- Embedded NUL bytes are kept.
- A leading U+FEFF is encoded as the ordinary bytes `ef bb bf`. Aura does not
  insert it, remove it, or treat it as a byte-order mark.
- There is no normalization, case folding, or newline replacement.

`from_bytes` validates strictly. It never inserts U+FFFD, and it never decodes
a valid prefix while discarding a malformed suffix. On success it keeps the
byte sequence exactly, including embedded NUL and a leading UTF-8 U+FEFF. So
when you match `str.from_bytes(text.to_bytes())`, the
`case Result.Ok(decoded):` branch always has `decoded == text`.

## Hexadecimal

Encoding emits exactly two lowercase ASCII digits per input byte, using `0`
through `9` and `a` through `f`. Empty input gives the empty `str`.

Decoding accepts lowercase or uppercase ASCII digits. It rejects a `0x`
prefix, signs, separators, whitespace, and non-ASCII digits. Empty text gives
an empty list. An even-length input is decoded from left to right, and the
first invalid byte produces `InvalidHexDigit`.

## Base64

Base64 uses the RFC 4648 standard alphabet: `A-Z`, `a-z`, `0-9`, `+`, and `/`.
Encoding always emits the canonical number of trailing `=` bytes. Empty input
gives empty text.

Decoding accepts only the standard alphabet with canonical padding. It rejects:

- the URL-safe characters `-` and `_`
- whitespace and separators
- missing padding and excess padding
- padding in a quartet that is not the last one
- data after padding
- nonzero discarded bits

The decoder does not skip or repair malformed bytes. Decoded output can hold
any bytes and need not be UTF-8.

## SHA-256

`bytes.sha256` is the FIPS 180-4 SHA-256 function. It returns a fresh list of
exactly 32 digest bytes. `bytes.sha256_string(text)` is exactly the same as
hashing `text.to_bytes()`. It adds no terminator and does no text
normalization or newline conversion.

The digest is raw bytes, not hexadecimal text. To get a text digest, compose
the calls: `bytes.hex_encode(bytes.sha256(payload))`.

SHA-256 is a general-purpose digest. It is not encryption, a message
authentication code, a signature, a password hash, a random generator, or a
constant-time comparison. This module makes no claim that it is fit for any of
those uses.

## Grammar

This surface adds no grammar. `list[uint8]`, `import bytes`, associated calls,
method calls, module calls, named arguments, and `Result` patterns use the
ordinary forms defined elsewhere in this Manual. Aura 0.3 has no byte-string
literal.

## Typing Rules

`list[uint8]` is the only built-in bytes representation. The signatures in
[Public API](#public-api) are normative. There is no implicit conversion
between `str` and a byte list, and no overload accepts another integer element
type.

`bytes.Error` is a copy-valued enum, because every payload is a copy type.

- Offsets and lengths are `int32`. This payload type is fixed and does not
  depend on the `str` and `list` length domains.
- The invalid hex byte payload is `uint8`.
- Required metadata above the `int32` maximum traps with `AU4005` instead of
  producing a lossy payload.
- Matching follows the ordinary exhaustive enum rules.

Every successful call returns an owned value. Bare inputs grant shared access
for the call, so you can reuse an input after `to_bytes`, `from_bytes`, an
encoder, a decoder, or a hash. These signatures neither require nor imply
`own`.

An `encoding` argument, positional or named, is not part of the 0.3
signature. Ordinary argument checking rejects it.

A source module you write whose last path component is `bytes` does not get
this API. The built-in behavior belongs only to the `bytes` module that the
compiler provides.

## Runtime Semantics

Every operation evaluates the receiver first, then the arguments in source
order. It reads the input as it is at that point and allocates a fresh result.
No operation changes an input list or str.

- UTF-8 validation reports where the first invalid sequence starts.
- Hex decoding checks for an even byte length first, then decodes pairs from
  left to right.
- Base64 decoding checks for the canonical standard-alphabet form. It has no
  whitespace-tolerant or unpadded mode.

SHA-256 follows FIPS 180-4 over the exact input bytes. Rendered through
`hex_encode`, the digest of empty input is
`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`, and the
digest of `abc` is
`ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.

## Ownership And Evaluation Order

Every input uses shared access, so each stays usable after the call:

- a `str` receiver after `to_bytes`
- a byte list after `from_bytes`, an encoder, or `sha256`
- a text argument after a decoder or `sha256_string`

A returned `str` or list never aliases mutable storage in the input.

Nested calls evaluate inside out. For example,
`bytes.hex_encode(bytes.sha256(payload))` first hashes a shared read of
`payload`, then passes the fresh digest to `hex_encode`. The caller still owns
`payload`. When arguments have other visible effects, they follow the
language-wide call order, which is source order.

## Diagnostics

Malformed UTF-8, hex, and base64 input returns a `bytes.Error` when the exact
offset or length fits the `int32` payload. These cases trap instead:

| Code | Cause |
| --- | --- |
| `AU4005` | A fresh codec output would exceed the fixed 2,147,483,647-byte safety ceiling. |
| `AU4005` | Computing the expanded output size overflows. |
| `AU4005` | Error metadata is above `2147483647` and does not fit the `int32` payload. |
| `AU4005` | Allocation fails. |

The output ceiling does not depend on the `str` and `list` length domains. A
trapping operation produces no partial result.

Static misuse reports the ordinary name, type, and argument codes, including
`AU2001`, `AU2002`, and `AU2004`.

## Backend Support

The MIR and direct backends implement the same UTF-8, hex, base64, and SHA-256
contract with the same strict codec policy. They must return identical bytes,
text, variants, offsets, and runtime diagnostics. The backend-parity fixture
matrix covers both valid and malformed inputs.

The compiler's analysis and the language server expose the same `bytes`
module, `bytes.Error` variants, `str` methods, named parameters, and return
types as the runtime.

## Limits And Implementation-Defined Behavior

Codec inputs have no separate byte-count cap. Each fresh `str` or
`list[uint8]` that a conversion, encoder, or decoder produces has a fixed
safety ceiling of 2,147,483,647 bytes. This ceiling does not depend on the
`str` and `list` length domains.

- Hex output needs `2 * input_length` bytes.
- Padded base64 output needs `4 * ceil(input_length / 3)` bytes.

Each operation checks the output size before it allocates. An output exactly
at the ceiling is accepted when allocation succeeds. The first larger output
traps with `AU4005`. Inputs can be larger than a `bytes.Error` payload can
describe, so malformed input whose exact offset or length exceeds
`2147483647` also traps with `AU4005`.

Whether an allocation within the ceiling succeeds depends on the host. SHA-256
output is always 32 bytes. Codec output, errors, and offsets do not depend on
the host.

Aura 0.3 does not provide:

- other text encodings
- URL-safe or unpadded base64
- streaming codecs or incremental hashing
- HMAC or password hashing
- constant-time digest comparison
- a separate mutable byte buffer type
- implicit text conversion

The reserved `encoding` parameter is not implemented.

## Status

`list[uint8]` is the Aura 0.3 bytes type, and the conversion, codec, error,
and hash behavior on this page is implemented. Aura 0.3 does not derive codecs
or schemas for classes and enums.

Design record: [ADR-0023: Byte-vector codecs and hashing policy](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0023-byte-vector-codecs-and-hashing-policy.md).
