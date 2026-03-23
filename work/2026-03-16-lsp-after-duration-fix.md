# 2026-03-16 LSP After/Duration Fix

## Summary

- Fixed false VS Code diagnostics in `examples/concurrency/select_send.au` for `after` and `5ms`.
- Added regression coverage against the real example in the Aurora language-server test suite.

## Root Cause

- The language server did not treat `after(...)` as a builtin callable during diagnostics or hover.
- The identifier-chain scanner incorrectly extracted `ms` as a standalone identifier from the duration literal `5ms`.

## Changes

- Added `after(duration: Duration) -> Duration` to the builtin function table in `tools/aurora-language-server/src/analysis.js`.
- Updated identifier-chain scanning so alphabetic suffixes immediately following numeric literals are skipped instead of being diagnosed as names.
- Added regression assertions in `tools/aurora-language-server/test/analysis.test.js` to ensure `select_send.au` does not report `unknown name \`after\`` or `unknown name \`ms\`` and that hover resolves `after(...)`.

## Verification

- `npm run test:lsp`
- `npm run check:extension`
- `npm run test:extension`
