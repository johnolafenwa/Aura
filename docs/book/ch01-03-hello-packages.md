# Hello, packages

A single `.au` file is enough for a small program, but most code grows past
one file. It picks up helper modules, tests, and eventually dependencies on
other people's code. Aura's answer to all of that is the *package*: a
directory with a manifest called `Aura.toml` and a fixed layout that the
`aura` commands understand. Let's create one, split our greeting into a
module of its own, write tests for it, and set up an editor to work on it.

## Creating a package with `aura new`

Go back to your `projects` directory and ask `aura` to create a new package
called `greeter`:

```console
$ cd ~/projects
$ aura new greeter
created `greeter`
$ cd greeter
```

`aura new` makes a directory with four files in it:

```text
greeter/
├── .gitignore
├── Aura.toml
├── src/
│   └── main.au
└── tests/
    └── smoke.au
```

It refuses to overwrite a directory that already exists, so you can't run it
by accident over code you care about.

Open `Aura.toml` first:

*Filename: Aura.toml*

```toml
[package]
name = "greeter"
version = "0.1.0"
edition = "2026"
```

<span class="caption">Listing 1-4: The manifest `aura new` generates</span>

This file is in [TOML](https://toml.io), a simple configuration format.
`[package]` starts a section, and the three lines under it describe the
package. All three are required. `name` is how other packages will refer to
this one, so it must look like an Aura identifier: letters, digits, and
underscores, not starting with a digit. That means `word_count` is a valid
name and `word-count` is not. `version` is the package's own version number.
`edition` selects which edition of the language the package is written in.
There is only one edition so far, `"2026"`, but the field exists so that a
future edition can change the language without breaking packages written for
this one. When a package depends on others, they are listed in a
`[dependencies]` section below this one;
[Chapter 7](/book/ch07-00-modules-and-packages) covers that.

The generated `.gitignore` holds one line, `target/`, which is where we will
put build output. Now look at the program:

*Filename: src/main.au*

```aura
def main() -> int32:
    print("Hello from Aura")
    return 0
```

<span class="caption">Listing 1-5: The `src/main.au` that `aura new` generates</span>

It is the same shape as the `main` we wrote by hand in the last section. All
of a package's source code lives under `src/`, and `src/main.au` is the
conventional entry point. Run it by naming the file, the same as before:

```console
$ aura run src/main.au
Hello from Aura
```

The difference from a loose file is invisible here, but it matters as soon as
there is more than one file. Because `src/main.au` is inside a package,
`aura` finds the `Aura.toml` above it, treats `src/` as the root for imports,
and can resolve modules and dependencies by name.

## Splitting code into a module

Let's give the greeting a home of its own. Every file under `src/` is a
module, named after its path without the `.au`. Create `src/greeting.au`:

*Filename: src/greeting.au*

```aura
def greet(name: str) -> str:
    return f"Hello, {name}!"
```

This defines a function, `greet`, that takes one parameter called `name` of
type `str` (a string) and returns a `str`. The body uses an *f-string*, which
works as it does in Python: the `f` before the opening quote means that
anything inside `{` and `}` is evaluated and its value is placed into the
string. We will look at functions properly in
[Chapter 3](/book/ch03-03-functions).

Now replace `src/main.au` so that it uses the new function:

*Filename: src/main.au*

```aura fragment
from greeting import greet

def main() -> int32:
    print(greet("world"))
    return 0
```

<span class="caption">Listing 1-6: Importing `greet` from the `greeting` module</span>

`from greeting import greet` reads the same as it would in Python: from the
module `greeting`, bring in the name `greet`. Let's check the program:

```console
$ aura check src/main.au
error[AU2999]: item `greet` is private in module `greeting`
 --> src/main.au:1:22
  |
1 | from greeting import greet
  |                      ^
```

The compiler found the module and found `greet` in it, and then refused to
let us use it. In Aura, every top-level name is private to its module unless
you say otherwise. Python has a convention that names starting with an
underscore are private, but nothing enforces it. Aura makes the default the
safe one, so that the only names other modules can depend on are the ones you
chose to offer. That matters more as a program grows, because anything that
is public is something you have promised not to break.

To offer `greet`, mark it `public`:

*Filename: src/greeting.au*

```aura
public def greet(name: str) -> str:
    return f"Hello, {name}!"
```

<span class="caption">Listing 1-7: Making `greet` public</span>

Now the check passes and the program runs:

```console
$ aura check src/main.au
ok
$ aura run src/main.au
Hello, world!
```

## Testing with `aura test`

`aura new` also made a `tests/` directory with one file in it, `smoke.au`,
which contains a `main` that returns `0`. It is there so that `aura test` has
something to run from the very first minute:

```console
$ aura test
ok tests/smoke.au
1 passed; 0 failed
```

With no arguments, `aura test` looks for `.au` files under `tests/` and runs
the tests it finds in each one. A file like `smoke.au` with no test functions
counts as a single test, which passes if its `main` returns `0`. More often,
a test file contains functions whose names start with `test_`, and each of
those is its own test. Let's write some for `greet`. Create
`tests/greeting_test.au`:

*Filename: tests/greeting_test.au*

```aura fragment
from greeting import greet

def test_greets_by_name():
    assert greet("Ada") == "Hello, Ada!"

def test_keeps_full_name():
    assert greet("Grace Hopper") == "Hello, Grace Hopper"
```

<span class="caption">Listing 1-8: Two tests for `greet`, one of them wrong</span>

Test files in a package can import the package's modules in the same way
`src/main.au` does. Each test calls `greet` and uses `assert` to state what
the result should be. `assert` takes a condition, and if the condition is
false, the test fails at that line. We have made a deliberate mistake in the
second test: the expected string is missing its `!`. Run the tests:

```console
$ aura test
ok tests/greeting_test.au::test_greets_by_name
FAILED tests/greeting_test.au::test_keeps_full_name
error[AU4001]: assertion failed
 --> tests/greeting_test.au:7:5
  |
7 |     assert greet("Grace Hopper") == "Hello, Grace Hopper"
  |     ^
  = note: left = Hello, Grace Hopper!
  = note: right = Hello, Grace Hopper
  = note: Aura call chain (innermost first): test_keeps_full_name at 6:1
ok tests/smoke.au
2 passed; 1 failed
```

Each test is reported by its file and function name. The failing one shows
the same kind of diagnostic as a compiler error, but this one happened while
the program ran: the assertion on line 7 was false. Because the condition
compared two values with `==`, `aura` shows both sides, the `left` value that
`greet` returned and the `right` value we expected, and the difference jumps
out. The last note is the *call chain*, the list of functions that were
running when the failure happened, innermost first. Here there is only one,
the test itself.

Fix the expected string by adding the `!`, and run the tests again:

```console
$ aura test
ok tests/greeting_test.au::test_greets_by_name
ok tests/greeting_test.au::test_keeps_full_name
ok tests/smoke.au
3 passed; 0 failed
```

When you only want to run some of the tests, `-k` picks the ones whose names
contain a piece of text:

```console
$ aura test -k full
ok tests/greeting_test.au::test_keeps_full_name
1 passed; 0 failed
```

`aura test` exits with a non-zero status when any test fails, so a CI job can
run it directly. [Chapter 11](/book/ch11-00-testing) covers testing in depth,
including setup and teardown functions and parameterized tests.

## Formatting with `aura fmt`

Aura comes with a formatter, so that code written by different people looks
the same and nobody has to argue about it in review. `aura fmt --check`
reports the files it would change without touching them:

```console
$ aura fmt --check .
would format `./src/main.au`
would format `./tests/greeting_test.au`
```

and `aura fmt` with no arguments rewrites every `.au` file under the current
directory in place:

```console
$ aura fmt
$ cat src/main.au
from greeting import greet


def main() -> int32:
    print(greet("world"))
    return 0
```

The formatter wants two blank lines between top-level declarations, the same
rule as Python's PEP 8. The listings in this book use one blank line to save
space, so `aura fmt` will always find something to change in code copied from
the book. That's fine; the two layouts mean the same program.

## Building the package

`aura build` works on a package's entry point in the same way it works on a
loose file. The generated `.gitignore` ignores `target/`, which makes it a
good place for build output:

```console
$ aura build -o target/greeter src/main.au
aura: built `target/greeter` with direct backend
$ ./target/greeter
Hello, world!
```

`aura build` creates the `target` directory if it does not exist.

## Setting up an editor

Now that you have a project, open it in an editor. The VS Code extension is
called **Aura Programming Language**, from the publisher **JohnOlafenwa**.
Install it from the Extensions view, or from a terminal:

```console
$ code --install-extension JohnOlafenwa.vscode-aura-lang
```

Editors that use the Open VSX registry, such as VSCodium, have the same
extension; with VSCodium, use `codium --install-extension` with the same name.
On Windows, open the project from the Ubuntu terminal with `code .` and
install the extension into the **WSL: Ubuntu** window, so that the extension
runs on the same side as `aura`.

The extension has very little logic of its own. It starts `aura lsp`, a
long-running mode of the same `aura` tool, and asks it about the files you
are editing. That means diagnostics, completions, hover information, and
go-to-definition all come from the real compiler, and your editor can never
disagree with `aura check`. The extension finds `aura` on your `PATH`; if
VS Code was started from somewhere that cannot see it, set
`AURA_LSP_AURA_PATH` to the full path of the binary before launching the
editor.

To check that everything is connected, open `src/greeting.au`. The language
mode in the bottom-right corner of the window should read **Aura**. Hover over
`greet` and you should see its signature. Then delete the closing `"` of the
f-string: within a moment, the editor should underline the line with the same
kind of `AU` diagnostic you have seen on the command line. Put the quote back
before you move on. The [VS Code guide](/install/vscode) has troubleshooting
steps if any of this does not work. VS Code's Format Document command runs
the same formatter as `aura fmt`.

## Summary

You have installed `aura`, and you have run a program as a script, with a
`main` function, and as a native binary. You have read compiler errors and
a test failure, created a package with `aura new`, split it into modules, and
tested it with `aura test`. These are the commands you will use throughout
the book:

- `aura run file.au` checks a program and runs it.
- `aura check file.au` checks it without running it.
- `aura build -o output file.au` compiles a native executable.
- `aura new name` creates a package.
- `aura test` runs the tests under `tests/`.
- `aura fmt` formats your code.

It's time to write something more substantial. In the next chapter we build a
program that reads text from standard input and counts the words in it, and
along the way we meet most of the ideas the rest of the book explains.
