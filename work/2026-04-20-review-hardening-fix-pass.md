## Goal

Close the externally reviewed ownership, concurrency, parser/runtime, process, and I/O/networking defects end to end, with failing regressions first and full verification at the end.

## Work Completed

- Added failing-first compiler fixtures for consume-plus-borrow call arguments, borrowed-vector iteration mutation during iteration, explicit non-copy vector indexing, and ambiguous overlapping trait impls.
- Tightened the semantic checker so by-value call arguments conflict with borrowed aliases in the same call, borrowed vector iteration forbids mutation of the iterated vector inside the loop body, non-copy `Vec` indexing requires an explicit `get(...)` path instead of implicit cloning, and equally specific overlapping trait impls are rejected while more-specific impls beat blanket impls deterministically.
- Changed `TaskGroup` scope cleanup in both maintained runtime paths to cancel blocked children before joining, fixed sleep cancellation propagation so cancelled sleepers surface task cancellation instead of resuming normal execution, and improved queue receive fairness so one blocking consumer does not starve the rest of the group.
- Hardened the HTTP client request builder against CR/LF header injection, added size caps to unbounded `read_all` paths, fixed large TCP writes and large HTTP response bodies on the evented runtime path, and corrected filesystem directory errors so `remove_file` on directories and `create_dir` on existing directories surface precise outcomes.
- Added left-associative parser chain limits plus larger-stack harness execution where needed, restored clean recursion-depth diagnostics in both MIR and direct native execution, and updated the direct native runtime and exported entrypoints to remove the latent fake-`'static` task-context reference and harden the direct root entry ABI.
- Fixed websocket runtime stability by moving client and server handshakes onto the blocking-I/O pool and constructing explicit client requests with secure random websocket keys instead of relying on the coroutine stack to host the tungstenite handshake.
- Added lexer support for UTF-8 BOM stripping, `\0` / `\xNN` / `\u{...}` escapes, f-string `{{` / `}}` escapes, and clearer float-exponent diagnostics.
- Updated maintained examples, tutorials, and compiler-bridge/LSP tests to match the hardened semantics, including explicit `Vec.get(...)` usage where implicit cloning used to be accepted and updated editor regression coverage for set/map-entry completions and indexed lookup analysis.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler --test fixtures check_fail_fixtures_match_expected_diagnostics -- --nocapture`
- `cargo test -p aurora-compiler --test fixtures run_fail_fixtures_match_expected_diagnostics -- --nocapture`
- `cargo test -p aurora-compiler runtime_scheduler_wakes_sleep_on_cancellation -- --nocapture`
- `cargo test -p aurora-compiler read_all_surfaces_size_limits_for_unbounded_resources -- --nocapture`
- `cargo test -p aurora-compiler tcp_and_http_helpers_handle_large_payloads -- --nocapture`
- `cargo test -p aurora-compiler lightweight_tasks_observe_blocking_io_completion_before_parent_timeout -- --nocapture`
- `cargo test -p aurora-compiler lightweight_scheduler_handles_http_after_blocking_io_server_step -- --nocapture`
- `cargo test -p aurora-compiler additional_categorized_examples_run_with_expected_output -- --nocapture`
- `cargo test -p aurora-compiler async_file_io_keeps_the_scheduler_running_while_a_fifo_read_waits -- --nocapture`
- `cargo test -p aurora-compiler mir_runtime_reports_recursion_limit_before_overflowing_the_host_stack -- --nocapture`
- `cargo test -p aurora-compiler broad_scratch_corpus_runtime_paths_do_not_panic -- --nocapture`
- `cargo test -p aurora-compiler`
- `cargo test -p aura task_group_scope_exit_cancels_blocked_children -- --nocapture`
- `cargo test -p aura check_rejects_huge_left_associative_expression_chains_without_crashing -- --nocapture`
- `cargo test -p aura direct_backend_reports_recursion_overflow_without_signalling -- --nocapture`
- `cargo test -p aura queue_consumers_share_work_without_starvation -- --nocapture`
- `cargo test -p aura cancelled_sleeping_children_stop_without_leaking_scheduler_panics -- --nocapture`
- `cargo test -p aura large_http_responses_complete_without_timing_out -- --nocapture`
- `cargo test -p aura`
- `cargo run -q -p aura -- run crates/aurora-compiler/tests/fixtures/run-pass/default_trait_methods.au`
- `cargo run -q -p aura -- run examples/io/websocket_roundtrip.au`
- `cargo run -q -p aura -- run test_edge/test_recursive_medium.au`
- `npm run test:lsp`
- `npm run check:extension`
- `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`

## Follow-up

- None from the reviewed defect list. Remaining future work is separate product scope, not unfinished fallout from this pass.
