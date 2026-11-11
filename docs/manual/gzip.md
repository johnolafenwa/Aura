# Gzip Module

The `gzip` module compresses and decompresses byte lists in the gzip format
(RFC 1952), the format of `.gz` files and of HTTP `Content-Encoding: gzip`.

```aura
import gzip
```

## Public API

| API | Signature | Result |
| --- | --- | --- |
| `gzip.compress` | `compress(data: list[uint8], level: int64 = 6) -> list[uint8]` | `data` as one gzip stream. |
| `gzip.decompress` | `decompress(data: list[uint8], max_bytes: int64 = 268435456) -> Result[list[uint8], gzip.Error]` | The data a gzip stream holds, or a typed error. |

Both parameters are shared borrows. Each call returns a fresh owned list and
changes nothing else.

## Example

```aura
import gzip

def main():
    text = "the same line again and again\n".repeat(100)
    packed = gzip.compress(text.to_bytes())
    print(packed.len() < text.byte_len())

    match gzip.decompress(packed):
        case Result.Ok(plain):
            print(plain == text.to_bytes())
        case Result.Err(error):
            print(error)

    match gzip.decompress(packed, max_bytes=1000):
        case Result.Ok(_):
            print("fits")
        case Result.Err(gzip.Error.OutputTooLarge(limit_bytes)):
            print(f"more than {limit_bytes} bytes")
        case Result.Err(gzip.Error.InvalidData(message)):
            print(message)
```

The program prints `true`, `true`, and `more than 1000 bytes`.

## Example: Sign And Compress A Payload

A sender compresses a payload and signs the compressed bytes with [`bytes.hmac_sha256`](/manual/bytes#message-authentication). The receiver checks the signature first and then decompresses with a limit:

<<< ../../examples/io/sign_and_compress.au{aura}

It prints `true`, `64`, `true`, `bad signature`, `more than 4096 bytes`, and `not gzip`.

## Compression

`level` runs from `0` to `9`. Level `0` stores the data without compressing
it, `1` is the fastest compression, `9` the smallest output, and the default
`6` is the usual balance. A level outside that range is a programming error
and traps with `AU4001`.

The output is a single gzip member with no file name and no modification
time, so the same input and level always give the same bytes on one Aura
version. The exact bytes can differ between Aura versions and from other
gzip tools. Only the decompressed data is guaranteed. Empty input is valid
and compresses to a 20-byte stream.

## Decompression

`gzip.decompress` reads a whole gzip stream and checks its CRC-32 and length
trailer. When the input holds several gzip members one after another, as
`cat a.gz b.gz` produces, the result is their data joined in order.

Compressed data can expand more than a thousand times, so a small input can
ask for a great deal of memory. `max_bytes` bounds the result. Decompression
stops as soon as the output would pass the limit, and the call returns
`gzip.Error.OutputTooLarge`. Set the limit to the largest result the program
is prepared to hold when the data comes from outside. The default is 256 MiB.
`max_bytes` must be from `0` to `2147483647`. A value outside that range traps
with `AU4001`.

## Error Model

| Variant | Meaning |
| --- | --- |
| `InvalidData(message: str)` | The input is not a complete, well-formed gzip stream: it is empty, has a wrong header, is cut short, is corrupted, fails its checksum, or has bytes after the last member. `message` describes the fault for a log and is not a stable format. |
| `OutputTooLarge(limit_bytes: int64)` | The decompressed data is longer than `max_bytes`. `limit_bytes` is the limit that was passed. |

Allocation failure is not a `gzip.Error`. It traps with `AU4005`.

## Backend Support

The MIR runtime and the direct native backend implement both functions with
the same implementation, so their results match byte for byte. A program
that does not call `gzip` does not link it.

## Limits

- Both functions work on whole buffers. To compress or decompress a large
  file in constant memory, read it with
  [`fs.File.read_chunk`](/manual/filesystem#reading-a-file-in-pieces) and
  compress each chunk as its own member. The members written one after
  another form one valid gzip file.
- The zlib (RFC 1950) and raw deflate (RFC 1951) framings, and the zstd and
  brotli formats, are not provided.
- The gzip header fields for a file name, a comment, and a modification time
  are neither written nor returned.

Design record: [ADR-0073: Streaming reads, gzip, and HMAC](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0073-streaming-reads-gzip-and-hmac.md).
