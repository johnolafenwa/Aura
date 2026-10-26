"""Image processing: blur, edge detection, histogram, downscale, PGM encoding.

Generates an N x N grayscale image (a gradient plus noise), applies a 3 x 3 box
blur with clamped edges, runs a Sobel edge detector on the result, builds a
16-bin histogram of edge strength, halves the image with 2 x 2 averaging, and
encodes the small image as a binary PGM file in memory. Pixels live in flat
integer lists indexed by y * width + x.
"""
import sys

MOD = 1000000007


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def blur(pixels, n):
    out = [0] * (n * n)
    for y in range(n):
        for x in range(n):
            total = 0
            for dy in range(-1, 2):
                yy = min(n - 1, max(0, y + dy))
                for dx in range(-1, 2):
                    xx = min(n - 1, max(0, x + dx))
                    total += pixels[yy * n + xx]
            out[y * n + x] = total // 9
    return out


def sobel(pixels, n):
    out = [0] * (n * n)
    for y in range(1, n - 1):
        for x in range(1, n - 1):
            i = y * n + x
            gx = (pixels[i - n + 1] + 2 * pixels[i + 1] + pixels[i + n + 1]) - (
                pixels[i - n - 1] + 2 * pixels[i - 1] + pixels[i + n - 1]
            )
            gy = (pixels[i + n - 1] + 2 * pixels[i + n] + pixels[i + n + 1]) - (
                pixels[i - n - 1] + 2 * pixels[i - n] + pixels[i - n + 1]
            )
            out[i] = min(255, abs(gx) + abs(gy))
    return out


def downscale(pixels, n):
    half = n // 2
    out = []
    for y in range(half):
        for x in range(half):
            i = 2 * y * n + 2 * x
            out.append((pixels[i] + pixels[i + 1] + pixels[i + n] + pixels[i + n + 1]) // 4)
    return out


def main():
    n = int(sys.argv[1])
    rng = Rng(71)
    pixels = []
    for y in range(n):
        for x in range(n):
            pixels.append((x * x + 3 * y * y + rng.next() % 32) % 256)
    blurred = blur(pixels, n)
    edges = sobel(blurred, n)
    histogram = [0] * 16
    strong = 0
    for value in edges:
        histogram[value // 16] += 1
        if value > 128:
            strong += 1
    small = downscale(blurred, n)
    half = n // 2
    pgm = bytearray(("P5\n" + str(half) + " " + str(half) + "\n255\n").encode())
    for value in small:
        pgm.append(value)
    blur_check = 0
    for value in blurred:
        blur_check = (blur_check * 31 + value) % MOD
    pgm_check = 0
    for value in pgm:
        pgm_check = (pgm_check * 131 + value) % MOD
    print(
        f"image_filters size={n} blur={blur_check} strong={strong} histogram={','.join(str(h) for h in histogram)} "
        f"pgm_bytes={len(pgm)} pgm={pgm_check}"
    )


if __name__ == "__main__":
    main()
