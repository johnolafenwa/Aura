# ADR-0023: Byte-vector codecs and hashing policy

- Status: Accepted
- Date: 2026-07-23
- Roadmap decision: Phase 3 Bytes gap-fill policy

## Context

The Phase 3 roadmap establishes `Vec[uint8]` as Aurora's bytes type and
requires UTF-8 conversion between `String` and bytes, hexadecimal and base64
codecs, and SHA-256 over both byte vectors and strings. It reserves an encoding
parameter but deliberately leaves the public module shape, error model,
canonical spellings, malformed-input policy, ownership, output form, and
resource-failure boundary to a documented gap-fill decision.

These choices are observable protocol behavior. Permissive whitespace,
unpadded base64, a `0x` prefix, Unicode replacement, uppercase hashes, or a
hexadecimal digest result would all make otherwise identical Aurora programs
interoperate differently. The MIR and direct backends therefore need one
shared policy and one shared codec implementation.

## Accepted decision

- `Vec[uint8]` is the only built-in bytes representation. Phase 3 does not add
  a nominal `Bytes` type, implicit conversion between `String` and
  `Vec[uint8]`, or a mutable byte-buffer type.
- The public conversion methods are
  `String.to_bytes() -> Vec[uint8]` and
  `String.from_bytes(bytes: Vec[uint8]) -> Result[String, bytes.Error]`.
  `to_bytes` encodes the receiver as UTF-8. `from_bytes` performs strict UTF-8
  validation and never inserts the Unicode replacement character.
- The public `bytes` module contains:

  - `bytes.hex_encode(value: Vec[uint8]) -> String`
  - `bytes.hex_decode(text: String) -> Result[Vec[uint8], bytes.Error]`
  - `bytes.base64_encode(value: Vec[uint8]) -> String`
  - `bytes.base64_decode(text: String) -> Result[Vec[uint8], bytes.Error]`
  - `bytes.sha256(value: Vec[uint8]) -> Vec[uint8]`
  - `bytes.sha256_string(text: String) -> Vec[uint8]`

- `bytes.Error` is the enum
  `InvalidUtf8(index: int32)`,
  `InvalidHexLength(length: int32)`,
  `InvalidHexDigit(index: int32, byte: uint8)`, and
  `InvalidBase64(index: int32)`.
  Indices and lengths are zero-based UTF-8 byte positions or byte counts, not
  Unicode-scalar positions. A missing base64 byte is reported at the position
  immediately after the final input byte.
- `String.to_bytes` preserves the exact UTF-8 encoding, including embedded
  NULs and a leading U+FEFF. `String.from_bytes` preserves valid input exactly;
  it performs no Unicode normalization and gives a leading UTF-8 BOM no
  special treatment.
- Hexadecimal encoding emits exactly two lowercase ASCII digits per input
  byte. Decoding accepts lowercase and uppercase ASCII digits. It rejects a
  `0x` prefix, separators, whitespace, signs, and non-ASCII text. An odd byte
  length produces `InvalidHexLength` before digit validation; otherwise the
  first invalid byte produces `InvalidHexDigit`.
- Base64 uses the RFC 4648 standard alphabet with canonical `=` padding.
  Encoding always emits canonical padding. Decoding rejects URL-safe alphabet
  characters, whitespace, ignored separators, missing padding, excess
  padding, nonzero discarded bits, and trailing data. Empty input is valid.
- SHA-256 is the FIPS 180-4 SHA-256 function. Both hash functions return a
  fresh 32-element `Vec[uint8]` containing the raw digest. They do not return
  hexadecimal text; callers that need that representation compose
  `bytes.hex_encode(bytes.sha256(...))`.
- `bytes.sha256_string(text)` hashes exactly the UTF-8 bytes that
  `text.to_bytes()` returns. It performs no normalization, case folding,
  newline conversion, or implicit terminator insertion.
