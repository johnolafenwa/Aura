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

Verification is in progress.
