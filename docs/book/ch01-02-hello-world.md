# Hello, world

Now that `aura` is installed, it's time for the program every language book
starts with: one that prints `Hello, world!` to the screen. We will write it
twice, once as a script and once with a `main` function, and then compile it
to a native executable so that you can see both ways Aura runs code.

## Making a directory

Aura does not care where your code lives, but it helps to keep the examples
from this book together. Make a `projects` directory in your home directory
and a directory for this program inside it:

```console
$ mkdir -p ~/projects/hello_world
$ cd ~/projects/hello_world
```

If you are on Windows, run these commands in the Ubuntu terminal of WSL, as
described in the [previous section](/book/ch01-01-installation).

## Writing and running a script

Create a file called `hello.au` and put one line in it:

*Filename: hello.au*

```aura
print("Hello, world!")
```

<span class="caption">Listing 1-1: A program that prints `Hello, world!`</span>

Aura source files end in `.au`. If a file name has more than one word, the
convention is to separate the words with underscores, as in
`hello_world.au`.

Save the file and run it with `aura run`:

```console
$ aura run hello.au
Hello, world!
```

That's the whole program. If you see `Hello, world!`, you have written and run
your first Aura program. If you don't, go back to
[Checking the installation](/book/ch01-01-installation#checking-the-installation)
and make sure `aura --version` works.

A file like Listing 1-1, with statements written directly at the top level,
is a *script*. Aura runs it from the top to the bottom and then exits, the
same way Python runs a `.py` file. Scripts are a good fit for quick
experiments and small tools, and you will see many short examples in this
book written as scripts for that reason.

`print` writes its argument to standard output, followed by a newline. The
argument here is a string literal. Aura strings can use double quotes or
single quotes, `"Hello, world!"` or `'Hello, world!'`, and the two mean exactly
the same thing. Unlike Python, Aura does not need a `#!` line or an
`if __name__ == "__main__":` check: what is at the top level is what runs.

## Adding a `main` function

Scripts are convenient, but most programs want an explicit starting point.
Change `hello.au` so that it defines a function called `main`:

*Filename: hello.au*

```aura
def main():
    print("Hello, world!")
```

<span class="caption">Listing 1-2: The same program, with a `main` function</span>

Run it again, and the output is the same:

```console
$ aura run hello.au
Hello, world!
```

When a file defines `main`, Aura calls it to start the program. The first line
declares a function named `main` that takes no parameters: `def`, the name,
an empty parameter list in parentheses, and a colon. The body of the function
is the indented block below it. As in Python, indentation is how Aura knows
which lines belong to the function, and the convention is four spaces per
level.

A file has to choose one style. It can have top-level statements, or it can
have a `main` function, but not both. Let's see what happens if we try to mix
them. Make a new file, `mixed.au`:

```aura check-fail:AU2999
print("starting up")

def main():
    print("Hello, world!")
```

```console
$ aura run mixed.au
error[AU2999]: files cannot mix top-level statements with an explicit `main` function
 --> mixed.au:3:1
  |
3 | def main():
  | ^
  = help: imports, constants, and declarations can sit beside `main`; move the top-level statements into `main`
```

This is our first compiler error, so let's read it carefully. The first line
starts with `error` and a code in square brackets, `AU2999`. Every Aura
diagnostic has a code like this, and the codes are stable, so you can search
for one in the [Manual](/manual/diagnostics) or in your editor's help. After
the code comes the message: the file mixes top-level statements with a
`main` function. The second line, starting with `-->`, gives the file, line,
and column where the compiler noticed the problem, here line 3, column 1. The
lines after that show the source with a caret (`^`) under the place it is
pointing at.

The rule exists because "where does this program start?" should have exactly
one answer. If a file had both, you would have to know whether the top-level
`print` runs before `main`, after it, or not at all. Aura avoids the question.
The error also tells you something else: the compiler checked the whole file
before it ran any of it. Nothing was printed, not even `starting up`. A
program with an error like this never starts. Delete `mixed.au` before moving
on.

As the help line says, other `def` functions and constants such as
`LIMIT = 10` are fine next to `main`, and we will write plenty of files like
that. It is statements that *do* something at the top
level, like this `print` call, that clash with it.

## Exit codes

`main` can also report how the program went. Every process returns an exit
code to whatever started it, and by convention `0` means success and anything
else means some kind of failure. Shell scripts, CI systems, and process
supervisors all look at that number. In Aura, `main` returns it:

*Filename: status.au*

```aura
def main() -> int32:
    print("something went wrong")
    return 2
```

<span class="caption">Listing 1-3: A `main` function that returns an exit code</span>

The `-> int32` after the parameter list is the function's return type. It
says `main` returns a 32-bit signed integer, which is what operating systems
use for exit codes. `return 2` hands back the value. Run the program and then
ask the shell for the exit code of the last command with `echo $?`:

```console
$ aura run status.au
something went wrong
$ echo $?
2
```

`main` must take no parameters, and it must return either `int32` or nothing
at all. A `main` with no `->` returns `None`, Aura's value for "nothing", and
the program exits with `0` when it finishes normally. Trying to return some
other type, such as `int64`, is an error, because the operating system has no
way to receive a 64-bit exit code.

## Checking without running

You will often want to know whether a program is correct without running it.
`aura check` does everything `aura run` does up to the point of execution: it
reads the file, checks the syntax, works out the type of every expression,
and checks the ownership rules, but it does not run anything.

```console
$ aura check status.au
ok
```

On a correct program it prints `ok`. On a program with a mistake, it prints
the same diagnostics `aura run` would. Let's leave out a closing parenthesis
to see one:

*Filename: broken.au*

```aura check-fail:AU1001
def main():
    print("Hello, world!"
```

```console
$ aura check broken.au
error[AU1001]: unclosed delimiter `(`; expected `)` before end of file
 --> broken.au:3:1
  = related broken.au:2:10: opening delimiter `(` is here
```

The primary location is line 3, column 1, the end of the file, because that is
where the compiler finally gave up waiting for the `)`. A message that points
at the end of the file is not much help on its own, so the diagnostic adds a
`related` line that points back at the `(` on line 2 that was never closed.
Many Aura diagnostics have related lines like this, and they are often the
part that tells you where to look. Add the `)` back, and `aura check` is happy
again.

`aura check` is fast, and it is the command you will run most while you
write code. Editors run the same check on every keystroke, which we will set
up in the [next section](/book/ch01-03-hello-packages).

## Compiling a native executable

So far, `aura run` has done all the work. It checks the program, translates
it into an internal form called MIR (Aura's mid-level intermediate
representation), and executes that directly. There is no separate compile
step to wait for, which is what you want while you are writing and testing.

When you want to give a program to someone else, or run it without `aura`,
you compile it instead. `aura build` turns the source into a native
executable for your machine:

```console
$ aura build -o hello hello.au
aura: built `hello` with direct backend
$ ./hello
Hello, world!
```

`-o hello` names the output file. The result is an ordinary executable, as if
you had compiled a C program: it does not need `hello.au` any more, it does
not need `aura` to be installed, and you can copy it to another machine with
the same operating system and processor and run it there. The phrase
"direct backend" in the message refers to the part of the compiler that
generates machine code directly through LLVM.

This is the step that needs the C toolchain from the
[installation section](/book/ch01-01-installation#installing-a-c-toolchain-for-native-builds).
Aura generates the machine code itself, but it uses the system linker to
produce the final file. If `aura build` reports a linker problem, check that
`cc --version` (Linux) or `xcode-select -p` (macOS) works.

The exit code of a native binary comes from `main` in the same way:

```console
$ aura build -o status status.au
aura: built `status` with direct backend
$ ./status
something went wrong
$ echo $?
2
```

A native binary also keeps the file, line, and column information it needs to
report runtime errors, so a failure in a built program points at the same
source line that `aura run` would.

## `aura run` or `aura build`?

Both commands run the same checks, and a program that passes them behaves the
same way under either one. The difference is when the work happens. Use
`aura run` while you are developing, because it starts executing straight
away. Use `aura build` when you want the speed of native code, or a single
file you can ship. We will come back to what goes into a native binary, and
how to choose between the two, in
[Chapter 17](/book/ch17-00-shipping).

Writing one file at a time is fine for small programs. Real projects have
several files, tests, and maybe dependencies on other code, and for that Aura
has packages. Let's make one.
