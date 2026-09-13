# Batch 1 phase 1 review

Reviewed `7685199..d38381c4` on `main`, with the completion records at
`bb41ef5f113ffe90b4c9d9c6001ab0cd1fd2450a`. The latter changes only the two work
records. Review performed on 2026-09-12; this report uses the requested filename.
No source, test, documentation, existing record, or Git state was changed.

The final scope covers ratified-answer conformance, ordinary source/backend
behavior, fail-closed validation, test quality, tooling, documentation, and record
accuracy. Reproduced source failures are distinguished from static findings and
forged-MIR findings.

Security threat model: Aura is a local compiler/runtime library whose public
`run_mir` and native-object APIs accept deserialized `MirModule` values. The
relevant attacker controls that module or corrupts an intermediate artifact; the
shared validator is the boundary expected to reject invalid ownership, callable,
union, and returned-view metadata before either backend relies on it. Assets are
host-process availability, ownership and task-isolation guarantees, and agreement
between interpreter and native code generation. Repository `SECURITY.md` states
that Aura is an early preview and is not a hardened sandbox, so missing checks are
reported at calibrated severity rather than treated as remotely exploitable by
default.

Fresh functional verification: `cargo build -p aura`; ordinary source probes and
positive fixture controls on both backends; the existing child-stack diagnostic
test (1/1); reference, tutorial, and hygiene gates; 111 LSP tests; and 27 extension
tests against the existing built extension. The tutorial gate checked 343 fences.
Independent Daybreak Blue passes reviewed the security-sensitive paths. Existing
focused binaries ran 303 tests across validator, union-pattern, union-loan,
union-injection, native-coverage, callable-contract, callable-packing,
callable-view, bound-method, union-layout, and runtime-coverage suites; all passed,
including the test that deliberately catches an internal Array panic. Fresh
throwaway serde probes exercised `run_mir` and `emit_host_native_object`; native
objects were not executed, and all temporary artifacts were removed.
The complete workspace suites, forced fixture-parity matrix, compiler coverage,
and Clippy were not rerun locally. Their successful hosted runs were verified
below. `target/` ended at approximately 25 GiB; no cleanup of existing build
outputs or coverage rebuild was performed.

## Findings

### 1. Should-fix — generic enum inference disagrees with the checker for named arguments

**Location:** `crates/aura-compiler/src/mir.rs:19110`, `:19128`, `:19131`;
`crates/aura-compiler/src/sema.rs:8842`.

**Reproduction, executed:**

```aura
enum Duo[T]:
    Pair(first: T, second: T)

def main():
    small: int32 = 4
    pair = Duo.Pair(second=1, first=small)
    wrapped: Duo[int32] | str = pair
    match wrapped:
        case Duo[int32] as values:
            match values:
                case Duo.Pair(first, second):
                    print(first)
                    print(second)
        case str as text:
            print(text)
```

`aura check` succeeds. MIR execution rejects with AU4001, and direct execution
with AU2002: `invalid MIR union injection operand does not have its selected member
type`. The MIR dump gives `pair` type `Duo[int64]`. Expected output is `4`, then `1`.

The checker binds declaration slots first, infers `T=int32` from `first=small`,
and contextually types the other literal. The new lowerer helper visits supplied
arguments in source order, infers `int64` from `second=1`, and ignores the failed
match against `small`. Thus the work note's claim that inference works “exactly
as the checker does” (`work/2026-09-09-batch-1-phase-1.md:1538`) is incorrect.
Retain the checker's resolved expression type, or share its slot binding and
contextual inference; checking the discarded boolean alone would still reject
valid source.

### 2. Should-fix — explicit imported enum type arguments are lost

**Location:** `crates/aura-compiler/src/mir.rs:19669`; specialization is stripped
by `qualified_module_item` at `:9764`.

**Reproduction, executed:** `api.au`:

```aura
public enum Slot[T]:
    Filled(T)
    Empty
```

`main.au`, in the same directory:

```aura
import api

def main():
    slot = api.Slot[int64].Empty()
    match slot:
        case api.Slot.Filled(value):
            print(value)
        case api.Slot.Empty:
            print("empty")
```

Checking succeeds, but the lowered local is `Named("api.Slot", [])`. MIR rejects
with AU4001 and direct with AU2002: the enum has incorrect type arity. Expected:
`empty`. The imported constructor arm unconditionally uses payload inference,
which has no evidence for a payloadless variant, instead of preserving the
written `int64`. Explicit specialization is part of Q7 and
`docs/manual/generics-and-traits.md:23`. Prefer explicit arguments and infer only
for an unspecialized constructor. This is another incomplete edge of the enum
fix in `110a35b8`.

### 3. Should-fix — grouped alias repair handles only one group

**Location:** `crates/aura-compiler/src/mir.rs:17347`, `:19191`.

**Reproduction, executed:**

```aura
class Box[T]:
    value: T

type Wrapped[T] = Box[T]

def main():
    grouped = ((Wrapped[int64]))(value=1)
    print(grouped.value)
```

