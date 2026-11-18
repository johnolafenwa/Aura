# Image decoding, encoding and resizing

The `image` module of ADR-0114 beside the same work with Rust's `image`
crate and with Pillow and NumPy in Python.

`make_images.py` draws a 1600 by 1200 picture with smooth gradients, edges
and noise, and saves it as `photo.png` and `photo.jpg`. Each side then
times seven operations on it:

| Row | Work |
| --- | --- |
| `decode_png` | PNG bytes to RGB samples |
| `decode_jpeg` | JPEG bytes to RGB samples |
| `encode_png` | RGB samples to a PNG at the library's default level |
| `encode_png_fast` | the same at the fastest level that still compresses |
| `encode_jpeg` | RGB samples to a JPEG at quality 90 |
| `resize` | 1600 by 1200 to 800 by 600 with a bilinear (triangle) filter |
| `to_floats` | the samples as `float32` from 0 to 1 |

- **Aura** (`image_codec.au`) is built with `aura build`.
- **Rust** (`rust/`) uses `image` 0.25.10 with its default features.
- **Python** (`image_codec.py`) uses Pillow and NumPy.

Every row prints `<name> <milliseconds> <check>`. The time is the best of
ten passes. The check is computed outside the timed part: a sum of samples
for the decoders, the file length for the encoders, and a mean for the
rest. The reads of the files from disk are not timed.

## Running

```sh
cargo build --release -p aura -p aura-runtime
bash benchmarks/image_codec/run.sh
```

The host needs Python 3 with Pillow and NumPy, and a Rust toolchain that
can download the `image` crate.

## Results

Milliseconds on a four-core x86-64 Linux cloud machine, 2026-11-16, with
Pillow 12.3.0 and NumPy 2.5.3.

| Row | Aura | Rust | Python | Aura against Rust | Aura against Python |
| --- | ---: | ---: | ---: | ---: | ---: |
| `decode_png` | 14.5 | 14.3 | 32.8 | 1.0x | 2.3x faster |
| `decode_jpeg` | 8.3 | 8.8 | 8.1 | 0.9x | level |
| `encode_png` | 107.7 | 511.5 | 657.0 | 0.2x | 6.1x faster |
| `encode_png_fast` | 14.2 | 11.6 | 145.5 | 1.2x | 10x faster |
| `encode_jpeg` | 5.0 | 49.1 | 5.8 | 0.1x | 1.2x faster |
| `resize` | 3.2 | 18.9 | 9.7 | 0.2x | 3.0x faster |
| `to_floats` | 6.4 | 2.6 | 7.5 | 2.5x | 1.2x faster |

How to read the rows:

- The three sides do not choose the same settings for `encode_png`. The
  files were 2,343,176 bytes from Aura, 2,359,062 from Rust and 2,607,801
  from Pillow, so Aura's file is the smallest of the three. At the fast
  level Aura and Rust wrote the same 3,558,765 bytes and Pillow wrote
  2,700,318 with a slower method.
- `decode_jpeg` is level with Pillow, whose decoder is libjpeg-turbo. The
  two were within 3% of each other from run to run.
- `encode_jpeg` and `resize` use all four cores in Aura. With
  `AURA_ARRAY_THREADS=1` the resize takes about 5.4 ms, and the JPEG
  encoder about 18 ms. Rust's `image` crate and Pillow run both on one
  thread. Aura's and Pillow's JPEG files are the same length because both
  use the standard tables and 4:2:0 colour sampling at quality 90.
- `to_floats` is the one row where Aura is behind Rust. The Rust side
  writes into a vector it sized in advance. Aura builds an `Array` value
  with its own storage.

The checks agree between the sides where the work is exact. The decoders
return the same samples for the PNG. For the JPEG and the resize the means
agree, and single samples may differ by one step between libraries.
