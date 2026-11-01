# To trap or not to trap

You now have two ways for a function to fail. It can trap, which ends the task
with a diagnostic, or it can return `Result.Err`, which hands the problem to
its caller. Neither is better in general. Each one makes a different promise,
and choosing well is mostly a matter of asking one question about the failure:
whose mistake is it?

If the failure means the program itself is wrong, it should trap. If the
failure comes from the world the program lives in, such as a person, a file,
the network, or another service, it should be a `Result`. The rest of this
section works through that rule with the settings loader and shows where the
edges are.

## Mistakes in the world are values

Everything a user can put in a settings file is something our loader has to
expect. A missing `=`, a misspelled number, a forgotten key: none of these mean
our code is wrong, and all of them will happen. That's why Listing 9-5 turns
each one into a `ConfigError`. The caller can print it, log it, or fall back to
defaults, and the program keeps whatever state it had.

The same goes for anything that crosses the program's edge. Aura's own
libraries follow this rule. Reading a file returns `Result[str, io.Error]`,
because files go missing on working systems. Spawning a process, opening a
socket, and parsing JSON all return typed results. Even timeouts and
cancellations in the concurrency APIs come back as values you match on, not as
traps. If the standard library had to guess what your program wanted when a
file was missing, it would guess wrong for half its callers, so it doesn't
guess.

There's a second reason to keep input checks out of assertions. Aura never
strips `assert`, so an assertion would technically catch bad input in every
build. But a trap is a bad way to tell a user they made a typo. It prints a
diagnostic aimed at a programmer, names a line of *our* source, and exits with
status 1 as though the program crashed. A `ConfigError` lets us say "line 2:
expected `key = value`" and exit with a status we chose.

## Mistakes in the program are traps

Now consider the other side. Suppose the loader also checks that `workers` is
between 1 and 64, and reports `ConfigError.OutOfRange` otherwise. Once the
loader has done that, a `Settings` value with zero workers can only exist if
some code built one without going through the loader. That's a bug, and a bug
is exactly what a trap is for:

```aura
enum ConfigError:
    OutOfRange(str, int64)

class Settings:
    port: int64
    workers: int64

def checked_workers(workers: int64) -> Result[int64, ConfigError]:
    if workers < 1 or workers > 64:
        return Result.Err(ConfigError.OutOfRange("workers", workers))
    return Result.Ok(workers)

def share(budget: int64, settings: Settings) -> int64:
    assert settings.workers >= 1, "Settings.workers is checked when settings are loaded"
    return budget // settings.workers

def main():
    print(checked_workers(0))
    match checked_workers(4):
        case Result.Ok(workers):
            settings = Settings(port=9000, workers=workers)
            print(share(1000, settings))
        case Result.Err(error):
            print(error)
```

<span class="caption">Listing 9-6: Validate at the edge with `Result`, then state the invariant with `assert`</span>

```console
$ aura run src/main.au
Result.Err(ConfigError.OutOfRange(workers, 0))
250
```

`checked_workers` is the edge, so it returns a `Result`. `share` sits inside
the program, where the check has already happened, so it asserts. Giving
`share` a `Result[int64, ConfigError]` return type would be a mistake: every
caller would have to handle an error that can't happen, and sooner or later
someone would handle it with a `match` arm that does nothing. The assertion
says something truer. It tells the reader what `share` relies on, and if a
later change breaks that, the trap will name `share` and the message will say
where the guarantee was supposed to come from.

The same reasoning applies to the operations that trap on their own. An index
out of range, a division by zero, an integer overflow: when these happen, the
code made an assumption that wasn't true. Aura traps on them because returning
a `Result` from every `+` and every `[i]` would bury real errors under
impossible ones.

## When you know more than the compiler

Sometimes the types say a call can fail and you know it can't. `parse_int64`
returns a `Result` whatever you pass it, even a constant you wrote yourself.
Aura has no `unwrap` method to say "I'm sure, trap if I'm wrong", so you write
that intent out with a `match` and an assertion in the branch that shouldn't
happen. Our first try is missing something:

