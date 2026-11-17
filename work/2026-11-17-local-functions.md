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
which has the same runtime sources. The focused language-server and grammar
tests passed. Z3 proved both assertions in a local function's two branches
and reported a real overflow for an unbounded counter. Full-suite checks
are in progress.
