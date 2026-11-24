# Bytes, digests, and compression

Text is for people. Networks, files, hash functions, and compressors work
on bytes, and the first job of `courier` is to get a message from one form
to the other and back without losing anything. Aura has no separate bytes
type. A sequence of bytes is a `list[uint8]`, an ordinary list whose
elements are numbers from 0 to 255, so everything you know about lists
applies to it.

## From text to bytes and back

A `str` is always valid UTF-8, and `to_bytes()` hands you those UTF-8 bytes
as a new list:

*Filename: src/main.au*

```aura
import bytes

def main():
    note = "café ☕"
    raw = note.to_bytes()
    print(note.len())
    print(raw.len())
    print(raw)
    print(bytes.hex_encode(raw))

    match str.from_bytes(raw):
        case Result.Ok(text):
            print(text)
        case Result.Err(error):
            print(error)

    cut_short: list[uint8] = [99, 97, 102, 195]
    print(str.from_bytes(cut_short))
```

<span class="caption">Listing 20-1: Converting between `str` and `list[uint8]`</span>

```console
$ aura run src/main.au
6
9
[99, 97, 102, 195, 169, 32, 226, 152, 149]
636166c3a920e29895
café ☕
Result.Err(bytes.Error.InvalidUtf8(3))
```

The note has six characters and nine bytes. `c`, `a`, `f`, and the space
take one byte each, `é` takes two, and the cup takes three. That gap is why
[Chapter 8](/book/ch08-02-strings) kept `len()` and `byte_len()` apart, and
it is why "the first 100 bytes of a string" is not a safe thing to ask for:
the cut can land inside a character.

The last two lines of the listing show what happens when it does. Going from
bytes to text can fail, because most byte sequences are not valid UTF-8, so
`str.from_bytes` returns a `Result`. It is called on the type, as
`str.from_bytes(raw)`, and it is strict: it never replaces a bad byte with a
placeholder character and never returns the part that did decode. Our
four-byte list ends with `195`, the first half of `é`, and the error says
exactly that: an invalid sequence starts at byte 3. A program that reads
bytes from a socket or a file and needs text has to decide what to do in
that case, and Aura makes the decision visible.

