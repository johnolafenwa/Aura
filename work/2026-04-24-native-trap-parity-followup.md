# April 24 Native Trap Parity Follow-up

## Goal

Validate and fix the two Round 9 cosmetic parity findings in the native direct backend:

- cleanup traps should not replace the original body trap diagnostic
- recursion-limit unwinding through recursive `with` frames should match `aura run`

## Work Completed

- Reproduced both divergences locally with temporary Aurora programs before changing the runtime.
- Added failing-first CLI regressions for both cases.
- Recorded direct cleanup registrations with their current Aurora call depth.
- Preserved the primary runtime diagnostic while direct cleanup is being drained, so a trapping `close()` does not replace the body trap location.
- Matched interpreter behavior for recursion-limit unwinding by skipping the cleanup registration created at the saturated native call depth while still releasing its captured cleanup arguments.

## Verification

- `cargo test -p aura direct_backend_preserves_body_trap_when_cleanup_also_traps --test cli -- --nocapture`
- `cargo test -p aura direct_backend_recursion_with_with_frames_matches_run_cleanup_count --test cli -- --nocapture`
- `cargo test -p aura direct_backend_unwinds_with_resources --test cli -- --nocapture`
- `cargo test -p aura direct_backend_recursion_limit_uses_source_diagnostic --test cli -- --nocapture`
- `cargo fmt --all --check`
- `git diff --check`

## Follow-up

- The focused native cleanup/recursion parity cluster is green. The remaining long-term performance and surface-design items from the external review remain out of scope for this pass.
