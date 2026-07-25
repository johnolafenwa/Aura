# Batch 2 backend-default amendment

## Goal

Close the Batch 2 checkpoint discrepancy between the original interim-`auto`
roadmap clause and the implemented fast `mir` default for `aura run`, while
preserving the native-oriented `aura build` path and all backend-parity gates.

## Work completed

- Added Accepted ADR-0031, which ratifies `mir` as the `aura run` default and
  `auto` as the `aura build` default.
- Kept `mir`, `direct`, and `auto` available explicitly for `aura run`; only
  `auto` may visibly degrade from direct execution to MIR.
- Made forced MIR/direct parity independent of the default and retained it as a
  supported-platform release gate.
- Defined the evidence required for any later native-default proposal:
  supported-platform parity, current cold and warm launch measurements,
  acceptable artifact size, reliable cache behavior, and separate ratification.
- Aligned the normative Manual, Learn guide, root README, Phase 4 work notes,
  checkpoint report, ADR index, and executable reference guard.
- Corrected the conditional-expression coverage row and the final repeatable
  coverage baseline in the checkpoint report.
- Reclassified the stale in-progress Batch 2 task-board section and its two
  remaining `Active` labels as completed.

## Verification

- `npm run check:reference`
- `npm run docs:build`
- `git diff --check`
- `npm run ci`

The compiler behavior is unchanged. Existing CLI unit and product tests already
pin the accepted default, selector behavior, visible auto fallback, forced
direct failure, and backend parity.

## Follow-up

Do not promote `aura run` to a native-oriented default solely because parity is
green. Bring a new decision with the complete ADR-0031 product-readiness
evidence.
