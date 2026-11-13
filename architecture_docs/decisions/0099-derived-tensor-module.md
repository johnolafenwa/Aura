# ADR-0099: The compiler writes `collect` for a bodyless `impl tensor.Module`

- Status: Accepted
- Date: 2026-11-12

## Context

A model implements `tensor.Module` (ADR-0098) by writing `collect`, one
line for each field that holds weights. PyTorch finds those fields by
itself. Writing them by hand is the main place where an Aura model reads
unlike a PyTorch one, and a forgotten line is a weight that silently
does not train.

Aura already derives one trait: `impl json.Codec for Point` with no body
(ADR-0062).

## Decision

`impl Module for Net` with no body asks the compiler for `collect`. The
pass runs before checking and writes ordinary Aura source, as the JSON
derive does. It walks the class's fields in declared order:

- `Parameter[float32]` becomes `into.parameter(name, self.name)`.
- A type that implements `Module` becomes `into.add(name, self.name)`.
  That is a class of the same module with an `impl Module`, an imported
  class whose module has one, `dyn Module`, `dyn Layer`, and a trait
  object of a trait of the same module that extends one of them.
- A list of such a type becomes `into.add_list(name, self.name)`.
- A field of any other type is left out.

A field that holds weights in a shape the pass does not walk is an
error, `AU2046`: a union, a tuple, a dictionary, a nested list, a list
of parameters, a `Parameter` of another dtype. The model then writes
`collect` by hand. A generic class cannot derive `Module`.

The rule is by type, so nothing depends on how a field is assigned. A
plain `Tensor` field is a constant of the model and is not collected.

## Why not other ways

- **A default `collect` in the trait.** A default body cannot see the
  fields of the type it runs for.
- **Registration at construction, as PyTorch does.** It needs hidden
  state in every model and a base class, and Aura has neither.
- **Leaving unknown shapes out.** A weight that silently does not train
  is the failure this decision is meant to remove.

## Consequences

- The `mut` rule of ADR-0098 is unchanged. The derived method is the
  same `collect(self, into: mut Collector)` a program could write, and a
  `Collector` stays opaque.
- The pass decides by the written type and the `impl` declarations it
  can see. A trait object of a trait from another module that extends
  `Layer` is not recognised and is left out. This is the one silent
  case, and the manual names it.
- `model(x)` stays out of the language (ADR-0098).
