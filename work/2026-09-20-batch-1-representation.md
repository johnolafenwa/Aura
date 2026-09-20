# Batch 1 representation phase: inline unions and four-word callables

Started 2026-09-20 on `codex/batch-1-representation` from `main` after the
H2 completion record. Design authority: the ratified
[checkpoint](../architecture_docs/16-batch-1-design-checkpoint.md) answers
Q9 A (A9, one shared explicit-tag layout, no union niches), Q15 A (C3, a
three-word inline environment plus one operations-table pointer with one
checked overflow allocation), and Q16 A (C4, one immutable operations table
per packing adapter with an indirect drop entry), the 2026-09-13 owner ruling
that schedules them as their own phase after H2 with allocation measurements
(finding 11 of the phase 1 review), and the roadmap row "Ratified
union/callable layout and allocation measurement". Gate carried from the
ruling: no FFI/ABI stability claim and no 0.4 release before this phase is
complete.

## Target

Replace the disclosed interim, in which every union value is a boxed
`UnionValue` behind an opaque runtime handle on both backends and every
closure carries an `Arc<ClosureEnvironment>`, with the ratified physical
representation on the direct backend, and measure allocation behavior on
both backends.

- Unions (Q9 A): a direct-backend union value is the tag plus the payload
  words of the module's `MirUnionLayout` plan, carried inline like a plain
  class (Cranelift SSA words in registers and locals, flattened into plain
  class fields), with the active member's words reinterpreted per member.
  Injection, tag tests, payload projection, equality, copy, move, and drop
  of the active member run inline; only the active payload is destroyed,
  once. A union crosses into runtime containers, task and queue boundaries,
  and runtime helpers as the existing `Value::Union`, exactly as a plain
  class becomes an instance today, so payload types keep their own
  allocations and the shared helpers stay unchanged.
- Callables (Q15 A, Q16 A): a direct-backend `Callable[...]` /
  `TaskCallable[...]` / packed closure value is four words: one immutable
  operations-table pointer and three words of inline environment. The table
  is emitted per concrete packing adapter and records the native invocation
  entry, the runtime-boundary invocation entry, the environment drop entry,
  size, alignment, and the checked shape identity. An environment fits
  inline when its canonical size and alignment fit 24 word-aligned bytes;
  otherwise packing performs one checked allocation and stores the pointer,
  and controlled failure traps with `AU4005: cannot allocate callable
  environment` at packing. Zero-capture packing and moves never allocate.
  Captures initialize in lexical first-use or explicit capture-list order
  and drop in reverse order through the table's drop entry.
- The MIR interpreter keeps its `Value` model as the reference semantics:
  it is not an ABI, and a recursive `Value` cannot hold a union payload
  without indirection. Its per-injection cost drops from a boxed
  `UnionValue` carrying a cloned `Type` to the plan's tag plus a boxed
  payload, and its closure environment stays an owned environment; both
  counts are measured and published next to the direct backend's, but the
  ratified layout claims are made for the direct backend only. This scoping
  is stated here for the owner to confirm or override.

Nothing in the language surface changes. MIR carries the layout plans and
callable contracts already (schema 16); the direct backend now honors them
physically. Both backends must agree on every observable behavior, and the
shared validator keeps its refusals pinned at `run_mir` and
`emit_host_native_object` with the same reasons.

## Execution order

1. R0, failing measurements first: runtime allocation counters
   (`AURA_RUNTIME_STATS=1` prints, at exit and on both backends, the number
   of union payload boxes, closure environments, opaque runtime boxes, and
   callable overflow allocations); CLI tests that assert the counts for
   pinned programs (a scalar injected into `int64 | None`, a plain class
   injected into `Point | None`, a union stored in a class field and read
   back, a packed lambda with one, three, and four word-sized captures, a
   zero-capture packing, a move of a packed value, a bound method packing)
   on the direct backend, which fail today; a benchmark lane
   `benchmarks/representation` with the same programs and a
   `scripts/test_bench_representation.py` gate that records counts, wall
   time, and binary size with provenance.
