# Getting started

Before we can write any Aura, we need the tool that understands it. Aura ships
as one command, `aura`, which checks, runs, builds, tests, and formats your
code, and also powers the editor support. In this chapter we install it and
write our first few programs. By the end you will have `aura` on your path,
you will have run a program three different ways, and you will have a package
with its own tests that you can open in an editor.

We will install Aura on macOS, Linux, or Windows through WSL. Then we will
write the traditional program that prints `Hello, world!`, first as a
one-line script and then with an explicit `main` function, and turn it into a
native executable. Finally we will use `aura new` to create a package, which
is how most real Aura code is organized, and meet `aura check` and
`aura test`, the two commands you will run more than any others.

If you have used Python, most of this chapter will feel familiar. The one new
idea is the build step: the same `aura` tool that runs your program can also
compile it to a standalone binary. We will see both, and why you might want
each.
