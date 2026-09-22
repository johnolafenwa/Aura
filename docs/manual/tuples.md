# Tuples

A tuple is a fixed-size value that holds a known number of elements, each with its own type. Use tuples to return, pass, unpack, and pattern-match a small group of values. Tuples are not variable-size collections.

## Grammar

The normative productions are in [Complete Grammar](/manual/grammar):

```ebnf
tuple-expression = "(", expression, ",", ")"
                 | "(", expression, ",", expression,
                   { ",", expression }, ")" ;

tuple-type = "(", type, ",", ")"
           | "(", type, ",", type, { ",", type }, ")" ;

unpack-target
    = binding-target, ",", binding-target, { ",", binding-target }
    | "(", binding-target-list, ")" ;

binding-target-list
    = binding-target, ","
    | binding-target, ",", binding-target, { ",", binding-target } ;

binding-target
    = identifier
    | "(", binding-target-list, ")" ;

tuple-pattern = "(", pattern, ",", ")"
              | "(", pattern, ",", pattern, { ",", pattern }, ")" ;
```

Tuple values are always parenthesized:

- `(value)` is grouping.
- `(value,)` is a singleton tuple.
- `()` is not a tuple value.
- A multi-element tuple has no trailing comma.

```aura
def main():
    pair = ("north", 7)
    singleton = (true,)
    nested = (pair, (2, 3))
```

Top-level assignment and `for` binding lists use `left, right`. Inside a target, parentheses mark a nested target or a singleton target. Tuple types and tuple patterns are parenthesized.

## Typing Rules

A tuple type records one exact element type at each position:

```aura
def location() -> (str, int64):
    return ("north", 7)

point: (int64, int64) = (3, 4)
```

When an expected tuple type is present, the tuple expression's arity and element types must match it exactly. Otherwise each element is inferred in its own position.

Tuple types are structural. Two tuple types are equal exactly when they have the same arity and equal corresponding element types.

**Equality.** Tuple `==` and `!=` require both operands to have the same static tuple type. Equality compares corresponding element values recursively. When one operand is a tuple literal and the other has a known tuple type, that exact type contextually types the literal, recursively. This rule is symmetric.

`<`, `<=`, `>`, and `>=` are not defined for tuples. Aura does not infer a lexicographic ordering.

**Optional tuples.** An optional tuple is the ordinary union of a complete tuple type with `None`: `(str, int64) | None`.

**`indirect`.** `indirect` tuple types are rejected. `indirect` is the facility for recursive named fields. So a class field cannot place its recursive link inside a tuple. Put that link in a separately named `indirect` field instead. The compiler's diagnostic for the tuple case points to that fix.

**Unpacking and patterns.** An unpacking target or tuple pattern must have the scrutinee's exact recursive tuple shape. Each binding leaf receives its corresponding element type. The ordinary binding rules reject duplicate names and a leaf that shadows a visible name. A tuple binding leaf is a name, not a member or index place.

**Indexing.** A tuple index must be a non-negative integer literal known at compile time. It must select an existing position, and that element's type must be copyable. The expression has the selected element's type. These are static errors:

- a computed index
- a negative literal
- an out-of-bounds literal
- selection of a non-copy element

## Runtime Semantics

A tuple value stores its elements in source order. Construction evaluates and captures each element from left to right.

An unpacking operation evaluates its right side or iteration item exactly once. It then binds leaves left to right, following the recursive tuple shape.

A tuple-pattern match evaluates the scrutinee once and tests arms in source order. The first matching arm executes. A tuple pattern is irrefutable when all nested patterns are binding patterns or `_`. Literal and enum subpatterns keep their usual matching and exhaustiveness rules.

Constant tuple indexing selects the statically named position and returns a copy. There is no runtime index expression to evaluate.

**Equality.** Tuple `==` compares corresponding element values from left to right, using each element type's ordinary equality. Nested tuples follow the same rule recursively. The result is `true` only when every corresponding comparison is true. Comparison stops at the first unequal element. Tuple `!=` is the logical negation of tuple `==`.

Both complete operand expressions are evaluated once, left to right. The comparison reads the two resulting tuple values and consumes neither, even when an operand contains non-copy elements. Evaluating an operand expression still has its ordinary ownership effects. The equality operation itself adds no move.

A tuple value may carry runtime element-type, transport, or backend metadata. That metadata is not an additional equality component, because the checker has already required one common static tuple type.

Tuple equality links follow the ordinary comparison-chain contract. For example, `first == middle != last` evaluates `first`, then `middle`, and compares the first link. It evaluates `last` only when that link is true. Each evaluated operand, including `middle`, is evaluated once. Tuple ordering is a static error.

