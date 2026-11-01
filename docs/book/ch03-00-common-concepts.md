# Common programming concepts

The word counter in the last chapter used a lot of Aura without stopping to
explain it. This chapter slows down and goes through the pieces that nearly
every program is built from: bindings, the basic data types, functions,
comments, and control flow. None of these ideas is unique to Aura, and if you
know Python, much of the syntax will look familiar. What is worth your time
is the set of places where Aura's rules differ, and the reasons it chose
them.

Three themes run through the chapter. The first is that Aura is explicit
about change: bindings are immutable unless you mark them `mut`, so you can
see at a glance which values move under you. The second is that Aura is
explicit about types: it works out most of them for you, but it never turns
one type into another behind your back, not even an `int32` into an `int64`.
The third is that the compiler checks the whole program before any of it
runs, so the mistakes this chapter shows you are ones you find at your desk,
not in production.

The examples are small and mostly self-contained, so you can paste each one
into a file and run it with `aura run`. We'll come back to a small
temperature-conversion program in several sections, ending with a function
that prints a conversion table using everything the chapter covers.

The [Aura Manual](/manual/) has the complete rules for everything here. Its
[Types](/manual/types), [Functions](/manual/functions), and
[Statements](/manual/statements) pages are the ones to keep nearby.
