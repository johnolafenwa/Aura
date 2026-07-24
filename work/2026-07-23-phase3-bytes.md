# Phase 3 Bytes, codecs, and SHA-256

## Goal

Complete the Batch 2 Phase 3 Bytes surface: use `Vec[uint8]` as the bytes
representation; add strict UTF-8 String conversion, canonical hexadecimal and
base64 codecs, raw SHA-256, typed malformed-input errors, backend parity,
analysis/editor metadata, and a reference-quality maintained documentation
surface.

## Work completed

- Added the compiler-owned `bytes` module, the copy-valued `bytes.Error` enum,
  `String.to_bytes()`, and `String.from_bytes(...)` with exact shared-input
  signatures and named-argument metadata across checking, analysis, MIR
  lowering, MIR execution, and direct native code generation.
- Added one shared fallible UTF-8/hex/base64/SHA-256 codec policy and one shared
  runtime adapter for both maintained backends. Malformed data returns exact
  typed offsets; unrepresentable or failed allocation paths use `AU4005`.
- Made decoder validation and encoder expansion preflights happen before
  destination or runtime-vector materialization. Base64 decoding now reserves
  the exact decoded length, including at padded representability boundaries.
- Closed integration defects found during independent review: pattern-bound
  Strings now route `to_bytes()` through dynamic member dispatch; a local
  value named `String` correctly shadows the builtin associated-call spelling;
  and a user module whose final component is `bytes` executes its own
  same-named function on both backends.
- Closed the final resource-order audit defect test-first: MIR now borrows both
  place-backed and literal String receivers before `to_bytes()` enters the
  fallible shared adapter, matching direct-backend behavior and avoiding an
  infallible snapshot allocation ahead of the documented `AU4005` boundary.
- Added test-first tests and fixtures for exact UTF-8 bytes (including embedded
  NUL and non-ASCII text), leading U+FEFF preservation, String round trips,
  mixed-case hex input, lowercase hex output, canonical padded base64, raw
  SHA-256 vectors, empty inputs, named calls, shared input reuse, reserved
  positional and named encoding arguments, origin isolation, value-name
  shadowing, and all four `bytes.Error` variants with pinned byte offsets.
- Added the normative Bytes Manual chapter with the complete API, malformed
  input precedence, ownership, evaluation order, diagnostics, size
  preflights, backend requirements, limits, security boundaries, and
  Provisional ADR-0023 status.
- Added the maintained `examples/bytes/codecs_and_hashing.au` program and the
  book-style `tutorials/22-bytes.md` chapter.
- Updated the Manual/API/current-limits/conformance/navigation surface,
  tutorial and example inventories, current-language recap, root example
  index, and reference-integrity metadata.
- Added compiler-backed language-server coverage for module, enum-variant,
  String associated/instance method, named-parameter, return-type, hover,
  completion, and imported canonical-identity behavior.

## Verification

- Confirmed the initial run fixture fails before implementation specifically
  because the built-in `bytes` module is absent, establishing the intended
  test-first failure.
- Confirmed the origin-isolation fixture already checks successfully against
  the pre-Bytes compiler: a local module whose final name is `bytes` keeps its
  own incompatible `hex_encode(String) -> int32` declaration and does not gain
  the compiler built-in by textual-name coincidence.
- `python3 scripts/reference_integrity.py --inventory-only` passes with the new
  feature page classified, all eight normative sections present, and its
  executable example hash pinned.
- The compiler fixture harness passes all four check categories, including the
  reserved-encoding diagnostics, shared-input reuse, and local-module
  origin-isolation fixtures.
- The full MIR run-pass fixture category passes. Its maintained loopback
  network fixture requires running outside the filesystem/network sandbox;
  the initial sandboxed `PermissionDenied` was reproduced and the same test
  passed immediately with loopback access.
- All three Bytes run fixtures produce their exact pinned stdout on both the
  MIR and direct backends. This includes the regression where a `String`
  extracted by `Result.Ok` pattern matching calls `to_bytes()`, the complete
  typed-error payload and precedence matrix, and associated-call shared-borrow
  scope and input reuse.
- The compiler-backed language-server Bytes regression passes, covering the
  canonical module, all error variants, String static/instance separation,
  exact signatures, hover data, completion data, and canonical `from` imports.
- Full executable reference integrity passes: 32 pages, 22 feature pages, 107
  verified Aurora blocks, no missing normative section, and no feature page
  without a verified example.
- `npm run docs:build`, JavaScript syntax checking for the compiler-bridge
  regression, and `git diff --check` pass.
- Focused pure-codec and runtime-adapter tests pin exact allocation ordering,
  UTF-8 first-error equivalence with the standard validator, odd-length hex
  precedence over an invalid digit, padded base64 destination length, and
  deterministic `AU4005` propagation. Source-lowering regressions also pin
  borrowed literal, formatted-temporary, returned-temporary, and
  pattern-bound String receiver routes without snapshotting the input.
- Closed the frozen compiler line-coverage gap under the standing rule with
  observable behavior and diagnostic tests for canonical padding rejection,
  runtime type errors, shared-input preservation, MIR binding/place failures,
  direct FFI dispatch, backend return typing, and the exact
  `String.from_bytes` checker diagnostic.
- Removed rather than synthetically testing branches that canonical base64
  validation made mathematically unreachable. The decoder now processes the
  already-validated quartets directly with an exactly sized fallible buffer,
  and the shared one-argument adapter no longer repeats arity validation
  already performed by every maintained entry point.
- `npm run coverage:compiler:check` passes at 60,768/63,252 lines
  (96.072851451337%), 3,968/4,091 functions (96.993400146663%), and
  88,637/94,027 regions (94.267603986089%) against the frozen
  96.06/96.79/94.15 floors. All 251 instrumented CLI tests, 781 compiler
  library tests, and supporting instrumented suites pass. No synthetic
  coverage test or coverage exclusion was added.
- The exact-tree full `npm run ci` decision gate passes with the complete
  maintained compiler, backend-parity, language-server, extension, reference,
  documentation, audit, Clippy, coverage, and hygiene checks.

## Follow-up

- Present ADR-0023 with the other Provisional gap-fill decisions at the Phase 3
  checkpoint.
- Continue Phase 3 in the ratified order with `assert`, followed by the
  retry-worker gate.