Checking succeeds. MIR rejects with AU4001 for an unknown
`unsupported<Group(...Group(...Specialize...))>` function; direct rejects with
AU2002 because it cannot infer temporary `%t0`. One group prints `1` on both;
two or three groups fail. The repair described at
`work/2026-09-09-batch-1-phase-1.md:1539` unwraps exactly
`Group(Specialize(...))`. Repeated parentheses must preserve the accepted
constructor. Peel all groups before both dispatch and result-type inference.

### 4. Should-fix — the omitted generic imported associated-view case is reachable

**Location:** `crates/aura-compiler/src/mir.rs:11277`;
`crates/aura-compiler/src/sema.rs:10167`.

**Reproduction, executed:** `api.au`:

```aura
public class Box[T]:
    public left: T
    public right: int64

    public def associated(value: Box[T]) -> view T from value:
        return view value.left
```

`main.au`:

```aura
import api

def left(box: api.Box[int64]) -> view int64 from box:
    return view api.Box.associated(box)

def main():
    box = api.Box[int64](left=1, right=2)
    view chosen = left(box)
    print(chosen)
```

Checking succeeds. MIR reports AU4001 and direct AU2999:
`invalid returned MIR loan 'chosen' in 'main' follows call 'left' which does not
return a view`. The dump contains an ordinary move instead of the required
returned-loan handoff. Expected: `1`. The fixture-style `Holder.left()` form also
fails with either `api.Box.associated(self.box)` or
`api.Box[int64].associated(self.box)`.

The work note accurately says the patch covers non-generic imported classes
(`:1548`), but this omitted case is ordinary source. The generic restriction in
`docs/manual/classes.md:140` concerns associated **method values**, not this direct
call. Resolve the generic declaration and substitute explicit or inferred class
arguments in its view contract and projections; merely deleting the guard is
insufficient.

### 5. Should-fix — a stored TaskCallable returning a callable loses result provenance

**Location:** `crates/aura-compiler/src/mir.rs:3750`.

**Reproduction, executed:**

```aura
type Reader = def() -> int64
type Factory = TaskCallable[def() -> Reader]

def answer() -> int64:
    return 42

def make_reader() -> Reader:
    return answer

def run(factory: own Factory):
    with TaskGroup() as group:
        task = group.start(factory)
        match task.result(timeout=10s):
            case TaskResult.Ready(callback):
                print(callback())
            case TaskResult.Error(message):
                print(message)
            case TaskResult.Cancelled:
                print("cancelled")
            case TaskResult.TimedOut:
                print("timed out")

def main():
    factory = Factory(make_reader)
    run(factory)
```

Checking succeeds; MIR rejects with AU4001 and direct with AU2999:
`invalid MIR indirect call in 'run' has no authoritative callable contract`.
Expected: `42`. Named `group.start(make_reader)` and immediate
`group.start(Factory(make_reader))` controls print `42` on both backends. The
returned function is independent and capture-free.

`StartTask` result propagation handles `Function` and `Closure`, but omits
`Type::Callable`; passing the factory through its declared parameter exposes the
missing `__task_result` identity. This contradicts Q21 and the stored-target
completion claim at `work/2026-09-09-batch-1-phase-1.md:1210`. Extract the stored
contract's return type and preserve descendant callable identities through task
result helpers.

### 6. Should-fix — loop checking discards its own guard's narrowing, and a new test pins it

**Location:** `crates/aura-compiler/src/sema.rs:4219`;
`crates/aura-compiler/tests/batch1_coverage_sema.rs:159`.

**Reproduction, executed:**

```aura
def main():
    mut value: int64 | None = 1
    while value is not None:
        print(value + 1)
        value = None
```

Checking rejects with AU2003 at `value + 1`, treating it as `int64 | None`.
Expected: acceptance and output `2`. The guard is evaluated before every body
execution. The second body check removes a fact killed by the assignment but
does not reapply the condition's true-successor facts.

Checkpoint A4 (`architecture_docs/16-batch-1-design-checkpoint.md:225`, `:247`)
and `docs/manual/statements.md:233`, `:244` require the narrowed body.
`while_body_that_invalidates_its_entry_fact_is_rechecked_without_it` uses the
same shape with initial value `3` and explicitly expects AU2003. Recompute the
loop-header fixed point and then apply the condition before checking the body;
convert this defective oracle into a positive regression.

### 7. Should-fix — all-member union methods do not satisfy generic trait bounds

**Location:** `crates/aura-compiler/src/sema/traits.rs:908`, `:975`.

**Reproduction, executed:**

```aura
trait Named:
    def name(self) -> str

class Dog:
    tag: str

class Cat:
    tag: str

impl Named for Dog:
    def name(self) -> str:
        return self.tag.clone()

impl Named for Cat:
    def name(self) -> str:
        return self.tag.clone()

def show[T: Named](value: T):
    print(value.name())

def main():
    pet: Dog | Cat = Dog(tag="rex")
    print(pet.name())
    show(pet)
```

The direct method expression checks, but `show(pet)` rejects with AU2002:
`type Cat | Dog does not implement trait Named`. Expected: two `rex` lines.
Direct union dispatch uses all-member resolution; ordinary bound satisfaction
only searches concrete implementations. Ratified A8 (`checkpoint:410`) expressly
requires ordinary trait obligations to hold for a coherent all-member
specialization. Share that resolution with generic-bound checking and preserve
the dispatch through specialization.

