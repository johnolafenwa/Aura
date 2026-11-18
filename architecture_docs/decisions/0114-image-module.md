# ADR-0114: The `image` module

- Status: Accepted
- Date: 2026-11-16
- Builds on: ADR-0073, ADR-0088, ADR-0097

## Context

A program could not open a picture. The tensor chapters of the book train
on numbers the programs make up, because there was no way to read a
photograph, and a tool that makes thumbnails or checks an upload had to
call a C library through FFI. John approved an `image` module on
2026-11-16 as one of three graphics steps. The other two, a raylib example
package and FFI v2, are separate work. A window, a canvas and plotting are
not part of this decision.

## Decision

### An image is three integers and a `list[uint8]`

`image.Image` is an ordinary Aura class with the public fields `width`,
`height`, `channels` and `pixels: list[uint8]`. The samples are stored row
by row from the top with the channels of a pixel next to each other.
Channel counts are 1 (gray), 2 (gray, alpha), 3 (RGB) and 4 (RGBA).

A `list[uint8]` is already stored as packed bytes, a loop over it compiles
to the same code as a loop over any list, and `fs.read_bytes`, `gzip` and
the sockets use the same type. An opaque host resource would have made
every pixel access a host call. An `Array[uint8]` field was the other
candidate. It was not chosen because indexing an `Array` element by element
is slower than a list today, and a user who wants an `Array` gets one with
`to_array`.

### Typed API in Aura, codecs in the runtime

The public types and functions are Aura source in `std/image.au`. Nine host
functions (`image.__aura_info`, `__aura_decode`, `__aura_encode`,
`__aura_convert`, `__aura_resize`, `__aura_crop`, `__aura_flip`,
`__aura_to_array`, `__aura_from_array`) do the work. Each takes one large
value, which it reads in place without a copy, and plain integers. They
return tuples with a status code, and the Aura source turns the code into
an `image.Error`. This is the split ADR-0082 uses for `http`.

### Formats and codecs

The module reads and writes PNG, JPEG and the Netpbm family. PNG uses the
`png` crate, JPEG decoding `zune-jpeg`, and JPEG encoding `libjpeg-turbo-rs`.
All three are Rust with no C code, so the Windows and musl builds need
nothing new. Netpbm is written by hand: P1 to P6 are read, and P5 and P6
are written.

Samples are 8 bits. A 16-bit PNG keeps its high byte, a palette is
expanded, and a transparency chunk becomes an alpha channel. JPEG and
Netpbm cannot hold alpha, so encoding an image with alpha into them is
`Error.Unsupported` and not a silent drop.

The format of input is always found from the first bytes. `image.write`
chooses the output format from the extension and refuses a name whose
extension names no format. `image.write_as` takes the format.

### Untrusted input

Malformed data never traps. Every fault is one of `Error.InvalidData`,
`Error.Unsupported` or `Error.TooLarge`. The third-party decoders run
inside a guard that turns a panic into `InvalidData`, and a test flips
bytes of every fixture to check that nothing escapes.

A decoder reads the header and compares `width * height` with a limit
before it allocates. `decode` and `read` use 2^28 pixels. `decode_up_to`
takes the limit, at most 536,870,911. A programming error, such as a pixel
list whose length does not match the size, a PNG level outside 0 to 9, or
a crop outside the image, traps with `AU4001`.

### Operations

`resize` and `resize_with` (nearest, bilinear, bicubic), `crop`,
`flip_horizontal`, `flip_vertical` and the four channel conversions return
new images. When an image shrinks, the bilinear and bicubic filters widen
to cover every source pixel, as Pillow's do. Gray is the BT.601 weighting.
The module has no drawing operations.

### Arrays and tensors

`to_array` returns an `Array[float32]` of shape `[height, width,
channels]` with values from 0 to 1, and `to_planes` the shape `[channels,
height, width]`. `from_array` and `from_planes` reverse them with rounding
and clamping. The module does not import `tensor`. A tensor is made with
`Tensor[float32].from_array(picture.to_planes(), device)`. A `to_tensor`
method would have made every program that imports `image` compile and link
the tensor source.

### Threads

JPEG encoding splits a large image into strips of whole MCU rows, encodes
them on several threads, and joins them with restart markers into one
standard baseline file. The bilinear and bicubic resizes split their rows
the same way. The restart markers depend on the number of strips, and the
decoded samples are identical across thread counts. The limit is the one
Array kernels use, `AURA_ARRAY_THREADS`, at most 16.

### Binary size

The codecs are reached only from the nine host functions, and the linker
keeps a host function only when the program names it. A program that does
not import `image` has none of the codec code, and
`benchmarks/native_binaries/contents.json` checks that.

## Consequences

- A picture can be read, changed, written and given to a tensor without
  FFI, on every host and in both backends, with the same bytes out.
- Decoding and encoding are level with or faster than Rust's `image` crate
  and Pillow on the rows of `benchmarks/image_codec`, except that turning
  samples into floats is 2.5 times Rust's time and JPEG decoding is level
  with Pillow.
- The runtime has three new dependencies and their trees (`fdeflate`,
  `zune-core` and others).
- 16-bit samples, metadata, animation, other formats, and decoding by rows
  are left out. Each can be added without changing the `Image` class,
  except 16-bit samples, which would need a second pixel type.

## Alternatives considered

- **Bindings to libpng and libjpeg-turbo.** Rejected: a C toolchain or
  system libraries on every host, for a JPEG decoder no faster than the
  one chosen.
- **The `image` crate.** Rejected: it brings formats the module does not
  offer, and its PNG and JPEG encoders measured slower than the crates
  used directly.
- **A package and not a builtin module.** There is no package registry
  yet, and the tensor work needs pictures now.
