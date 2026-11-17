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
