# Comments and docstrings

Code is read far more often than it is written, and some of what a reader
needs to know can't be expressed in the code itself: why a constant has the
value it has, what a function promises, which input a strange-looking branch
is there to handle. Aura gives you two tools for this, and both work the way
they do in Python. Comments are notes for people reading the source.
Docstrings are documentation attached to a declaration, which your editor and
`aura doc` can show to people who never open the file.

## Comments

A comment starts with `#` and runs to the end of the line. The compiler
ignores it completely. A comment can sit on a line of its own or at the end of
a line of code:

```aura
# Print a few temperatures in both scales.
def main():
    readings = [0.0, 21.5, 100.0]  # Celsius
    for celsius in readings:
        # The f-string below contains a # that is not a comment.
        print(f"#{celsius} C")
```

```console
$ aura run src/main.au
#0.0 C
#21.5 C
#100.0 C
```

A `#` inside a string is part of the string, as the output shows. Aura has no
block-comment syntax. For a longer note, start each line with `#`, which is
what `aura fmt` and most editors' "toggle comment" commands produce anyway.

Comments are also allowed inside an expression that spans several lines,
such as a long list, which is handy for labeling individual entries:

```aura
def main():
    thresholds = [
        0.0,    # water freezes
        37.0,   # body temperature
        100.0,  # water boils
    ]
    print(thresholds)
```

A good comment says something the code can't. `# add one to count` above
`count += 1` repeats the code; `# the header line is not a record` above the
same line explains it. The most useful comments tend to answer "why?", because
the code already answers "what?".

## Docstrings

A *docstring* is a triple-quoted string placed as the first statement of a
module, function, class, or other declaration. Let's move the temperature
functions into a module of their own, `src/temperature.au`, and document
them:

*Filename: src/temperature.au*

```aura
"""Convert temperatures between Celsius and Fahrenheit."""

# Water freezes at 32 degrees on the Fahrenheit scale.
FREEZING_F = 32.0

public def to_fahrenheit(celsius: float64) -> float64:
    """Convert a Celsius reading to Fahrenheit.

    Args:
        celsius: The temperature in degrees Celsius.
    """
    return celsius * 9.0 / 5.0 + FREEZING_F

public def to_celsius(fahrenheit: float64) -> float64:
    """Convert a Fahrenheit reading to Celsius."""
    return (fahrenheit - FREEZING_F) * 5.0 / 9.0

def round_trip(celsius: float64) -> float64:
    """Not public, so `aura doc` leaves it out."""
    return to_celsius(to_fahrenheit(celsius))
```

<span class="caption">Listing 3-5: A documented module</span>

The string at the very top of the file documents the module as a whole. The
first string inside each function body documents that function. The
convention, as in Python, is a one-line summary, followed for anything
non-obvious by a blank line and more detail. To describe the parameters, list
them under an `Args:` heading, one `name: description` per line, indented
under the heading. Docstrings can use Markdown, so backticks around code work
as you would expect.

A docstring has no effect on what the program does. The compiler doesn't turn
it into a value, it doesn't allocate anything, and it doesn't change the
compiled code; it is stored as metadata about the declaration. So you can
write as much documentation as a function deserves without worrying about the
cost.

### Where docstrings show up

The first place you will see your docstrings is in your editor. With the VS
Code extension from [Chapter 1](/book/ch01-03-hello-packages), hovering over
a call to `to_fahrenheit` shows its signature followed by its docstring,
completion lists show the first line of the docstring next to each
suggestion, and while you type the arguments of a call, signature help shows
the `Args:` entry for the parameter you are on. That works for functions you
import from other modules too, which makes a well-documented module much
easier to use than one you have to open and read.

The second place is generated documentation. `aura doc` checks each source
file you give it and writes one Markdown page per module. Without `--out` it
prints the pages to standard output; with `--out` it writes them to a
directory:

```console
$ aura doc --out target/api src
wrote target/api/main.md
wrote target/api/temperature.md
```

Every module gets a page, so `main` has one too, even though it has nothing
public to show. The page for `temperature` starts like this:

```text
# Module `temperature`

Convert temperatures between Celsius and Fahrenheit.

## Function `to_fahrenheit`
```

Below each heading, the page gives the function's signature, its docstring,
and its parameters with their `Args:` descriptions. Only `public`
declarations appear. `round_trip` is missing, because it is private to the
module and nobody outside can call it, so it is not part of the module's
documented interface. `aura doc --json` writes the same information as JSON
for other tools to consume. The [Docstrings](/manual/docstrings) page of the
Manual describes the output in full.

### Where a docstring may go

A docstring must be the first statement of the body it documents, before
anything else, including a module's imports. A triple-quoted string later in a
function body isn't a docstring; it is an ordinary string expression that
does nothing, which Aura allows in a function body as Python does.

Fields of a class can have docstrings too, written on the line directly
after the field:

```aura
class Reading:
    """One temperature sample."""

    celsius: float64
    """The temperature in degrees Celsius."""
```

Inside a class body, a stray string has no other possible use, so Aura treats
one that isn't in a documentation position as a mistake. A blank line between
the field and its docstring, or a string after a method, is enough to trigger
it:

```aura check-fail:AU2042
class Reading:
    celsius: float64

    """This string is not directly after a field."""
```

```console
$ aura run src/main.au
error[AU2042]: misplaced docstring: a string in a class body must be the class docstring or directly follow a field
 --> src/main.au:4:5
  |
4 |     """This string is not directly after a field."""
  |     ^
```

Classes are the subject of [Chapter 5](/book/ch05-00-classes). For now, the
rule to remember is short: a docstring goes first, or, for a field, right
underneath it.

### What to document

Write a docstring for every `public` declaration, because those are the ones
other people will call without reading the body. Say what the function does
and what it returns, mention anything a caller could get wrong (a parameter
that must not be empty, a unit of measurement, a result that may be `None`),
and leave the how to the code and its comments. Private helpers benefit from
a docstring when their name doesn't say everything, but there's no need to
document `def is_empty(self) -> bool`.

With bindings, types, functions, and documentation covered, the last piece is
control flow: making decisions and repeating work.
