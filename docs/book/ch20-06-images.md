# Images

`courier` is finished, but the tour has one more stop, and it does not need
a project to carry it. Sooner or later a program has to open a picture: to
make a thumbnail for a web page, to check what a user uploaded, or to hand
a photograph to a neural network. The `image` module reads and writes PNG,
JPEG, and PPM files, and it keeps a picture in the plainest form it can.

## A picture is a list of bytes

An `image.Image` has four public fields: `width`, `height`, `channels`, and
`pixels`, which is a `list[uint8]`. The samples are stored row by row from
the top, with the channels of a pixel next to each other. One channel is
gray, three are red, green, and blue, and two or four add alpha as the last
channel. There is nothing hidden behind the type, so you can loop over
`pixels` like any other list. Let's draw a picture and save it:

*Filename: src/main.au*

```aura
import image

def sunset(width: int64, height: int64) -> image.Image:
    mut picture = image.Image.new(width, height, 3)
    for y in range(height):
        for x in range(width):
            picture.set(x, y, 0, (255 - y * 255 // height) as uint8)
            picture.set(x, y, 1, (x * 120 // width) as uint8)
            picture.set(x, y, 2, (y * 200 // height) as uint8)
    return picture

def main():
    picture = sunset(320, 240)
    match image.write("sunset.png", picture):
        case Result.Ok(None):
            print("wrote sunset.png")
        case Result.Err(image.Error as error):
            print(f"could not write: {error}")
```

```text
wrote sunset.png
```

`Image.new` gives a black image, and `set` writes one sample after checking
that the column, row, and channel exist. `image.write` looks at the
extension to choose the format: `png`, `jpg` or `jpeg`, and `ppm`, `pgm`,
or `pnm`. Any other extension is an error and not a guess, because a file
called `photo.dat` that silently became a PNG would surprise whoever reads
it next. When the name cannot say, `image.write_as` takes the format as an
argument.

## Reading a file you did not make

Reading is where the care goes, because the file may come from anyone.
`image.read` finds the format from the first bytes of the file and ignores
its name, so a JPEG that someone renamed to `.png` still opens. Here is the
thumbnail maker, with every way it can fail spelled out:

*Filename: src/main.au*

```aura
import fs
import io
import image

def thumbnail(source: str, target: str, longest: int64) -> Result[image.Info, image.Error]:
    picture = try image.read(source)
    mut width = longest
    mut height = longest
    if picture.width >= picture.height:
        height = max(1, picture.height * longest // picture.width)
    else:
        width = max(1, picture.width * longest // picture.height)
    small = picture.resize(width, height)
    try image.write(target, small)
    return Result.Ok(
        image.Info(format=image.Format.Jpeg, width=small.width, height=small.height, channels=small.channels)
    )

def describe(failure: image.Error) -> str:
    match failure:
        case image.Error.Io(error):
            return f"file error: {error}"
        case image.Error.InvalidData(message):
            return f"damaged image: {message}"
        case image.Error.Unsupported(message):
            return f"not supported: {message}"
        case image.Error.TooLarge(limit_pixels):
            return f"more than {limit_pixels} pixels"

def main():
    match thumbnail("sunset.png", "sunset-small.jpg", 96):
        case Result.Ok(image.Info as made):
            print(f"{made.width}x{made.height}")
        case Result.Err(image.Error as failure):
            print(describe(failure))
    match thumbnail("missing.png", "x.jpg", 96):
        case Result.Ok(image.Info as made):
            print(f"{made.width}x{made.height}")
        case Result.Err(image.Error as failure):
            print(describe(failure))
    match own fs.read_bytes("sunset.png"):
        case Result.Ok(list[uint8] as data):
            match image.decode(data[0:200]):
                case Result.Ok(image.Image as picture):
                    print(picture.width)
                case Result.Err(image.Error as failure):
                    print(describe(failure))
            match image.decode_up_to(data, 10000):
                case Result.Ok(image.Image as picture):
                    print(picture.width)
                case Result.Err(image.Error as failure):
                    print(describe(failure))
        case Result.Err(io.Error as error):
            print(f"{error}")
```

