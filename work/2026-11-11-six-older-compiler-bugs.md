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

## Found by CI

- The fix for 4 lowers an annotation that names the enclosing type
  parameter, `value: T`, as that parameter. A method call on such a value
  looks the method up through the parameter's bounds, and a bound written
  through an import alias (`T: sh.Named` after `import shapes as sh`) did
  not resolve: `module_namespace` found a module by its path only. The
  same call on a parameter of type `T` failed on `main` too. It now reads
  the name the import binds first.
  `module_qualified_traits_work_in_bounds_impls_and_supertraits_on_both_backends`
  covers both.

## 6. Method calls on list elements

Measured on `main`: about 2,800 instructions for `items[i].bump(1)`, for
`for item in mut items: item.bump(1)`, and for a shared `items[i].get()`
by index, against 10 for `for item in items: item.get()`, on a class of
scalars. The native backend followed an element loan only when the body
read or wrote the element's fields. A method call made it fall back to
the runtime, which takes the element out of the list and puts it back.

- `native_element_loans` now follows a loan whose element is the receiver
  of a call: a lend to one method call, a call on the loan itself (a trait
  method, or a call through `dyn`), and reads and writes of the element's
  fields by name inside a whole-element loan (`item.count += 1` in
  `for item in mut items:`).
- An element of a list of all-scalar classes is lent as its words in
  place. A `mut self` call copies the words in and the result back.
- A boxed element is handed to a `mut self` method without a retain, and
  put back only if the method replaced it. The task ledger does not hold
  the list's own reference, so this is only done for a method that never
  releases its receiver: `receiver_keepers` finds the methods that name
  `self` whole only to call another such method on it. A method that
  assigns `self` takes the old path.
- A `mut self` method from an `impl` block is called on the loan itself,
  and the ownership pass retains the loan before it. The loan analysis
  accepts that retain, and the emitter defers it to the call: the fast
  path hands the list's own reference over with no retain, and only a call
  that falls back to the general path makes it.
- `for item in items: item.get()` cloned each element of a class with a
  `str` field, because a call on a borrowed receiver did not count as a
  read of the lent element. It does now.

Instructions per call in the release build, method inlined, 200 passes
over 1,000 elements:

| loop | class of numbers | class with a `str` field |
| --- | ---: | ---: |
| `for item in items: item.get()` | 10 | 1,615 to 46 |
| `items[i].get()` | 2,814 to 15 | about 2,300 to 73 |
| `for item in mut items: item.bump(1)` | 2,827 to 36 | about 2,300 to 92 |
| `items[i].bump(1)` | 2,827 to 36 | about 2,300 to 92 |
| `items[i].grow(1)`, a trait method from an `impl` block | 3,347 to 50 | 3,150 to 88 |
| `for item in mut items: item.count += 1` | 3,637 to 15 | 4,243 to 37 |
| `items[i].count += 1` | 16 | 35 |

Probes `element_mut_call` and `element_plain_mut_call` read 94.1 and 64.1
against Rust's 12.0 and 16.0. There the method is not inlined, and its
frame entry and exit take about 50 of the instructions.

Tests: run fixtures `element_method_calls_plain_classes`,
`element_method_calls_plain_trait_impls`,
`element_method_calls_boxed_and_dyn`,
`element_method_calls_shared_in_mut_loops`,
`element_method_calls_replacing_receivers`, and
`element_method_calls_trait_impl_receivers`, the same on both backends and
clean under valgrind with the debug ownership ledger, and the library test
`native_element_loans_follow_method_calls_on_list_elements`.

## Open

- The first pass over a list still takes the runtime path for about one
  element in eight, until the list is promoted to hold its elements in a
  form the loan can reach. The promotion policy is unchanged.
- A call that is not inlined pays about 50 instructions for its frame
  (the stack-trace frame and saved registers), which keeps the two new
  probes at 7.8 and 4.0 times Rust.
- A generic function whose bound names a trait that no type implements
  (or only a union type implements) does not build: "no type declares
  `.name` for `T`". It runs under `aura run`. Found while testing 3.
