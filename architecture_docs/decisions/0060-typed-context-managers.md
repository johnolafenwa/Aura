# ADR-0060: Typed context managers and cleanup

- Status: Accepted; detailed design ratified in the Batch 2-3 checkpoint (Q17-Q23)
- Date: 2026-09-06; implementation record 2026-10-11
- Implementation: Implemented in Batch 3 phase 3b
- Roadmap: Batch 3
- Related: ADR-0022, ADR-0038, ADR-0054, ADR-0058, and ADR-0059

## Authority and current boundary

The user approved richer context managers in the
[roadmap](../14-priority-roadmap.md). Existing `with` forms manage builtin
resources and eligible non-generic user classes through `close(mut self)`.
Generic entry/exit protocols and multiple-resource syntax are future work.

## Accepted decisions

Support generic managers, multiple resources in one `with`, and statically
typed entry/exit operations. Entry may fail. Register exit only after entry
succeeds; earlier successful entries must still be cleaned up if a later
entry fails. Resources acquired inside a failed entry remain subject to
ordinary ownership cleanup.

After successful entry, exit runs when the scope leaves through normal
completion, return, break/continue out of the scope, typed error propagation,
maintained runtime failure, or cancellation. Nested managed lifetimes unwind
in reverse entry order. Each successful registration exits exactly once.

Preserve an existing body failure as primary and attach cleanup failures as
additional information. A manager cannot silently suppress the body failure.
Cancellation must still perform the required cleanup. A manager may expose a
scoped resource or view when its lifetime is tied to the managed scope;
that resource cannot escape or be invalidated before exit.

The intended applications include files, locks, transactions, temporary
directories, and scoped configuration. This acceptance defines management
semantics, not a new shared-memory locking API or a transaction commit policy.

## Remaining detailed design

### Open conflicts

- **Batch 3, ADR-0038:** reconcile cleanup under cancellation with the existing
  forced-frame-reset boundary: arbitrary source cleanup cannot run against a
  destroyed generated frame. Preserve the approved cleanup promise while
  specifying how cleanup completes before that boundary.
- **Batch 3, ADR-0038:** a manager created in a `with` header must expose any
  scoped entry view through a valid origin despite the current exclusion of
  temporary view origins. The origin and lifetime contract is to be designed
  in Batch 3; do not assume an implicit temporary exemption.
- **Batch 3, ADR-0059 / ADR-0062:** reuse one partial-construction cleanup
  mechanism extending ADR-0038's ordered exit-action stack for initialization,
  multi-resource entry, and failed decoding.

### Detailed contract

Specify protocol names/signatures, result/entry-view types, associated-type
needs, multi-resource grammar/evaluation, and interoperability with existing
close-only resources. Do not silently introduce Python exception suppression
through an exit return value.

Specify how a cleanup-only failure propagates, how typed and runtime failures
compose, and how callers observe additional cleanup failures. Define bounded
cleanup/cancellation interaction without choosing a new duration policy here.
Lock guards, commit/rollback, and restoration of scoped configuration need
their own concrete resource contracts under this protocol.

## Completion evidence required

Pin generic managers, multiple resources, failed first/later entry, partial
entry acquisition cleanup, every exit path, reverse order, exactly-once exit,
body/cleanup failure precedence, cancellation, and scoped-resource escape
rejection. Prove parity through both backends and update tooling, fixtures,
examples, and reference documentation with the implementation.

## Implementation record (2026-10-11)

Phase 3b implements
[checkpoint section F](../17-batch-2-3-design-checkpoint.md#f-typed-context-managers)
with option A for Q17-Q22.

- **Protocol.** A class is a manager when it declares
  `__exit__(mut self) -> None | Result[None, E]`. An optional
  `__enter__(mut self)` returns `None` or `view [mut] T from self`. A class
  without `__exit__` that declares `close(mut self) -> None` is a manager
  whose exit is `close`. Generic classes qualify. Builtin resources keep
  their existing cleanup. Every wrong shape is `AU2017`.
- **Headers.** `with a = x, y as b:` parses into nested single-item `with`
  statements, so items enter left to right, exit in reverse, and later items
  see earlier names. A malformed list is `AU1101`.
- **Entry views.** The item name binds the `__enter__` view. The manager
  lives in a hidden managed binding that diagnostics name by its `with`
  item position.
- **Exit results.** On a normal exit a `Result` exit propagates like `try`.
  It is dropped when the body is already propagating or cancelled, and is
  attached to a trap as a note.
- **Cleanup actions (G1).** `PushCleanup` carries a typed `CleanupAction`,
  `ResourceClose` or `ManagerExit { exit_result }`, and `PopCleanup` carries
  `run`. This is the `ManagerExit` half of G1. The `DropInitialized` half is
  replaced by phase 3a's absent-field slot; see the ADR-0059 implementation
  record. The validator rejects a manager registered as a `close` resource,
  a `ManagerExit` on a non-manager place, an exit-result mismatch, and a
  registration before `__enter__` returns, at both `run_mir` and
  `emit_host_native_object`.
- **Tooling.** Hover shows the entry view for a `with` name. The semantic
  interface schema is version 19.

Details and verification are in `work/2026-10-11-batch-3-phase-3b.md`.
