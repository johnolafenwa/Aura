# Verification gaps, 2026-11-24

John asked on 2026-11-24 for the gaps that a survey of `aura verify` found
on 0.17.0 to be closed. The work is in three stages. The rule behind each
item: `aura verify` never reports a correct program as failing with an
invented counterexample, and never reports a wrong program as proved.

## Stage 1: wrong answers

| Gap | Before | After |
| --- | --- | --- |
| Unmodeled results | `len(text.split(",")) >= 1` failed with `text = ""` | `AU5104`, naming `split` and its line, no counterexample |
| Contradictory contract | `requires x > 0 and x < 0` proved anything | `AU5120`, not counted as proved |
| `Span[T]` and views | `requires len(xs) > 0` then `xs[0]` failed | proved; a write through a `mut` span or view reaches its list |
| `Array[T]` | no index check at all | rank and each coordinate checked |
| Generators | the body did not assume the generator's `requires` | assumed, reported under the generator's name |
| Trait implementations | no class invariant in `impl Trait for Class` methods | assumed on entry, proved at calls, conversions and `mut self` returns |
| Frame rule | a `mut self` call forgot every field its `ensures` left out | fields the callee cannot write keep their values |

Two defects found on the way and fixed: a write through a view passed to a
`mut` parameter did not reach the viewed list in the proof, so an assertion
about the list could be proved wrongly; and a generator with no clauses was
checked a second time in its state-machine copy, which reported failures
under an internal name.

Decisions: ADR-0150 (unmodeled results) and ADR-0151 (contracts that cannot
hold).

## Left open by stage 1

- A one-coordinate `Array` read needs `requires len(a.shape()) == 1`: it
  traps on any other rank, and the type does not carry the rank.
- A generator cannot yet state what it yields. The checker rejects
  `ensures` on a generator function, so this needs a checker change.
- An object read out of a `list[Class]` is not known to hold its class
  invariant, so converting it to a trait object, or calling a trait
  implementation method on it, has an open class invariant check.
- A call on a union value proves nothing about the method it runs, not
  even its `requires`.
- A function that returns a `view Span[T]` cannot say what range it has.