When you know a file uses Latin-1, call `bytes.decode(raw, "latin-1")` and
match on the result to get its text. To write that text as UTF-16, use
`bytes.encode(text, "utf-16")`. The [bytes manual](/manual/bytes#text-encodings)
lists the supported encodings and the errors each call can return.

## Hexadecimal and base64

Printing a byte list as decimal numbers gets old quickly. The two standard
ways to write bytes as text are both in the `bytes` module. Hexadecimal
writes each byte as two digits, which is easy to read and doubles the
length. Base64 packs three bytes into four characters, which is harder to
read and only a third longer:

```aura
import bytes

def main():
    raw = "café ☕".to_bytes()
    print(bytes.hex_encode(raw))
    print(bytes.base64_encode(raw))

    print(bytes.hex_decode("636166c3a9"))
    print(bytes.hex_decode("63 61"))
    print(bytes.base64_decode("Y2Fmw6kg4piV"))
    print(bytes.base64_decode("Y2Fm-6kg"))
```

```console
$ aura run src/main.au
636166c3a920e29895
Y2Fmw6kg4piV
Result.Ok([99, 97, 102, 195, 169])
Result.Err(bytes.Error.InvalidHexLength(5))
Result.Ok([99, 97, 102, 195, 169, 32, 226, 152, 149])
Result.Err(bytes.Error.InvalidBase64(4))
```

Encoding cannot fail, so the encoders return a `str`. Decoding can, so the
decoders return a `Result` whose error says what was wrong and where. Both
decoders are strict. `hex_decode` accepts upper or lower case digits and
nothing else: no `0x` prefix, no spaces. `"63 61"` has five characters, an
odd number, and is refused for its length before the space is even looked
at. `base64_decode` wants the standard alphabet with its `=` padding, and it
rejected the `-` at index 4, a character from the URL-safe alphabet that
Aura 0.17 does not provide. If you receive URL-safe base64, replace `-` with
`+` and `_` with `/` first.

Strictness is deliberate here. A decoder that skips what it does not
understand lets two different texts decode to the same bytes, and that is a
bad property for anything you are about to sign.

## A fingerprint with SHA-256

A *digest* boils any amount of data down to a short, fixed-size fingerprint.
`bytes.sha256` computes the SHA-256 digest, which is 32 bytes whatever the
input:

```aura
import bytes

def fingerprint(text: str) -> str:
    return bytes.hex_encode(bytes.sha256_string(text))

def main():
    print(fingerprint("deploy finished"))
    print(fingerprint("deploy finished."))
    print(bytes.sha256("deploy finished".to_bytes()).len())
```

```console
$ aura run src/main.au
7820090191cfad16f72b9c9eb62f98ac64ee1d8b4284486d1e6a80c57ca5b72d
27dd38a4391cd0ba663f4ef090f98e81a967efb8b9c71163040eab16f7349cf1
32
```

`sha256` takes bytes and returns the raw 32 bytes, and `sha256_string(text)`
is a shorthand for hashing `text.to_bytes()`. We wrap the result in
`hex_encode` to get the familiar 64-digit form. Adding one period changed
every part of the fingerprint, which is the point: you can compare two
digests to learn whether two files are identical without comparing the
files.

What a digest cannot tell you is who made it. Anyone can compute the SHA-256
of anything. If `courier` sent a message together with its digest, someone
who changed the message in transit would recompute the digest and send that
along too.

## Signing with a shared secret

To know that a message came from someone we trust, the fingerprint has to
depend on something an attacker does not have. `bytes.hmac_sha256(key,
message)` mixes a secret key into the digest. Without the key nobody can
produce the right 32 bytes for a message, and with it the receiver can
recompute them and compare:

*Filename: src/main.au*

```aura
import bytes

def sign(secret: list[uint8], message: str) -> str:
    return bytes.hex_encode(bytes.hmac_sha256(secret, message.to_bytes()))

def verify(secret: list[uint8], message: str, signature: str) -> bool:
    match bytes.hex_decode(signature):
        case Result.Ok(received):
            expected = bytes.hmac_sha256(secret, message.to_bytes())
            return bytes.constant_time_eq(expected, received)
        case Result.Err(_):
            return false

def main():
    secret = "correct horse battery staple".to_bytes()
    signature = sign(secret, "deploy finished")
    print(signature)
    print(verify(secret, "deploy finished", signature))
    print(verify(secret, "deploy failed", signature))
    print(verify("a guess".to_bytes(), "deploy finished", signature))
    print(verify(secret, "deploy finished", "not hex"))
```

<span class="caption">Listing 20-2: Signing a message and checking the signature</span>

```console
$ aura run src/main.au
314e35afb3815292bb96160acbbc6d7943e2798f385f4a8b301c718f5305b121
true
false
false
false
```

A changed message, a wrong key, and a signature that is not even
hexadecimal all come out `false`. This is how webhooks from most services
are authenticated: the sender signs the request body with a secret you both
hold and puts the signature in a header.

`verify` compares the two digests with `bytes.constant_time_eq` and not with
`==`, and that is not fussiness. `==` on two lists stops at the first
element that differs. An attacker who can time many attempts can tell how
many leading bytes of a forged signature were right and work forward one
byte at a time. `constant_time_eq` reads every byte before it answers, so
the time tells them nothing. Use it whenever one side of a comparison is a
secret.

HMAC proves that the holder of the key produced the message. It does not
hide the message: anyone can read a signed envelope. Aura 0.17 has no
encryption functions in its library, and SHA-256 is its only hash. In
particular, do not store passwords with a bare `sha256`. Password storage
needs a deliberately slow hash, which the library does not provide.

## Compressing with gzip

Signed messages travel through chat windows and URLs, so shorter is better.
The `gzip` module compresses and decompresses byte lists in the format of
`.gz` files:

```aura
import gzip

def main():
    report = "status=ok ".repeat(200)
    packed = gzip.compress(report.to_bytes())
    print(f"{report.byte_len()} bytes pack into {packed.len()}")

    match gzip.decompress(packed):
        case Result.Ok(plain):
            print(plain == report.to_bytes())
        case Result.Err(error):
            print(error)

    print(gzip.decompress(packed, max_bytes=1000))
    print(gzip.decompress("not gzip".to_bytes()))
```

```console
$ aura run src/main.au
2000 bytes pack into 53
true
Result.Err(gzip.Error.OutputTooLarge(1000))
Result.Err(gzip.Error.InvalidData(unexpected end of file))
```

`compress` cannot fail and takes an optional `level` from 0 to 9. The
default, 6, is the usual balance of speed and size. Repetitive text like
ours shrinks dramatically, and that is exactly why `decompress` has a second
parameter. Two thousand bytes became 53, and a few kilobytes of carefully
built input can expand to gigabytes. A program that decompresses whatever it
is sent is handing strangers a way to exhaust its memory. `max_bytes` is the
largest result we are prepared to hold. Decompression stops the moment the
output would pass it and returns `OutputTooLarge`. The default is 256 MiB.
For data from outside, set it to what the program expects.

Both functions work on whole byte lists in memory. For a file too large for
that, read it in chunks and compress each chunk on its own. Chunks written
one after another form a valid `.gz` file, and the
[Gzip Module](/manual/gzip) page shows how.

## Sealing an envelope

We now have every piece of the first version of `courier`. To seal a
message we compress it, sign the compressed bytes, and join the base64 of
the bytes to the hex of the signature with a period, a character neither
encoding uses. To open an envelope we undo the steps in order, and every
one of them can fail:

*Filename: src/main.au*

```aura
import bytes
import gzip

enum OpenError:
    Malformed
    BadSignature
    TooLarge(limit: int64)

def seal(secret: list[uint8], message: str) -> str:
    packed = gzip.compress(message.to_bytes())
    signature = bytes.hmac_sha256(secret, packed)
    return f"{bytes.base64_encode(packed)}.{bytes.hex_encode(signature)}"

def unpack(packed: list[uint8]) -> Result[str, OpenError]:
    match gzip.decompress(packed, max_bytes=4096):
        case Result.Ok(plain):
            return str.from_bytes(plain).map_err(lambda _: OpenError.Malformed)
        case Result.Err(gzip.Error.OutputTooLarge(limit)):
            return Result.Err(OpenError.TooLarge(limit))
        case Result.Err(gzip.Error.InvalidData(_)):
            return Result.Err(OpenError.Malformed)

def unseal(secret: list[uint8], envelope: str) -> Result[str, OpenError]:
    parts = envelope.split(".")
    if parts.len() != 2:
        return Result.Err(OpenError.Malformed)
    packed = try bytes.base64_decode(parts[0]).map_err(lambda _: OpenError.Malformed)
    signature = try bytes.hex_decode(parts[1]).map_err(lambda _: OpenError.Malformed)
    if not bytes.constant_time_eq(bytes.hmac_sha256(secret, packed), signature):
        return Result.Err(OpenError.BadSignature)
    return unpack(packed)

def main():
    secret = "correct horse battery staple".to_bytes()
    envelope = seal(secret, "deploy finished: 14 services healthy")
    print(envelope)
    print(unseal(secret, envelope))
    print(unseal("wrong secret".to_bytes(), envelope))
    print(unseal(secret, envelope.replace("H4sI", "H4sJ")))
    print(unseal(secret, "hello"))
    print(unseal(secret, seal(secret, "z".repeat(100000))))
```

<span class="caption">Listing 20-3: `courier`, version one: compressed, signed envelopes</span>

```console
$ aura run src/main.au
H4sIAAAAAAAA/0tJLcjJr1RIy8zLLM5ITbFSMDRRKE4tKstMTi1WyEhNzCnJqAQAm/BbdSQAAAA=.a01ccfbe4c66486085e33683a0508f5a9491358f26a435784382fbb9297b4d21
Result.Ok(deploy finished: 14 services healthy)
Result.Err(OpenError.BadSignature)
Result.Err(OpenError.BadSignature)
Result.Err(OpenError.Malformed)
Result.Err(OpenError.TooLarge(4096))
```

The library's error types, `bytes.Error` and `gzip.Error`, collapse into
our own `OpenError` with `map_err` from
[Chapter 9](/book/ch09-02-recoverable-errors), so that `try` can return
early from `unseal` with one error type. A caller of `unseal` does not care
which decoder objected. It cares whether the envelope was garbage, forged,
or too big.

Look at the order of the steps in `unseal`, because it matters more than
any single call. The signature is checked *before* the bytes are
decompressed. The fourth line of `main` changes one character of the packed
data, and the answer is `BadSignature`, not a gzip error: the damaged bytes
never reached the decompressor. The same holds for the size limit. The last
line seals a hundred thousand `z` characters, which we signed ourselves, so
the signature is fine and the limit in `unpack` is what stops it. Check
authenticity first, then spend memory and time on the contents. An envelope
from a stranger should cost us one HMAC and nothing more.

`seal` also signs the *compressed* bytes, the same bytes that travel, and
not the original text. Then `unseal` can verify exactly what it received
without transforming it first.

Our envelope says who made it and that it is intact. It does not say when.
Someone who records a valid envelope can send it again next week, and it
will still verify. To close that gap, `courier` needs a clock.
