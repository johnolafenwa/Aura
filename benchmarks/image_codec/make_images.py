#!/usr/bin/env python3
"""Writes the two files every side of the image benchmark decodes.

The picture is 1600 by 1200 RGB: smooth colour fields with fine texture and
a little noise over them, so it compresses the way a photograph does.
Pillow writes it as `photo.png` and `photo.jpg` in the directory given.
"""
import math
import sys
from pathlib import Path

from PIL import Image

WIDTH, HEIGHT = 1600, 1200


def main():
    out = Path(sys.argv[1])
    state = 20261116
    data = bytearray(WIDTH * HEIGHT * 3)
    at = 0
    for y in range(HEIGHT):
        wave_y = math.sin(y / 37.0)
        for x in range(WIDTH):
            state = state * 48271 % 2147483647
            noise = state % 13 - 6
            wave = math.sin(x / 53.0) + wave_y
            red = 128 + 80 * math.sin((x + y) / 211.0) + 20 * wave + noise
            green = 120 + 70 * math.sin(x / 173.0 - y / 97.0) + 14 * wave + noise
            blue = 110 + 90 * math.cos(y / 131.0) + 10 * math.sin(x * y / 90000.0) + noise
            data[at] = max(0, min(255, int(red)))
            data[at + 1] = max(0, min(255, int(green)))
            data[at + 2] = max(0, min(255, int(blue)))
            at += 3
    image = Image.frombytes("RGB", (WIDTH, HEIGHT), bytes(data))
    image.save(out / "photo.png")
    image.save(out / "photo.jpg", quality=90)


if __name__ == "__main__":
    main()
