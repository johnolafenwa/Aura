#!/usr/bin/env python3
"""The Python side of the image benchmark: Pillow, and NumPy for floats."""
import io
import sys
import time
from pathlib import Path

import numpy
from PIL import Image

ROUNDS = 10


def phase(name, work, check):
    """Prints the fastest of ten runs of `work`, and a number computed from
    its last result outside the timed part."""
    best = float("inf")
    result = None
    for _ in range(ROUNDS):
        started = time.perf_counter()
        result = work()
        best = min(best, time.perf_counter() - started)
    print(f"{name} {best * 1000:.1f} {check(result)}")


def main():
    workdir = Path(sys.argv[1])
    png = (workdir / "photo.png").read_bytes()
    jpeg = (workdir / "photo.jpg").read_bytes()

    def decode(data):
        image = Image.open(io.BytesIO(data))
        image.load()
        return image

    picture = decode(png)
    pixels = picture.width * picture.height * 3

    def encode(format, **options):
        out = io.BytesIO()
        picture.save(out, format=format, **options)
        return out.tell()

    def total(image):
        return sum(image.tobytes())

    def same(value):
        return value

    phase("decode_png", lambda: decode(png), total)
    phase("decode_jpeg", lambda: decode(jpeg), lambda image: total(image) // pixels)
    phase("encode_png", lambda: encode("PNG"), same)
    phase("encode_png_fast", lambda: encode("PNG", compress_level=1), same)
    phase("encode_jpeg", lambda: encode("JPEG", quality=90), same)
    phase(
        "resize",
        lambda: picture.resize((picture.width // 2, picture.height // 2), Image.BILINEAR),
        lambda image: total(image) // (pixels // 4),
    )
    phase(
        "to_floats",
        lambda: (numpy.asarray(picture, dtype=numpy.float32) / 255.0).transpose(2, 0, 1).copy(),
        lambda planes: int(planes.sum(dtype=numpy.float64)),
    )


if __name__ == "__main__":
    main()