- All plain `String` and `Vec[uint8]` inputs use Aurora's ordinary shared
  parameter mode. These calls do not consume or mutate their receivers or
  arguments, and every successful conversion, decode, encode, or hash result
  is a fresh owned value.
- The `encoding` parameter is reserved, not implemented. In Phase 3,
  `to_bytes` accepts no arguments and `from_bytes` accepts only `bytes`;
  positional or named `encoding` arguments are rejected by ordinary static
  argument checking. A later compatibility decision may add the parameter
  without changing the meaning of calls accepted here.
- Malformed UTF-8, hex, and base64 are typed `bytes.Error` values because
  callers commonly process untrusted data. Output-size overflow, failure to
  represent an expanded encoded value, and allocation failure trap with
  `AU4005`; they do not add resource variants to `bytes.Error`. Encoders
  preflight their expanded output size before allocation.
- The shared codec layer preserves that boundary explicitly: malformed data is
  returned as a data error, while output-size and allocation failures are
  returned as a distinct resource error for both backend adapters to map to
  `AU4005`. Every allocating operation uses a fallible reservation. Decoders
  validate malformed input before reserving output, so an invalid encoding
  deterministically produces `bytes.Error` rather than depending on host
  memory pressure.
- No additional Phase 3 byte-count cap is imposed below Aurora's existing
  representability limits. Hex output size is `2 * input_bytes`; padded base64
  output size is `4 * ceil(input_bytes / 3)`; decoders check their destination
  size before allocation. Exact representable boundaries succeed when
  allocation succeeds.
- The MIR and direct backends call the same UTF-8, hex, base64, and SHA-256
  codec helpers and expose identical values, errors, offsets, and diagnostics.
  Built-in behavior is attached to compiler-synthesized declarations, so a
  user module named `bytes` does not acquire the built-in API by textual name.
- SHA-256 is a general-purpose digest, not a password hash, message
  authentication code, signature, encryption primitive, or source of
  randomness. Phase 3 does not add HMAC, incremental hashing, constant-time
  digest comparison, alternate base64 alphabets, streaming codecs, or
  non-UTF-8 text encodings.

These choices were accepted at the Batch 3 entry checkpoint. The
compiler, both maintained backends, analysis service, fixtures, maintained
example, and executable Manual fence implement and pin this contract.

## Completion tests

- Pure codec tests pin exact UTF-8 bytes, embedded NUL and non-ASCII text,
  strict invalid UTF-8 with the first invalid offset, empty inputs, every byte
  through hex and base64 round trips, lowercase hex output, mixed-case hex
  input, and rejection of prefixes, separators, whitespace, and odd lengths.
- Base64 tests pin the RFC 4648 vectors, required canonical padding, the
  standard alphabet, rejection of nonzero discarded bits, invalid-byte
  offsets, and binary data that is not valid UTF-8.
- Hash tests pin the empty and `abc` SHA-256 vectors, raw 32-byte output,
  UTF-8 string equivalence, no input mutation, and composition with
  `hex_encode`.
- Boundary and injected-allocation tests pin preflight arithmetic, the exact
  representable output limits, `AU4005` immediately above them, and no partial
  result. Tests must not perform multi-gigabyte success allocations merely to
  prove a boundary.
- Static and fixture tests pin all signatures, argument names, result types,
  shared-input ownership, named arguments, the reserved encoding-argument
  rejection, each typed error variant, and user-module origin isolation.
- MIR and direct run-pass and check-fail fixtures pin identical encoded bytes,
  decoded values, digest bytes, typed errors, offsets, reserved-argument
  diagnostics, and ownership behavior. Focused adapter tests use deterministic
  allocation-failure injection to pin identical `AU4005` resource diagnostics
  without attempting a multi-gigabyte fixture allocation.
- Analysis and language-server tests pin the `bytes` module, `bytes.Error`,
  String methods, module functions, parameter names, return types, completion,
  hover, and canonical identity through imports.
- The Manual, API index, current limits, one maintained bytes example, and the
  executable reference fence describe and exercise the same public contract.
