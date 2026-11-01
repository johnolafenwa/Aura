# Writing automated tests

By now the compiler checks a great deal for you. It knows every value's type,
it knows who owns what, and it won't let you reach the inside of a `Result`
without handling the error. What it can't know is what you meant. If a function
is supposed to apply a 10% discount and it applies 1%, the types are perfect
and the program is wrong. Tests are how you write down what you meant, so a
machine can check it every time the code changes.

Aura ships its own test runner as part of the `aura` command, so there's no
framework to choose and nothing to install. A test is an ordinary function
whose name starts with `test_`, it uses the `assert` statement you met in
[Chapter 9](/book/ch09-01-traps), and `aura test` finds it, runs it, and
reports the result.

In this chapter we'll create a small package called `checkout` that totals a
shopping cart, applies discounts, and formats prices. We'll write tests for it,
watch one of them catch a real bug in code that looks correct, learn to read
failure output and select which tests run, and then organize a growing test
suite with shared fixtures, parameterized cases, and machine-readable output
for CI.
