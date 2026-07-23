## Goal

Close the third-pass externally reviewed correctness, ownership, process/runtime, and networking defects end to end, with failing regressions first and full verification at the end.

## Work Completed

- Added failing-first regressions for the third-pass ownership and inference defects:
  - `match_borrow_mut_rebinding_moves_value` now rejects rebinding a non-`Copy` payload local from a sibling value and then using the source again.
  - `nested_borrow_and_consume_binary_operand` now rejects borrow-vs-move aliasing in either binary-operand order instead of only the syntactic order that happened to be visited first.
  - `option_none_binding_annotation` now locks in annotation-directed `Option.None` resolution.
  - The generic-class arithmetic run-pass fixture now covers the inferred constructor path, not only the annotated one.
  - The nested-pattern exhaustiveness fixture now covers multiple inner variants beneath one outer variant without requiring the user to rewrite into a second nested `match`.
- Reworked the semantic checker so ownership and match analysis close the new holes without reintroducing the earlier regressions:
  - sibling expression analysis now collects nested borrowed places from calls and method receivers before approving a move;
  - both statement-form and expression-form `match` merge move-state across arms;
  - nested enum-pattern coverage is unioned across arms for exhaustiveness;
  - payload-free enum variants such as `Option.None` resolve through the annotated target type in binding position.
- Fixed inferred generic-class constructor typing in MIR lowering so field arithmetic and comparisons on inferred generic class instances lower as ordinary scalar operations instead of leaking unsupported synthetic member calls.
- Fixed direct-backend trait impl resolution so it now prefers the most specific applicable impl instead of whichever impl happened to be declared first.
- Hardened the runtime process/networking layer:
  - `net.unix_listen(...)` now rejects any pre-existing filesystem entry, including live socket paths, instead of unlinking and stealing the endpoint;
  - TLS listeners now complete the server handshake during `accept(...)` with a bounded handshake timeout instead of deferring it indefinitely to later I/O;
  - restart-enabled supervisors now require a positive backoff floor to prevent zero-delay crash loops from becoming fork bombs;
  - HTTP request header names are now validated as ASCII token characters and header values reject non-ASCII bytes;
  - direct-backend filesystem `read_to_string` / `read_bytes` now honor the maintained 1 MiB read cap instead of bypassing it.
- Updated the maintained I/O tutorial to document the enforced supervisor backoff requirement and the handshake-at-accept behavior for TLS listeners.

## Verification

- Targeted regressions and external repros:
  - `cargo run -q -p aura -- check /tmp/aurora_tests/third_pass/ownership/26_order_hole_basic.au`
  - `cargo run -q -p aura -- check /tmp/aurora_tests/third_pass/ownership/51_minimal_divergence.au`
  - `cargo run -q -p aura -- check /tmp/option_none_bind.au`
  - `cargo run -q -p aura -- run crates/aurora-compiler/tests/fixtures/run-pass/generic_class_field_arithmetic.au`
  - `cargo run -q -p aura -- run crates/aurora-compiler/tests/fixtures/run-pass/nested_match_patterns_discriminate_same_outer_variant.au`
  - `cargo run -q -p aura -- run /tmp/aurora_tests/third_pass/sec_io/unix_live_socket.au`
  - `cargo run -q -p aura -- run /tmp/aurora_tests/third_pass/sec_io/sup_fork_bomb_throttle.au`
  - `cargo run -q -p aura -- run /tmp/aurora_tests/third_pass/sec_io/tls_silent.au`
  - `cargo run -q -p aura -- run /tmp/aurora_tests/third_pass/sec_io/http_big_body_resp.au`
- Maintained suite:
  - `cargo test -p aurora-compiler`
  - `cargo test -p aura`
  - `npm run test:lsp`
  - `npm run check:extension`
  - `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`

## Follow-up

- Remaining review notes that are still product-policy or larger-scope work rather than defects from this pass include the broader HTTP ergonomics work (`413` responses, keep-alive), additional stdlib APIs, and other previously documented language-surface limitations.