2. R1, inline unions on the direct backend: a `DirectType::Union` carrying
   the plan and each member's direct type; lowering of `UnionInject`,
   `UnionTagTest`, `UnionTakePayload`, payload projections, `NoneTest`,
   equality, `.clone()`, moves, and the drop plan; plain-class fields of
   union type flattened; views into union payloads through the existing
   `DirectViewPlace` alternatives; trait dispatch on union receivers
   switching on the inline tag; materialization to `Value::Union` at the
   container, task, queue, FFI nullable-handle, and runtime-helper
   boundaries and back; native cache identity bumped with the layout
   version. Every union fixture, the parity matrix, and the union security
   suites stay green; R0's union counts reach zero for inline cases.
3. R2, four-word callables on the direct backend: `DirectType::Callable`
   as four words; per-adapter operations tables emitted as object data;
   generated native and runtime-boundary invocation thunks and drop thunks;
   inline or checked-overflow environments; `CallableAdapt`, packing,
   bound-method packing, `TaskCallable` starts, and runtime container
   boundaries through a `Value` form that owns the four words; the
   `AU4005` packing failure pinned on both backends with the same reason.
   Every callable fixture and the callable security suites stay green;
   R0's closure environment counts reach zero for fitting environments and
   one for oversized ones.
4. R3, records: backend-boundary note, the manual's closures and types
   limits, the performance chapter's representation table with measured
   counts and sizes, CHANGELOG, this note's evidence and open items, task
   board; the phase 1 review's finding 11 closed by reference.

R0 and the inline-member half of R1 (unions whose members are all scalars,
`None`, or plain classes) merge as the first pull request; the
owned-handle half of R1 (`str | None` and every union with a runtime-object
member) shares its ownership machinery with the four-word callable
environment and merges with R2 and R3 as the second, so each stays
reviewable and each carries its own green local chain and hosted run. The
coverage floors may only rise.

## Risks and rules

- Reinterpreting payload words per member must round-trip floats,
  booleans, and narrow integers exactly; the plan's member size and
  alignment, not the Cranelift type, decide the word count.
- A view into a union payload on the direct backend is a view into the
  words of a stack-resident value; the existing writeback and selector
  machinery for plain-class fields is the model.
- Runtime helpers that receive callables (list `map`/`filter`/`sort_by`,
  `control.retry`, task starts) call through the runtime-boundary entry of
  the table, never through a `Value::Function` environment.
- Transfer across tasks and queues moves the four words or the boxed
  runtime form; no reference-counted sharing of a mutable environment is
  introduced.
- No FFI/ABI stability claim; the layout remains a target-dependent
  internal Aura ABI with a layout version in the cache identity.

## Evidence

- First pull request (R0 and inline-member unions): the four
  `representation_stats` CLI tests pass on both backends (three assert zero
  direct-backend union boxes and failed before the change); the nine fixture
  categories, the union layout, pattern, and loan-injection security suites,
  the validator coverage suite, the native coverage suite, and the 398 native
  codegen, native runtime, union-injection, and union-runtime unit tests
  pass; every runnable fixture emits a direct object; the forced backend
  parity matrix result and the complete local chain are recorded below when
  they finish.
- Counters measured by `scripts/bench-representation.py` on this host
  (reference numbers for the interpreter, layout claims for the direct
  backend): recorded in the performance chapter's representation table and
  archived in `work/2026-09-20-representation-measurements/`.

## Open items

- `.clone()`, rendering, hashing, and every runtime-helper argument of union
  type box the inline value for the call (the runtime's shared rules keep
  both backends identical); equality of two inline unions of one type
  compares tags and members inline, with a plain-class member boxed for the
  runtime's structural rule. Inline plain-class equality and hashing are an
  optimization for a later pass.
- A consuming `match own` take and the value-position `NoneTest` never
  reach an inline union (non-Copy payloads stay runtime values; lowering
  tests `None` through `UnionTagTest`), so the direct backend has no inline
  arms for them; the second pull request adds the take when non-Copy members
  go inline.
- Unions with a runtime-object member remain boxed on both backends until
  the second pull request lands owned-handle words with tag-selected retain
  and release.
- The interpreter boxes a union payload twice per `match` over a local
  (injection plus the scrutinee copy of a type pattern); it is a reference
  number, not an ABI claim, but the second box is avoidable.
