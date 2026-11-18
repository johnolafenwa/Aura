# Image Module

The `image` module reads and writes PNG, JPEG, and PPM pictures. A decoded
picture is an `image.Image`: a width, a height, a channel count, and its
samples as a `list[uint8]`. The module also crops, resizes, and converts
pictures, and hands their samples to an [`Array`](/manual/numeric-arrays)
for numeric work and [tensors](/manual/tensors).

```aura
import image
```

## Example

```aura
import image

def thumbnail(source: str, target: str) -> Result[None, image.Error]:
    picture = try image.read(source)
    small = picture.resize(160, 120)
    try image.write(target, small.to_rgb())
    return Result.Ok(None)

def main():
    match thumbnail("photo.png", "thumbnail.jpg"):
        case Result.Ok(_):
            print("written")
        case Result.Err(image.Error.Io(error)):
            print(error)
        case Result.Err(_):
            print("not an image this module reads")
```

`image.read` finds the format from the file's first bytes. `image.write`
chooses it from the extension of the name.

## The Image Class

```text
public class Image:
    public width: int64
    public height: int64
    public channels: int64
    public pixels: list[uint8]
```

`channels` says what a pixel holds:

| `channels` | A pixel is |
| --- | --- |
| `1` | gray |
| `2` | gray, alpha |
| `3` | red, green, blue |
| `4` | red, green, blue, alpha |

`pixels` holds `width * height * channels` samples from `0` to `255`. Rows
run from the top, pixels in a row from the left, and the channels of a pixel
sit next to each other. The sample of channel `c` at column `x`, row `y` is
`pixels[(y * width + x) * channels + c]`. Alpha `255` is opaque, and colour
is not multiplied by alpha.

The fields are public, so a program can build an `Image` from samples it
computed and can read or write `pixels` directly in a loop:

```aura
import image

def invert(picture: mut image.Image) -> None:
    for index in range(picture.pixels.len()):
        sample = picture.pixels[index]
        picture.pixels[index] = 255 - sample

def main():
    mut picture = image.Image.filled(2, 2, 1, 200)
    invert(picture)
    print(picture.pixels)
```

It prints `[55, 55, 55, 55]`.

An operation that takes an `Image` checks that `pixels` has exactly
`width * height * channels` samples, that both sides are at least `1`, and
that `channels` is from `1` to `4`. An image that breaks one of these is a
programming error and traps with `AU4001`.

| Member | Signature | Result |
| --- | --- | --- |
| `Image.new` | `new(width: int64, height: int64, channels: int64) -> Image` | A black image. With an alpha channel it is transparent. |
| `Image.filled` | `filled(width: int64, height: int64, channels: int64, value: uint8) -> Image` | An image with every sample set to `value`. |
| `get` | `get(self, x: int64, y: int64, channel: int64) -> uint8` | One sample. |
| `set` | `set(mut self, x: int64, y: int64, channel: int64, value: uint8) -> None` | Sets one sample. |
| `has_alpha` | `has_alpha(self) -> bool` | Whether `channels` is `2` or `4`. |
| `to_gray` | `to_gray(self) -> Image` | One gray channel. Alpha is dropped. |
| `to_gray_alpha` | `to_gray_alpha(self) -> Image` | Gray and alpha. |
| `to_rgb` | `to_rgb(self) -> Image` | Red, green, blue. Alpha is dropped. |
| `to_rgba` | `to_rgba(self) -> Image` | Red, green, blue, alpha. |
| `resize` | `resize(self, width: int64, height: int64) -> Image` | The image scaled with `Filter.Bilinear`. |
| `resize_with` | `resize_with(self, width: int64, height: int64, filter: Filter) -> Image` | The image scaled with `filter`. |
| `crop` | `crop(self, left: int64, top: int64, width: int64, height: int64) -> Image` | The part whose top left pixel is at column `left`, row `top`. |
| `flip_horizontal` | `flip_horizontal(self) -> Image` | The image mirrored left to right. |
| `flip_vertical` | `flip_vertical(self) -> Image` | The image mirrored top to bottom. |
| `to_array` | `to_array(self) -> Array[float32]` | The samples as numbers from `0` to `1`, shape `[height, width, channels]`. |
| `to_planes` | `to_planes(self) -> Array[float32]` | The samples as numbers from `0` to `1`, shape `[channels, height, width]`. |
| `Image.from_array` | `from_array(values: Array[float32]) -> Image` | The reverse of `to_array`. |
| `Image.from_planes` | `from_planes(values: Array[float32]) -> Image` | The reverse of `to_planes`. |

Every method that returns an `Image` returns a new one and leaves the
receiver unchanged. `get` and `set` trap with `AU4001` for a pixel or a
channel outside the image, and so do `new` and `filled` for a side below `1`
or a channel count outside `1` to `4`.

### Converting Channels

Gray is `0.299 * red + 0.587 * green + 0.114 * blue`, rounded. Gray to
colour copies the gray value into all three channels. A new alpha channel
is `255`. Dropping alpha discards it and leaves the colour as it is.

### Resizing

