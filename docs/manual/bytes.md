# Bytes, Text Codecs, And SHA-256

This page covers byte data, UTF-8 conversion, byte search, hexadecimal and
base64 codecs, SHA-256 hashing, and HMAC-SHA256 message authentication.

Aura represents an owned sequence of bytes as `list[uint8]`. There is no
separate `Bytes` type and no implicit conversion between text and bytes.
A byte string, `b"..."`, is a literal of that type. UTF-8 conversion is a pair
of `str` methods. Other text encodings, byte search, hexadecimal, base64, and
SHA-256 live in the built-in `bytes` module.

## Public API

| API | Signature | Result |
| --- | --- | --- |
| `str.to_bytes` | `to_bytes() -> list[uint8]` | A fresh list holding the exact UTF-8 encoding of the receiver. |
| `str.from_bytes` | `from_bytes(bytes: list[uint8]) -> Result[str, bytes.Error]` | A fresh str when `bytes` is valid UTF-8, otherwise a typed error. |
| `bytes.encode` | `encode(text: str, encoding: str) -> Result[list[uint8], bytes.EncodingError]` | A fresh list holding `text` in the named encoding. |
| `bytes.decode` | `decode(value: list[uint8], encoding: str) -> Result[str, bytes.EncodingError]` | A fresh str when `value` is valid in the named encoding, otherwise a typed error. |
| `bytes.hex_encode` | `hex_encode(value: list[uint8]) -> str` | Lowercase hexadecimal, two ASCII digits per byte. |
| `bytes.hex_decode` | `hex_decode(text: str) -> Result[list[uint8], bytes.Error]` | Strict hexadecimal decoding. |
| `bytes.base64_encode` | `base64_encode(value: list[uint8]) -> str` | RFC 4648 standard-alphabet base64 with canonical padding. |
| `bytes.base64_decode` | `base64_decode(text: str) -> Result[list[uint8], bytes.Error]` | Strict canonical RFC 4648 decoding. |
| `bytes.sha256` | `sha256(value: list[uint8]) -> list[uint8]` | The raw 32-byte SHA-256 digest of `value`. |
| `bytes.sha256_string` | `sha256_string(text: str) -> list[uint8]` | The raw SHA-256 digest of the exact UTF-8 bytes of the text. |
| `bytes.hmac_sha256` | `hmac_sha256(key: list[uint8], message: list[uint8]) -> list[uint8]` | The raw 32-byte HMAC-SHA256 of `message` under `key`. |
| `bytes.constant_time_eq` | `constant_time_eq(left: list[uint8], right: list[uint8]) -> bool` | `true` when the two lists hold the same bytes, in time that does not depend on where they differ. |
| `bytes.starts_with` | `starts_with(value: list[uint8], prefix: list[uint8]) -> bool` | `true` when `value` begins with `prefix`. |
| `bytes.ends_with` | `ends_with(value: list[uint8], suffix: list[uint8]) -> bool` | `true` when `value` ends with `suffix`. |
| `bytes.find` | `find(value: list[uint8], needle: list[uint8]) -> int64 \| None` | The byte index of the first occurrence of `needle`, or `None`. |

The parameter names shown are part of each signature, so you can pass them as
named arguments. Bare `str` and `list[uint8]` parameters are shared borrows,
the ordinary default. No call consumes or changes an input. Every returned
list or str is a fresh owned value.

`str.from_bytes` is an associated method, so you call it on the type:
`str.from_bytes(payload)`. It is not a `str(...)` constructor.

