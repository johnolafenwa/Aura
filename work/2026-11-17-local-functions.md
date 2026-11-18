# Local Functions

The approved design gives a local function a written signature and a
suite of statements. The checker and both backends must agree on capture
ownership and on the environment used by a recursive call.

The new fixture exercises a private mutable counter, returning a recursive
shared closure, a capture-free factorial helper and a local function with
an explicit mutable loan. The baseline compiler rejected its first local
`def` with AU1101. Separate diagnostic fixtures cover recursion through
mutated state, reassignment of the declared name and escape of a capture
loan. Editor coverage checks visibility and navigation inside the body.

The MIR interpreter and optimized native executable produced identical
output for the positive fixture. All three negative fixtures produced their
expected diagnostics. The native runtime came from the stored-call build,
which has the same runtime sources. All 159 language-server tests and the capture-list grammar
test passed. All 1,096 documentation examples passed. Z3 proved both assertions in a local function's two branches
and reported a real overflow for an unbounded counter. Full-suite checks
are in progress.

A recursive name resolves to a direct call, so the generated function has
no runtime slot for its own name. Native code for a local function and a
lambda with the same captured arithmetic now has the same hot path.
Measurements are in `benchmarks/closures/README.md`. A parameter can
shadow the local function's name. Shared recursion over a captured list,
mutation of private state and calling a captured list element by a
captured index produced matching MIR and native output.

The call kind accounts for captures consumed on an early return. The
checker had previously discarded the move state of returning branches,
which allowed a recursively called closure to consume its environment on
one branch. The regression covers a local function whose base case moves
a captured list.

The Rust and Python equivalents of the named-local benchmark also give
473,089 after 48 million calls. In three rotating, CPU-pinned runs the
medians were 220 ms for the Aura local function, 80 ms for Rust and
10,872 ms for Python. All 159 language-server tests passed again. On a
busy-worker repeat of the documentation suite, eight of the 1,096 examples
hit their 30-second process limit. Each passed when rerun on a reserved CPU.

## November 18 native calls

Native invoke adapters keep their shape descriptor across calls and update
captured values in the environment. The compiler uses a pointer-relative
address to reach inline captures. The optimized local-function and lambda
loops both call their known targets directly. Fourteen fixtures covering
capture modes, stored and returned callables, tasks, recursion and temporary
values matched all 81 expected output lines on MIR and native execution.

The benchmark passes the same runtime seed and count to Aura, Rust and
Python. The default 48-million-call loops returned 473,089. Three full
runs measured median wall times of 386 ms for the named Aura function,
388 ms for the Aura lambda, 296 ms for Rust and 18,443 ms for Python.
See [the benchmark notes](../benchmarks/closures/README.md) for the
separate process CPU measurements.
