# ADR-0031: CLI backend defaults

- Status: Accepted
- Date: 2026-07-25
- Roadmap decision: Batch 2, Phase 4 checkpoint amendment

## Context

The original Phase 4 roadmap introduced `aura run --backend mir|direct|auto`
and named `auto` as the interim default until forced-direct parity was proven
on every supported platform. Implementation and benchmark work then exposed a
product distinction that the roadmap did not account for: `aura run` is the
interactive edit-run path, while `aura build` is the artifact-producing path.

On the checkpoint workstation, a MIR hello-world run took about 0.01 seconds.
A direct cold compile and launch took about 1.3 seconds. The content-addressed
cache removed compilation and linking on a hit, but the first launch of a fresh
direct binary still took about 0.8 seconds because the statically linked binary
was about 57 MB. Programs changed during normal development continually produce
new cache keys, so making `auto` the `run` default would impose the native cold
path on the workflow most sensitive to latency.

Backend parity remains a correctness requirement regardless of which backend a
command selects by default. The forced MIR/direct fixture matrix is therefore a
release gate, not a consequence of the default.

## Decision

- Bare `aura run` defaults to the MIR runtime.
- `aura run --backend mir` selects MIR explicitly.
- `aura run --backend direct` requires a direct native build and reports build
  or launch failure instead of degrading.
- `aura run --backend auto` prefers direct native execution and visibly reports
  any fallback to MIR.
- Bare `aura build` continues to default to `auto`; `--backend direct` remains
  available when fallback is unacceptable.
- The forced MIR/direct parity matrix must pass on every supported CI platform
  independently of these defaults.
- Promoting `aura run` to a native-oriented default requires a separate
  accepted decision backed by supported-platform parity, current cold and warm
  launch measurements, acceptable artifact size, and reliable cache behavior.

This decision explicitly amends only the original roadmap's interim `auto`
default for `aura run`. It does not weaken the backend selector, visible
fallback, cache, or parity requirements.

Batch 3 hardened the cache requirement without changing this default split.
Every hit now verifies key-bound entry metadata, bounded regular files, the
artifact's recorded digest, and native launch state, then launches a private
copy of the verified bytes without an ENOEXEC shell fallback. Corrupt or
format-invalid entries rebuild; environmental launch failures preserve valid
cache state. Keys bind the exact runtime archive bytes and ordered native link
arguments used by the cold build. An inherited launch lease prevents
interrupted-parent cleanup from removing an executable while its native child
is still live. The cache root is explicitly a private, trusted-user boundary.
This work intentionally invalidates the earlier resident-cache latency as a
current guarantee: verified hits measured `0.81s` on the checkpoint workstation
after development-profile SHA-256 optimization. Any future native-default
proposal must benchmark this integrity-preserving path, not the pre-verification
Phase 4 implementation.

## Consequences

The common edit-run loop remains fast and predictable. Users can still exercise
or require native execution explicitly, and release gates continue to catch any
semantic disagreement between MIR and direct execution.

`aura run` and `aura build` intentionally have different defaults because they
serve different jobs. A future native-default proposal must demonstrate product
readiness as well as correctness rather than treating parity alone as a latency
or distribution argument.

## Completion tests

- `run_backend_parsing_defaults_to_mir_and_accepts_every_selector` pins the
  default and all three selector spellings.
- `only_auto_run_degrades_to_the_mir_runtime` and
  `forced_direct_backend_never_invokes_fallback` pin fallback behavior.
- The CLI backend-selector regression pins stdout, arguments, and exit status
  across the default, MIR, direct, and auto paths.
- The ignored forced-backend parity test is invoked explicitly by the full
  repository gate on each supported CI host.
- The CLI README, Learn guide, normative Manual, checkpoint report, and work
  tracking state record the accepted split.
