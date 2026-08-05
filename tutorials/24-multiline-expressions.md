# Multiline Expressions

Aura can keep one logical expression readable across several physical lines.
The rule is simple: the line continues while `(`, `[`, or `{` remains open.

## Calls And Signatures

```aura check-pass
def combine(
    left: int64,
    right: int64
) -> int64:
    return left + right

answer = combine(
    20,
    22
)
```

The closing `)` returns to the surrounding block indentation. The newline
after that line ends the logical statement.

## Collections And Grouping

```aura check-pass
values = [
    10,
    20
]

labels = {
    "first": values[0],
    "second": values[1]
}

total = (
    values[0]
    + values[1]
)
```

Continuation indentation is for readers. It does not create an Aura suite,
and the compiler would accept different numbers of leading spaces. Use one
extra four-space level so the structure remains obvious.

## Comments And Blank Lines

A trailing comment can end one continued physical line:

```aura check-pass
values = [
    10, # first input

    20
]
```

The blank line and comment do not close `[`, so the list continues.

## Match Expressions Keep Their Layout

`match` still needs an indented `case` block even when it appears inside a
call:

```aura fragment
print(
    match status:
        case Ready: "ready"
        case _: "waiting"
)
```

The `case` block is a layout island inside the continued call. Every arm keeps
the normal match layout. The containing
delimiter can close after the last inline arm or on its own following line.

## Delimiters Must Pair

Delimiters may nest and mix, but the most recently opened delimiter must close
first with the matching kind. A mismatched or unclosed delimiter is a lexical
`AU1001` diagnostic. The diagnostic points at the wrong closer or end of file
and relates it back to the opening delimiter.

Token locations still use their physical line and column. Joining the lines
does not change types, ownership, borrow duration, or evaluation order.

## What Does Not Continue

Aura does not use a trailing backslash:

```text
value = left + \        # invalid Aura
    right
```

A comma or operator at the end of a line is not enough by itself. Keep a
delimiter open. A multi-element list still rejects a trailing comma, so write:

```aura check-pass
values = [
    10,
    20
]
```

not a comma after `20`.

Ordinary strings and f-strings are still single-line. Break a larger
calculation across delimiters outside the string; do not put a physical newline
inside `f"..."`.

Run the maintained example:

```bash
cargo run -p aura -- run examples/basics/multiline_expressions.au
```

It prints `80` and `20`.

## Next

Delimiter continuation changes source layout only. Values have the same types,
ownership, evaluation order, and backend behavior as the equivalent one-line
program. Continue with [Tuples](25-tuples.md).
