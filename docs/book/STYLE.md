# Writing the Aura book

This file is the voice and structure guide for *The Aura Programming
Language*, the book under `docs/book/`. It is not published on the site. Read
it before you write or edit a chapter.

The model is *The Rust Programming Language* (https://doc.rust-lang.org/book).
We are not copying its text or its examples. We are borrowing the way it
teaches: a patient, experienced colleague sitting next to the reader, writing
a program with them, and explaining why the language works the way it does.

## Who we are writing for

The reader already programs in some language, most often Python. They do not
know Aura, and they have probably never thought hard about ownership. They
are smart and busy. They want to build something real, and they want to know
the reasons behind the rules so the rules stop feeling arbitrary.

## The voice

**We write as "we" and talk to "you".** "Let's add a function that reads the
file." "You'll see this error the first time you forget `mut`." The book and
the reader are working on the program together.

**We teach by doing.** Most sections start from code the reader can run,
show what happens, and then explain. When the obvious first attempt fails,
we write the obvious first attempt, show the real compiler error, and read it
together line by line before fixing it. Errors are part of the lesson, not
something to hide.

**We explain why.** Every rule gets its reason: what goes wrong in languages
without it, what the compiler can guarantee because of it, and what it costs.
"Aura does this because..." is a sentence the book should use often.

**We are honest about trade-offs.** When a rule is inconvenient, say so, then
say what it buys. When Aura is a preview and something is missing, say that
plainly and point to the workaround.

**We are warm, not cute.** Encouragement is fine where a topic is genuinely
hard ("Ownership takes a while to settle in. Keep going."). Jokes are rare and
small. No exclamation marks except in program output, and at most one or two
per chapter in prose.

**We use full paragraphs.** Ideas connect with "because", "so", "but",
"which means". A paragraph usually carries one idea over three to six
sentences. Bullet lists are for genuinely list-shaped things: steps the
reader follows, the fields of a type, a set of options. They are not a way to
avoid writing transitions.

## Things that make prose sound generated

Avoid all of these. They are the reason the current docs read as generic.

- Bold-label bullets (`- **Captures:** ...`, `- **Storage:** ...`) used to
  pack explanations. Write the explanation as a paragraph instead.
- Openers that announce structure: "This chapter covers...", "In this
  section we will explore...", "Let's dive in", "Let's take a look".
  Start with the problem or the code. A one-sentence orientation at the top
  of a chapter is fine when it names what we will build.
- Inflated or empty words: "powerful", "seamless", "robust", "leverage",
  "crucial", "essential", "ecosystem", "unlock", "elevate", "delve",
  "comprehensive", "it's worth noting", "in summary", "simply", "just",
  "easily", "effortlessly".
- The "not X, but Y" flourish, triplets of adjectives, and rhetorical
  questions answered in the next sentence.
- Em-dashes. Use a comma, a colon, parentheses, or a new sentence.
- Sentence fragments as style ("Fast. Safe. Simple.").
- Closing paragraphs that restate the section. A section ends by leading
  into the next one.
- Title Case Headings. Use sentence case: "Reading a line of input", not
  "Reading A Line Of Input".

## Structure

### Chapters and sections

The book is split into numbered chapters. A chapter with several sections is
a folder of files named like the Rust book:

```text
ch04-00-understanding-ownership.md      chapter landing page
ch04-01-what-is-ownership.md            section 4.1
ch04-02-shared-and-mutable-access.md    section 4.2
```

The `-00-` page introduces the chapter in two to four paragraphs: what
problem the chapter solves and what the reader will be able to do at the
end. It does not list the sections; the sidebar does that.

Each section's first heading is its title with the number, for example
`# Shared and mutable access`. Do not put numbers in headings; the sidebar
numbers them.

### Running examples

Chapters build programs. Pick one small program per chapter (or per group of
sections) and grow it, so code from one section is still on screen in the
next. Chapter 2 and the project chapters build a whole tool from an empty
directory.

When a block is the full contents of a file, put the file name on the line
above it in italics:

```markdown
*Filename: src/main.au*
```

Number the important code blocks as listings so prose can refer to them. Put
the caption on its own line directly under the block:

```markdown
<span class="caption">Listing 4-2: Moving a list into a function</span>
```

Short throwaway snippets do not need a listing number.

### Showing output and errors

Show what the reader will actually see. Run the program with the `aura` you
built, paste the real output into a `console` block, and include the command:

````markdown
```console
$ aura run src/main.au
total: 180
```
````

For an error, show the full diagnostic exactly as `aura` prints it, then walk
through it: which line, what the compiler noticed, what the note or help line
suggests, and why that rule exists. Never invent output.

Code blocks fenced as `aura` are compiled in CI by
`scripts/check_doc_examples.py`:

- ```` ```aura ```` must pass `aura check`.
- ```` ```aura check-fail:AU3004 ```` must fail with exactly that code. Use
  it for every "this won't compile" listing.
- ```` ```aura fragment ```` is a partial snippet that must not compile alone.
  Prefer complete programs; use `fragment` only for a few lines shown out of
  context.

Run the checker on your files before you finish:

```console
$ python3 scripts/check_doc_examples.py docs/book/ch04-*.md
```

### Endings

A chapter's last section ends with a short `## Summary` of two or three
paragraphs: what the reader can now do, and what the next chapter adds. Only
chapter-final sections get a summary.

### Links to the Manual

The Manual is the reference. When a section simplifies a rule, link the
Manual page for the full contract: "The [Manual](/manual/ownership-and-borrowing)
has the complete rules." Do not duplicate reference tables in the book.

## Spelling and source layout

Use American spelling (behavior, organize, labeled), except where the prose
names an Aura API that spells a word differently: the language says
`cancelled()`, so the book says "cancelled" too. Wrap prose at about 80
columns.

## Terms

Use these consistently.

- *binding*, not variable, for `name = value`. "Mutable binding" for `mut`.
- *shared access* for a bare parameter, *mutable access* for `mut`, and
  *ownership transfer* or *moving* for `own`.
- *copy values* for the types that copy, and *owned values* for the rest.
- *trap* for a runtime failure that stops the task (failed assertion, index
  out of range). Not "panic", not "crash".
- *the compiler* or *`aura`* for the tool. *The checker* only when talking
  about the type checking phase specifically.
- *task*, *task group*, *queue* for concurrency. Not "thread" unless the
  text is about OS threads.

## A sample of the voice

Here is the register to aim for, taken from the ownership chapter's outline:

> Let's try to use a list after we have handed it to another function. Most
> languages would let this through. Aura won't:

```aura check-fail:AU3001
def total(values: own list[int64]) -> int64:
    mut sum: int64 = 0
    for value in values:
        sum += value
    return sum

def main():
    scores = [10, 20, 30]
    print(total(scores))
    print(scores)
```

> The compiler stops at the last `print`, and the message names both places
> that matter: the `print` that tried to use `scores`, and the call where it
> moved into `total`. Because `total` asked for `own`, it now
> owns the list and is free to drop it when it returns. If Aura let us print
> `scores` after that, we would be reading memory that no longer belongs to
> us. Python avoids the problem with a garbage collector that keeps the list
> alive as long as anyone can reach it. Aura has no garbage collector, so it
> makes the hand-off visible in the signature instead, and checks it before
> the program runs.

(The diagnostic code above must match what `aura check` really reports.
Verify every code you use. Note also that the example lives in `main`: a
top-level binding is module storage, and moving out of it is a different
error. Facts like this are why every claim in the book gets checked against
the real compiler.)
