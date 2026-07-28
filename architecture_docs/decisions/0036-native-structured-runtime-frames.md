# ADR-0036: Native structured runtime frames

- Status: Provisional
- Date: 2026-07-28
- Roadmap decision: Batch 4, Phase 5.10
- Related: ADR-0011, ADR-0032, and ADR-0033

## Context

Aurora runtime traps already carry a stable diagnostic code, message, primary
source span, semantic notes, help, and edits. The MIR runtime also records the
active Aurora call chain and the ancestry of a failing structured child task.
It currently flattens that information into three human-oriented strings in
the diagnostic `notes` array:

```text
Aurora call chain (innermost first): ...
Aurora task entry: ...
Aurora task ancestry (youngest first): ...
```

Those strings are useful to a person but are not a tooling contract. The
Manual therefore tells tools not to parse them. They also exist only on the
MIR path. The direct runtime receives a function name and declaration span
when generated code enters a call, but currently retains only a numeric depth.
Its task-start ABI likewise discards the start expression's span and carries
no task-entry or parent metadata. Forced-backend parity consequently removes
the three MIR-only note families before comparing runtime failures.

The missing native frames also expose a CLI transport problem. A direct
program renders a trap to its inherited standard error and exits with status
`1`. The parent `aura run --format json` process sees only that status, which
is indistinguishable from a successful Aurora `main() -> int32` returning
`1`. It cannot recover a structured diagnostic by parsing human text. If the
parent also buffered a native-build progress notice, standard error can contain
human diagnostic text followed by a JSON status document instead of the
promised single JSON document.

Phase 5.10 establishes one compiler-owned frame model for MIR, direct-native,
standalone, CLI JSON, and editor-tooling surfaces. It does not expose host Rust,
Cranelift, scheduler, or service-thread backtraces.

## Provisional decision

### Structured frame records

Every compiler diagnostic has two frame-list fields:

```text
call_frames: RuntimeCallFrame[]
task_ancestry: RuntimeTaskFrame[]
```

Their conceptual records are:

```text
RuntimeCallFrame {
    function: String,
    span: SourceFrameSpan
}

RuntimeTaskFrame {
    task_function: String,
    task_entry_span: SourceFrameSpan,
    parent_function: String,
    spawn_span: SourceFrameSpan
}

SourceFrameSpan {
    path: String,
    start: { line: integer, column: integer },
    end: { line: integer, column: integer }
}
```

CLI diagnostic JSON uses one-based line and column positions, matching its
existing primary and secondary spans. A runtime frame currently identifies a
point span, so its end is one column after its start. This does not imply that
the whole function name or call expression is one column wide; a later
token-range improvement may widen the end without changing the record shape.

Each span carries its own source path. A call frame uses the path of the module
that defines the function. A task-entry span uses the target function's
defining path, while a spawn span uses the path of the spawning function.
Imported-module frames must never be labelled with the entry module's path.
The virtual path supplied with `--stdin` is a real path for this purpose.

Internal source-only helpers such as `run_source` may construct a program
without a stored file path. A raw internal frame may retain an absent path in
that case, and `structured(path)` or `render_with_source(path, source)` uses
its caller-supplied path as the fallback. File-backed, package-backed,
stdin-with-virtual-path, and built-program execution must retain exact
per-module paths rather than taking that fallback.

Both arrays are present even when empty. Compile-time diagnostics,
pre-user-code runtime-configuration diagnostics, and runtime failures outside
an Aurora function therefore serialize `call_frames: []` and
`task_ancestry: []`.

### Ordering and meaning

`call_frames` is ordered innermost first. It contains the Aurora functions
that were successfully entered and remain active when the primary trap is
established. The primary diagnostic span continues to identify the faulting
operation; a call-frame span identifies the corresponding function entry.

A maximum-call-depth failure does not add the rejected attempted callee as an
active frame because that call was not entered. Its primary diagnostic still
names and locates the attempted call.

