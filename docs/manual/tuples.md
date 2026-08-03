# Tuples

Tuples are fixed-size, heterogeneous product values. Aura's minimal tuple
surface is intended for returning, passing, unpacking, and pattern-matching a
known number of values. Tuples are not variable-size collections.

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

Tuple value expressions are always parenthesized. `(value)` remains grouping,
while `(value,)` is a singleton tuple. `()` is not a tuple value. A
multi-element tuple has no trailing comma:

```aura
def main():
    pair = ("north", 7)
    singleton = (true,)
    nested = (pair, (2, 3))
```

Top-level assignment and `for` binding lists use `left, right`; parentheses
represent a nested target or a singleton target. Tuple types and tuple patterns
are parenthesized.

## Typing Rules

A tuple type records one exact element type at each position:

```aura
def location() -> (str, int64):
    return ("north", 7)

point: (int64, int64) = (3, 4)
```

The tuple expression's arity and element types must exactly match an expected
tuple type when one is present. Otherwise each element is inferred in its own
position. Tuple types are structural: two tuple types are equal exactly when
they have the same arity and equal corresponding element types.

Tuple value `==` and `!=` require both operands to have the same static tuple
type. Equality then compares corresponding element values recursively.
When one operand is a tuple literal and the other has a known tuple type, that
exact type contextually types the literal recursively; this rule is symmetric.
`<`, `<=`, `>`, and `>=` are not defined for tuples; Aura does not infer a
lexicographic ordering.

The ordinary optional-type suffix applies to a complete tuple type:
`(str, int64)?` is `Option[(str, int64)]`. `indirect` tuple types are
rejected; `indirect` remains the recursive named-field facility. Consequently,
a class field cannot place its recursive link inside a tuple. Put that link in
a separately named `indirect` field instead; the compiler diagnoses the tuple
case with that exit.

An unpacking target or tuple pattern must have the scrutinee's exact recursive
tuple shape. Each binding leaf receives its corresponding element type.
Duplicate names and a leaf that shadows a visible name are rejected by the
ordinary binding rules. A tuple binding leaf is a name, not a member or index
place.

Tuple indexing accepts only a non-negative integer literal known at compile
time. The literal must select an existing position, and that element's type
must be copyable. The expression's type is the selected element type. A
computed index, a negative literal, an out-of-bounds literal, or selection of a
non-copy element is a static error.

## Runtime Semantics

A tuple value stores its elements in source order. Construction evaluates and
captures each element from left to right. An unpacking operation evaluates its
right side or iteration item exactly once, then binds leaves left to right
according to the recursive tuple shape.

A tuple-pattern match evaluates the scrutinee once and tests arms in source
order. The first matching arm executes. Tuple patterns are irrefutable when
all nested patterns are binding patterns or `_`; literal and enum subpatterns
retain their existing matching and exhaustiveness rules.

Constant tuple indexing selects the statically named position and returns a
copy. It has no runtime index expression to evaluate.

Tuple `==` compares corresponding element values from left to right using each
element type's ordinary equality semantics. Nested tuples apply the same rule
recursively. The result is `true` only when every corresponding comparison is
true; comparison stops at the first unequal element. Tuple `!=` is the logical
negation of tuple `==`.

Both complete operand expressions are evaluated once, left to right. The
comparison reads the two resulting tuple values and consumes neither, even
when an operand contains non-copy elements. Runtime element-type, transport,
or backend metadata carried with a tuple value is not an additional equality
component; the checker has already required one common static tuple type.
Evaluating an operand expression still has its ordinary ownership effects;
the equality operation itself adds no move.

Tuple equality links use the ordinary comparison-chain contract. For example,
`first == middle != last` evaluates `first`, then `middle`, compares the first
link, and evaluates `last` only when that link is true. Each evaluated operand,
including `middle`, is evaluated once. Tuple ordering remains a static error.

Tuple rendering uses parentheses, `, ` between elements, and one final comma
for a singleton: `(1, 2)` and `(1,)`. Each element uses its ordinary Aura
rendering, so a contained `str` is not quoted. `print`, f-string
interpolation, and backend diagnostics use this same format. Rendering is not
part of tuple equality, and it does not define tuple ordering.

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

A tuple is copyable if and only if every element type is copyable. Assignment,
owned argument passing, returns, and pattern flow then follow the ordinary copy
or move rule for the tuple as a whole.

Unpacking a copy tuple copies its elements and leaves the source usable.
Unpacking a non-copy tuple consumes the whole source exactly once and gives
owned leaf bindings. Aura does not turn positional fields into independently
reusable partial-move places; any later source use is diagnosed as use after
move.

Tuple `==` and `!=` are shared-read operations rather than unpacking or
ownership transfer. They leave both operands usable, including a non-copy
tuple such as `(str, int64)`.

For collection iteration, tuple leaves inherit the ownership provenance of the
yielded element:

- bare shared iteration retains the collection and gives shared
  leaf provenance for non-copy tuple elements
- `own` iteration consumes the collection and gives owned leaves
- bare Queue iteration receives an owned tuple item and gives owned leaves

Mutable-borrow iteration with a tuple target is rejected. Aura does not
reconstruct and write a recursively unpacked tuple back into a collection
element.

`match own` consumes a non-copy tuple scrutinee and gives owned leaf bindings.
Bare `match` retains the tuple and gives shared leaf provenance.
`match mut` with a tuple pattern is rejected; mutable tuple-pattern
writeback is outside this surface.

## Diagnostics

Malformed tuple expressions, types, targets, patterns, or comma placement are
`AU1101`. An annotated tuple element-type mismatch is `AU2002`; tuple
shape/arity mismatches use the checker's general `AU2999` code. Unsupported
tuple operations, including non-constant or invalid indexing and mutable tuple
writeback forms, are rejected at check time with a diagnostic that identifies
the restriction and the supported alternative.

Using a non-copy tuple after whole-source unpacking or `match own` is
`AU3001` and points to the move. Attempting to move an element through shared
unpacking or bare `match` is `AU3002`.

## Backend Support

Tuple construction, fixed structural types, function returns, recursive
assignment/loop unpacking, tuple patterns, whole-source ownership, and
copy-only constant indexing are implemented alongside recursive structural
equality for MIR execution and direct native generation. Maintained parity
fixtures require both backends to produce the same output and primary
diagnostics.

## Limits And Implementation-Defined Behavior

Aura 0.3 has no empty tuple, multi-element trailing tuple comma, tuple
iteration, tuple methods, tuple ordering, named tuple elements, rest/star
unpacking, mutable tuple-target writeback, tuple slicing, or dynamic tuple
indexing. A tuple is not implicitly converted to or from `list`.

Tuple element order, left-to-right construction, recursive shape matching,
copy classification, whole-source non-copy moves, constant-index results, and
recursive equality are language-defined rather than implementation-defined.
Runtime tuple metadata cannot change the equality result.

## Status

The minimal tuple kernel and its Batch 3 B3.0-c equality amendment are Accepted
under ADR-0026. The maintained implementation includes parenthesized tuple
values and types, function returns, recursive assignment and `for` unpacking,
recursive tuple patterns, structural copy classification, whole-source moves,
shared borrowed destructuring, copy-only constant indexing, and
same-static-type recursive `==` and `!=`. The limits above remain intentional
parts of the accepted boundary.