```text
96x72
file error: io.Error.NotFound
damaged image: invalid PNG: unexpected end of file
more than 10000 pixels
```

`image.Error` has four variants, and each one calls for a different
response. `Io` wraps the `io.Error` from the file system. `InvalidData`
means the bytes are not a complete, well-formed image, which is what we got
when we cut the file off after 200 bytes. `Unsupported` means the image is
fine but this module cannot do what was asked: a JPEG that uses arithmetic
coding, or an image with alpha going into a JPEG, which has nowhere to put
it.

`TooLarge` deserves a closer look. A compressed image can be tiny on disk
and enormous in memory. A few kilobytes of PNG can claim to be 60,000
pixels on each side, and a program that believes the header will try to
allocate gigabytes. Aura reads the header first and compares the pixel
count with a limit before it decodes anything. `image.decode` and
`image.read` allow 268,435,456 pixels, which is far more than any camera
produces. A service that takes uploads should choose a smaller number with
`image.decode_up_to`, as the last `match` does. When you only want to know
what a file holds, `image.info` returns the format and size from the header
and decodes no pixels at all.

Notice that `resize` returned a new image and left `picture` alone. Every
whole-image operation works that way: `resize`, `crop`, `flip_horizontal`,
`flip_vertical`, and the conversions `to_gray`, `to_rgb`, and `to_rgba`.
`resize` uses a bilinear filter, and when it shrinks an image it averages
over all the pixels each new pixel covers, so a thumbnail does not shimmer
with the fine detail it could not keep. `resize_with` takes
`image.Filter.Nearest` when you want hard edges, or `image.Filter.Bicubic`
for a sharper result.

## From pixels to numbers

Bytes are the right form for a file, but arithmetic wants floating-point
numbers. `to_array` gives the samples as an `Array[float32]` of shape
`[height, width, channels]`, with every value divided by 255 so it lies
from 0 to 1. `to_planes` gives the same numbers with one plane a channel,
`[channels, height, width]`, which is the layout the convolution layers of
[Chapter 23](/book/ch23-00-neural-networks) take. From there a picture
becomes a tensor in one call:

*Filename: src/main.au*

```aura
import image
import tensor
from tensor import Device, Tensor

def main():
    match image.read("sunset.png"):
        case Result.Ok(image.Image as picture):
            gray = picture.to_gray()
            print(f"{gray.width}x{gray.height} channels={gray.channels}")
            planes = picture.to_planes()
            print(planes.shape())
            print(planes[0, 0, 0])
            batch = Tensor[float32].from_array(planes, Device.cpu())
            brighter = (batch * 1.5).to_array()
            match image.write("sunset-bright.png", image.Image.from_planes(brighter)):
                case Result.Ok(None):
                    print("wrote sunset-bright.png")
                case Result.Err(image.Error as failure):
                    print("failed")
        case Result.Err(image.Error as failure):
            print("failed")
```

```text
320x240 channels=1
[3, 240, 320]
1.0
wrote sunset-bright.png
```

`Image.from_planes` and `Image.from_array` go the other way. They multiply
by 255, round, and keep each value inside 0 to 255, so the over-bright
pixels we made by multiplying by 1.5 come out white and do not wrap around
to black.

The `image` module does not import `tensor`. The bridge is the `Array` both
of them understand, which means a program that only resizes thumbnails does
not carry the tensor library in its binary, and a program that never
imports `image` does not carry the codecs.

## Where the module stops

It handles 8-bit images. A PNG with 16 bits a sample is narrowed to 8 as
it is read, an animated PNG gives its first frame, and GIF, WebP, and TIFF
are not read.
It does not draw: there are no lines, text, or blending, only the
whole-image operations above. Colour profiles and EXIF data are ignored
on reading and not written, so a photograph a phone marked as rotated
comes out the way its pixels are stored.

Within those limits it is quick. Decoding and encoding run at the speed of
the Rust libraries that do the same job, and JPEG encoding and resizing use
all the cores of the machine. The [Image Module](/manual/image) page of the
Manual has every function and the measured times.
