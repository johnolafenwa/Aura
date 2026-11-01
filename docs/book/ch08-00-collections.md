# Common collections

Most of the data a program handles comes in groups: the lines of a file, the
scores of a class, the words in a document and how often each one appears.
Aura gives you a small set of collection types for holding those groups, and
if you've written Python they will look familiar. A `list[T]` keeps values in
order. A `dict[K, V]` maps keys to values. A `set[T]` holds each value once.
`str`, which you've been using since the first chapter, is a sequence of
text with its own rules, and `Array[T]` is a block of numbers for arithmetic
on a whole table at a time.

The familiar shapes come with two differences that run through the whole
chapter. First, every collection holds exactly one type of element, written
in its type: a `list[int64]` can't hold a string, and the compiler checks
that when it reads the literal. Second, collections own what they hold. When
you put a string into a list, the list becomes its owner, and when you take
something out, ownership moves to you. The ownership rules from
[Chapter 4](/book/ch04-00-understanding-ownership) explain almost every error
you'll meet in this chapter, and we'll point back to them as they come up.

We'll work through these types with a gradebook: a few students, a few
assignments, and the questions a teacher asks of them: who passed, what the
class average is, and who needs help. By the end of the chapter you'll be
able to pick the right collection for a job, read and change it without
fighting the ownership checker, and use comprehensions and
builtins such as `sum`, `sorted`, and `max` to answer questions in a line or
two.
