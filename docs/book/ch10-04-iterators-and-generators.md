# Iterators and generators

A `for` loop over a list walks values that already exist. Sometimes the values
don't exist yet. They might come from a file read a line at a time, from a
counter that never stops, or from a calculation you'd rather not run for items
nobody asks for. Python handles this with iterators and generators, and Aura
does too. The difference is that Aura writes the protocol down as a trait, so
the checker knows exactly what a loop over your type will do.

## The `Iterator` trait

The builtin `iter` module defines two things a loop needs. `Step[T]` is the
answer to "what's next?": either `Item(value)` or `End`. `Iterator[T]` is the
trait with one method, `next(mut self) -> Step[T]`. Any type that implements
it can sit after `in`:

```aura
from iter import Iterator, Step


class Countdown:
    n: int64


impl Iterator[int64] for Countdown:
    def next(mut self) -> Step[int64]:
        if self.n == 0:
            return Step.End
        self.n -= 1
        return Step.Item(self.n + 1)


def main():
    for x in Countdown(n=3):
        print(x)
```

```console
$ aura run countdown.au
3
2
1
```

`next` takes `mut self` because it changes the iterator: each call moves the
countdown one step. The loop calls `next` until it gets `End`, binds each
`Item` value to `x`, and runs the body. It also consumes the `Countdown`. Once
the loop has used the iterator up there's nothing sensible left to do with it,
so Aura doesn't let you try.

Sometimes you do want the iterator afterwards, for example to read a header
and then keep going with the rest. Write `for x in mut source:` and the loop
borrows the iterator instead of consuming it:

```aura
from iter import Iterator, Step


class Countdown:
    n: int64


impl Iterator[int64] for Countdown:
    def next(mut self) -> Step[int64]:
        if self.n == 0:
            return Step.End
        self.n -= 1
        return Step.Item(self.n + 1)


def main():
    mut countdown = Countdown(n=5)
    for first in mut countdown:
        print(f"first: {first}")
        break
    for rest in countdown:
        print(rest)
```

```console
$ aura run header.au
first: 5
4
3
2
1
```

The first loop stops after one item, and the second loop picks up where the
first left off, because both loops advance the same `countdown`.

## Generators

Writing a class and an `impl` for every sequence gets tedious, and the state
gets awkward fast. The countdown only needed one number, but a sequence that
walks a tree or pairs up lines from two files has to remember where it was
between calls, so you end up writing the loop inside out.

A *generator function* lets you write the loop the right way round. Declare
the result as `Generator[T]` and use `yield` to hand each value back:

```aura
from iter import Generator


def evens(limit: int64) -> Generator[int64]:
    mut n = 0
    while n < limit:
        yield n
        n += 2


def main():
    for x in evens(7):
        print(x)
```

```console
$ aura run evens.au
0
2
4
6
```

Calling `evens(7)` doesn't run any of its body. It returns a generator, and
each time the loop asks for the next item, the body runs until it reaches the
next `yield`, hands over `n`, and pauses there with its local variables
intact. When the body finishes, the loop gets `End`. `Generator[T]` implements
`Iterator[T]`, so everything in the previous section works on it, including
`for x in mut gen:`.

Because nothing runs until it's asked for, a generator can describe a sequence
that never ends. Let's number some words by zipping an endless counter with a
list:

```aura check-fail:AU2002
from iter import Generator


def naturals() -> Generator[int64]:
    mut n = 0
    while true:
        yield n
        n += 1


def main():
    for index, word in zip(naturals(), ["ant", "bee", "cat"]):
        print(f"{index}: {word}")
```

```console
$ aura run zip.au
error[AU2002]: `zip` cannot pair a collection with an iterator
```

That doesn't compile, and the reason is worth a moment. `zip` of two lists pairs elements by position, and `zip` of two iterators
pulls one item from each, so mixing the two would quietly mean one or the
other. Aura makes you choose. Here we want the lazy kind, so we give `zip` two
iterators. A list becomes an iterator through a small generator, and
`enumerate` would also do the job:

```aura
from iter import Generator


def naturals() -> Generator[int64]:
    mut n = 0
    while true:
        yield n
        n += 1


def each(words: own list[str]) -> Generator[str]:
    for word in words:
        yield word.clone()


def main():
    for index, word in zip(naturals(), each(["ant", "bee", "cat"])):
        print(f"{index}: {word}")
```

