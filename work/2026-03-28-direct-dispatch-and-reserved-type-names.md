# 2026-03-28: Direct Dispatch And Reserved Type Names

## Goal

Close the remaining review gaps around native direct builds for bounded generic trait dispatch and the cryptic `Task` type-name collision diagnostic.

## Work Completed

- Added a maintained example for bounded generic trait dispatch across multiple concrete receiver types at `examples/traits/generic_dispatch_multiple_types.au`.
- Added direct-build CLI coverage for that example so native backend regressions now fail at the product layer.
- Fixed the direct native backend's multi-candidate dynamic-method dispatch chain so later receiver checks continue from the previous fallback block instead of generating invalid Cranelift control flow.
- Replaced the direct backend's old compile-time bailout for multi-candidate dispatch with emitted fallback trap code, which lets the backend build valid dynamic dispatch chains.
- Added a compiler regression fixture for user-defined `Task` declarations and now reject built-in type names up front for user-defined classes, enums, and traits.
- Updated the examples/tutorials and task board so the maintained surface reflects both the new runnable trait example and the reserved built-in type-name rule.

## Verification

- `cargo test -p aura build_with_direct_backend_supports_multi_type_trait_dispatch_example -- --nocapture`
- `cargo test -p aurora-compiler --test fixtures check_fail_fixtures_match_expected_diagnostics -- --nocapture`
- `cargo run -p aura -- build -o /tmp/generic_dispatch_multi examples/traits/generic_dispatch_multiple_types.au`

## Follow-Up

- Keep broadening direct-backend product coverage whenever a new maintained trait/generic example is added.
- Consider surfacing Cranelift verifier details behind a debug flag if future direct-backend regressions need deeper field diagnostics without increasing normal CLI noise.
