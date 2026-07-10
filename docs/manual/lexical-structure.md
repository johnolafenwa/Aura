# Lexical Structure

Aurora source files use the `.au` extension. The compiler accepts UTF-8 text and ignores a UTF-8 BOM at the beginning of the file.

## Comments

Line comments begin with `#` and continue to the end of the line.

```python
# This is a comment.
print("ready") # trailing comment
```

## Indentation

Blocks are indentation-based:

```python
if ready:
    print("yes")
else:
    print("no")
```

Use spaces. Tabs in indentation are rejected. Blank lines and comment-only lines do not affect indentation.

## Identifiers

Identifiers name variables, functions, classes, enums, traits, modules, and fields. Built-in type and function names are reserved and cannot be redefined.

## Keywords

Current language keywords include:

| Keyword | Use |
| --- | --- |
| `public` | Export a top-level declaration from a module. |
| `class`, `copy class` | Declare record-like types. |
| `enum` | Declare tagged alternatives. |
| `trait`, `impl`, `for` | Declare traits and implementations. |
| `def` | Declare functions and methods. |
| `return` | Return from a function. |
| `if`, `elif`, `else` | Conditional control flow. |
| `while`, `for`, `in` | Loops and iteration. |
| `match`, `case` | Pattern matching. |
| `with` | Scoped resource cleanup. |
| `break`, `continue`, `pass` | Block and loop control. |
| `borrow`, `mut`, `self` | Ownership and receiver forms. |
| `try` | Propagate `Result.Err`. |
| `as` | Explicit numeric casts. |
| `and`, `or`, `not` | Boolean operators. |
| `true`, `false`, `None` | Builtin values. |
| `indirect` | Recursive field marker. |

## Literals

| Literal | Examples | Notes |
| --- | --- | --- |
| Integer | `0`, `42`, `-7` | Can adopt any supported integer type when expected. |
| Float | `1.0`, `3.5` | Defaults to `float64`; may adopt `float32`. |
| String | `"hello"` | Supports escape sequences and f-strings. |
| Boolean | `true`, `false` | Type `bool`. |
| None | `None` | Unit value and bare option pattern when expected. |
| Duration | `10ms`, `2s`, `1m` | Type `Duration`. |

## String Interpolation

F-strings evaluate expressions inside braces:

```python
name = "aurora"
print(f"hello {name}")
```

Interpolations can include indexed expressions and nested strings.
