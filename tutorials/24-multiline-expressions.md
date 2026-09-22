# Multiline Expressions

You can split one logical expression across several physical lines. The rule
is short: a line continues while a `(`, `[`, or `{` is still open.

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

The closing `)` goes back to the indentation of the surrounding block. The
newline after that line ends the logical statement.

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

Continuation indentation is for the reader. It does not open an Aura suite,
and the compiler also accepts other amounts of leading space. Use one extra
four-space level so the structure is easy to see.

## Comments And Blank Lines

A trailing comment can end a continued line:

```aura check-pass
values = [
    10, # first input

    20
]
```

Neither the blank line nor the comment closes the `[`, so the list continues.

## Match Expressions Keep Their Layout

A `match` still needs an indented `case` block, even inside a call:

```aura fragment
print(
    match status:
        case Ready: "ready"
        case _: "waiting"
)
```

The `case` block keeps its own layout inside the continued call. Every arm
uses the normal match layout. The enclosing delimiter can close right after
the last inline arm or on its own line below it.

## Delimiters Must Pair

Delimiters can nest and mix. The most recently opened delimiter must close
first, with the matching kind.

A mismatched or unclosed delimiter is a lexical error, `AU1001`. The
diagnostic points at the wrong closer or at the end of the file, and links it
back to the opening delimiter. To fix it, close each delimiter in reverse
order of opening.

Token locations still use their physical line and column. Joining the lines
does not change types, ownership, borrow duration, or evaluation order.

## What Does Not Continue

Aura has no trailing-backslash continuation:

```text
value = left + \        # invalid Aura
    right
```

A comma or operator at the end of a line does not continue it on its own.
Keep a delimiter open instead.

A list with more than one element still rejects a trailing comma. Write this:

```aura check-pass
values = [
    10,
    20
]
```

Do not put a comma after `20`.

Ordinary strings and f-strings stay on one line. To split a long calculation,
break it across delimiters outside the string. Do not put a physical newline
inside `f"..."`.

Run the maintained example:

```bash
cargo run -p aura -- run examples/basics/multiline_expressions.au
```

It prints `80` and `20`.

## Next

Delimiter continuation changes only the source layout. Values have the same
types, ownership, evaluation order, and backend behavior as in the equivalent
one-line program. Continue with [Tuples](25-tuples.md).
