# Managing growing programs with modules and packages

Every program we have written so far fits in one file. That works for a
while, and then it doesn't. The file gets long enough that you scroll past
the class definitions to find `main`. Two unrelated helpers both want to be
called `format`. A function you wrote for one program would be handy in the
next one, and the only way to share it is to copy and paste. Python solves
these problems with modules, packages, and `pip`, and Aura solves them with
the same three ideas, held to stricter rules.

In Aura a *module* is one `.au` file. A file decides which of its names other
files may use by marking them `public`; everything else stays private, and
the compiler enforces that. A *package* is a directory with an `Aura.toml`
manifest and a `src/` folder of modules. Packages can depend on other
packages, from a path on your disk or from a git repository, and a
*workspace* lets several packages live side by side in one repository with a
single lockfile.

We'll learn all of this on one small program. It starts as a single file that
tracks a reading list, and over the next three sections we split it into
modules, decide which parts of it other code is allowed to touch, and finally
pull a reusable piece out into its own package that the reading list depends
on. By the end of the chapter you'll be able to lay out a multi-file Aura
project, read the compiler's import and visibility errors, and manage
dependencies with `Aura.toml` and `Aura.lock`.
