# The Aura Programming Language

This is a book about Aura, a compiled programming language that reads like
Python. You write indented blocks, `def`, `class`, f-strings, and
`for name in names`, much as you would in a Python file. The difference is
what happens before your program runs: the compiler checks the type of every
value, checks who owns each piece of data and who is only borrowing it, and
checks that every failure your code can meet is handled somewhere. Then it
either runs the program straight away or compiles it to a native executable
with no runtime to install alongside it.

The book follows Aura 0.11, a technical preview. Most of the language is in
place and stable enough to learn from, and the places where it is still
moving are called out where we meet them.

## Who this book is for

We assume you already program in some language. Most readers come from
Python, and the book leans on that: when Aura does something the way Python
does, we say so and move on, and when it does something differently, we stop
and explain why. If your background is JavaScript, Go, Java, or C, you will
still be at home, because nothing here depends on knowing Python's corners.

We do not assume you have used Rust or thought about ownership before. That
idea gets its own chapter, and we build up to it slowly, with programs that
break and compiler messages that we read together.

## How to read this book

The chapters are meant to be read in order. Early chapters introduce an idea
briefly so that we can write a working program, and a later chapter comes back
to explain it properly. Chapter 2, for example, builds a word counter that
uses ownership and pattern matching before either has been explained. That is
on purpose: it is easier to understand a rule once you have run into it.

Some chapters teach concepts, and some are projects. The project chapters
(2, 12, and 19) build a complete tool from an empty directory, one step at a
time. If you like to learn by building, you can read them first and dip back
into the concept chapters when something puzzles you.

The first half of the book is about the language itself. After installing
Aura in Chapter 1 and writing the word counter in Chapter 2, we go through
the building blocks every program uses in Chapter 3, and then spend Chapter 4
on ownership, the idea the rest of the book leans on. Chapters 5 and 6 cover
the two ways to define your own types, classes and enums, and Chapter 7 shows
how to split a growing program into modules and packages. Chapter 8 tours
the collections, Chapter 9 covers error handling, Chapter 10 covers generics,
traits, and conversions, and Chapter 11 covers testing. Chapter 12 puts all of
that to work in a log analyzer.

The second half is about programs that deal with the world outside their own
memory. Chapter 13 covers closures and decorators, Chapter 14 covers resources
and cleanup with `with`, and Chapter 15 covers structured concurrency.
Chapter 16 reads and writes files, runs other programs, talks to the network,
and handles JSON. Chapter 17 takes a program from `aura run` to a shipped
binary that you can debug and measure, and Chapter 18 calls into C. Chapter 19
brings most of the book together in a supervised job runner. The appendices
at the end are for looking things up: keywords, operators, a guide for Python
developers, the tools, and the diagnostics you will meet most often.

You will see a lot of code that does not compile. When the obvious first
attempt at something is wrong, we write the obvious first attempt, show what
the compiler says about it, and then fix it. The error messages are part of
the lesson. Learning to read them is most of what it takes to get comfortable
with a new compiler.

This book teaches. It does not try to list every method or every rule. When
you want the complete contract for something, the
[Aura Manual](/manual/) is the reference, and the book links to the right
page of it whenever we simplify.

## Every example is compiled

Each Aura code block in this book is checked by the repository's continuous
integration on every change. A block that should compile is run through
`aura check` and must pass. A block that we show as an error is marked with
the diagnostic code it should produce, and CI fails if the compiler stops
reporting exactly that code. Short pieces of code shown out of context are
marked as fragments, and CI checks that they really are incomplete. The
output shown after a `$` prompt was produced by running the command, not
written by hand.

That means the book cannot quietly drift away from the compiler. If you find
an example that behaves differently on your machine, check `aura --version`
first, and then please report it, because it is a bug either in the book or
in Aura.

## Getting started

To follow along you need the `aura` command. The
[installation guide](/install/) has instructions for macOS, Linux, and
Windows through WSL, and [Chapter 1](/book/ch01-00-getting-started) walks
through the same steps with a little more explanation. The source for Aura
and for this book lives at
[github.com/johnolafenwa/Aura](https://github.com/johnolafenwa/Aura), in the
`docs/book/` directory.

When you are ready, start with the [Foreword](/book/foreword), or go straight
to [Getting started](/book/ch01-00-getting-started).
