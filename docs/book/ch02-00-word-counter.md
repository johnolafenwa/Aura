# Programming a word counter

Let's jump into Aura by writing a real program together. We will build a
small command-line tool that reads text from standard input, counts how often
each word appears, and prints the most common ones. By the end, you will be
able to feed it any text file and get back something like this:

```console
$ aura run src/main.au < poem.txt
    6 the
    4 rain
    3 it
    3 falls
    3 on
```

Tools like this are the bread and butter of the command line. They read from
standard input, so you can pipe another program's output into them, and they
write plain text to standard output, so you can pipe their output onward.

Along the way we will meet a lot of Aura at once: bindings and `mut`,
`match` and the `Result` type, loops, dictionaries and lists, f-strings, and
the first hints of ownership. We won't explain everything fully here. The
goal of this chapter is to write a working program and to see what the
compiler says when we get something wrong; the chapters after it go back
over each idea in detail. If something feels unexplained, keep going, and
trust that it will come back.

## Setting up a new project

We'll make a package for the tool, as we did in
[Chapter 1](/book/ch01-03-hello-packages). Go to your `projects` directory
and run `aura new`:

```console
$ aura new word_count
created `word_count`
$ cd word_count
```

The package name uses an underscore rather than a hyphen, because a package
name must be a valid Aura identifier. Run the generated program to make sure
everything is in place:

```console
$ aura run src/main.au
Hello from Aura
```

We also need some text to count. Save this as `poem.txt` in the `word_count`
directory, next to `Aura.toml`:

*Filename: poem.txt*

```text
The rain, the rain! It falls on the plain.
It falls on the hills, and it falls on the lane.
"Rain," said the girl. "Rain again."
```

It is short, so we can check the counts by eye, and it has capital letters
and punctuation, which will cause us trouble in a useful way.

## Reading a line of input

The first job is to read text. Aura's functions for standard input and output
live in a module called `io`, and the one we want is `io.read_line`, which
reads one line. Replace the contents of `src/main.au` with Listing 2-1.

*Filename: src/main.au*

```aura check-fail:AU2013
import io

def main() -> int32:
    match io.read_line():
        case Result.Ok(str as line):
            print(f"read: {line}")
        case Result.Err(error):
            print(f"could not read input: {error}")
    return 0
```

<span class="caption">Listing 2-1: A first attempt at reading one line from standard input</span>

There's a lot in these few lines. `import io` makes the `io` module available
in this file, so that we can call `io.read_line()`. The rest is a `match`
statement, which looks at a value and picks one of several `case` branches
depending on its shape. Before we go through it, let's see what the compiler
thinks:

```console
$ aura run src/main.au
error[AU2013]: non-exhaustive match over `Result`: missing `Ok(None)`
 --> src/main.au:4:5
  |
4 |     match io.read_line():
  |     ^
```

The program didn't run. The compiler says our `match` is *non-exhaustive*:
there is a possible value that none of our cases handles, and it even names
the missing one, `Ok(None)`. To see where that comes from, we need to know
what `io.read_line` returns. Its type is:

```text
Result[str | None, io.Error]
```

Reading input can fail. The input might not be valid text, or the operating
system might report an error. Many languages handle this with exceptions,
which means the possibility of failure is invisible at the call site: nothing
about `line = input()` in Python tells you it can raise `EOFError`. Aura puts
the possibility in the return type instead. A `Result` is either
`Result.Ok(value)`, holding the successful value, or `Result.Err(error)`,
holding a description of what went wrong. Here the error is an `io.Error`.

The successful value has its own twist. Its type is `str | None`, which means
"either a `str` or `None`". `io.read_line` returns `Ok` with a string for each
line it reads, and `Ok(None)` when it reaches the end of the input and there
are no more lines. So there are three outcomes, not two: a line, the end of
the input, and an error. Our `match` handled the first and the last, and the
compiler noticed that we forgot the middle one.

This check is one of the things that makes Aura programs hold up well. If we
had been allowed to run Listing 2-1, it would have worked when we tested it
with a line of text, and then done something unpredictable the first time
someone ran it on an empty file. Because `match` must be exhaustive, the
question "what happens at end of input?" has to be answered before the
program can run at all.

