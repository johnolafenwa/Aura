# ADR-0112: Set algebra as methods and operators of `set[T]`

- Status: Accepted
- Date: 2026-11-16

## Context

`set[T]` had `add`, `remove`, `discard`, membership, and iteration. The
union or intersection of two sets was a hand-written loop, and the
manual listed "set algebra and set relations" as unavailable.

## Decision

`set[T]` adds these methods. `other` is a `set[T]` of the same element
type and is only read:

| Method | Result |
| --- | --- |
| `union(other)` | the receiver's elements, then the elements only `other` has |
| `intersection(other)` | the receiver's elements that `other` also has |
| `difference(other)` | the receiver's elements that `other` lacks |
| `symmetric_difference(other)` | the elements only the receiver has, then the elements only `other` has |
| `is_subset(other)`, `is_superset(other)`, `is_disjoint(other)` | `bool` |
| `update(other: own set[T])` | moves `other`'s elements into the receiver |

The operators `|`, `&`, `-`, and `^` between two sets of one type are
the first four methods, and `|=`, `&=`, `-=`, `^=` follow from them.
The four that build a set copy elements, so they need a clone-safe
element type, like `copy`. `update` moves and has no such need.

A result keeps insertion order as stated in the table, so a program
prints the same thing on every run and every backend.

The methods are builtin members implemented in the runtime, one
routine shared by the interpreter and compiled code. Operators are
rewritten to the method calls before code generation.

## Why not other ways

- **Operator traits (`BitOr` and so on) implemented for `set`.** Aura
  has operator traits only for arithmetic and ordering. Adding four
  traits for one type is more surface than the feature needs. The
  operator table can gain them later without changing set programs.
- **Aura source in the prelude.** A generic loop over a set costs far
  more per element today than the runtime routine, which hashes each
  element once and never boxes.
- **Comparison operators for subsets (`<=`).** `a <= b` on sets reads
  as ordering to many readers. The named methods are unambiguous.

## Consequences

- `-` on two sets is no longer a type error.
- Sets of different element types do not combine: `AU2002`.
- `intersection` and `difference` probe `other` once per element of
  the receiver. `is_disjoint` probes the larger set with the smaller.
