#!/usr/bin/env python3
"""Writes the image fixtures the `image` module's tests read.

Each fixture is a small file written by Pillow in a form the module must
read, with a `.raw` file beside it holding the pixels Pillow decodes it to:
8-bit samples, row by row. Run from the repository root, with Pillow
installed; the files are committed, so tests do not need Pillow.
"""
from pathlib import Path

from PIL import Image

OUT = Path("crates/aura-runtime/tests/data/image")
W, H = 16, 12


def rgb(x, y):
    return ((x * 16 + y) % 256, (y * 21 + 3 * x) % 256, (x * y * 5 + 40) % 256)


def base(mode):
    image = Image.new("RGBA", (W, H))
    for y in range(H):
        for x in range(W):
            image.putpixel((x, y), rgb(x, y) + ((x * 17) % 256,))
    return image.convert(mode)


def save(name, image, raw_mode, **options):
    path = OUT / name
    image.save(path, **options)
    decoded = Image.open(path)
    decoded.load()
    (OUT / (name + ".raw")).write_bytes(decoded.convert(raw_mode).tobytes())


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    save("rgb.png", base("RGB"), "RGB")
    save("rgba_interlaced.png", base("RGBA"), "RGBA", interlace=True)
    save("gray.png", base("L"), "L")
    save("gray_alpha.png", base("LA"), "LA")
    save("palette.png", base("RGB").quantize(8), "RGB")
    palette = base("RGB").quantize(8)
    save("palette_transparent.png", palette, "RGBA", transparency=0)
    save("bilevel.png", base("L").point(lambda v: 255 if v > 127 else 0).convert("1"), "L")
    wide = Image.new("I;16", (W, H))
    for y in range(H):
        for x in range(W):
            wide.putpixel((x, y), (x * 4096 + y * 300) % 65536)
    wide.save(OUT / "gray16.png")
    (OUT / "gray16.png.raw").write_bytes(
        bytes(((x * 4096 + y * 300) % 65536) >> 8 for y in range(H) for x in range(W))
    )
    photo = Image.new("RGB", (W, H))
    for y in range(H):
        for x in range(W):
            photo.putpixel((x, y), (40 + x * 12, 60 + y * 14, 200 - x * 6 - y * 4))
    save("baseline.jpg", photo, "RGB", quality=92)
    save("progressive.jpg", photo, "RGB", quality=92, progressive=True)
    save("full_chroma.jpg", photo, "RGB", quality=92, subsampling=0)
    save("gray.jpg", photo.convert("L"), "L", quality=92)
    save("cmyk.jpg", photo.convert("CMYK"), "RGB", quality=92)
    save("restart.jpg", photo, "RGB", quality=92, restart_marker_blocks=1)


if __name__ == "__main__":
    main()