**Rendering.** A rendered tuple uses parentheses, `, ` between elements, and one final comma for a singleton: `(1, 2)` and `(1,)`. Each element uses its ordinary Aura rendering, so a contained `str` is not quoted. `print`, f-string interpolation, and backend diagnostics all use this format. Rendering plays no part in tuple equality and does not define tuple ordering.

```aura
def make_record() -> (str, int64):
    return ("Aura", 7)

def main():
    record = make_record()
    assert record == ("Aura", 7)
    assert record != ("Aura", 8)
    name, version = record
    print(name)
    print(version)

    copy_pair = (10, 20)
    print(copy_pair[1])

    for label, count in [("ready", 2), ("done", 3)]:
        print(f"{label}:{count}")

    nested = ((1, 2), true)
    assert nested == ((1, 2), true)
    assert nested != ((1, 3), true)
    assert (1, 2) == (1, 2) != (2, 1)
    match nested:
        case ((left, right), flag):
            print(left + right)
            print(flag)
```

```text
Aura
7
20
ready:2
done:3
3
true
```

## Ownership And Evaluation Order

A tuple is copyable if and only if every element type is copyable. Assignment, owned argument passing, returns, and pattern flow then follow the ordinary copy or move rule for the tuple as a whole.

**Unpacking.** Unpacking a copy tuple copies its elements and leaves the source usable. Unpacking a non-copy tuple consumes the whole source exactly once and gives owned leaf bindings. Aura does not turn positional fields into independently reusable partial-move places. Any later use of the source is diagnosed as use after move.

**Equality.** Tuple `==` and `!=` are shared reads. They do not unpack or transfer ownership. Both operands stay usable, including a non-copy tuple such as `(str, int64)`.

**Iteration.** In collection iteration, tuple leaves inherit the ownership provenance of the yielded element:

- bare shared iteration retains the collection and gives shared
  leaf provenance for non-copy tuple elements
- `own` iteration consumes the collection and gives owned leaves
- bare Queue iteration receives an owned tuple item and gives owned leaves

Mutable-borrow iteration with a tuple target is rejected. Aura does not rebuild a recursively unpacked tuple and write it back into a collection element.

**Matching.** `match own` consumes a non-copy tuple scrutinee and gives owned leaf bindings. Bare `match` retains the tuple and gives shared leaf provenance. `match mut` with a tuple pattern is rejected, because tuples do not support mutable tuple-pattern writeback.

## Diagnostics

| Code | Cause |
| --- | --- |
| `AU1101` | a malformed tuple expression, type, target, or pattern, or misplaced commas |
| `AU2002` | an annotated tuple element-type mismatch |
| `AU2999` | a tuple shape or arity mismatch, reported with the checker's general code |
| `AU3001` | using a non-copy tuple after whole-source unpacking or `match own`, reported at the move |
| `AU3002` | trying to move an element through shared unpacking or bare `match` |

Unsupported tuple operations are rejected at check time. These include non-constant or invalid indexing and mutable tuple writeback forms. The diagnostic names the restriction and the supported alternative.

## Backend Support

Both backends implement tuples: MIR execution and direct native generation. MIR is the compiler's mid-level intermediate representation. The implemented surface covers:

- tuple construction and fixed structural types
- function returns
- recursive assignment and loop unpacking
- tuple patterns
- whole-source ownership
- copy-only constant indexing
- recursive structural equality

Maintained parity fixtures require both backends to produce the same output and primary diagnostics.

## Limits And Implementation-Defined Behavior

Aura 0.3 does not have these tuple features:

- an empty tuple
- a trailing comma on a multi-element tuple
- tuple iteration
- tuple methods
- tuple ordering
- named tuple elements
- rest or star unpacking
- mutable tuple-target writeback
- tuple slicing
- dynamic tuple indexing

A tuple is not implicitly converted to or from `list`.

These behaviors are language-defined, not implementation-defined:

- tuple element order
- left-to-right construction
- recursive shape matching
- copy classification
- whole-source non-copy moves
- constant-index results
- recursive equality

Runtime tuple metadata cannot change the equality result.

## Status

Tuples are implemented as described on this page:

- parenthesized tuple values and types
- function returns
- recursive assignment and `for` unpacking
- recursive tuple patterns
- structural copy classification
- whole-source moves
- shared borrowed destructuring
- copy-only constant indexing
- same-static-type recursive `==` and `!=`

The limits above are intentional parts of the design.

Design record: [ADR-0026](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0026-minimal-tuples.md).