Add the missing case, as in Listing 2-2.

*Filename: src/main.au*

```aura
import io

def main() -> int32:
    match io.read_line():
        case Result.Ok(str as line):
            print(f"read: {line}")
        case Result.Ok(None):
            print("no input")
        case Result.Err(error):
            print(f"could not read input: {error}")
    return 0
```

<span class="caption">Listing 2-2: Handling a line, the end of input, and an error</span>

Each `case` is a *pattern*. `Result.Ok(str as line)` matches an `Ok` whose
contents are a `str`, and binds that string to the name `line` so the body
can use it. `Result.Ok(None)` matches an `Ok` holding `None`. And
`Result.Err(error)` matches any error and binds it to `error`. The body of
each case uses an f-string to build its message, like the ones we saw in
Chapter 1. Patterns can do a good deal more than this, and
[Chapter 6](/book/ch06-00-enums-and-match) covers `match` in full;
[Chapter 9](/book/ch09-00-error-handling) does the same for `Result`.

Now let's run it. Piping text into a program with `echo` is a quick way to
give it a line of input:

```console
$ echo "the quick brown fox" | aura run src/main.au
read: the quick brown fox
```

The shell's `<` operator sends a file to standard input instead. With
`/dev/null`, which is always empty, we get the end-of-input case straight
away:

```console
$ aura run src/main.au < /dev/null
no input
$ aura run src/main.au < poem.txt
read: The rain, the rain! It falls on the plain.
```

With `poem.txt`, the program read the first line and stopped, because we only
asked for one. Notice that the line has no newline character at the end:
`io.read_line` removes the line ending for us. It does not trim anything else,
so leading and trailing spaces are kept.

We can even trigger the error case, by sending bytes that are not valid UTF-8
text. `printf` with `\xe9` produces a byte that is a lone accented `é` in an
old encoding, but is not allowed on its own in UTF-8:

```console
$ printf 'caf\xe9\n' | aura run src/main.au
could not read input: io.Error.InvalidData
```

Aura strings are always valid UTF-8, so input that isn't becomes an error
rather than a string with garbage in it.

If you run the program with no redirection at all, it waits for you to type a
line. Type something and press Enter, or press Ctrl-D on an empty line to
send end-of-input.

## Reading until the end of the input

We want every line, not only the first. Let's put the `match` in a loop that
keeps going until the end of the input. To check that it works before we
count any words, we'll count lines first. Change `src/main.au` to Listing
2-3.

*Filename: src/main.au*

```aura check-fail:AU3003
import io

def main() -> int32:
    lines = 0
    while true:
        match io.read_line():
            case Result.Ok(str as line):
                lines += 1
            case Result.Ok(None):
                break
            case Result.Err(error):
                print(f"could not read input: {error}")
                return 1
    print(f"lines: {lines}")
    return 0
```

<span class="caption">Listing 2-3: Counting lines until the end of input</span>

`while true:` loops forever, and the `break` in the `Ok(None)` case gets us
out when the input runs out. If reading fails, we print a message and
`return 1` from `main`, which ends the program with a non-zero exit code to
tell the shell that something went wrong. The line count lives in a binding
called `lines`, which starts at `0` and goes up by one for every line. Let's
try it:

```console
$ aura run src/main.au < poem.txt
error[AU3003]: cannot assign to immutable binding `lines`; declare it with `mut` to rebind it
 --> src/main.au:8:17
  |
8 |                 lines += 1
  |                 ^
```

This is the error that almost every newcomer to Aura meets first, so it's
good to get it out of the way. In Aura, a binding created with `name = value`
is *immutable*: once it has a value, it keeps that value. `lines += 1` tries
to change it, and the compiler stops us. The message also says exactly how to
fix it: declare the binding with `mut`.

Why make immutability the default? Because most bindings in a program never
change, and when you read code, knowing which ones *can* change is valuable.
If a binding is not marked `mut`, you know that its value at the bottom of a
function is the value it was given at the top, without reading every line in
between. The `mut` marks are the places to look when you want to know what
changes. [Chapter 3](/book/ch03-01-bindings-and-mutability) has more on this.

