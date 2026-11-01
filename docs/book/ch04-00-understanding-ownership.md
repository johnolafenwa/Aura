# Understanding ownership

Every program has to answer a question that most languages hide from you:
when is a piece of memory finished with, and who gets to say so? Python
answers it at run time. Each object carries a count of the references
pointing at it, the interpreter frees the object when the count reaches zero,
and a cycle collector sweeps up groups of objects that only point at each
other. You never write any of that down, and you never have to think about it
until a program uses too much memory, pauses at a bad moment, or holds a file
open longer than you expected.

Aura answers the question before the program runs. Each value has exactly one
owner, the compiler tracks that owner through every assignment, call, and
loop, and the value is cleaned up at a point you can find by reading the code.
There is no garbage collector and no reference counting. The rules that make
this work are called *ownership*, and they shape almost everything else in the
language: how functions receive their arguments, how methods treat `self`,
how collections hold their elements, how tasks share data, and how files and
sockets get closed.

The rules are small, but they are new if you come from Python, and the first
few compiler errors can feel like the language is being stubborn. In this
chapter we will trigger those errors on purpose. We will hand a list to a
function and try to use it afterwards, mutate a value that we only borrowed,
and pass the same list to a function twice. For each error we will read the
diagnostic together and work out what would go wrong if Aura let the program
through.

By the end of the chapter you will be able to look at a function signature
and know what it does with each argument, choose between letting a function
read a value, change it, or take it over, and make an independent copy when
you need two owners. Ownership takes a while to settle in. Keep going: the
rest of the book leans on it, and it gets easier with every program you write.