`task_ancestry` is ordered youngest first. Its first item describes the
currently failing spawned task. Each later item describes the task that
spawned the preceding item, continuing toward the Aurora root. The first
item's `task_function` and `task_entry_span` are also the task-entry record;
there is no redundant third structured field. Root execution has an empty
ancestry even though the runtime may internally host it on a scheduler task.

A child begins a new call chain. Parent call frames are not copied into the
child's `call_frames`; the parent relationship is represented only by
`task_ancestry`. Runtime helpers, service workers, scheduler functions, host
Rust frames, and generated Cranelift implementation frames never appear.
Function names use the compiler's existing Aurora diagnostic display names,
including its ordinary qualification or specialization where required to
distinguish source functions.

### Once-only capture and propagation

The runtime snapshots both frame lists exactly once when the primary runtime
diagnostic is established. Capture happens before any of these can alter task
state:

- source-level cleanup
- direct cleanup-stack draining
- cancellation cleanup
- generated-stack reset
- scheduler retirement or forced-exit containment
- task-local runtime-state discard
- propagation to another task or worker

The diagnostic carries a private, non-serialized capture marker. Empty arrays
are a valid completed snapshot, so emptiness is not a sufficient once-only
test. A diagnostic that already has a completed snapshot propagates unchanged
through callers, Task results, TaskGroup failure observation, workers, and
backend boundaries; an observer must not append its own frames.

If a cleanup establishes the first trap, its active Aurora frames are
captured. If a body trap is already primary, a later cleanup trap cannot
replace either the primary diagnostic or its captured frames.

### MIR and direct-native state

The MIR runtime retains typed call and task records instead of formatting them
inside trap annotation. MIR functions preserve their defining source path,
and a task start records both the target entry and the exact parent start
site.

Direct task-local runtime state retains the active call-frame stack and task
ancestry in addition to its existing depth, cleanup, cancellation, and primary
diagnostic state. Generated call entry pushes a complete frame only after the
depth check succeeds; normal call exit pops exactly one frame. Task identity,
not worker thread identity, owns this state because several suspended
coroutines can share one pinned worker.

Direct task-start lowering forwards the target function's name, defining path,
and entry span, plus the active parent function and the start expression's
path and span. The child receives a cloned ancestry with its own youngest
record installed before its thunk begins. This metadata may cross a worker
boundary but must not make a coroutine stack migratable. The extended
codegen/runtime ABI remains private and unstable.

### Human diagnostics

Structured frame records, not prose notes, are the source of truth. Human
rendering synthesizes the existing lines exactly:

```text
note: Aurora call chain (innermost first): ...
note: Aurora task entry: ...
note: Aurora task ancestry (youngest first): ...
```

Existing non-frame notes retain their order. The renderer emits them first,
then the synthesized call-chain line when `call_frames` is non-empty, then the
task-entry and task-ancestry lines when `task_ancestry` is non-empty, followed
by help and edits. The synthesized strings do not appear in the structured
`notes` array and therefore are never duplicated in JSON or LSP metadata.

This rule removes only generated frame prose from `notes`. Existing
operational strings, including buffered native rebuild/wait progress and
direct-to-MIR fallback detail on a failed JSON invocation, retain their
current documented placement in `notes`.

Human frame lines continue to show each frame as `function at line:column`.
The typed fields carry the complete per-frame path for tools. The existing
human spelling and ordering remain byte-compatible while the structured form
becomes unambiguous across modules.

### Public JSON compatibility

The public diagnostic document remains schema version `1`. Each diagnostic
entry adds the two arrays without changing its existing code, severity,
message, primary span, secondary spans, notes, help, or edits:

```json
{
  "schema_version": 1,
  "diagnostics": [
    {
      "code": "AU4003",
      "severity": "error",
      "message": "vector index `9` is out of bounds for length `2`",
      "primary_span": {
        "path": "/workspace/worker.au",
        "start": { "line": 3, "column": 18 },
        "end": { "line": 3, "column": 19 }
      },
      "secondary_spans": [],
      "notes": [],
      "help": [],
      "edits": [],
      "call_frames": [
        {
          "function": "child",
          "span": {
            "path": "/workspace/worker.au",
            "start": { "line": 1, "column": 1 },
            "end": { "line": 1, "column": 2 }
          }
        }
      ],
      "task_ancestry": [
        {
          "task_function": "child",
          "task_entry_span": {
            "path": "/workspace/worker.au",
            "start": { "line": 1, "column": 1 },
            "end": { "line": 1, "column": 2 }
          },
          "parent_function": "main",
          "spawn_span": {
            "path": "/workspace/main.au",
            "start": { "line": 8, "column": 15 },
            "end": { "line": 8, "column": 16 }
          }
        }
      ]
    }
  ]
}
```