### 8. Should-fix — concrete trait-method values expose implementation-local names

**Location:** `crates/aura-compiler/src/sema/callables.rs:2974`, `:3013`;
`crates/aura-compiler/src/sema/program.rs:1466`.

**Reproduction, executed:**

```aura
trait Apply:
    def apply(self, value: int64) -> int64

class Worker:
    pass

impl Apply for Worker:
    def apply(self, local: int64) -> int64:
        return local

def main():
    worker = Worker()
    f = worker.apply
    print(f(value=7))
```

Checking rejects with AU2004: no parameter named `value`. Changing the call to
`f(local=7)` prints `7` on both backends. Q19 (`checkpoint:834`) allows local
implementation names to differ by ordinal, but requires method values to expose
the trait's public contract. The binding copies implementation parameters;
`docs/manual/closures.md:338` omits the required trait exception. Synthesize the
public contract from the selected trait and forward to implementation slots by
ordinal, including returned-view origins.

### 9. Should-fix — binding a method through a Copy view is rejected

**Location:** `crates/aura-compiler/src/sema/loans.rs:2115`;
`crates/aura-compiler/src/sema.rs:4707`.

**Reproduction, executed:**

```aura
copy class Counter:
    value: int64

    def read(self) -> int64:
        return self.value

def main():
    original = Counter(value=4)
    view snapshot = original
    selected = snapshot.read
    print(selected())
```

Checking rejects with AU3010: a view must initialize an explicit view binding.
Explicit packing of the selection also rejects it as storing a view in an owned
value. Expected: an independent Copy snapshot, printing `4`, under Q18
(`checkpoint:785`). The bound-method checker correctly chooses a snapshot at
`sema/callables.rs:3032`, but `direct_view_value_kind` mistakes the resulting
non-Copy closure for a borrowed projection. Give it the bound-method exception
already present in `expr_borrow_info` (`sema/loans.rs:2724`).

A Copy receiver reached through a `mut Counter` parameter does bind and print `4`
on both backends. Therefore the manual's blanket refusal of borrowed parameters
at `docs/manual/closures.md:346`, `:384` is also too broad.

### 10. Should-fix — written destinations silently restrict contracts contrary to Q17 A

**Location:** `crates/aura-compiler/src/sema.rs:6167`;
`crates/aura-compiler/src/sema/callables.rs:450`.

**Reproduction, executed:**

```aura
type Unary = def(int64) -> int64

def increment(value: int64 = 1) -> int64:
    return value + 1

def main():
    step: Unary = increment
    print(step(4))
```

Checking succeeds and both backends print `5`. The destination removes a name
and default availability without an explicit adapter. Checkpoint C5 (`:749–759`)
requires `Unary(increment)` and calls a differing bare assignment an error;
`:778` identifies implicit restriction as the alternative. ADR-0058 `:12–14`
makes the checkpoint authoritative.

The implementation is internally documented: work note `:1067–1084`, Manual
`functions.md:395–415`, and backend boundary `:349–359` reinterpret a written
destination as the explicit restriction. Tests at `sema_tests.rs:23583` and
`:23934` pin this interpretation. It is nevertheless a ratification deviation.
Require identity at ordinary boundaries and admission through explicit adapters,
or obtain an explicit design amendment before claiming exact Q17 A conformance.

### 11. Should-fix — physical union/callable storage guarantees remain undelivered

**Location:** `crates/aura-compiler/src/union_runtime.rs:237`;
`crates/aura-compiler/src/mir_runtime.rs:3845`;
`crates/aura-compiler/src/native_runtime.rs:4071`;
`work/2026-09-09-batch-1-phase-1.md:54`, `:57`, `:956`.

**Illustrative ordinary sources; allocation conclusion is static:**

```aura
def main():
    value: int64 | None = 1
    print(value)
```

```aura
type Reader = Callable[def() -> int64]

def make_reader(base: int64) -> Reader:
    offset = base + 1
    return Reader(lambda: offset)
```

Union injection allocates a `Box<UnionValue>` and its metadata even for a scalar
payload. Closure construction uses an `Arc<ClosureEnvironment>` with vector
captures; the interpreter also boxes its function value. There is no implemented
four-word inline callable representation/shared operations table or the promised
`cannot allocate callable environment` diagnostic.

Q9/A9 (`checkpoint:439–446`) prohibits mandatory per-union allocation. Q15/Q16
(`:660–680`, `:701–705`) require fitting inline environments, checked overflow,
and shared operations tables; `:692` expressly excludes always-allocated
environments as a ratifiable alternative. Shared metadata layout plans are not
the physical representation guarantee.

Backend boundary `:315–327`, `:370–378`, Manual `closures.md:442`, and the work
note `:1153` openly describe the boxed stand-in. This is not a concealed backend
divergence, but disclosure does not make the ratified work complete. Implement
and measure the promised representation/failure behavior, or explicitly amend
the design and completion scope. No allocation benchmark was run here; “no
second allocation at packing” does not establish allocation-free fitting
environments.