Change the line that creates the binding to:

```aura fragment
    mut lines = 0
```

and run the program again:

```console
$ aura run src/main.au < poem.txt
lines: 3
```

Three lines, which is right. It also handles input whose last line has no
newline, and empty input:

```console
$ printf 'one\ntwo' | aura run src/main.au
lines: 2
$ printf '' | aura run src/main.au
lines: 0
```

## Counting words

Now for the real work. For each line, we need to split it into words and
keep a tally of how many times we have seen each word. A *dictionary* is the
natural fit: a `dict` maps keys to values, so we can map each word to its
count. Listing 2-4 is our first attempt.

*Filename: src/main.au*

```aura check-fail:AU3002
import io

def main() -> int32:
    mut counts: dict[str, int64] = {}
    while true:
        match io.read_line():
            case Result.Ok(str as line):
                for word in line.split_whitespace():
                    if word in counts:
                        counts[word] += 1
                    else:
                        counts[word] = 1
            case Result.Ok(None):
                break
            case Result.Err(error):
                print(f"could not read input: {error}")
                return 1
    print(counts)
    return 0
```

<span class="caption">Listing 2-4: A first attempt at counting words in a dictionary</span>

The line `mut counts: dict[str, int64] = {}` creates an empty dictionary.
This time we wrote the type ourselves, after the colon: a `dict` whose keys
are `str` and whose values are `int64`, Aura's standard 64-bit integer. We
have to, because `{}` on its own says nothing about what will go in it. When
we wrote `lines = 0`, Aura could tell from the `0` that `lines` was an
integer; with an empty dictionary there is nothing to go on. The binding is
`mut` because we will add entries to it.

Inside the loop, `line.split_whitespace()` splits the line at runs of spaces
and tabs, and gives us a list of the pieces. The `for` loop visits each one.
If the word is already a key in the dictionary (`word in counts`), we add one
to its count; otherwise we store it with a count of `1`. At the end we print
the whole dictionary. Let's run it:

```console
$ aura run src/main.au < poem.txt
error[AU3002]: cannot move borrowed value `word`
 --> src/main.au:10:32
   |
10 |                         counts[word] += 1
   |                                ^
  = related src/main.au:8:21: value is borrowed here
  = help: take `word` as `own str` when ownership is required, or call `.clone()` to consume an independent copy
  = fix: replace src/main.au:10:36-10:36 with `.clone()`
```

This error is our first meeting with *ownership*, the idea at the heart of
Aura, and it deserves a careful read. The message says we tried to *move* a
*borrowed* value, `word`. The primary location is the `word` inside
`counts[word]` on line 10. The `related` line points at line 8, column 21,
which is the `word` in `for word in line.split_whitespace()`, and says that
is where the value is borrowed.

Here is what the compiler sees. In Aura, every value has exactly one owner,
and the owner is responsible for cleaning the value up when it is no longer
needed. The list that `split_whitespace()` returns owns its strings. When a
`for` loop walks over a list, it doesn't take the strings out of it; it
*borrows* each one in turn, which means `word` lets us look at a string that
still belongs to the list. That's fine for reading, and `word in counts` only
reads.

Storing a key in a dictionary is different. Once the dictionary has the
string, the dictionary has to own it, because it will keep it long after this
loop iteration is over. But we only borrowed `word`. If Aura let the
dictionary take it, the list and the dictionary would both think they owned
the same string, and when the list was cleaned up at the end of the loop, the
dictionary would be left holding a string that no longer existed. Languages
with a garbage collector, like Python, avoid this by keeping every value alive
as long as anything can reach it. Aura has no garbage collector, so it tracks
owners instead, and it checks that tracking before the program runs.