This is an additive schema-1 extension. A schema-1 reader must ignore
unrecognized object members while continuing to require the fields it uses.
The frame arrays are nevertheless always emitted by the updated compiler so a
consumer need not distinguish absence from an empty stack.

A successful progress-only direct-run status document remains distinct from a
diagnostic report and need not invent a `diagnostics` member. Existing
single-document progress and fallback shapes remain unchanged.

### JSON-mode native diagnostic channel

A CLI-managed direct run in JSON mode establishes one private inherited
diagnostic channel before launching either a newly built or a verified cached
binary. On an Aurora runtime trap, the native runtime writes one bounded,
compiler-owned structured diagnostic record to that channel instead of
rendering human diagnostic text on the child's standard error. The `aura`
parent validates that record and emits the one public schema-version-1
document.

The channel is an internal execution mechanism, not a public environment,
foreign-function, or file-format API. Human-mode `aura run` does not require
it. A standalone built program without the channel continues to render its
human diagnostic directly to standard error.

Native execution has three distinct outcomes:

- ordinary completion with the Aurora program's requested integer status
- an Aurora runtime trap carrying a `Diagnostic`
- a host build, launch, wait, or diagnostic-channel failure

An ordinary nonzero `main() -> int32` result is not a trap and must not produce
a diagnostic record. Conversely, `auto` must not interpret an Aurora trap as a
backend failure or fall back to MIR. Backend fallback remains limited to
failure to build or launch the selected backend.

Program standard output and ordinary status semantics remain unchanged.
Buffered JSON progress retains the existing one-document rules: successful
direct progress remains a top-level `progress` array, successful automatic
fallback retains top-level `progress` and `fallback`, and progress associated
with a failed diagnostic remains in its non-frame `notes`.

### LSP and compiler-service compatibility

The compiler analysis representation carries the same two always-present
arrays in its zero-based editor-coordinate shape. A frame span there contains
`file_path`, `line`, `start_character`, and `end_character`. The JavaScript
bridge preserves the arrays in LSP `Diagnostic.data`; it does not parse the
human strings or reconstruct runtime frames independently. When consuming an
older compatible record that omits the additive fields, the bridge treats
them as empty.

The compiler-service semantic-interface version remains `2`. These additive
diagnostic fields neither change checked-source meaning nor incompatibly
change ownership metadata. Advancing that version would also invalidate every
native artifact because the semantic identity is part of the native cache
key, which is neither necessary nor desirable for an additive diagnostic
extension. A future incompatible semantic-service change still requires the
ordinary version bump.

The current editor service normally publishes compile-time diagnostics, so
its frame arrays are empty. Carrying the shape now prevents a second bridge
contract when a future editor workflow presents runtime diagnostics.

### Backend parity and compatibility

MIR, `aura run --backend direct`, and standalone direct binaries produce the
same Aurora frame content and human spelling for the same trap. The forced
backend parity gate removes its three-note exception and compares the complete
runtime diagnostic. Runtime-failure fixture oracles likewise pin complete
human output without filtering frame lines.

No new `AU####` code is introduced. Existing primary diagnostic codes,
messages, spans, non-frame notes, help, edits, partial standard output,
cleanup precedence, and integer exit behavior remain unchanged. The only
structured compatibility change is the additive pair of frame arrays and the
removal of generated frame prose from `notes`; the existing Manual already
forbids tools from parsing those prose strings.

## Consequences

