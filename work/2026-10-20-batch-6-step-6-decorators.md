# Batch 6 Step 6: Decorators

## Goal

Implement decorator syntax on module-level functions with concrete
decorators, the ratified order, and recursion through the decorated binding,
as section E of the Batch 6 checkpoint describes, on both backends.

## Work Completed

- The parser reads one or more `@expression` lines above a module-level
  `def`. Comments and blank lines may sit inside the group.
  `FunctionDecl.decorators` keeps each expression, its `@` span, and its
  source text. `@property` keeps its own path. A group with no function
  after it is `AU2047`. A general decorator in a function or class body stays
  `AU2005`, with a help line that names module-level functions.
- `sema/decorators.rs` runs before the derived-codec pass. It rewrites
  `@d1 @d2 def f` into a private function `__aura_undecorated_f`, a private
  alias `__aura_decorated_f` for `Callable[def(...) -> R]`, and a constant
  `f: Callable[...] = d1(d2(__aura_decorated_f(__aura_undecorated_f)))` at
  the function's place among the module constants. A call evaluates its
  callee first, so decorator expressions evaluate top to bottom and apply
  bottom to top. The body's own name resolves to the constant slot.
- `AU2047` rejects a generic decorated function, `main`, a parameter
  default, and a view result. A type mismatch at a decorator application
  keeps `AU2002` and names the type the decorator must take and return.
- The checker calls a shared callable module constant through a module
  member, as in `tools.fact(3)`. Before, only functions and classes were
  callable members.
- Analysis hides the generated names, and hover on a decorated function
  shows its decorator lines over the signature and docstring. `aura doc`
  lists it as a function with its decorators. `aura fmt` keeps a decorator
  line directly above its definition and no longer writes `@ property`.
- Three direct-backend callable bugs blocked decorated bindings. A contract
  invoke released the handle of a callable result it had unboxed. An invoke
  adapter could declare a contract descriptor after descriptors were
  defined. Boxing an owned callable argument, return, or capture kept an
  extra reference.
- The semantic interface version is 20 and the native cache format is `v8`.

## Deviation From The Checkpoint

The checkpoint's E3 example types decorators with thin `def` types. A thin
`def` value cannot carry a closure, so a wrapper could not capture `f`. The
decorated binding is a `Callable[def(...) -> R]` instead. A decorator takes
`own Callable[...]` and returns the same type. A closure cannot promise
parameter defaults, so a decorated function cannot declare one.

## Verification

- Run-pass `decorators` covers order, keyword-only parameters, a decorator
  built by a call, and recursion. Run-fail `decorator_trap` stops at the
  first decorator that traps.
- Check-fail `decorator_generic_function`, `decorator_main`,
  `decorator_parameter_default`, `decorator_reads_slot`,
  `decorator_wrong_input`, and `decorator_wrong_result`. Parse-fail
  `decorator_without_function`, `decorator_on_class`, and
  `decorator_in_function_body`.
- CLI test `decorated_functions_work_across_modules_on_both_backends`, and
  `stored_callables_return_callables_on_both_backends` for the native fixes.
- Analysis, API docs, and formatter unit tests.
- `examples/basics/decorators.au` on both backends.

## Follow-Up

- Step 7: generic decorators that return a new closure, and the `@retry`
  ownership rule. Packing a closure needs a non-generic alias today.
- Decorators on methods, classes, and enums are outside this batch.
