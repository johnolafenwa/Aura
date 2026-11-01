# Generics, traits, and conversions

Every program eventually grows two functions that are the same except for a
type. One finds the largest `int64` in a list, the other the largest `str`. One
stack holds undo actions, another holds numbers. Copying the code works for a
while, and then a bug gets fixed in one copy and not the other. Python avoids
the problem by not checking types until the program runs, so one function can
accept anything and fail later if the thing doesn't support what the function
does with it. Aura checks types before the program runs, so it needs a way to
write code once and still say exactly what that code requires.

That way has two parts. *Generics* let a function, class, or enum take type
parameters, so `Stack[T]` works for any `T`. *Traits* describe behavior that a
type can promise, such as "has a price" or "can be compared with `<`", and let
generic code ask for exactly the behavior it uses. You've already been using
both without seeing their definitions: `Result[T, E]`, `Lookup[T]`, and
`list[T]` are generic types, and the `From` trait in the previous chapter told
`try` how to convert one error into another.

The third part of this chapter is conversions. Aura never converts a value from
one type to another unless you write the conversion, not even an `int32` to an
`int64`. We'll look at why, at the three ways to convert numbers and text, and
at how traits let your own types join in.

We'll build a generic stack and a `largest` function that first refuses to
compile, then use traits to describe priced items in a small shop and fix
`largest` along the way. By the end you'll be able to write generic code,
define and implement traits, give your own types operators and custom printing,
and convert between types deliberately.
