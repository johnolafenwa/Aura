# The `image` module

Date: 2026-11-16

## Ask

John chose options 1, 2 and 3 from a list of graphics steps ("lets do
1,2,3"). This note covers option 2: a standard-library module that reads
and writes PNG, JPEG and PPM into a pixel buffer, with a way to tensors.
Options 1 and 3 (a raylib example package and FFI v2) are separate work.

## What was built

- `crates/aura-runtime/src/image_codec.rs`: the codecs and the whole-image
  operations, independent of runtime values.
- `crates/aura-runtime/src/image_host.rs`: nine host functions over them,
  reached from the MIR runtime and from native code.
- `crates/aura-compiler/std/image.au`: the public API (`Image`, `Info`,
  `Format`, `Filter`, `Error` and the functions).
- `examples/io/image_thumbnail.au`, `docs/manual/image.md`, book section
  20.6, ADR-0114, and `benchmarks/image_codec`.
- `scripts/make-image-fixtures.py` writes the test fixtures with Pillow.
  The fixtures are committed, so the tests do not need Python.

## Decisions

They are recorded in ADR-0114. The ones that took measurement:

- `zune-jpeg` 0.4 panicked on a corrupted fixture. 0.5 does not, and the
  third-party decoders also run inside a panic guard.
- The JPEG encoder on one thread took 18 ms for 1600 by 1200 against
  Pillow's 6. Encoding strips on several threads and joining them with
  restart markers brought it to 5 ms on four cores.
- The first resize took 18 ms against Pillow's 10. Trimmed integer filter
  taps, a choice of which axis to do first, and threads brought it to 3 ms.

## Results

`benchmarks/image_codec/README.md` has the table. Aura is ahead of Pillow
on six of seven rows and level on JPEG decoding. It is ahead of or within
1.2 times Rust's `image` crate on six rows, and 2.5 times on the
conversion of samples to floats.

## Not done

- 16-bit samples, EXIF orientation and colour profiles, GIF, WebP, TIFF.
- Decoding by rows for images that do not fit in memory.
- Drawing operations.
- JPEG decoding on several threads, which would be the way past
  libjpeg-turbo.
- Timings on macOS and Windows. The numbers are from x86-64 Linux only.

## Integration On 2026-11-17

The original Linux verify job passed. On macOS the compiler library test
`lightweight_tasks_scale_to_thousands_of_waiting_tasks` exceeded its
15-second readiness deadline. The other platform jobs passed. Main at
`02b1104f` includes the book fixes. The image branch incorporates that
commit and regenerates the documentation for the combined source.

The next Linux run passed compiler coverage at 96.56% of lines and
97.40% of functions. The npm audit then reported GHSA-fc5q-39xc-vjpf in
the documentation toolchain. The recommended VitePress downgrade also
failed the audit. The lockfile pins the patched `@ryotsu/picomatch`
4.0.7-a fork until upstream ships a fixed release. Both npm and Cargo
audits pass locally. All 11 documentation-site tests pass, and
VitePress builds and renders the site with the pinned package.


## JPEG encoding on 2026-11-18

The pure Rust `libjpeg-turbo-rs` encoder averages colour across each 2 by 2
group of RGB pixels. On a red, green, blue, and white checkerboard, the
previous top-left filter produced a red cast, with a maximum channel
difference of 254. The regression checks that the decoded cells are neutral.
The strip layout, quality range, grayscale support, and image-size limits
are covered by the codec tests. Timings and sample conditions are in
`benchmarks/image_codec/README.md`.