### 12. Should-fix — H1 editor support is materially incomplete

**Location:** `crates/aura-compiler/src/analysis.rs:302`, `:1334`, `:4364`,
`:4379`; `tools/aura-language-server/src/server.js:66`;
`tools/vscode-aura/syntaxes/aura.tmLanguage.json:455`.

**Reproductions, executed through `aura analyze --stdin` and `aura complete`:**

```aura
class Box:
    value: int64
type Wrapped = Box
def main():
    box = Wrapped(value=1)
    print(box.value)
```

No diagnostics, but analysis gives `binding box: None`; alias use and member use
lack occurrences/definitions, alias names are missing from completion, and
`box.` offers no members. Alias declarations currently supply outline symbols
without the needed resolution/inference.

```aura
type Doubler = Callable[def(value: int64) -> int64]
def double(value: int64) -> int64:
    return value * 2
def main():
    callback: Callable[def(value: int64) -> int64] = Doubler(double)
    result = callback(21)
    print(result)
```

The callback hover is correct, but `result` has type `None` in analysis: call
inference handles `Function`/`Closure` and omits `Callable`.

Checkpoint H1 (`:1529–1534`) also requires argument help and rename/reference
behavior. The server has no signature-help, reference, or rename handlers. The
TextMate grammar lacks alias/contextual `is`/Callable rules and omits `|` from
its operator rule; the phase's grammar change only adjusts the returned-view
terminator. These omissions contradict the completed tooling ledger at work
note `:62`. Complete compiler-owned analysis and the promised protocol/grammar
surfaces, and add real bridge regressions. The passing 111 tests and recorded
100% coverage do not test absent features.

### 13. Blocker — stored TaskCallable arguments bypass Transfer enforcement

**Location:** `crates/aura-compiler/src/sema.rs:12235`, `:12321`;
`crates/aura-compiler/src/mir.rs:2568`;
`crates/aura-compiler/src/mir_runtime.rs:5707`;
`crates/aura-compiler/src/native_codegen.rs:15403`.

**Reproductions, executed:** the omitted-default case is:

```aura
type Job = TaskCallable[def(group: own TaskGroup = ...) -> int64]

def worker(group: own TaskGroup = TaskGroup()) -> int64:
    return 1

def main():
    target = Job(worker)
    with TaskGroup() as group:
        task = group.start(target)
        print(task.result_or(-1, timeout=1s))
```

The named-slot case uses
`TaskCallable[def(first: int64 = ..., *, resource: own TaskGroup) -> int64]`
and calls `group.start(target, resource=TaskGroup())`. Checking accepts both.
MIR execution accepts and prints `1`; native object emission accepts both lowered
modules, producing objects of 114,000 and 114,456 bytes. The
objects were not executed. MIR demonstrates that a value the compiler explicitly
classifies as a live, non-Transfer host resource enters child argument handling;
native evidence establishes validator/codegen acceptance only.

The stored-target checker zips declaration slots directly with the raw supplied
argument list. It skips an omitted default and compares a named argument with the
first declaration slot instead of its bound slot. The named-function path below
it correctly calls `bind_call_arguments`; the shared validator binds names but
also skips every omitted default and never proves the parameter type Transfer.
The MIR runtime materializes defaults in the parent and transfers the completed
argument vector. The generated native path is statically wired to invoke the
selected default binder with `transfer_defaults = 1`; that object was not run.

C8/Q21 requires every explicit **and default** argument to be concretely Transfer
(`checkpoint:931–935`), and the completed ledger says the same at work note
`:1210–1227`. Bind stored-callable arguments canonically before all checks, apply
Transfer to every supplied or defaulted concrete slot, and make `StartTask`
validation independently reject every non-Transfer parameter/result type. Add
both-boundary default and out-of-order named regressions. This is a phase blocker
because it violates the task-isolation boundary from ordinary accepted source.

### 14. Should-fix — shared task validation admits a returned-view result

**Location:** `crates/aura-compiler/src/sema.rs:12126`;
`crates/aura-compiler/src/sema/callables.rs:1807`, `:2797`;
`crates/aura-compiler/src/mir.rs:6009`;
`crates/aura-compiler/src/mir_runtime.rs:5764`.

**Reproduction, executed:**

```aura
class Pair:
    left: str

def pick_left(pair: Pair) -> view str from pair:
    return view pair.left

def main():
    with TaskGroup() as group:
        pair = Pair(left="ada")
        task = group.start(pick_left, pair)
        print(task.result_or("fallback", timeout=1s))
```

`aura check` returns `ok`, contrary to the documented AU3008 rule. Unmodified
lowering then accidentally fails closed because it erases the function operand's
returned-view wrapper, so both public boundaries reject a signature mismatch.
After restoring only the authentic declaration signature at
`StartTask.function.Function.signature.Function.return_type` to
`ReturnedView { pointee: str, origin: 0, mutable: false }`, `run_mir` accepts and
prints `ada`; native object emission accepts and produces 113,768 bytes.

