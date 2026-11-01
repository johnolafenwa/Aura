# Enums and pattern matching

A class describes data where every part is present at once. A book always has
a title, a page count, and a position. A lot of data has a different shape.
A book on your shelf is unread, in progress, or finished: exactly one of the
three, never two. A command typed into our reading tracker is "read some
pages of this book" or "list the shelf" or "quit", and each kind carries
different information. A search finds a result or finds nothing.

Python programs usually model these with strings (`status = "reading"`), with
a class full of attributes that are sometimes `None`, or with a family of
subclasses. Each of those leaves room for values that make no sense, such as
a misspelled status, or a "list" command that somehow has a page count, and
the program finds out only when it runs into one.

Aura's answer is the *enum*: a type whose value is exactly one of a fixed set
of named *variants*, each carrying its own data. Paired with it is `match`,
which takes an enum apart, runs the code for whichever variant is there, and
is checked by the compiler to handle every variant. In this chapter we'll
define enums for a book's status and for the tracker's commands, write
`match` expressions over them (including a small recursive one that
evaluates arithmetic), and then look at `T | None`, the type Aura uses for a
value that might be missing.

By the end you'll be able to model "one of several things" so that the
nonsense cases can't be written down, and handle each case in a way the
compiler checks for completeness. Enums show up everywhere in Aura,
including in its error handling, so the patterns here will come back in
[Chapter 9](/book/ch09-00-error-handling).
