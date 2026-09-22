# Bytes, Encodings, And Hashes

Aura represents raw bytes as `list[uint8]`. File, socket, process, and
secure-random byte APIs all return that same type, so bytes pass between them
with no wrapper conversion.

There is no implicit conversion between `str` and bytes. Text has a character
encoding and bytes do not, so Aura makes you cross the UTF-8 boundary in
plain sight.

## Convert UTF-8 Explicitly

Call `to_bytes()` on a str:

```aura check-pass
import bytes

text = "Aura 🌌"
payload = text.to_bytes()
print(bytes.hex_encode(payload))
```

This prints `4175726120f09f8c8c`. The list holds the exact UTF-8 bytes.
Embedded NULs, non-ASCII text, and a leading U+FEFF are all kept. Aura does
not normalize the text or rewrite line endings.

The reverse direction can fail, because an arbitrary byte list need not be
valid UTF-8:

```aura check-pass
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

`str.from_bytes` validates strictly. It never swaps bad bytes for a
replacement character. `InvalidUtf8(index)` gives the zero-based byte offset
where the first invalid sequence starts.

Both conversions take shared access to their input. `payload` is still
available after `from_bytes`, and the original str is still available after
`to_bytes`.

## Hexadecimal Is A Text Representation

Hex encoding writes two lowercase digits per byte:

```aura check-pass
import bytes

payload: list[uint8] = [0, 1, 254, 255]
text = bytes.hex_encode(payload)
print(text)
```

This prints `0001feff`. Decoding accepts uppercase or lowercase ASCII:

```aura fragment
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

The decoder is strict. It rejects a `0x` prefix, spaces, separators, signs,
and non-ASCII digits. It checks for odd length before it checks the digits.

## Base64 Uses The Canonical Standard Alphabet

Base64 lets a text protocol carry arbitrary bytes:

```aura check-pass
import bytes

payload: list[uint8] = [0, 1, 254, 255]
encoded = bytes.base64_encode(payload)
print(encoded)

match bytes.base64_decode(encoded):
    case Result.Ok(decoded):
        print(decoded)
    case Result.Err(bytes.Error.InvalidBase64(index)):
        print(f"invalid base64 at byte {index}")
    case Result.Err(error):
        print(error)
```

This prints `AAH+/w==` and then `[0, 1, 254, 255]`.

Aura uses the RFC 4648 standard alphabet with canonical `=` padding. The
decoder does not repair input. It rejects:

- the URL-safe characters `-` and `_`
- whitespace
- missing or extra padding
- trailing data
- nonzero discarded bits

Decoded bytes are not assumed to be UTF-8. Call `str.from_bytes` yourself when
you need text.

## Hash Exact Bytes

`bytes.sha256` returns a raw 32-byte SHA-256 digest:

```aura check-pass
import bytes

payload = "abc".to_bytes()
digest = bytes.sha256(payload)
print(digest.len())
print(bytes.hex_encode(digest))
```

This prints `32`, then this hex line:
`ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.

For a str, `bytes.sha256_string(text)` hashes exactly the bytes that
`text.to_bytes()` produces. It adds no terminator and does not normalize the
text. So these two expressions give equal digest lists:

```aura fragment
bytes.sha256_string("café")
bytes.sha256("café".to_bytes())
```

SHA-256 is a digest. It is not encryption, a password hash, a signature, a
message-authentication code, or random data. When you need one of those
properties, use a cryptographic construction built for it.

## Typed Data Errors And Runtime Failures

Malformed UTF-8, hex, and base64 are expected data problems. They return a
`bytes.Error` inside `Result` when the exact offset or length fits the
variant's `int32` payload. Match the variant, then report, reject, or retry.

Some failures trap with `AU4005` instead:

- The malformed-data offset or length is above `2147483647`. Aura never
  truncates or wraps the value.
- A new codec output would cross its fixed 2,147,483,647-byte safety ceiling.
  This ceiling is separate from the public str and `list` length domains.
- Computing the output size overflows.
- Allocation fails.

A codec never returns a partial successful value.

The optional `encoding` parameter is reserved and not implemented. These are
the complete 0.3 conversion calls:

- `text.to_bytes()`
- `str.from_bytes(payload)`

Do not pass `"utf-8"`, either positionally or as `encoding=...`. Ordinary
argument checking rejects it.

## Run The Maintained Example

From the repository root:

```bash
cargo run -p aura -- run examples/bytes/codecs_and_hashing.au
```

The normative [Bytes, Text Codecs, And SHA-256](../docs/manual/bytes.md)
chapter has the exact signatures, malformed-input precedence, error offsets,
size preflights, backend parity, and non-features.