| `Filter` | A pixel is | Use it for |
| --- | --- | --- |
| `Nearest` | the nearest source pixel | pixel art, masks, and label maps, where values must not mix |
| `Bilinear` | a weighted mean of the pixels around the point | photographs. This is what `resize` uses. |
| `Bicubic` | a wider weighted mean | a sharper result, at about twice the time |

When an image shrinks, `Bilinear` and `Bicubic` widen to cover every source
pixel, so a thumbnail is a true average and fine patterns do not turn into
noise. `Bicubic` can overshoot at a hard edge. Results stay inside `0` to
`255`. A target side below `1`, or a target of more than 536,870,911 pixels,
traps with `AU4001`.

`crop` traps with `AU4001` unless the part lies inside the image.

## Reading

| API | Signature | Result |
| --- | --- | --- |
| `image.read` | `read(file_path: str) -> Result[Image, image.Error]` | The image in a file. |
| `image.decode` | `decode(data: list[uint8]) -> Result[Image, image.Error]` | The image in a byte list. |
| `image.decode_up_to` | `decode_up_to(data: list[uint8], max_pixels: int64) -> Result[Image, image.Error]` | The image, when it has at most `max_pixels` pixels. |
| `image.info` | `info(data: list[uint8]) -> Result[image.Info, image.Error]` | The format, width, height, and channel count from the header. No pixel is decoded. |

The format comes from the first bytes of the data, never from a file name.
`image.Info` has the public fields `format: Format`, `width`, `height`, and
`channels`.

A decoded image has the channels its file has:

| Format | What is read | `channels` |
| --- | --- | --- |
| PNG | Every colour type and bit depth, interlaced or not. A palette expands to colour. A transparency key becomes an alpha channel. A 16-bit sample keeps its high byte. | `1` to `4` |
| JPEG | Baseline and progressive files at 8 bits. A CMYK file is converted to red, green, blue. | `1` or `3` |
| PPM | The Netpbm family, text or binary: PBM (`P1`, `P4`), PGM (`P2`, `P5`), and PPM (`P3`, `P6`). A maximum value other than `255` is scaled to it. | `1` or `3` |

A file's header can claim any size, so a few bytes can ask for gigabytes.
The module reads the size from the header and refuses a larger image with
`image.Error.TooLarge` before it decodes anything. `read` and `decode` allow
268,435,456 pixels. A JPEG can be up to 65,535 pixels wide or tall. For
example, a 20,000 by 1 image counts as 20,000 pixels. When files come
from outside the program, pass the largest image the program is prepared
to hold to `decode_up_to`:

```aura
import fs
import image

def load_avatar(file_path: str) -> image.Image | None:
    match own fs.read_bytes(file_path):
        case Result.Ok(data):
            match own image.decode_up_to(data, 1024 * 1024):
                case Result.Ok(picture):
                    return picture
                case Result.Err(_):
                    return None
        case Result.Err(_):
            return None
```

`max_pixels` must be from `1` to `536870911`. A value outside that range
traps with `AU4001`.

## Writing

| API | Signature | Result |
| --- | --- | --- |
| `image.write` | `write(file_path: str, picture: Image) -> Result[None, image.Error]` | Writes the file in the format its extension names. |
| `image.write_as` | `write_as(file_path: str, picture: Image, format: Format) -> Result[None, image.Error]` | Writes the file in `format`, whatever its name. |
| `image.encode` | `encode(picture: Image, format: Format) -> Result[list[uint8], image.Error]` | The file's bytes, at PNG level `6` or JPEG quality `90`. |
| `image.encode_png` | `encode_png(picture: Image, level: int64 = 6) -> Result[list[uint8], image.Error]` | A PNG file. |
| `image.encode_jpeg` | `encode_jpeg(picture: Image, quality: int64 = 90) -> Result[list[uint8], image.Error]` | A JPEG file. |
| `image.encode_ppm` | `encode_ppm(picture: Image) -> Result[list[uint8], image.Error]` | A binary PGM file for gray, or a binary PPM file for colour. |
| `image.format_of` | `format_of(file_path: str) -> Format \| None` | The format an extension names. |

`write` replaces a file that exists. Extensions are read in any letter case:

| `Format` | Extensions |
| --- | --- |
| `Png` | `png` |
| `Jpeg` | `jpg`, `jpeg` |
| `Ppm` | `ppm`, `pgm`, `pnm` |

A name with any other extension, or none, is `image.Error.Unsupported`.

**PNG** loses nothing and holds every channel count. `level` runs from `0`
to `9`: `0` stores the samples uncompressed, `1` is the fastest compression,
and `9` gives the smallest file. Levels `1` to `3` use a fast compressor
made for PNG. Use them when speed matters more than size.

**JPEG** is smaller and loses detail. `quality` runs from `1`, the smallest
file, to `100`, the closest to the original. JPEG holds gray and colour
only. An image with alpha is `image.Error.Unsupported`: call `to_rgb()` or
`to_gray()` first, which states that the alpha is dropped. JPEG stores
brightness at full resolution and averages the colour of each 2 by 2 group
of pixels (4:2:0 sampling). A side longer than 65,535 pixels is
`image.Error.Unsupported`.

