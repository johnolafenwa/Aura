# Book Sections For The Library, Shipping, And Windows

## Goal

John asked on 2026-11-15 for the book to cover what the documentation audit
found missing. This note covers everything except the tensor and machine
learning chapters, which are written separately as chapters 21 and later.

## Work Completed

- 8.6 Views and spans: `view` and `view mut` bindings, `-> view T from`,
  `Span[T]`, and a quicksort over a `mut Span[int64]`.
- 8.7 Lane values: the `simd` module, a lane loop with a scalar tail, timed
  against the plain loop, `I64x4` overflow, and `--target-cpu`.
- 16.5 HTTP clients: response bodies and headers, posting, redirects,
  server-sent events, and `control.retry`.
- 16.6 Other protocols: UDP, WebSockets, Unix sockets, and TLS with a
  self-signed certificate.
- 17.4 Building for other machines: target packs, a static musl binary run
  in an empty container, an arm64 build run under emulation, and
  `--target-cpu`.
- Chapter 20, a tour of the standard library built around one tool,
  `courier`: bytes and codecs, time and dates, regular expressions,
  randomness, and logging, tracing, and metrics.
- Appendix F, following the book on Windows.
- The chapter summaries of 8, 16, and 17 moved to the new last sections.

## Corrections Made On The Way

Plain `aura run` uses the MIR backend on every platform
(`DEFAULT_RUN_BACKEND` in `crates/aura/src/main.rs`) and needs no linker.
The book's installation section, the Windows install guide,
`SUPPORTED_PLATFORMS.md`, and the current limits page said that `aura run`
needs the Microsoft linker on Windows. The released Windows archive runs a
program without it; `aura build` and `aura run --backend direct` print the
linker error.

## Verification

- Every listing that is shown with output was written to disk and run with
  the released `aura 0.16.0-preview` Linux archive, and the printed text was
  compared with the page. Output that depends on the clock, on random
  identifiers, or on timing differs by nature.
- The servers and clients of chapter 16 ran against each other on loopback.
- The cross builds used the released target packs; the arm64 binary ran
  under `qemu-aarch64-static` and the musl binary in a `FROM scratch`
  container.
- The Windows appendix was checked against the released Windows archive run
  under Wine and against the install guide, not on a Windows machine.
- `scripts/check_doc_examples.py` over the book, and
  `scripts/generate_llms.py`.

## Compiler Findings

Found while writing the samples and not fixed here:

- A comprehension whose element is an indexed clone,
  `[ids[i].clone() for i in range(2)]`, fails with AU4001 "invalid MIR loan
  flow". The loop form works.
- `socket.send_bytes(address, [100, 105], timeout=1s)` with an inline list
  of integer literals passes the checker and traps at run time with AU4001
  "expects list[uint8]".
- A static musl program run where `/proc` is not mounted gets an empty
  `sys.args()`.

## Thinner Than The Section Titles Suggest

The book says so in each place: logs, spans, and counters are single JSON
lines on standard error with no timestamps, level filter, or exporter;
`control.retry` retries every `Err` and adds no jitter; `time` has no named
zones or month arithmetic; `regex` has no backreferences or look-around;
`random` has no distributions; cross builds go from a Linux host to Linux
targets only.
