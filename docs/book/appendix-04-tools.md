# Appendix D: Useful tools

The `aura` command does more than compile and run programs. It formats code,
runs tests, generates API documentation, and powers the editor support in VS
Code. This appendix is a tour of those tools, in roughly the order you will
want them. The [CLI and tooling](/manual/cli-and-tooling) page of the Manual
lists every command and flag.

## The everyday commands

You have used these throughout the book, so this is only a reminder of how
they divide the work:

- `aura check file.au` parses and type-checks without running anything. It is
  the fastest way to find out whether a change compiles, and it prints `ok`
  when it does.
- `aura run file.au` checks and then runs the program. Arguments after `--`
  reach the program through `sys.args()`.
- `aura build -o path file.au` produces a standalone native executable.
  Add `--debug` for a build a debugger can step through.
- `aura new name` creates a project directory with `Aura.toml`,
  `src/main.au`, `tests/smoke.au`, and a `.gitignore`.
- `aura version` prints the build channel and source commit, which is the
  first thing to include in a bug report:

```console
$ aura version
aura 0.11.0-dev (9e279b31f356)
```

`aura check` and `aura run` also accept `--format json`, which prints
diagnostics as a JSON document instead of the human-readable form. That is
what you want when another program is reading the output, such as a CI script
that turns diagnostics into annotations:

```console
$ aura check --format json src/err.au
{"diagnostics":[{"call_frames":[],"code":"AU2001","edits":[],"help":[],"message":"unknown name `missing`","notes":[],"primary_span":{"end":{"column":12,"line":2},"path":"src/err.au","start":{"column":11,"line":2}},"secondary_spans":[],"severity":"error","task_ancestry":[]}],"schema_version":1}
```

The fields are the same ones the human form shows: the stable code, the
message, the primary span, related spans, notes, help, and machine-applicable
edits.

## Formatting with `aura fmt`

Aura has one formatting style, and `aura fmt` applies it. There are no
options to configure, on purpose: if every Aura project is formatted the same
way, nobody has to argue about it in code review, and code you read in
someone else's project looks like code you wrote.

Let's see it at work. Here is a file written in a hurry:

*Filename: src/main.au*

```aura
import sys
def label(count:int64,unit:str="job")->str:
  if count==1:
    return f"{count} {unit}"
  return f"{count} {unit}s"
def main()->int32:
  print(label(len(sys.args()),
      unit = "argument"))
  return 0
```

It compiles, since Aura only cares that indentation is consistent, but it is
hard on the eyes. `aura fmt --check` reports which files would change, writes
nothing, and exits with status 1 if any would:

```console
$ aura fmt --check .
would format `./src/main.au`
$ echo $?
1
```

That makes `aura fmt --check .` a good step for CI. Running `aura fmt` with a
path rewrites the files in place:

```console
$ aura fmt src
$ cat src/main.au
import sys


def label(count: int64, unit: str = "job") -> str:
    if count == 1:
        return f"{count} {unit}"
    return f"{count} {unit}s"


def main() -> int32:
    print(label(len(sys.args()), unit="argument"))
    return 0
```

The formatter set four-space indentation, put single spaces around operators
and after colons and commas, removed the spaces around `=` in the keyword
argument (but kept them around the parameter default `= "job"`, where a type
annotation comes first), and put two blank lines between top-level
declarations. It also joined the call that was split over two lines, because
the whole statement fits in 88 columns. A statement that does not fit is
broken at its outermost bracket, one element per line, with a trailing comma.
You can see that style in the longer `Report(...)` constructor calls in
Chapter 19, which are formatter output.

If you want a list to stay one element per line even when it would fit,
write it with a trailing comma, and the formatter will respect that. Comments
stay on the lines where you wrote them, and string literals, including
docstrings, are never changed.

The formatter only touches code that parses. If a file has a syntax error,
`aura fmt` reports the usual diagnostic and leaves the file alone:

```console
$ aura fmt src/bad.au
error[AU1001]: unclosed delimiter `(`; expected `)` before end of file
 --> src/bad.au:2:1
  = related src/bad.au:1:9: opening delimiter `(` is here
```

After formatting, it parses its own output and checks that the tokens are
unchanged, so a formatter bug cannot silently change what your program means.
`aura fmt --stdin` reads source from standard input and writes the formatted
result to standard output, for editors and scripts that want to format a
buffer without a file.

## Testing with `aura test`

`aura test` finds the test programs under `tests/` and runs each one. A test
passes when it compiles, runs within its timeout (30 seconds by default), and
returns 0 from `main`. The project `aura new` creates has one placeholder
test:

```console
$ aura test
ok tests/smoke.au
1 passed; 0 failed
```

A failing assertion shows the values on each side, which saves a round of
adding `print` calls:

```console
$ aura test
FAILED tests/fail.au
error[AU4001]: math is broken
 --> tests/fail.au:2:5
  |
2 |     assert 1 + 1 == 3, "math is broken"
  |     ^
  = note: left = 2
  = note: right = 3
  = note: Aura call chain (innermost first): main at 1:1
ok tests/smoke.au
1 passed; 1 failed
```

`-k substring` runs only the tests whose names match, and `--timeout-ms`
changes the per-test limit. Chapter 11 covers testing properly, including
test functions, setup and teardown, and parameterized cases.

## API documentation with `aura doc`

A triple-quoted string as the first statement of a module, class, or function
is a docstring. Parameters are documented under an `Args:` heading:

*Filename: src/units.au*

```aura
"""Helpers for printing counts."""


public def label(count: int64, unit: str) -> str:
    """Return `count` followed by `unit`, pluralised when needed.

    Args:
        count: How many things there are.
        unit: The singular name of one thing.
    """
    if count == 1:
        return f"{count} {unit}"
    return f"{count} {unit}s"
```

`aura doc src/units.au` prints a Markdown page for the module: a
`# Module units` heading, the module docstring, and a section for each public
function with its signature, its docstring, and a parameter list built from
the `Args:` entries, with each parameter's type filled in from the signature.
With `--out`, it writes one file per module instead:

```console
$ aura doc --out api src/units.au
wrote api/units.md
```

`--json` produces the same information as JSON, for tools that render their
own documentation sites. The same docstrings also show up in the editor, on
hover and in completions.

## The VS Code extension

The **Aura Programming Language** extension for VS Code gives you syntax
highlighting, diagnostics as you type, completion, hover with types and
docstrings, signature help, go-to-definition (including into other modules),
find references, rename, document symbols, formatting, and debugging. It is
published by **JohnOlafenwa** on both the Visual Studio Marketplace and Open
VSX, so it works in VS Code and in editors such as VSCodium. Install it from
the Extensions view, or from a terminal:

```bash
code --install-extension JohnOlafenwa.vscode-aura-lang
```

The extension does not contain its own copy of the language. All of its
understanding of your code comes from the `aura` compiler you installed,
which it starts in the background with `aura lsp`. That command runs a
persistent compiler service that the extension's language server talks to;
it is not something you run by hand. Because of this design, the editor and
the command line can never disagree: a red squiggle in VS Code is exactly the
diagnostic `aura check` would print, with the same code.

That also means the extension needs to find `aura`. It looks on your `PATH`.
If you keep several builds of the compiler around, or `aura` is not on the
`PATH` that VS Code sees, start VS Code with `AURA_LSP_AURA_PATH` set to the
binary you want:

```bash
AURA_LSP_AURA_PATH="$HOME/tools/aura/bin/aura" code .
```

On Windows with WSL, install both the `aura` CLI and the extension inside
WSL. Open the project from the WSL terminal with `code .`, and install the
extension with **Install in WSL** in that remote window. An extension running
on the Windows side cannot reliably reach a compiler inside Linux.

To check that everything is connected, open any `.au` file and look at the
language mode in the lower-right corner: it should say **Aura**. Then type
something invalid and wait for an `AU` diagnostic to appear, and hover over a
binding to see its type. If you get colours but no diagnostics or hover, the
extension cannot start the compiler. Run `aura --version` in VS Code's
integrated terminal to see what the editor's environment finds, and open
**View → Output**, then **Aura Language Server**, to read the startup log.

Formatting in the editor uses the same rules as `aura fmt`. Run **Format
Document**, or set `"editor.formatOnSave": true` to format every time you
save. A buffer with a syntax error is left as it is.

For debugging, the extension adds a **Debug** link above `def main` and an
**Aura: Debug File** command. Both build the file with `aura build --debug`
and launch it under LLVM's `lldb-dap`, so breakpoints, stepping, the call
stack, and local variables work as in any other VS Code debugger. You need
`lldb-dap` installed; the extension looks for it in the `aura.lldbDapPath`
setting, then through `xcrun` on macOS, then on your `PATH`. Chapter 17 walks
through a debugging session.