The child result path discards the returned-view descriptor and surfaces its
pointee as an owned task result. No mutable alias, panic, or memory-safety failure
was demonstrated, but the serialized-MIR boundary does not enforce the ownership
rule. The checkpoint requires every result to be Transfer (`:935`), and the Manual
and work note expressly say a TaskCallable view result is AU3008
(`docs/manual/concurrency.md:88`; work note `:1363`). Reject `ReturnedView` before
`returned_view_pointee` erases it in every named/stored checker path, and reject it
again in shared `StartTask` validation. Preserve the lowering wrapper so the
validator regression exercises the intended reason.

### 15. Should-fix — named builtin calls can reach backend-only type assumptions

**Location:** `crates/aura-compiler/src/mir.rs:6131`;
`crates/aura-compiler/src/mir_runtime.rs:1516`, `:4489`;
`crates/aura-compiler/tests/batch1_coverage_runtime.rs:135`.

**Forged-MIR reproduction, executed:** lower this valid source:

```aura
def main():
    values = Array[int64].from_list([1, 2], [2])
    print(values.len())
```

In the serialized `Array.from_list` call, replace argument zero's operand with
`{"Int": 1}` and deserialize it back to `MirModule`. `run_mir` passes shared
validation, reaches `checked_mir_vec_ref`, and panics at its `unreachable!`; the
outer runtime boundary converts that to `Aura MIR runtime panicked while executing
the program`. `emit_host_native_object` also accepts the same module. The emitted
object was not executed, so no native crash is claimed.

The generic rvalue arm does no direct checking for `CallTarget::Name`; later
logic validates names present in `context.functions`, but builtin names such as
`Array.from_list` are absent and receive no arity, argument-type, or result-type
contract check before either backend relies on them. The new test currently
requires the contained panic, which proves post-validation containment rather
than fail-closed validation. Add a
shared named-builtin contract table (beginning with `Array.from_list`) and require
both public boundaries to reject this mutation with the same validation reason.
Under the stated local/untrusted-MIR precondition the executed evidence supports
a low validation/robustness issue: the interpreter catches the panic and native
execution was not attempted. It is not evidence of a remote exploit.

### 16. Should-fix — callable provenance merging ignores complete slot contracts

**Location:** `crates/aura-compiler/src/mir.rs:3965`, `:4127`, `:4150`, `:4190`;
`crates/aura-compiler/src/sema/types.rs:666`;
`crates/aura-compiler/src/native_runtime.rs:4127`.

**Forged-MIR reproduction, executed:** lower two functions
`first(value: int64)` and `second(value: int64)` stored in
`list[def(value: int64) -> int64]`, select index 1, and invoke
`tool(value=7)`. Change `second.params[0].name` and every authenticated
`Operand::Function` signature for `second` from `value` to `other`; leave the list
element type, selected local type, and call argument unchanged.

The shared validator accepts the merge. MIR then fails only in its later binder
with `unknown MIR argument 'value'`; native object emission accepts and produces a
114,840-byte object. Native binding has already compiled the written name to a
slot before the selected value's default binder runs. As a static consequence,
multiple same-typed slots can route a named value to a semantic slot different
from the selected declaration, so this is a complete-contract integrity failure
as well as backend divergence. That consequence was not executed, and the native
object was not run.

`merge_validated_callables` and candidate return comparison use `Type::PartialEq`,
whose callable equality intentionally compares ABI types/modes but omits names,
keyword-only boundaries, and defaults. This contradicts C5's complete-contract
identity (`checkpoint:735–740`) and the work note's promise that differing runtime
candidates poison to `Unknown` (`:515–519`). Compare full recursive callable
contracts at every merge/member-result candidate, teach that comparator about
closures, and poison any disagreement. Add both-boundary tests for names,
keyword-only slots, defaults, containers, and CFG joins.

### 17. Should-fix — the stack headroom probe counts the guard page and reserves less than one measured transition

**Location:** `crates/aura-compiler/src/runtime_value.rs:2854`, `:5571`;
`crates/aura-compiler/src/mir_runtime.rs:417`, `:3046`;
`crates/aura-compiler/src/native_runtime.rs:3889`.

**Static reproduction:** `DefaultStack::limit()` is documented by corosensei
0.3.3 as including guard pages; on Unix it returns the mapping start while writable
memory starts one page later. Aura stores that address and reports
`stack_pointer - limit` as usable. On this 16 KiB-page host, a reported 128 KiB is
therefore at most 112 KiB writable. The phase's own measurement says one debug
interpreter level costs roughly 150 KiB (work note `:1431–1447`), which also
exceeds the 128 KiB reserve before guard adjustment. A call can pass the probe and
cross the guard during its next transition, before another probe can run.

The impact is potential interpreter process termination from legal Aura
recursion; the guard still limits memory-corruption impact. No guard fault
was induced during review. The existing 256 KiB child regression did produce
AU4005. A fresh root probe with a captured packed callable per recursive level ran
to depth 200; at requested depth 1,000 it reached the independent 256-call AU4001
depth limit, not AU4005. Root AU4005 reachability therefore remains unproved for
that ordinary shape. Direct execution has only its 256-call depth guard and no
headroom/AU4005 parity, despite the Manual's backend-neutral statement.
On the downward-growing corosensei stacks reviewed here, counting the guard
overstates headroom, so this defect delays AU4005 rather than firing it
spuriously. No supported-platform or exact-boundary sweep independently proves
the absence of other false-positive cases.

