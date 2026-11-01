# Closures and decorators

So far, the functions in this book have been things we call. In this chapter
they become things we pass around. A sort that should order requests by
duration needs to be told how to get the duration out of a request. A filter
that keeps "requests slower than the limit the user typed" needs to carry that
limit with it. A tool runner that should log every call needs a way to put the
same logging around many different functions without editing each one.

Aura's answer to the first two is the *closure*, written with `lambda`: a small
function value that can capture bindings from the place where it was created.
If you know Python's lambdas, the syntax will look familiar. What is new is
that every capture has an owner. A closure either copies a value, takes it
over, or borrows it for a limited time, and the compiler checks which one you
meant, as it does for any other binding. That is what lets Aura run
closures without a garbage collector, and it is why some closures can be
called many times while others can be called only once.

The answer to the third is the *decorator*. A line such as `@traced("count")`
above a function replaces that function, once, when the module starts, with a
wrapped version built from a closure. We'll use decorators together with
derived JSON codecs to build the core of a typed tool runner: a program that
receives a tool call as JSON text, decodes its arguments into a class, calls a
decorated function, and sends back JSON, with every malformed call turned into
a precise error message.

By the end of the chapter you'll be able to read and write function types such
as `def(ms: int64) -> bool`, choose between capturing by copy, by move, and by
loan, store closures in `Callable[...]` values that can be returned and kept,
and write decorators, including generic ones that adapt to each function they
wrap.
