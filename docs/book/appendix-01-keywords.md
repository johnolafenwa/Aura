# Appendix A: Keywords

This appendix lists the words Aura's lexer treats specially. Most of them are
*reserved*: you cannot use them as the name of a binding, a parameter, a
function, or a field. A handful more are *contextual*: they are ordinary
identifiers almost everywhere, and take on a special meaning only in one
position. The [Lexical structure](/manual/lexical-structure) page of the
Manual has the exact rules.

## Reserved keywords

If you try to use one of these as a name, the parser stops you. It does not
know you meant a name, so the message describes the token it found instead:

```aura check-fail:AU1101
def area(class: str) -> str:
    return class
```

```console
$ aura check kw.au
error[AU1101]: expected identifier
 --> kw.au:1:10
  |
1 | def area(class: str) -> str:
  |          ^
```

When you see `expected identifier` pointing at a perfectly reasonable word,
check this list. The usual fix is a slightly longer name: `kind` or
`class_name` for `class`, or `is_match` for `match`.

Here are the reserved words, grouped by what they do.

Declarations:

- `def` declares a function or method.
- `class` declares a class. Put `copy` right before it (`copy class Point:`)
  to make a class whose values copy instead of moving.
- `enum` declares an enum with variants.
- `trait` declares a trait, a named set of methods that types can implement.
- `impl` implements a trait for a type: `impl Display for Account:`.
- `public` marks a declaration as visible to modules that import this one.
- `import` brings a module into scope: `import fs`, `import process as p`.
- `extern` and `opaque` declare C functions and C handle types for the FFI
  (Chapter 18).
- `indirect` marks a class field that is stored indirectly, which lets a
  class refer to itself: `next: indirect Node | None = None`.

Ownership and access:

- `mut` declares a mutable binding (`mut total = 0`), a parameter the callee
  may change (`items: mut list[str]`), or a receiver that changes its object
  (`def push(mut self, ...)`). It also appears in `for ... in mut ...` and
  `match mut`.
- `own` declares a parameter the callee takes ownership of (`job: own Job`), a
  consuming receiver (`own self`), and consuming loops and matches
  (`for job in own jobs`, `match own result`).

Control flow:

- `if`, `elif`, and `else` choose between branches. `else` also appears in
  the conditional expression `a if condition else b`.
- `while` loops while a condition holds.
- `for` and `in` loop over a collection, a range, or a queue. `in` is also the
  membership operator: `"a" in counts`.
- `break` leaves the innermost loop; `continue` starts its next iteration.
- `return` leaves a function, with or without a value.
- `match` and `case` choose a branch by pattern.
- `pass` does nothing, for a block that must contain a statement.
- `assert` checks a condition and traps if it is false:
  `assert total > 0, "no items"`.

Values, operators, and other forms:

- `true` and `false` are the two `bool` literals. They are lowercase.
- `and`, `or`, and `not` are the boolean operators.
- `try` passes an `Err` up to the caller: `text = try fs.read_to_string(path)`.
- `with` opens a scope that owns a resource and cleans it up on exit:
  `with group = TaskGroup():`.
- `as` names the value a pattern matched (`case str as line:`), renames an
  import (`import process as p`), and casts numbers (`count as int64`).
- `from` starts an import of specific names: `from fs import exists`.

`from` is a special case worth knowing. It is only a keyword at the start of
a module-level import, so it remains usable as a parameter name, a field name,
or a named argument. This is legal:

```aura
class Message:
    from: str
    to: str


def main():
    note = Message(from="ada", to="grace")
    print(note.from + " -> " + note.to)
```

## Contextual words

These words are ordinary identifiers to the lexer. They mean something special
only in a particular position, and you can use them as names elsewhere,
although doing so tends to confuse readers.

- `self` names the receiver in a method.
- `Self` refers to the current type inside a trait or an `impl`.
- `copy` makes a class a copy class when it appears directly before `class`.
- `None` is the unit value, and the "nothing here" member of a `T | None`
  union.
- `is` appears only in `value is None` and `value is not None`. Aura has no
  identity comparison between arbitrary values, so `a is b` is a syntax error.
- `lambda` starts an anonymous function: `lambda n: n + 1`.
- `view` creates a named alias for a place without moving or copying it:
  `view first = names[0]`, or `view mut` for a writable alias.
- `type` declares a type alias at module level:
  `type Reader = Callable[def() -> int64]`.
- `set` names the builtin set type.
- `_` is the wildcard in a pattern (`case _:`), and a conventional name for a
  value you do not use (`for _ in range(3):`).

## Words Aura does not reserve

If you come from Python, you may reach for words that Aura does not treat as
keywords at all: `null`, `yield`, `async`, `await`, `del`, `global`,
`nonlocal`, `raise`, `except`, and `finally`. They are plain identifiers, so
the error you get depends on how you used them. `x = null` is reported as an
unknown name (`AU2001`). `raise ValueError("x")` and `yield 1` read to the
parser as a name followed by more tokens than a statement can hold, so you get
`AU1101: expected Newline` pointing right after the word. `try:` followed by
`except` gets a targeted `AU2005` message that points you at `Result` and
`match`. Appendix C has the Aura way to express each of these ideas.