```console
$ aura run zip.au
0: ant
1: bee
2: cat
```

`zip` stops at the shorter source, so the endless `naturals()` is asked for
exactly three numbers.

There's one rule about generator parameters that surprises people coming from
Python. The body runs after the call has returned, so it can't hold a borrow
of the caller's values. A parameter that isn't a copy type has to be `own`, as
`words` is in `each` above. Forget it and the checker tells you:

```aura check-fail:AU3002
from iter import Generator


def each(words: list[str]) -> Generator[str]:
    for word in words:
        yield word.clone()


def main():
    for word in each(["ant"]):
        print(word)
```

## Comprehensions, `collect`, and cleanup

A comprehension can take an iterator too. It builds the whole result at once,
so it collects the iterator into a list first, and `Iterator[T]` gives you that
step directly as `collect()`:

```aura
from iter import Generator


def evens(limit: int64) -> Generator[int64]:
    mut n = 0
    while n < limit:
        yield n
        n += 2


def main():
    print([x * x for x in evens(7)])
    print(evens(5).collect())
```

```console
$ aura run collect.au
[0, 4, 16, 36]
[0, 2, 4]
```

A generator that is paused at a `yield` might be holding a file open inside a
`with` block. If the loop stops early, the generator is closed: its paused
`yield` returns as a bare `return` would, so its `with` exits run once, in the
usual order. You can also call `close()` yourself when the cleanup should
happen at a particular point.

## Making your own types iterable

`Countdown` was an iterator itself. More often you have a container, like a
playlist, and you want `for song in playlist:` to work without the playlist
having to keep track of a position. The `IntoIterator[T]` trait covers that
case. Its implementation says which iterator the value turns into, through an
*associated type* called `Iter`, and `into_iter` builds one:

```aura
from iter import Generator, IntoIterator


class Playlist:
    songs: list[str]


def play(songs: own list[str]) -> Generator[str]:
    for song in songs:
        yield song.clone()


impl IntoIterator[str] for Playlist:
    type Iter = Generator[str]

    def into_iter(own self) -> Self.Iter:
        return play(self.songs)


def main():
    playlist = Playlist(songs=["Intro", "Theme", "Outro"])
    for song in playlist:
        print(song)
```

```console
$ aura run playlist.au
Intro
Theme
Outro
```

The `type Iter = Generator[str]` line fixes the associated type, and inside the
implementation `Self.Iter` means that type. The trait requires `Iter` to be an
`Iterator[str]`, so the checker rejects an implementation whose `Iter` isn't
one. A `for` loop over a `Playlist` calls `into_iter()` for you, and so do
comprehensions, `enumerate`, and `zip`.

A generic function can accept anything a loop can consume by bounding its
parameter with `IntoIterator`:

```aura
from iter import IntoIterator


def longest[S: IntoIterator[str]](words: own S) -> str:
    mut best = ""
    for word in words:
        if len(word) > len(best):
            best = word.clone()
    return best
```

`longest(playlist)` works for a `Playlist`, and for any other type with an
`IntoIterator[str]` implementation. Your own traits can declare associated
types the same way; the [Manual](/manual/generics-and-traits#associated-types)
has the details.

## Summary

Generics let you write a function, class, or enum once and use it with many
types. Type parameters are checked once, for every possible type, so the
ownership modes and operations in a generic body have to make sense for all of
them: `own T` for values the body keeps, and a trait bound for any method or
operator it calls. Traits name behavior, implementations promise it explicitly,
and bounds put the requirement in the signature where callers can read it. A
few traits connect to the language itself: `Display` controls printing, `Add`,
`Ord`, and friends give your types operators, `From` lets `try` convert
errors, and `Iterator` and `IntoIterator` decide what a `for` loop does with
your types.

Conversions follow the same philosophy of saying what you mean. `as` converts
numbers exactly or traps, `.to_float()` rounds on purpose, parsing text returns
a `Result`, and `str()` and f-strings render anything. Nothing converts behind
your back.

Generators let you write a lazy sequence as an ordinary loop with `yield`.
Nothing runs until a loop asks for the next item, which makes endless sequences
safe to zip with finite ones, and a generator that stops early still runs its
cleanup.

With error handling and generics in hand, you're ready to make sure your code
does what you think it does. [Chapter 11](/book/ch11-00-testing) introduces
`aura test`, and we'll use it to find a real bug in some code that looks
correct.
