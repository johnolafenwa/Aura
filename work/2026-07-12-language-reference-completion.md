# Language Reference Completion Pass

## Goal

Make the maintained Aurora reference authoritative and complete enough that a reader can derive an accurate understanding of the implemented language and use it as the source for a future language book.

## Work Completed

- Started a compiler-, runtime-, and fixture-backed conformance audit of the existing Manual.
- Added the normative specification entrypoint, complete lexical and syntactic grammar, names/scopes, static semantics, execution model, diagnostics, and conformance chapters.
- Expanded the topic chapters with precise call, ownership, match, module, runtime API, limit, rendering, CLI format, and backend contracts.
- Added a reference-integrity gate to the full repo CI path.
- Added test-first fixes for parser contextual-word/receiver behavior, duplicate parameter and trait-signature checking, bounded zero-byte reads, metrics overflow, and stale Vec hover contracts found during the audit.
- Corrected stale Learn/tutorial claims about native auto fallback, file-read streaming, resource members, and maintained execution paths.
- Documented the remaining observable 0.1 defects and boundaries, including repeated HTTP headers, `fs.read_dir` entry loss, task/resource result aliasing, blocking-worker cancellation, WebSocket cleanup/cancellation, argument transport, and timed-out test workers.

## Verification

- `npm run check:reference` passes.
- `npm run docs:build` passes.
- Parser, semantic, runtime, fixture, MIR, native/direct, package, I/O, process, and CLI focused regressions pass.
- Exact `npm run ci` passes, including serialized Rust tests, full backend parity, compiler coverage (96.05% lines / 96.87% functions / 93.95% regions), LSP 100% coverage, extension tests, reference/docs build, npm and RustSec audit policy, Clippy with warnings denied, and repository hygiene.

## Follow-up

- Generate a compiler-owned builtin API snapshot and compare it with the Manual so signature/default/hover drift is detected structurally rather than only by prose checks.
- Add stable rule identifiers and a checked rule-to-fixture/backend matrix when the 0.1 specification begins release versioning.
- Fix the runtime/tool limitations now recorded in Current Limits without weakening the reference's explicit contracts.
