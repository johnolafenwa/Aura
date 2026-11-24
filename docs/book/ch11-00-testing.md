# Testing and proving

By now the compiler checks a great deal for you. It knows every value's type,
it knows who owns what, and it won't let you reach the inside of a `Result`
without handling the error. What it can't know is what you meant. If a function
is supposed to apply a 10% discount and it applies 1%, the types are perfect
and the program is wrong. Aura gives you two ways to write down what you
meant so a machine can check it every time the code changes, and this
chapter is about both.

The first is a test. Aura ships its own test runner as part of the `aura`
command, so there's no framework to choose and nothing to install. A test is
an ordinary function whose name starts with `test_`, it uses the `assert`
statement you met in [Chapter 9](/book/ch09-01-traps), and `aura test` finds
it, runs it, and reports the result. A test tells you the code is right for
the inputs you wrote down.

The second is a proof. A function can state what it needs from its callers
and what it promises back, in `requires` and `ensures` lines called
contracts, and `aura verify` hands those statements to a solver that either
proves they hold for every input or shows you an input that breaks one. It
proves the checks you didn't write, too: that no addition overflows, no
divisor is zero, no index runs off the end of a list, and no dictionary read
misses its key. The two halves work on the same code and find different
bugs, and by the end of the chapter you'll reach for both without thinking
about which is which.

We'll create a small package called `checkout` that totals a shopping cart,
applies discounts, and formats prices. In the first half we'll write tests
for it, watch one of them catch a real bug in code that looks correct, learn
to read failure output and select which tests run, and organize a growing
suite with shared fixtures, parameterized cases, and machine-readable output
for CI. In the second half we'll add contracts to the same code, let the
verifier find two bugs the tests missed, and then prove the loops, the
collections, and the classes of `checkout`, including that a gift card's
balance can never go below zero.