Use a platform-aware conservative writable-headroom limit: on Unix exclude the
fixed guard pages; on Windows account for corosensei's moving soft guard/guarantee
region. Reserve more than the worst supported transition high-water plus unwind
and diagnostic margin, or probe before the large transition frames. Add root and
child boundary tests across build profiles/page sizes and state the distinct
native behavior explicitly.

### 18. Note — ReturnedView canonical keys bypass the nominal byte limit

**Location:** `crates/aura-compiler/src/sema/type_budget.rs:386`;
`crates/aura-compiler/src/sema/types.rs:382`, `:521`;
`crates/aura-compiler/src/sema/type_budget_tests.rs:643`.

**Static reproduction by exact encoding:** the estimator charges 15 bytes plus a
JSON-quoted origin for each `ReturnedView`. The actual canonical value is
`["returned_view", mutable, numeric_origin, pointee]`. It omits the boolean and
some punctuation while quoting a number, undercounting 7 bytes for `true` and 8
for `false` per node. Allocation follows immediately after the preflight.

Deep returned-view callable metadata can therefore pass the nominal 1 MiB check
and allocate a larger key. The 65,536-node limit bounds the maximum discrepancy at
about 512 KiB per canonical key; no repeated-allocation exhaustion path was
demonstrated. This is a low, bounded limit-accounting defect. Count the boolean,
numeric origin, separators, and closing bracket exactly; add exact-limit and
one-byte-below tests for both mutabilities and multi-digit origins. The current
byte sweep omits ReturnedView. The reviewed Callable header repair itself is
conservative and has no equivalent production bypass.

### 19. Should-fix — security tests often prove containment on only one boundary

**Location:** `crates/aura-compiler/tests/batch1_coverage_runtime.rs:135`;
`crates/aura-compiler/tests/batch1_union_loan_injection_security.rs:40`, `:134`;
`crates/aura-compiler/tests/batch1_union_injection_metadata_security.rs:44`;
`crates/aura-compiler/tests/batch1_union_pattern_security.rs:90`;
`crates/aura-compiler/tests/batch1_validator_coverage.rs:663`;
`crates/aura-compiler/tests/batch1_coverage_native.rs:176`.

The Array test above explicitly accepts a caught interpreter panic. The union-loan
test at line 40 wraps all assertions in `if let Ok(mir)`, so it can execute none;
its three forged parameter cases test only `run_mir`. Forty-three malformed
payload/loan/authority cases in `batch1_union_pattern_security.rs` are likewise
interpreter-only, and all six serialized union-injection corruptions omit native
emission. `container_insert_without_an_operand_poisons_every_element` removes all
arguments, accepts any interpreter error, and can pass on trivial arity before it
exercises poisoning. Seven native-coverage mutations discard the emission result
or explicitly accept either outcome, including missing indirect operands,
receiver/writeback metadata, a rewritten trait callee, task function metadata,
and labels.

All focused suites passed; those assertions still do not establish the claimed
shared fail-closed boundary. Use the existing two-path helper at
`batch1_validator_coverage.rs:49`: deserialize a guaranteed mutation, require both
`run_mir` and `emit_host_native_object` to fail, and compare the complete shared
validator suffix. Split arity checks from well-arity identity/provenance checks,
and add positive paired tests where inference is intentional.

### 20. Should-fix — the Callable estimator regression test also passes the old defect

**Location:** `crates/aura-compiler/src/sema/type_budget_tests.rs:656`.

**Reproduction by arithmetic on the existing benign sample:** its rendered key
is 191 bytes. The old estimator accepts at 189 bytes; the repaired one accepts
at 193. `threshold.abs_diff(exact) <= 4` accepts both, and the remaining loop only
checks rejection below the estimator's own threshold. Thus
`byte_limit_sweep_rejects_every_prefix_of_each_key_shape` does not establish its
name or prevent the reported four-byte-header regression.

Require `threshold >= exact`, reject every limit below the actual serialized
length, and cover zero/one-parameter callables. Retain legitimate conservative
overestimation without permitting underestimation.

### 21. Should-fix — native diagnostic-channel tests share an unguarded global transaction

**Location:** `crates/aura-compiler/src/native_runtime_tests.rs:22367`,
`:17885`, `:17889`.

**Static CI-flake evidence:** the new
`internal_diagnostic_channels_reject_shared_descriptors_and_recover_from_poison`
test installs/takes process-global `INTERNAL_DIAGNOSTIC_CHANNELS`. Another test
installs real channels and then requires a `SignaledWithoutRecord` result. If the
new test installs empty channels between those operations, the older test gets
`NoChannel`. The reverse interleaving can break the new test. The mutex serializes
individual operations, not each test's install/emit/take sequence. The closed-file
descriptor check at `:22373` also assumes another test cannot reuse the number.