The `help` line offers two ways out. We could call `.clone()` to make an
independent copy of the string for the dictionary to own; the `fix` line even
gives the exact edit, as a line and column range and the text to insert there.
That would work.
But our counter has another problem we haven't seen yet: `The` and `the`
would be counted as different words. The usual fix for that is to convert
every word to lowercase, and `to_lower()` returns a brand-new string, which
means a string that we own. One change solves both problems. Update the loop
as in Listing 2-5.

*Filename: src/main.au*

```aura
import io

def main() -> int32:
    mut counts: dict[str, int64] = {}
    while true:
        match io.read_line():
            case Result.Ok(str as line):
                for raw in line.split_whitespace():
                    word = raw.to_lower()
                    if word in counts:
                        counts[word] += 1
                    else:
                        counts[word] = 1
            case Result.Ok(None):
                break
            case Result.Err(error):
                print(f"could not read input: {error}")
                return 1
    print(counts)
    return 0
```

<span class="caption">Listing 2-5: Lowercasing each word gives the dictionary a string it can own</span>

The loop now borrows each piece as `raw`, and `word = raw.to_lower()` makes a
new lowercase string owned by the binding `word`. When we store it with
`counts[word] = 1`, ownership moves from `word` to the dictionary. That's
allowed, because `word` was ours to give. Run it:

```console
$ aura run src/main.au < poem.txt
{the: 6, rain,: 1, rain!: 1, it: 3, falls: 3, on: 3, plain.: 1, hills,: 1, and: 1, lane.: 1, "rain,": 1, said: 1, girl.: 1, "rain: 1, again.": 1}
```

It works, and `The` and `the` are now counted together: six of them. The
dictionary prints its entries in the order the keys were first added, which
is the order Aura dictionaries always keep. (Printing shows strings without
quotes, which is why the keys look bare.)

The output also shows our next problem. `rain,`, `rain!`, `"rain,`, and
`"rain` are all counted as different words, because the punctuation is still
attached. We'll come back to that. First, let's turn this dump into a
readable report.

Ownership is the subject of all of
[Chapter 4](/book/ch04-00-understanding-ownership). For now, the thing to
take away is that the compiler knows, at every line, who owns each value and
who is only borrowing it, and it will tell you when those two get mixed up.

## Sorting and printing the top words

We want the most common words first, and only the top few. A dictionary isn't
sorted by its values, so we'll copy its entries into a list and sort that.
Replace the `print(counts)` line and add a constant at the top of the file,
as in Listing 2-6.

*Filename: src/main.au*

```aura
import io

TOP: int64 = 5

def main() -> int32:
    mut counts: dict[str, int64] = {}
    while true:
        match io.read_line():
            case Result.Ok(str as line):
                for raw in line.split_whitespace():
                    word = raw.to_lower()
                    if word in counts:
                        counts[word] += 1
                    else:
                        counts[word] = 1
            case Result.Ok(None):
                break
            case Result.Err(error):
                print(f"could not read input: {error}")
                return 1

    mut entries = counts.items()
    entries.sort(key=lambda entry: entry[1], reverse=true)
    for word, count in entries[:TOP]:
        print(f"{count:>5} {word}")
    return 0
```

<span class="caption">Listing 2-6: Sorting the entries by count and printing the top five</span>

`TOP: int64 = 5` is a binding at the top level of the file, outside any
function. A top-level binding is a *module constant*: it can't be changed,
and every function in the file can read it. Writing its name in capitals is a
habit from Python that makes constants stand out.

`counts.items()` returns a new list of the dictionary's entries. Each entry is
a *tuple*, a fixed-size group of values written in parentheses: here
`(word, count)`, of type `(str, int64)`. We sort that list in place with
`entries.sort(...)`, and because sorting changes the list, `entries` is `mut`.

The two arguments to `sort` are passed by name, as you can in Python. `key`
says what to sort by. Its value is a *lambda*, a small unnamed function
written inline: `lambda entry: entry[1]` takes an entry and returns its
second element, the count. (`entry[0]` would be the word.) `reverse=true`
sorts from largest to smallest. Note that `true` is lowercase in Aura.

