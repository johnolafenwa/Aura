# Bytes, Encodings, And Hashes

Aura uses `list[uint8]` whenever an API needs raw bytes. That is the same type
returned by file, socket, process, and secure-random byte APIs, so data can move
between those boundaries without a wrapper conversion.

There is deliberately no implicit conversion between `str` and bytes.
Text has a character encoding; bytes do not. Aura makes the UTF-8 boundary
visible.

## Convert UTF-8 Explicitly

Call `to_bytes()` on a str:

```aura
import bytes

text = "Aura 🌌"
payload = text.to_bytes()
print(bytes.hex_encode(payload))
```

This prints `4175726120f09f8c8c`. The returned list contains the exact
UTF-8 bytes. Embedded NULs, non-ASCII text, and a leading U+FEFF are preserved;
Aura does not normalize the text or rewrite line endings.

Going the other way can fail because an arbitrary byte list need not be
valid UTF-8:

```aura
import bytes

payload: list[uint8] = [65, 117, 114, 97]

match str.from_bytes(payload):
    case Result.Ok(text):
        print(text)
    case Result.Err(bytes.Error.InvalidUtf8(index)):
        print(f"invalid UTF-8 at byte {index}")
    case Result.Err(error):
        print(error)
```

`str.from_bytes` validates strictly. It never replaces bad bytes with a
replacement character. `InvalidUtf8(index)` points to the zero-based byte
offset where the first invalid sequence begins.

The conversion functions share their inputs. `payload` remains available
after `from_bytes`, and the original str remains available after
`to_bytes`.

## Hexadecimal Is A Text Representation

Hex encoding uses two lowercase digits per byte:

```aura
import bytes

payload: list[uint8] = [0, 1, 254, 255]
text = bytes.hex_encode(payload)
print(text)
```

The result is `0001feff`. Decoding accepts uppercase or lowercase ASCII:

```aura
match bytes.hex_decode("0001FeFf"):
    case Result.Ok(payload):
        print(payload)
    case Result.Err(bytes.Error.InvalidHexLength(length)):
        print(f"odd byte length: {length}")
    case Result.Err(bytes.Error.InvalidHexDigit(index, byte)):
        print(f"invalid byte {byte} at {index}")
    case Result.Err(error):
        print(error)
```

The decoder is strict. It does not accept a `0x` prefix, spaces, separators,
signs, or non-ASCII digits. Odd length is checked before digit validity.

## Base64 Uses The Canonical Standard Alphabet

Base64 is useful when a text protocol needs to carry arbitrary bytes:

```aura
import bytes

payload: list[uint8] = [0, 1, 254, 255]
encoded = bytes.base64_encode(payload)
print(encoded)

match bytes.base64_decode(encoded):
    case Result.Ok(decoded):
        print(decoded)
    case Result.Err(bytes.Error.InvalidBase64(index)):
        print(f"invalid base64 at byte {index}")
```

This prints `AAH+/w==` and then `[0, 1, 254, 255]`.

Aura uses the RFC 4648 standard alphabet with canonical `=` padding. The
decoder rejects URL-safe `-`/`_`, whitespace, missing or extra padding,
trailing data, and nonzero discarded bits. It does not quietly repair input.
Decoded bytes are not assumed to be UTF-8; call `str.from_bytes` separately
when text is required.

## Hash Exact Bytes

`bytes.sha256` returns a raw 32-byte SHA-256 digest:

```aura
import bytes

payload = "abc".to_bytes()
digest = bytes.sha256(payload)
print(digest.len())
print(bytes.hex_encode(digest))
```

The output length is `32`, and the hex line is:
`ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.

For a str, `bytes.sha256_string(text)` hashes exactly the bytes produced by
`text.to_bytes()`. It does not add a terminator or normalize text. These two
expressions therefore produce equal digest vectors:

```aura
bytes.sha256_string("café")
bytes.sha256("café".to_bytes())
```

SHA-256 is a digest, not encryption, a password hash, a signature, a
message-authentication code, or random data. Use a protocol-specific
cryptographic construction when one of those properties is required.

## Typed Data Errors And Runtime Failures

Malformed UTF-8, hex, and base64 are expected data problems, so they return a
`bytes.Error` inside `Result` when the exact offset or length fits the retained
`int32` payload. Match the variant and report, reject, or retry as the
application requires. If required malformed-data metadata exceeds
`2147483647`, Aura traps with `AU4005`. It never truncates or wraps the value.

Each fresh codec destination has a fixed 2,147,483,647-byte safety ceiling
independent of the public str and `list` length domains. Crossing that
ceiling, arithmetic overflow while calculating the destination size, or
allocation failure traps with `AU4005`. A codec never returns a partial
successful value.

The optional `encoding` parameter is reserved but not implemented. These are
the complete 0.2 conversion calls:

- `text.to_bytes()`
- `str.from_bytes(payload)`

Do not pass `"utf-8"` positionally or as `encoding=...`; ordinary argument
checking rejects it.

## Run The Maintained Example

From the repository root:

```bash
cargo run -p aura -- run examples/bytes/codecs_and_hashing.au
```

For exact signatures, malformed-input precedence, error offsets, size
preflights, backend parity, and non-features, read the normative
[Bytes, Text Codecs, And SHA-256](../docs/manual/bytes.md) chapter.
