# Using classes to structure data

A program that tracks a book you're reading needs a few facts about it: a
title, an author, a page count, and how far you've got. You can keep those in
four separate bindings, and for a dozen lines that works. Once there are
several books, or functions that need all four facts at once, loose bindings
start to drift apart. Nothing in the code says that `pages` and `pages_read`
describe the same book, and nothing stops a function from being handed the
page count of one book and the progress of another.

A *class* gives a group of related values a name and a shape. In this chapter
we'll declare classes, build instances of them, and read and change their
fields. Then we'll write a small reading-tracker program twice: once with
loose values and once with a class, so you can see what the class buys.
Finally we'll move the functions that work on a book into the class itself as
*methods*, and look at how the three kinds of `self` say what each method does
to its instance.

If you have written Python classes, much of this will look familiar, from
`__init__` to `@property`. Two things will be new. Every field has a declared
type, and every method says, in its first parameter, whether it reads the
instance, changes it, or consumes it, using the ownership rules from
[Chapter 4](/book/ch04-00-understanding-ownership). And Aura classes have no
inheritance. We'll talk about why, and what to use instead, at the end of the
chapter.