The `for` loop then unpacks each tuple into two bindings, `word` and `count`,
the same way Python does. `entries[:TOP]` is a *slice*, the first `TOP`
entries of the list. Finally, `{count:>5}` in the f-string formats the count
right-aligned in a field five characters wide, so the words line up. The
format specifications after the `:` work much like Python's.

Let's see the report:

```console
$ aura run src/main.au < poem.txt
    6 the
    3 it
    3 falls
    3 on
    1 rain,
```

That looks like a word counter. `it`, `falls`, and `on` all have three, and
they come out in the order they first appeared in the poem, because Aura's
sort is *stable*: entries that compare equal keep their original order.

Let's try a different input, a famous line with fewer distinct words:

```console
$ echo "to be or not to be" | aura run src/main.au
error[AU4003]: slice end `5` is outside `0..=4`
 --> src/main.au:24:32
   |
24 |     for word, count in entries[:TOP]:
   |                                ^
  = note: Aura call chain (innermost first): main at 5:1
```

This time the program passed every compile-time check and started running.
The error happened at run time, on line 24, and stopped the program. Aura
calls this kind of failure a *trap*, and
[Chapter 9](/book/ch09-01-traps) has the full story. The message is precise: we asked for a
slice ending at position `5`, but the list only has four entries
(`to`, `be`, `or`, `not`), so valid end positions run from `0` to `4`. The
note gives the call chain, the functions that were running when it happened;
here it was `main`, defined at line 5.

If you know Python, this may be a surprise, because Python quietly shortens a
slice that runs past the end of a list. Aura does not, on purpose. A slice
that runs off the end is often a sign that the code's idea of the data is
wrong, and quietly returning fewer items hides that until something further
along goes wrong in a more confusing way. Aura would rather stop at the line
that made the bad assumption. The compiler could not catch this one before
running, because the length of `entries` depends on the input, so it is
checked at the moment the slice happens.

Here, though, "fewer than five words" is a perfectly normal situation, so the
code should handle it. The fix is to slice to whichever is smaller, `TOP` or
the length of the list. Aura has a `min` builtin, as Python does. Replace the
last loop with this:

```aura fragment
    shown = min(TOP, entries.len())
    for word, count in entries[:shown]:
        print(f"{count:>5} {word}")
```

`entries.len()` is the number of entries, and `min` returns the smaller of its
arguments. Now the short input works:

```console
$ echo "to be or not to be" | aura run src/main.au
    2 to
    2 be
    1 or
    1 not
```

## Cleaning up punctuation

The last problem is the punctuation stuck to the words. We'll write a
function, `clean`, that strips punctuation characters from the start and end
of a word and lowercases what's left. While we are at it, we'll move the
counting loop into a function of its own, so that `main` reads as a summary
of what the program does. Listing 2-7 is the finished program.

*Filename: src/main.au*

```aura
import io

TOP: int64 = 5

def clean(word: str) -> str:
    """Lowercase `word` and strip punctuation from both ends."""
    punctuation = ".,;:!?\"'()"
    mut start: int64 = 0
    mut stop = word.len()
    while start < stop and word[start:start + 1] in punctuation:
        start += 1
    while stop > start and word[stop - 1:stop] in punctuation:
        stop -= 1
    return word[start:stop].to_lower()

def count_words(line: str, counts: mut dict[str, int64]):
    for raw in line.split_whitespace():
        word = clean(raw)
        if word == "":
            continue
        if word in counts:
            counts[word] += 1
        else:
            counts[word] = 1

def main() -> int32:
    mut counts: dict[str, int64] = {}
    while true:
        match io.read_line():
            case Result.Ok(str as line):
                count_words(line, counts)
            case Result.Ok(None):
                break
            case Result.Err(error):
                print(f"could not read input: {error}")
                return 1

    mut entries = counts.items()
    entries.sort(key=lambda entry: entry[1], reverse=true)
    shown = min(TOP, entries.len())
    for word, count in entries[:shown]:
        print(f"{count:>5} {word}")
    return 0
```

<span class="caption">Listing 2-7: The finished word counter</span>