Native traps become as useful as MIR traps without exposing unstable host
backtraces. Tools receive exact names, paths, ordering, and task relationships
without scraping human prose. Human users keep the established compact
backtrace spelling, and a direct JSON trap again satisfies the one-document
standard-error contract.

Per-task direct runtime state grows with active Aurora call depth and task
ancestry. Call depth is already bounded. Task ancestry retains the complete
existing MIR relationship rather than silently truncating it. Frame metadata
must be released with the same exact-once task-state containment as cleanup
and diagnostic state.

The JSON-only channel adds a private parent/child protocol and failure mode.
Malformed, oversized, missing, or multiply emitted records are host execution
failures; they may not be accepted as a partial Aurora diagnostic or confused
with an ordinary nonzero result.

This decision does not add source syntax, user-visible reflection over call
stacks, host backtraces, debugger APIs, exception catching, or a public
standalone-binary JSON switch. Those require separate decisions.

## Completion-test matrix

| Contract | Required evidence |
| --- | --- |
| Diagnostic shape | Compiler unit tests pin empty and populated `call_frames`/`task_ancestry`, exact serialization, per-frame paths, clone/equality behavior, the private once-only marker, and absence of generated frame prose from structured notes. |
| Human rendering | Exact tests pin non-frame-note order followed by the synthesized call chain, task entry, and youngest-first ancestry, then help and edits, with no duplicate line. |
| MIR call capture | Nested calls, recursion-depth rejection, body-primary cleanup, cleanup-primary failure, and repeated propagation prove the exact innermost-first snapshot and no observer-frame append. |
| MIR task capture | One-level and multi-level TaskGroup failures prove exact youngest-first target entry, parent function, and spawn-site ancestry, including propagation through task and group observation. |
| Source paths | File, `--stdin` virtual path, imported-module call, and cross-module task-start cases prove that every frame uses its own defining or spawning path; source-only helpers prove the documented caller-path fallback. |
| Direct call state | Native-runtime unit tests pin enter/push, exit/pop, depth rejection, cleanup preservation, forced reset, and task-local frame isolation across many suspended tasks sharing workers. |
| Direct ancestry | Native codegen and runtime tests prove that task-start metadata is not dropped, nested children inherit ancestry once, cross-worker submission preserves it, and retirement releases it exactly once. |
| Trap versus status | A normal `main` returning `1` emits no diagnostic, while a native trap carrying status `1` transports the exact diagnostic and never triggers `auto` fallback. |
| Native JSON transport | Cold build, verified cache hit, concurrent-build wait, malformed/missing/oversized channel record, and trap-with-progress cases preserve one JSON document and unchanged program stdout. |
| Standalone behavior | A directly built binary without the private channel renders the same complete human call/task frames as MIR and performs the same cleanup. |
| CLI structured parity | Forced MIR and direct nested-call and nested-task traps produce byte-equivalent codes/messages/spans/non-frame notes and structurally equal frame arrays, including imported paths. |
| LSP bridge | Compiler analysis pins empty arrays; bridge tests pin populated zero-based frame metadata, missing-field compatibility, and preservation in `Diagnostic.data` while the semantic-interface handshake stays at version `2`. |
| Parity mask removal | The MIR-only three-note normalizers and their self-test are deleted from both fixture and forced-backend harnesses; every runtime-failure comparison uses complete diagnostics. |
| Oracle saturation | Every maintained `run-fail` oracle is regenerated or audited to pin its complete human frame output; previously present but filtered frame lines become real assertions. |
| Compatibility and reference | Compile diagnostics retain empty arrays, existing JSON progress/fallback forms stay valid, schema-1 unknown-member tolerance is normative, maintained Manual/Learn/tutorial/README/architecture text describes the implemented contract, and stale deferred-frame claims are rejected by the reference gate. |
| Full gates | Focused compiler/native/CLI/LSP/extension tests, forced backend parity, standalone tests, reference integrity, docs build, audits, Clippy, full CI, and frozen Batch-4 coverage floors pass without synthetic-coverage tests. |

The ADR moves from Provisional only after the complete focused matrix, removal
of every parity/oracle carve-out, reference update, full-CI gate, and frozen
coverage check pass.