`to_bytes` and `from_bytes` are always UTF-8 and take no encoding argument.
For another encoding use `bytes.encode` and `bytes.decode`, described under
[Text Encodings](#text-encodings).

## Byte Strings

A byte string is a literal of type `list[uint8]`:

```aura
import bytes

def main():
    line_end = b"\r\n"
    magic = b"\x89PNG"
    quote = b'say "hi"'
    print(len(line_end))
    print(bytes.hex_encode(magic))
    print(len(quote))
```

- The body holds printable ASCII characters. Each is its ASCII byte.
- The escapes are those of an ordinary string except `\u{...}`: `\n`, `\t`,
  `\r`, `\0`, `\\`, `\"`, `\'`, and `\xHH`. In a byte string `\xHH` is the
  byte `HH`, for every value from `00` to `ff`.
- A character outside ASCII, a `\u{...}` escape, and a triple-quoted byte
  string are lexical errors, `AU1001`. Write a non-ASCII character as its
  bytes, or use `"é".to_bytes()`.
- There is no raw byte string and no byte f-string.

Each evaluation produces a fresh list, as a list literal does, so a byte
string bound with `mut` can be appended to.

## Text Encodings

`bytes.encode` and `bytes.decode` convert between `str` and bytes in a named
encoding:

<<< ../../examples/bytes/byte_strings_and_encodings.au{aura}

This program prints:

```text
16
true
00ff
utf-8: 636166c3a9
latin-1: 636166e9
utf-16-le: 630061006600e900
ascii: cannot hold character 3
no encoding named ebcdic
“quoted”
```

| Name | Encoding |
| --- | --- |
| `utf-8` | UTF-8, the same bytes as `to_bytes`. |
| `utf-8-sig` | UTF-8 with a byte-order mark. Encoding writes the mark. Decoding strips one if present. |
| `utf-16` | UTF-16. Encoding writes a byte-order mark and little-endian units. Decoding reads the order from the mark, and little-endian when there is none. |
| `utf-16-le`, `utf-16-be` | UTF-16 in a fixed byte order. No mark is written, and none is stripped. |
| `utf-32` | UTF-32, with the same mark rules as `utf-16`. |
| `utf-32-le`, `utf-32-be` | UTF-32 in a fixed byte order. |
| `latin-1` | ISO-8859-1. Byte `HH` is the character `U+00HH`. |
| `ascii` | Seven-bit ASCII. |
| `windows-1252` | The Windows Western European code page. |

A name is compared without regard to letter case, and `-`, `_`, and spaces
are ignored, so `"UTF-16"`, `"utf_16"`, and `"utf16"` are one encoding.

`bytes.EncodingError` has three variants:

| Variant | Meaning |
| --- | --- |
| `Unknown(name: str)` | No encoding has this name. |
| `Invalid(index: int64)` | Decoding failed. `index` is the offset of the first byte that is not valid. |
| `Unencodable(index: int64)` | Encoding failed. `index` is the position, in characters, of the first character the encoding cannot hold. |

Decoding is strict. There is no replacement character and no option to skip
invalid input. A truncated UTF-16 or UTF-32 unit, an unpaired surrogate, a
value above `U+10FFFF`, a byte above `7f` in ASCII, and one of the five
unassigned Windows-1252 bytes are all `Invalid`.

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

## Message Authentication

`bytes.hmac_sha256` computes HMAC (RFC 2104) with SHA-256. Use it to sign a
request or a webhook payload with a shared secret, and to check a signature
you receive:

```aura
import bytes

def main():
    secret = "Jefe".to_bytes()
    body = "what do ya want for nothing?".to_bytes()
    signature = bytes.hmac_sha256(secret, body)
    print(bytes.hex_encode(signature))

    received = signature
    print(bytes.constant_time_eq(bytes.hmac_sha256(secret, body), received))
```

The program prints
`5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843`, which is
test case 2 of RFC 4231, and `true`.

- The key may have any length, including zero. A key longer than 64 bytes is
  hashed with SHA-256 first, as the RFC says.
- The result is always 32 bytes. Encode it with `bytes.hex_encode` or
  `bytes.base64_encode` to put it in a header.
- Compare a received signature with `bytes.constant_time_eq`, not with `==`.
  `==` stops at the first byte that differs, so the time it takes tells an
  attacker how much of a forged signature was right. `constant_time_eq` reads
  every byte of two lists of equal length before it answers. Lists of
  different lengths compare unequal at once, so the lengths themselves are
  not secret.

## Byte Search

`bytes.starts_with`, `bytes.ends_with`, and `bytes.find` compare raw bytes.
They need no UTF-8 and do no normalization:

```aura
import bytes

def main():
    packet = "AURA\x00\x01payload".to_bytes()
    print(bytes.starts_with(packet, "AURA".to_bytes()))
    print(bytes.ends_with(packet, "load".to_bytes()))
    match bytes.find(packet, [0, 1]):
        case None:
            print("no header end")
        case int64 as at:
            print(at)
```

The program prints `true`, `true`, and `4`.

- An empty `prefix` or `suffix` always matches.
- `bytes.find` returns the index of the first matching byte, counted in bytes.
  An empty `needle` is found at index `0`. A `needle` longer than `value`
  returns `None`.
- The search runs in time linear in the two lengths.
- Both arguments are shared borrows. The same list may be passed twice.
- The text methods `str.starts_with`, `str.ends_with`, and `str.find` do the
  same jobs for `str`. `str.find` counts Unicode scalar values, not bytes.

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

The byte-string token is defined in
[Lexical Structure](/manual/lexical-structure#byte-string-literals). It is a
primary expression of type `list[uint8]`. The rest of this surface adds no
grammar: `list[uint8]`, `import bytes`, associated calls, method calls, module
calls, named arguments, and `Result` patterns use the ordinary forms defined
elsewhere in this Manual.

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

`to_bytes` and `from_bytes` take no `encoding` argument. Ordinary argument
checking rejects one. `bytes.EncodingError` is not a Copy enum, because
`Unknown` holds a `str`.

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

The MIR and direct backends implement the same UTF-8, byte search, hex,
base64, and SHA-256 contract with the same strict codec policy. They must return identical bytes,
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

Aura 0.16 does not provide:

- text encodings other than those listed under [Text Encodings](#text-encodings)
- decoding that replaces or skips invalid input
- raw, triple-quoted, or formatted byte strings
- URL-safe or unpadded base64
- streaming codecs or incremental hashing
- password hashing, or a hash other than SHA-256
- a separate mutable byte buffer type
- implicit text conversion

## Status

`list[uint8]` is the Aura 0.16 bytes type, and the conversion, codec, error,
and hash behavior on this page is implemented. The bytes module derives no
codecs for classes and enums. For JSON codecs and schemas, see
[Typed Codecs](/manual/json#typed-codecs).

Design record: [ADR-0023: Byte-vector codecs and hashing policy](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0023-byte-vector-codecs-and-hashing-policy.md).