Let's start with `clean`. Its signature, `def clean(word: str) -> str`, says
it takes a string called `word` and returns a string. Every parameter in Aura
has a type written after it, and so does the return value, after the `->`.
Aura could work out the types of local bindings for us, but at the boundary
of a function it wants them spelled out, so that you can tell what a function
accepts by reading one line. The triple-quoted string on the first line of the
body is a *docstring*, as in Python; your editor shows it when you hover over
a call to `clean`.

The body walks two positions inward from the ends of the word. `start` begins
at `0` and `stop` at `word.len()`, the number of characters. The first
`while` loop moves `start` forward as long as the character at that position
is punctuation. `word[start:start + 1]` is a slice of the string containing
only that one character, and `in punctuation` asks whether that character
appears in our string of punctuation marks. Inside that string, `\"` is an
escaped double quote, so the string contains `"` along with `'`. The second
loop does the same from the other end. The `start < stop` and `stop > start`
conditions stop the loops from crossing, which matters for a "word" made
entirely of punctuation, such as `--` or `...`. Finally, we slice out the
middle and lowercase it.

We wrote `mut start: int64 = 0` with a type, while `mut stop = word.len()`
needed none. The two mean the same kind of thing; `int64` is what `0` would
have been anyway. Writing it here only makes the two loop positions look
alike. Both are `mut`, because the loops change them.

Now `count_words`. Look at its second parameter:
`counts: mut dict[str, int64]`. The `mut` in a parameter's type means the
function is allowed to change the caller's dictionary. When `main` calls
`count_words(line, counts)`, it lends its dictionary to the function, the
function adds to it, and when the call returns, `main` has the updated
dictionary back. Without `mut`, the compiler would reject the
`counts[word] += 1` lines, because a plain parameter can only be read. The
first parameter, `line: str`, is plain: `count_words` only reads the line.
This is how Aura function signatures tell you what a function will do to each
argument, and it's covered in
[Chapter 4](/book/ch04-00-understanding-ownership).

Notice that the call site doesn't change: there's no special marker on
`counts` in `count_words(line, counts)`. The function's signature decides
what kind of access it gets.

Inside `count_words`, a word that was nothing but punctuation is now an empty
string, and `continue` skips it and moves on to the next one. And because
`clean` returns a new string, `word` is owned, and storing it in `counts` is
allowed for the same reason as before.

Let's run the finished program:

```console
$ aura run src/main.au < poem.txt
    6 the
    4 rain
    3 it
    3 falls
    3 on
$ echo "to be or not to be" | aura run src/main.au
    2 to
    2 be
    1 or
    1 not
$ echo '...' | aura run src/main.au
```

`rain` is now counted four times, as it should be, and input with no real
words prints nothing at all.

## Building the tool

A word counter is the kind of program you want to keep around, so let's
compile it:

```console
$ aura build -o target/word_count src/main.au
aura: built `target/word_count` with direct backend
$ ./target/word_count < poem.txt
    6 the
    4 rain
    3 it
    3 falls
    3 on
```

The binary works anywhere a pipe does. Here it reads two copies of the poem,
one after the other, from `cat`:

```console
$ cat poem.txt poem.txt | ./target/word_count
   12 the
    8 rain
    6 it
    6 falls
    6 on
```

If you want to experiment further, here are some ideas to try. Each uses
only what we have seen in this chapter:

- Change `TOP` and see how the output changes.
- Skip very short words, such as those with fewer than three characters.
- Print the total number of words, and the number of distinct words, before
  the list.

## Summary

That was a lot of ground for one program. We used `io.read_line` to read
input, and saw that it returns a `Result` whose type spells out every outcome,
including the end of the input. We used `match` to handle each case, and the
compiler made sure we did not forget one. We met immutable bindings and `mut`,
dictionaries, lists, tuples, lambdas, slices, and f-strings. And we ran into
three different kinds of error: a non-exhaustive `match`, an assignment to an
immutable binding, and a borrowed value that we tried to store, all caught
before the program ran, plus a slice past the end of a list that was caught
the moment it happened.

The rest of the book takes those ideas one at a time. The next chapter starts
with the building blocks that almost every program uses: bindings, the basic
data types, functions, comments, and control flow.
