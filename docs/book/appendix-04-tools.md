# Appendix D: Useful tools

The `aura` command does more than compile and run programs. It formats code,
runs tests, proves that a program's checks cannot fail, generates API
documentation, and powers the editor support in VS Code. This appendix is a
tour of those tools, in roughly the order you will want them. The
[CLI and tooling](/manual/cli-and-tooling) page of the Manual lists every
command and flag.

## The everyday commands

You have used these throughout the book, so this is only a reminder of how
they divide the work:

- `aura check file.au` parses and type-checks without running anything. It is
  the fastest way to find out whether a change compiles, and it prints `ok`
  when it does.
- `aura run file.au` checks and then runs the program. Arguments after `--`
  reach the program through `sys.args()`.
- `aura verify file.au` proves that the program's contracts, arithmetic,
  and indexes cannot fail, without running it. It has
  [its own section](#proving-with-aura-verify) below.
- `aura build -o path file.au` produces a standalone native executable.
  Add `--debug` for a build a debugger can step through.
- `aura new name` creates a project directory with `Aura.toml`,
  `src/main.au`, `tests/smoke.au`, and a `.gitignore`.
- `aura version` prints the build channel and source commit, which is the
  first thing to include in a bug report:

```console
$ aura version
aura 0.17.0-dev (9e279b31f356)
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

## Proving with `aura verify`

A test runs the inputs you wrote down. `aura verify` reasons about all of
them. It reads the contracts in a file (`requires`, `ensures`, loop and
class `invariant`, `decreases`) and proves that each one holds on every
path, and it proves the checks you never wrote as well: that no integer
operation overflows, no divisor is zero, no list index is out of range, and
no dictionary read misses its key. For each check it cannot prove, it
prints a diagnostic, with the inputs that break the check when the solver
can name them. [Section 11.4](/book/ch11-04-contracts-and-proofs) teaches
the method; this is the tool.

It needs an SMT solver, a separate program that answers logic questions,
and it looks for `z3` and then `cvc5` on your `PATH`. Install Z3 with
`brew install z3`, `apt install z3`, or `pip install z3-solver`. Nothing is
added to the compiler or to the programs you build.

Here is a function with a contract and a classic mistake:

*Filename: src/mid.au*

```aura
def midpoint(low: int64, high: int64) -> int64:
    requires 0 <= low and low <= high
    ensures low <= result and result <= high
    return (low + high) // 2


def main():
    print(midpoint(10, 20))
```

```console
$ aura verify src/mid.au
error[AU5101]: cannot prove integer result in range
 --> src/mid.au:4:13
  |
4 |     return (low + high) // 2
  |             ^
  = note: in `midpoint` (overflow check)
  = note: counterexample: low = 9223372036854775807, high = 9223372036854775807
verified src/mid.au: 4 of 5 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
$ echo $?
1
```

The diagnostic has the same parts as a compiler error, plus two notes: the
function and the kind of check, and a counterexample. The last line is the
summary, and it ends with the solver that was used. The exit status is 0
only when every check was proved, so `aura verify` can gate a CI job the
way `aura test` does. Writing the sum as `low + (high - low) // 2` fixes
the overflow:

```console
$ aura verify src/mid.au
verified src/mid.au: 6 of 6 checks proved in 2 functions, 1 from the proof cache (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

The count went from five to six because the new expression has one more
arithmetic step to prove. `main` did not change between the two runs, so
its result came from the proof cache. `aura verify` stores each function's
results under `~/.cache/aura/verify` (or under `verify` in
`AURA_CACHE_DIR`), and answers from there when the function, the contracts
it relies on, the solver version, and the time limit are all unchanged.
Verifying again after a small edit solves only what the edit touched.

Given one file, the command verifies the functions that file defines and
uses the contracts of what it imports. Given a directory, several files, or
no path at all, it verifies every module of the package and ends with one
line of totals. Its options are:

- `--function name` verifies only the functions whose name contains `name`.
  Use it to work on one function of a large file.
- `--timeout ms` sets the time limit for each check. The default is 10000.
  A check the solver cannot decide in time is reported as `AU5102`.
- `--solver path` names the solver to run. Without it, `aura verify` uses
  the `AURA_SMT_SOLVER` environment variable if it is set, then `z3`, then
  `cvc5`.
- `--no-cache` solves every check again and ignores the proof cache.
- `--deny not-modeled`, `--deny undecided`, or `--deny all` makes a package
  run fail on those outcomes too. A package run otherwise fails only when a
  check failed. A run on one file fails unless every check is proved.
- `--must-verify name` names a function whose checks must all be proved,
  and reports `AU5122` when one is not. Give it once for each function.
- `--format json` prints the report as JSON: one entry for each check with
  its kind, function, line, column, and status (`proved`, `failed`,
  `unknown`, or `unsupported`), followed by the diagnostics in the same form
  as `aura check --format json`.

Six diagnostic codes belong to the verifier. `AU5101` means a check can
fail, or the verifier is missing a fact it needs. `AU5102` means the solver
gave up. `AU5103` means no solver could be started. `AU5104` means the check
depends on something the verifier does not model, and names that step and
its line. `AU5120` means a function's `requires` clauses can never hold
together. `AU5122` means a function named with `--must-verify` has a check
that is not proved.
[Appendix E](/book/appendix-05-common-diagnostics#proofs) shows each one
and what to do about it.

Two things about a proof are easy to get wrong. The first is that it is
about the contracts as written: a function that proves is a function that
keeps its `ensures` whenever its `requires` holds. A `requires` that no
input can meet is reported as `AU5120`, but one that is merely stronger than
any real caller can supply still makes the proof say very little. The
second is that the verifier knows nothing about what most library calls
return, including everything read from a file, a process, or a socket.
Code that checks such a value before using it verifies; code that trusts it
does not. The [Verification](/manual/verification) page of the Manual lists
what is modeled.

`aura verify` changes nothing in the program. Every check it proved is
still made when the program runs. To build a native binary without them,
pass `--remove-proved-checks` to `aura build`, which runs the verifier and
reports how many checks it removed:

```console
$ aura build --remove-proved-checks -o scores src/main.au
aura: 7 checks proved and removed (1 bounds, 4 arithmetic, 2 assertion)
aura: built `scores` with direct backend
```

That run is from [Chapter 17](/book/ch17-01-run-and-build#proving-the-trap-away),
which explains what stays in the binary and what the build trusts when a
check goes.

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

With a native Windows install, the extension runs in the ordinary VS Code
window and finds `aura.exe` on `PATH`. If you installed Aura inside WSL
instead, install both the `aura` CLI and the extension inside WSL. Open the project from the WSL terminal with `code .`, and install the
extension with **Install in WSL** in that remote window. An extension running
on the Windows side cannot reliably reach a compiler inside Linux.

To check that everything is connected, open any `.au` file and look at the
language mode in the lower-right corner: it should say **Aura**. Then type
something invalid and wait for an `AU` diagnostic to appear, and hover over a
binding to see its type. If you get colours but no diagnostics or hover, the
extension cannot start the compiler. Run `aura --version` in VS Code's
integrated terminal to see what the editor's environment finds, and open
**View → Output**, then **Aura Language Server**, to read the startup log.

The extension also runs `aura verify` each time an Aura file is opened or
saved. A check it cannot prove shows up at the check as a diagnostic with
its counterexample, and a line whose checks are all proved ends with a
`✓ proved` hint, until you edit the file again. Set `aura.verifyOnSave` to
`false` to turn this off.

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
