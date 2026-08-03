# ADR-0057: Clean-slate pre-adoption policy

- Status: Accepted
- Date: 2026-08-03
- Roadmap decision: Batch S1.1, Aura 0.3 checkpoint closure
- Supersedes: compatibility windows, aliases, and retirement paths previously
  proposed for pre-adoption Aura surfaces

## Context

Aura has no users whose source code must be preserved. Carrying compatibility
machinery for syntax and APIs that never formed an adopted language contract
would make the compiler, diagnostics, reference, and implementation describe
several languages at once.

During Batch S1 the user directed that the 0.3 surface be treated as a clean
language design. On 2026-08-03 the user confirmed this as the standing policy
and requested an ADR so later work cannot reinterpret precise errors for old
spellings as a compatibility obligation.

## Decision

Until the user explicitly replaces this decision, a source change made before
Aura has users lands as the only maintained language surface:

- old syntax, names, methods, manifests, environment variables, and other
  product identities are treated as though Aura never supported them;
- the compiler and tooling provide no aliases, shims, compatibility modes,
  grace periods, reserved retired spellings, deprecation paths, or specialized
  old-to-new diagnostics and fix-its;
- an old spelling receives only the ordinary diagnostic appropriate to an
  unknown or malformed program;
- maintained documentation contains no migration guidance or compatibility
  narrative for the superseded surface; and
- internal one-shot scripts may update this repository atomically, but they
  are not shipped or documented as user migration tools.

Historical ADRs and work notes remain historical records. Their compatibility
proposals are not rewritten, but this ADR supersedes those proposals wherever
they would expose a second maintained path in Aura 0.3.

This decision does not claim that breaking changes are harmless after Aura has
users. A future adoption and stability policy requires a new explicit ADR.

## Consequences

Every source migration uses inventory, an atomic implementation and reference
flip, and a permanent identity gate over maintained surfaces. Tests prove the
one canonical language. Negative tests may pin ordinary parser, name, type, or
member errors, but must not preserve knowledge of a superseded spelling solely
to provide migration help.

Reviewers should reject compatibility code unless a later accepted ADR
explicitly replaces this policy.

## Verification

- `scripts/test_aura_identity.py` guards maintained source, documentation, and
  generated reference artifacts against superseded product and language
  identities.
- Feature migrations keep compiler, LSP, extension, examples, tutorials,
  Manual pages, generated LLM artifacts, and fixtures synchronized.
- Batch work notes record inventory counts and confirm that no compatibility
  path or public migration surface was added.