Ordinary Cargo CI runs these tests concurrently; the serial coverage run does
not fix that race. Isolate the cases in subprocesses or serialize every
channel-owning test with the same test-only guard. No repeated stress run was
performed to force an interleaving.

### 22. Should-fix — an HTTP test assumes one TCP read contains a complete status prefix

**Location:** `crates/aura-compiler/src/mir_runtime_tests.rs:22188`, `:22207`.

**Static CI-flake evidence:**
`http_listener_reassembles_chunked_request_bodies_split_across_reads` calls
`Read::read` once, then requires the buffer to begin with the complete
`HTTP/1.1 200` prefix. A valid short read can contain only part of that prefix.
Read through the status-line terminator, and preserve a read error instead of
converting it to zero bytes. The test's request-side fragmentation does not make
the response a single-read message.

### 23. Should-fix — documented child stack default is still 512 KiB

**Location:** `crates/aura-compiler/src/runtime_value.rs:2920` versus
`crates/aura-compiler/src/call.rs:2120` and
`docs/manual/concurrency.md:105`, `:367`, `:608`.

**Static comparison:** child stacks are 768 KiB, already at the review baseline.
The Manual, hover text, API index (`:269`), execution model (`:454`), current limits
(`:188`), status page (`:172`), ADR-0032, and additional tutorial/export text still
say 512 KiB. New backend-boundary text at `:462` correctly says 768 KiB, while the
phase work note at `:1438` calls that the documented default.

This underlying drift predates the range; the stack change preserved the actual
default but failed to reconcile its maintained surface. Document/ratify 768 KiB
consistently, or change the implementation under a separately verified decision.
The new 16 MiB root does not silently change ordinary children. The existing
explicit-256-KiB child test produced AU4005. The fresh root probe described in
finding 17 reached the separate call-depth limit instead, so AU4005 reachability
on the root remains unproved.

### 24. Should-fix — normative grammar and callable documentation contradict delivered behavior

**Locations and minimal comparisons:**

| Location | Contradiction | Required correction |
| --- | --- | --- |
| `docs/manual/grammar.md:228`, `:253`, `:322` | `type Choice = int64 | str` and `type Grouped = (int64 | str)` are implemented, but the item/type productions omit aliases, unions, grouped types, and prose says `(T)` is not a type. | Reconcile the normative grammar with Q1/Q11 and existing parser fixtures. |
| `docs/manual/closures.md:255` versus `:269` | Earlier prose says by-value environments are read-only; later prose and `callable_store_mutable_counter` implement owned-capture mutation. | State the current call-kind/capture rules consistently. |
| `docs/manual/closures.md:461` versus `:180` | Unconditional no-merge wording contradicts explicitly packed common contracts and `callable_merge_common_contract`. | Qualify the restriction to contexts without the explicit common contract. |
| `docs/manual/types.md:508`, `:549` | Method-value types are listed as unavailable despite bound and associated values. | Describe implemented values and their actual restrictions. |
| `docs/manual/control-plane.md:106`, `:187`; `docs/manual/current-limits.md:286` | Callback descriptions omit packed Shared retry workers and the Mutable restriction. | Match the implemented Shared callback family and its fixture. |

The mutable-counter, common-contract, generic-bound-method, argument-origin
bound-view, and Shared callback fixtures all produced their pinned outputs on
both backends during this review. These are stale descriptions of delivered
behavior, separate from the implementation gaps above.

### 25. Note — schema examples and several phase records are stale

**Reproduced schema mismatch:** the JSON examples at
`docs/manual/cli-and-tooling.md:179` and
`tools/aura-language-server/README.md:46` send schema 6. Sending the documented
request to `aura lsp` returns a schema-mismatch error requiring 14. Adjacent prose
and compiler/LSP constants correctly require 14. Update the examples and export.

**Static record comparisons:**

- `work/task-board.md:35` still calls type patterns in progress; `:175` says
  coverage floors remain unchanged, contradicting the completed/raised-floor
  entries at `:160` and the script.
- The family ledger at `work/2026-09-09-batch-1-phase-1.md:59` still refuses
  generic and view-returning bound methods; the later generic follow-up at
  `:1328` and explicit-argument view results at `:1381` are implemented.
- The claim at work note `:279` that intermediate schema bumps are recorded in
  family sections misses schema 9. Git history establishes 8→9 at `9ab3176b`,
  before 9→10 at `87b35947`.
- ADR-0052 `:7`, `:39` still says implementation is in progress/unimplemented,
  despite the completion record. Retain the genuine H2 exclusions when updating.
- `docs/manual/static-semantics.md:238` assigns `None` to the true branch of both
  `is None` and `is not None`; the latter removes `None`.
- `union_properties_equality_unavailable.au/.diag` pins AU2008, repeated by the
  work note at `:935`, whereas checkpoint H1 `:1513` and Q24 prescribe AU2003 for
  incomparable union members. The rejection is correct; its diagnostic differs.

These require editorial/diagnostic reconciliation, not another hosted CI rerun.

### 26. Note — additional coverage cases document limits or weak observations