```aura check-fail:AU1101
DEFAULT_PORT = "9000"

def parse_constant(text: str) -> int64:
    match parse_int64(text):
        case Result.Ok(value):
            return value
        case Result.Err(message):
            assert false, f"built-in constant `{text}` is not a number: {message}"

def main():
    print(parse_constant(DEFAULT_PORT))
```

```console
$ aura check src/main.au
error[AU1101]: function `parse_constant` is missing a return
 --> src/main.au:3:1
  |
3 | def parse_constant(text: str) -> int64:
  | ^
```

The checker treats `assert` as a statement that can fall through, even when its
condition is the literal `false`, so it sees a path through the `Err` branch
that never returns an `int64`. Adding `return 0` after the assertion satisfies
it. That line never runs, but writing it keeps the rule simple: only `return`
ends a function. With the return in place, changing the constant to `"9OOO"`
shows what the trap looks like:

```console
$ aura run src/main.au
error[AU4001]: built-in constant `9OOO` is not a number: invalid digit found in string
 --> src/main.au:8:13
  |
8 |             assert false, f"built-in constant `{text}` is not a number: {message}"
  |             ^
  = note: Aura call chain (innermost first): parse_constant at 3:1 -> main at 11:1
```

Use this sparingly, and only when the input really is under your control. If
the text could ever come from a file or a user, it belongs in a `Result`.

## Examples, prototypes, and tests

There are three places where trapping freely is fine. In a short example,
careful error handling can hide the idea being shown, so the example might
index a list without checking it. In a prototype, you often don't know yet how
a failure should be handled, and a trap marks the spot clearly. When you come
back to make the program solid, searching for `assert` and for direct indexing
gives you the list of places to decide.

Tests are the third place, and there trapping is the whole mechanism. A failed
`assert` in a test is how the test reports failure, as [Chapter
11](/book/ch11-00-testing) shows. Tests also push in the other direction,
though. Aura's test runner has no way to say "this case should trap", because
nothing in Aura can catch a trap. A function that returns `Result.Err` for bad
input can be tested directly: call it and match on the error. That makes
`Result` the easier design to test, which is one more reason to use it at your
program's edges.

## A few rules of thumb

When you're unsure, these questions usually settle it:

1. Could this failure happen while the program is working correctly, because of
   something outside it? Return a `Result`.
2. Would a caller ever want to react differently to different failures? Give
   the error an enum type, not a `str`.
3. Can the failure happen only if some code in this program is wrong? Trap, and
   use an `assert` with a message that explains the broken assumption.
4. Is a missing value normal and not an error at all? Return `T | None`, or, for collection lookups, use the `Lookup` that `get` gives you.

One honest warning before we move on. Aura makes you handle a `Result` before
you can use the value inside it, but it doesn't currently stop you from calling
a function that returns `Result[None, str]` and ignoring the result entirely:

```aura
def save(path: str) -> Result[None, str]:
    return Result.Err(f"disk full while writing {path}")

def main():
    save("settings.conf")
    print("saved")
```

```console
$ aura run src/main.au
saved
```

The error was dropped without a word. When a function returns a `Result` only
to report failure, make sure every call site matches on it or passes it on with
`try`.

## Summary

Aura gives failures two shapes. A trap stops the current task at the line that
failed, runs cleanup, and reports a diagnostic with a code, the values
involved, and the call chain. Traps can't be caught, which keeps bugs from
hiding. A `Result[T, E]` is an ordinary value that carries either a success or
an error. The compiler makes you look inside it before using the success,
`match` makes you handle both cases, and `try` passes an error up to the
caller, changing its type only through a `From` conversion you wrote. A small
enum like `ConfigError` lets callers act on errors without parsing messages,
and `Lookup` separates "not there" from "there, but `None`".

You've seen generic types several times now without our stopping to explain
them: `Result[T, E]` works for any `T` and `E`, `Lookup[T]` for any `T`, and
the `From` conversion was a trait we declared ourselves. [Chapter
10](/book/ch10-00-generics-traits-conversions) explains how to write generic
code of your own, how traits describe what a type can do, and how Aura converts
between types without ever doing it behind your back.
