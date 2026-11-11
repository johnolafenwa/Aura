# Six older compiler bugs

Date: 2026-11-11. Asked for by John: fix the six bugs the trait objects
work turned up and left open, beside the fixes for the external review.

## Goal

1. `lambda: items[0].area()` fails at run time with `AU4001`.
2. In a shared `match`, `case Square as square:` then `other = square`
   fails with `AU4001`.
3. `impl Named for int64 | None` does not build.
4. `apply(run_one[T], job)` inside a function generic over `T` fails
   under `aura run` with "type `T` has no MIR implementation".
5. `aura verify` cannot prove an `ensures` after a `for` loop whose
   invariant bounds a running sum, and reports a counterexample.
6. A `mut self` method call on a list element costs thousands of
   instructions.

## Work completed

All of 1 to 5 reproduced on `main` @ `0694dba` before the fix.

### 1. Lambda that lends an element

- A lambda body was lowered as one expression followed by `Return`, with
  no loan scope. The hidden loan `items[0].area()` takes on `items[0]`
  ends with its statement, and there was none, so the function returned
  with the loan active.
- The body is now lowered as a `return` statement in a statement list,
  which ends the loan before the return.

### 2. Locals that share a borrowed value

- `other = square` moved out of `square`, which is a loan of the union's
  payload. A name bound to a loan is now a non-owning place, as a
  borrowed parameter is, so the local shares the value.
- The same failure with an enum payload (`case Shape.Named(name):` then
  `label = name`, reported on 2026-10-27 in the rust-speed work) and with
  a second local (`first = param` then `second = first`) is fixed the
  same way: the lowerer keeps the set of locals that share a value another
  place keeps, and a read of one for an owned value is a share.

### 3. `impl` for a union type

- A call on a union receiver only looked at the members. The lowerer's
  result type for the call and the dispatch in `mir_methods.rs` now look
  for an `impl` whose type is that union first.
- A union `impl` is not a runtime candidate for a call on an erased
  receiver: a union never reaches one, because a bound and a trait object
  conversion still ask every member to implement the trait. The manual
  now says so; it used to say there is no `impl` for a union at all.

### 4. Enclosing type parameters in written types

- A type written in a function body was lowered with no type parameters
  in scope, so the `T` of `run_one[T]` became a type named `T`. Types in a
  body are now lowered with the enclosing function's type parameters,
  taken from its bounds and from its parameter, local, and return types.

### 5. `for` invariants at the end of the loop

- A `for` invariant was checked at the start of the body. The edge that
  leaves the loop when it runs out never passed a check, so the verifier,
  which cuts a loop at its header, knew nothing about the loop's locals
  after it.
- A clause that does not read the loop target is now checked in the
  loop's header block, ahead of the test for the next element, as a
  `while` invariant is. A clause that reads the target stays in the body.
  A `range` loop already checked its clauses in the header.
- This changes run-time behavior: such a clause is now also checked when
  the loop runs out and on entry to a loop that runs no iteration.

### Tests

- Run fixtures `lambda_element_method_calls`,
  `match_shared_binding_aliases`, `union_type_trait_impls`,
  `explicit_enclosing_type_arguments`, `for_invariants_at_loop_end`, and
  the failing run `for_invariant_fails_at_end`. Each prints the same on
  both backends and is clean under valgrind natively.
- Library tests run the same files, and `verify_tests.rs` proves the
  `ensures` after list, dictionary, `enumerate`, and set loops.

## Open

- 6, the cost of a `mut self` call on a list element, is next. Measured
  on `main`: about 2,800 instructions for `items[i].bump(1)` and for
  `for item in mut items: item.bump(1)`, and the same for a shared
  `items[i].get()` by index, against 7 for `for item in items: item.get()`
  on a class of scalars.
- A generic function whose bound names a trait that no type implements
  (or only a union type implements) does not build: "no type declares
  `.name` for `T`". It runs under `aura run`. Found while testing 3.