**PPM** is uncompressed and trivial to parse, which makes it useful for
tests and for tools that read nothing else. An image with alpha is
`image.Error.Unsupported`.

A level outside `0` to `9` or a quality outside `1` to `100` is a
programming error and traps with `AU4001`.

The same image and options give the same bytes on one Aura version and one
machine. The bytes of a PNG or a JPEG can differ between Aura versions, and
a JPEG's bytes depend on the number of processors. Only the decoded pixels
are guaranteed.

## Arrays And Tensors

`to_array` and `to_planes` copy the samples into an `Array[float32]` and
divide them by `255`, so each number is from `0` to `1`:

- `to_array` keeps the order of `pixels`. The shape is
  `[height, width, channels]`.
- `to_planes` puts each channel in its own plane. The shape is
  `[channels, height, width]`, the layout
  [`Conv2d`](/manual/tensors) and most trained models take.

`Image.from_array` and `Image.from_planes` go back. Each number is
multiplied by `255`, rounded, and kept inside `0` to `255`. Both accept an
Array of two axes, `[height, width]`, as a gray image. An Array of another
rank, of more than four channels, or with no elements traps with `AU4001`.

A tensor is one call further, on the device the program chooses:

```aura
import image
import tensor
from tensor import Device, Tensor

def main():
    picture = image.Image.filled(4, 2, 3, 51)
    device = Device.cpu()
    planes = Tensor[float32].from_array(picture.to_planes(), device)
    print(planes.shape())
    brighter = planes * 2.0
    back = image.Image.from_planes(brighter.to_array())
    print(back.get(0, 0, 0))
```

It prints `[3, 2, 4]` and `102`.

`Array` has no `uint8` element type, so the exact samples stay in `pixels`.
Work on `pixels` when a result must be exact to the byte.

## Error Model

| Variant | Meaning |
| --- | --- |
| `Io(error: io.Error)` | `read`, `write`, or `write_as` could not read or write the file. `error` is the [`io.Error`](/manual/io#io-error) from the filesystem. |
| `InvalidData(message: str)` | The bytes are not a complete, well-formed image: they are empty, cut short, or corrupted. `message` describes the fault for a log and is not a stable format. |
| `Unsupported(message: str)` | The data is in a format the module does not read, the file uses a feature it does not handle, or the format asked for cannot hold the image. |
| `TooLarge(limit_pixels: int64)` | The image has more pixels than the limit. `limit_pixels` is the limit that applied. |

Bad data is never a trap. A file from outside the program can only produce
one of these values. Allocation failure is not an `image.Error`. It traps
with `AU4005`.

## Speed

Decoding and PNG encoding run on the calling thread. JPEG encoding and the
`Bilinear` and `Bicubic` resizes split a large image into bands and work on
them on several threads, up to 16. The pixels do not depend on the number of
threads. Set the environment variable `AURA_ARRAY_THREADS` to a positive
number to bound the threads. With `1`, all the work stays on the calling
thread.

`benchmarks/image_codec` in the repository times each operation against
Rust's `image` crate and Python's Pillow on a 1600 by 1200 photograph-like
picture. These are the best of ten passes in milliseconds on a four-core
x86-64 Linux machine:

| Operation | Aura | Rust `image` 0.25 | Pillow 12 |
| --- | ---: | ---: | ---: |
| Decode PNG | 14.5 | 14.3 | 32.8 |
| Decode JPEG | 8.3 | 8.8 | 8.1 |
| Encode PNG, level 6 | 107.7 | 511.5 | 657.0 |
| Encode PNG, level 1 | 14.2 | 11.6 | 145.5 |
| Encode JPEG, quality 90 | 5.0 | 49.1 | 5.8 |
| Resize to 800 by 600, bilinear | 3.2 | 18.9 | 9.7 |
| Samples to `float32` | 6.4 | 2.6 | 7.5 |

The default settings of the three libraries differ, so the PNG rows at
level 6 do not produce files of the same size. The benchmark's README
gives the sizes. JPEG decoding is level with Pillow, which uses
libjpeg-turbo.

## Backend Support

The MIR runtime and the direct native backend run the same implementation,
so their results match byte for byte. A program that does not import `image`
links none of it.

## Limits

- Samples are 8 bits. A 16-bit PNG or PPM is narrowed when it is read, and
  no format is written at 16 bits.
- GIF, BMP, WebP, TIFF, and AVIF are not provided.
- An animated PNG gives its first frame.
- Metadata is neither returned nor written: EXIF, including the orientation
  a camera records, colour profiles, gamma, and text chunks. A photograph
  taken with the camera turned appears turned.
- JPEG files that use arithmetic coding, lossless mode, or 12-bit samples
  are `image.Error.Unsupported`.
- Reading and writing work on a whole image in memory. There is no decoding
  by rows or tiles.
- There are no drawing operations, such as lines, shapes, or text.

Design record: [ADR-0114: The `image` module](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0114-image-module.md).