**Location:** `crates/aura-compiler/tests/batch1_coverage_mir.rs:403`;
`crates/aura-compiler/tests/batch1_coverage_sema_modules.rs:521`, `:903`, `:962`;
`crates/aura-compiler/src/runtime_value_tests.rs:18057`.

The deep-field callable test intentionally expects checker-valid source beyond
eight field levels to be rejected by the validator. The self-recursive trait-view
test checks acceptance only and explicitly acknowledges lowering rejection.
These are disclosed limitations, not end-to-end success coverage. The latter's
runtime failure was not rerun here.

The task-result test returns `twice` and uses the same `twice` as its timeout
fallback, so output `4` cannot establish successful transfer. Give the fallback a
distinct result. Several other success-output cases use one-second deadlines;
host scheduling can select their timeout branches. The TLS contention test uses
a 50 ms sleep but also passes under sequential reads, so it does not establish
the advertised contending branch. The tuple-impl acceptance test contradicts
`docs/manual/generics-and-traits.md:228`, which requires a named outer impl type.

## Verification and record assessment

`gh run view` verified the following metadata and, for the branch/main coverage
and both cancellations, actual logs:

| Run | Observed result |
| --- | --- |
| [PR #9 CI 34676571772](https://github.com/johnolafenwa/Aura/actions/runs/34676571772) | Success on Ubuntu and macOS at `110a35b8`. Ubuntu: 4,157 missed lines, 96.47%; macOS: 4,160 missed, 96.46%. Both: 97.33% functions, 95.24% regions. |
| [Main CI 34681069434](https://github.com/johnolafenwa/Aura/actions/runs/34681069434) | At `1b130a03`, macOS succeeded. Ubuntu attempts 1 and 2 were cancelled after approximately 120 minutes. Attempt 1 was in hygiene; attempt 2 was in compiler-coverage processing with `llvm-profdata` still running. No preceding gate failure was found. |
| [PR #10 CI 34692511751](https://github.com/johnolafenwa/Aura/actions/runs/34692511751) | Success on both platforms at `0b770137`. |
| [Final main CI 34698888164](https://github.com/johnolafenwa/Aura/actions/runs/34698888164) | Success on both platforms at `d38381c4`; logs include the full `npm run ci` chain, raised compiler floors, parity, LSP coverage, docs, audits, Clippy, and hygiene. |

The cited branch Tutorial Examples/Docs runs `34676571746`/`34676571773` and
post-merge runs `34681069450`/`34681069559` also report success at their stated
commits. This supports the hosted completion claims. It does not independently
reproduce the earlier local coverage session or prove broader semantics than the
tests exercise. The reported local line/function percentages are consistent with
the hosted totals. Coverage floors are exactly 96.46 / 97.33 / 95.23 in
`scripts/coverage-compiler.sh:29`; reporting excludes test modules and uses the
documented single-threaded instrumented run.

The `110a35b8` test count is correct: 227 integration tests plus 123 private tests
= 350 newly added `#[test]` attributes. Its three shipped lowering-fix fixtures
pass both backends, but findings 1–4 show their incomplete coverage. The Callable
header fix is correct: 12→16 accounts for the missing header bytes and retains
conservative per-parameter slack. Its regression-test qualification is described
in the Callable estimator finding.
The relaxed prefix in `native_runtime_entrypoint_guards_invalid_inputs`
(`native_runtime_tests.rs:11877`) is acceptable: the process-wide source `OnceLock`
can legitimately add a location trailer, while the stable code/message prefix
remains checked. This does not cure the separate channel transaction race.

## Overall verdict

**Do not start H2 on the premise that H1 exactly implements the ratified
answers.** Hosted integration is real and the reference agent works, but the
source-reachable TaskCallable Transfer bypass is a phase blocker. Additional
fail-closed validation gaps, accepted-source failures, incorrect positive-program
rejections, incomplete tooling, and unratified representation/contract deviations
remain. Fix the task boundary and validator findings, correct the functional
findings or explicitly amend the accepted scope, then update the handoff. Phase
2's Option removal should not absorb them as unexplained new regressions.

Checked, no finding beyond the qualifications above: parser contextual surface;
union normalization/aliases and singleton collapse; injection/literal ambiguity;
direct-member type patterns and exhaustiveness; generic substitution/flattening;
Q6 B equality/hash and structural Copy/clone/Transfer; shared layout metadata,
interfaces and drop plans; exact nullable-handle FFI result recognition;
explicit packing/kind weakening, owned-capture mutation, specialized bound methods,
stored explicit-argument views and Shared callback positive controls; reference
agent version 1 and header-only stdout change; listed Manual pages/family commits;
schema progression 6→7→8→9→10→11→12→13→14 and current constants; union-layout/direct-FFI
versions 1; CI/floors/test counts and the diagnostic-prefix adjustment; Option and
all 39 H2 signature migrations remain excluded, and Display, collection-entry
loans, and decorator syntax are not taught as delivered. The security review found
no confirmed union tag/layout or payload-proof bypass, additional returned-view
validation gap beyond finding 14, bound-receiver ownership bypass, TypeParam capture
bypass, or ReadLoan-merge bypass.
