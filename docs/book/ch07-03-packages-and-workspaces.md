# Packages, dependencies, and workspaces

Modules organize the code inside one program. Packages are how programs
share code with each other. Our `report.progress` module draws a text
progress bar, and nothing about it is specific to books: a download tool, a
test runner, or a backup script could all use it. Copying the file into each
of those projects would work until the day we fix a bug in one copy and
forget the others. A package lets us write it once and depend on it from
anywhere.

We've been inside a package all along. `aura new` made one for us, and the
compiler has been reading its manifest every time we ran the program. Let's
look at that manifest properly before we make a second package.

## The manifest

*Filename: Aura.toml*

```toml
[package]
name = "shelf"
version = "0.1.0"
edition = "2026"
```

All three fields are required. The `edition` names the version of the
language the package is written against, and today `"2026"` is the only
edition there is; anything else is refused:

```console
$ aura run src/main.au
error[AU2999]: manifest `Aura.toml` uses unsupported package edition `2024`; the current Aura compiler supports edition `2026`
 --> src/main.au
```

The `version` is the package's own version number, which must start with a
digit. The `name` is the one with real consequences, because it's more than
a label. When another package depends on this one, the name becomes the first
part of every import path, as in `import shelf.books`. That's why a package
name has to be a valid Aura identifier: letters, digits, and underscores, not
starting with a digit. A hyphenated name, common in other languages, is
refused:

```console
$ aura run src/main.au
error[AU2999]: manifest `Aura.toml` has an invalid package name `my-shelf`; package names must match `[A-Za-z_][A-Za-z0-9_]*`
 --> src/main.au
```

If it were allowed, `import my-shelf.books` would parse as `my` minus
`shelf.books`. `aura new` knows this rule and fixes the name for you: `aura new
my-shelf` creates a directory called `my-shelf` whose manifest says `name =
"my_shelf"`.

Two more rules come with every package. The source root is always `src/`,
and the file you hand to `aura run`, `aura check`, or `aura build` has to live
under it (test files under `tests/` are the exception, as you'll see in
[Chapter 11](/book/ch11-00-testing)). There's no setting to move it, which
means every Aura package you open is laid out the same way.

## The lockfile

You've probably noticed `Aura.lock` sitting next to the manifest. Here is
what it says for `shelf` today:

*Filename: Aura.lock*

```toml
version = 1

[[package]]
name = "shelf"
version = "0.1.0"
source = "path"
path = "."
```

The lockfile records how the compiler resolved every package in the program:
which packages there are, and exactly where each one came from. Right now
that's one package, found at `.`. It will become more interesting in a
moment. The compiler writes this file itself when `check`, `run`, or `build`
succeeds, so you never edit it by hand. Commit it to version control along
with the manifest, so that everyone who builds the project resolves the same
code.

## Making a library package

Let's make the progress bar into a package called `gauge`. It goes next to
`shelf`, not inside it, so first we step up a directory. We don't need the
`main.au` that `aura new` creates, because `gauge` is a library: other
programs will import it, and it has no program of its own to run.

```console
$ cd ..
$ aura new gauge
created `gauge`
$ rm gauge/src/main.au
```

Now we move the bar into `gauge/src/bar.au`. In `shelf` it read its width
from `settings`, but a library can't import its users' modules, so the width
becomes a parameter:

*Filename: gauge/src/bar.au*

```aura
"""Text progress bars."""

public def progress_bar(done: int64, total: int64, width: int64) -> str:
    """Draw `done` out of `total` as a bar `width` characters wide."""
    filled = done * width // total
    return "[" + segment("#", filled) + segment("-", width - filled) + "]"

def segment(mark: str, count: int64) -> str:
    return mark.repeat(count)
```

<span class="caption">Listing 7-14: The progress bar as a library module</span>

Back in `shelf`, we delete `src/report/progress.au` and import the bar from
its new home. Imports from another package start with that package's name, so
the path is `gauge.bar`:

*Filename: shelf/src/main.au*

```aura fragment
import books
import settings
from gauge.bar import progress_bar

def main() -> int32:
    mut reading_list = [
        books.Book(title="Dune", author="Frank Herbert", pages=412),
        books.Book(title="Kindred", author="Octavia Butler", pages=264),
        books.Book(title="Piranesi", author="Susanna Clarke", pages=272),
    ]
    reading_list[0].read_to(412)
    reading_list[1].read_to(100)
    total = books.total_pages(reading_list)
    done = total - books.pages_left(reading_list)
    bar = progress_bar(done, total, settings.bar_width)
    print(settings.shelf_name)
    print(f"{bar} {done}/{total} pages")
    return 0
```

<span class="caption">Listing 7-15: `shelf` importing from the `gauge` package</span>

```console
$ cd shelf
$ aura run src/main.au
error[AU2999]: cannot resolve module `gauge.bar` at `src/gauge/bar.au`
 --> src/main.au:3:1
  |
3 | from gauge.bar import progress_bar
  | ^
```

The compiler doesn't know `gauge` is a package, so it reads `gauge.bar` as a
local module and looks for `src/gauge/bar.au` inside `shelf`. Aura never goes
looking for packages on its own: not in the parent directory, not in a global
install location, not on the network. A package uses another package only if
its manifest says so. Let's say so:

*Filename: shelf/Aura.toml*

```toml
[package]
name = "shelf"
version = "0.1.0"
edition = "2026"

[dependencies]
gauge = { path = "../gauge" }
```

<span class="caption">Listing 7-16: Declaring a path dependency</span>

```console
$ aura run src/main.au
Reading list
[##########----------] 512/948 pages
```

A *path dependency* points at another package's directory, relative to the
manifest that declares it. It's the natural choice for packages that live in
the same repository, or for trying out a change to a library before you
publish it. The two packages now look like this:

```text
projects
├── gauge
│   ├── Aura.toml
│   ├── src
│   │   └── bar.au
│   └── tests
│       └── smoke.au
└── shelf
    ├── Aura.lock
    ├── Aura.toml
    ├── src
    │   ├── books.au
    │   ├── main.au
    │   └── settings.au
    └── tests
        └── smoke.au
```

and the lockfile has grown an entry for the dependency:

*Filename: shelf/Aura.lock*

```toml
version = 1

[[package]]
name = "gauge"
version = "0.1.0"
source = "path"
path = "../gauge"

[[package]]
name = "shelf"
version = "0.1.0"
source = "path"
path = "."
```

Inside `gauge`, imports are still relative to `gauge`'s own `src/`. If
`gauge` later splits `bar.au` into several modules, `bar.au` would import
them as, say, `import blocks`, without a `gauge.` prefix, exactly the way
`shelf/src/main.au` imports `books`. Only code *outside* a package uses its
name as a prefix.

Package boundaries add no new visibility rules, because the module rules
already cover them. `segment` is private to `gauge.bar`, and that doesn't
change because `shelf` has declared a dependency:

```console
$ aura run src/main.au
error[AU2999]: item `segment` is private in module `gauge.bar`
 --> src/main.au:3:37
  |
3 | from gauge.bar import progress_bar, segment
  |                                     ^
```

### The dependency's name is not a nickname

In the `[dependencies]` table, the key on the left of `=` must be the
dependency's real package name. You might want to call `gauge` something else
in `shelf`:

```toml
[dependencies]
meter = { path = "../gauge" }
```

```console
$ aura run src/main.au
error[AU2999]: dependency name `meter` does not match package name `gauge` at `/home/you/projects/gauge`
 --> src/main.au
```

The compiler opened `../gauge/Aura.toml`, found `name = "gauge"`, and refused
the mismatch. (It prints the full path of the package it found; we've written ours as
`/home/you/projects`.) This rule means that the name you read in an import, the
name in your manifest, and the name in the dependency's own manifest are
always the same word, so you can find the code behind any import without
guessing. If you want a shorter spelling in one file, that's what `import
gauge.bar as bars` is for.

## Git dependencies

Path dependencies are perfect for packages that sit next to each other on
your disk. For a package someone else maintains, you point at a git
repository instead:

```toml
[dependencies]
gauge = { git = "https://github.com/example/gauge.git", tag = "v0.1.0" }
```

You import a git dependency exactly as you'd import a path dependency, by its
package name. What changes is how the compiler finds the code. It fetches the
repository into a cache under your home directory, checks out the version you
asked for, and records the exact commit in `Aura.lock`. A git dependency can
say which version it wants in one of three ways. `rev = "..."` names one
commit by its hash, and never moves. `tag = "v0.1.0"` names a tag, and
`branch = "release"` names a branch; with none of the three, the dependency
follows the `main` branch. You may pick at most one:

```toml
[dependencies]
gauge = { git = "https://github.com/example/gauge.git", tag = "v0.1.0", branch = "main" }
```

```console
$ aura run src/main.au
error[AU2999]: dependency `gauge` must choose at most one git selector: `rev`, `tag`, or `branch`
 --> src/main.au
```

A branch is a moving target: tomorrow `main` may point at a different commit
than it does today. If the compiler followed it on every build, the same
source could compile one day and fail the next, with no change on your side.
So it doesn't. The first time it resolves a branch or tag, it writes the
commit it found into `Aura.lock`, and every later build uses that commit. For
a branch dependency the lock entry looks something like this:

```toml
[[package]]
name = "gauge"
version = "0.1.0"
source = "git"
git = "https://github.com/example/gauge.git"
rev = "0123456789abcdef0123456789abcdef01234567"
branch = "main"
```

When you *want* the newer code, ask for it by running `aura deps update`
from inside the package. That re-resolves every branch and tag dependency in
the package and rewrites the lockfile. `aura deps update gauge` refreshes one dependency by name. A
`rev` dependency is already pinned to one commit, so there's nothing to
update, and naming a path dependency is an error:

```console
$ aura deps update gauge
error[AU2999]: package `gauge` is not a git dependency in the current package graph
 --> /home/you/projects/shelf
```

That's the whole story for where packages come from today: a path or a git
repository. Aura has no package registry yet, so there's no `pip install`
equivalent and no version solving. If you write a dependency the way you
would in many other languages, with only a version number, the compiler tells
you what to do instead:

```toml
[dependencies]
gauge = "0.1.0"
```

```console
$ aura run src/main.au
error[AU2999]: version-only dependencies are not supported yet for `gauge` (requested `0.1.0`); use `gauge = { path = "..." }` or `gauge = { git = "..." }` instead
 --> src/main.au
```

One thing to keep in mind about git dependencies: Aura is careful about how
it fetches and caches them, but it can't tell you whether the code inside is
trustworthy. Pin a `rev` or a `tag` for anything you run in production, and
read what you depend on. The [Manual](/manual/packages) has the details of
the cache and the checks it performs.

## Workspaces

`shelf` and `gauge` are separate packages, and right now each would keep its
own lockfile. That's awkward for two packages that we develop together in one
repository: the two lockfiles could disagree, and every command has to be run
from inside one package or the other. A *workspace* ties them together. It's
a manifest in the parent directory that lists the member packages and has no
`[package]` table of its own:

*Filename: projects/Aura.toml*

```toml
[workspace]
members = ["shelf", "gauge"]
```

<span class="caption">Listing 7-17: A workspace manifest</span>

Each member is still an ordinary package with its own `Aura.toml`, its own
`src/`, and its own dependencies. What changes is where the lockfile lives.
Let's delete `shelf/Aura.lock` and run the program from the workspace root:

```console
$ cd ..
$ rm shelf/Aura.lock
$ aura run shelf/src/main.au
Reading list
[##########----------] 512/948 pages
```

The compiler found `shelf/Aura.toml`, noticed that the manifest one level up
lists `shelf` as a member, and wrote one lockfile for the whole workspace, at
the root:

*Filename: projects/Aura.lock*

```toml
version = 1

[[package]]
name = "gauge"
version = "0.1.0"
source = "path"
path = "gauge"

[[package]]
name = "shelf"
version = "0.1.0"
source = "path"
path = "shelf"
```

Paths in the lockfile are now relative to the workspace root. Here is the
final layout:

```text
projects
├── Aura.lock
├── Aura.toml
├── gauge
│   ├── Aura.toml
│   ├── src
│   │   └── bar.au
│   └── tests
│       └── smoke.au
└── shelf
    ├── Aura.toml
    ├── src
    │   ├── books.au
    │   ├── main.au
    │   └── settings.au
    └── tests
        └── smoke.au
```

`aura deps update` run from the root works on every member at once, and
`aura test` can run the tests of both packages in one go:

```console
$ aura deps update
Aura.lock is already up to date
$ aura test shelf/tests gauge/tests
ok gauge/tests/smoke.au
ok shelf/tests/smoke.au
2 passed; 0 failed
```

Being in the same workspace doesn't make packages depend on each other. If we
took the `[dependencies]` table out of `shelf/Aura.toml`, the workspace would
not fill the gap:

```console
$ aura run shelf/src/main.au
error[AU2999]: cannot resolve module `gauge.bar` at `shelf/src/gauge/bar.au`
 --> shelf/src/main.au:3:1
  |
3 | from gauge.bar import progress_bar
  | ^
```

This is deliberate. A workspace is about building packages together, and a
dependency is about one package using another. Keeping them separate means
you can read any one package's manifest and see everything it uses, and that
`gauge` stays usable on its own when someone outside the workspace adds it as
a git dependency. Membership is also listed by exact path; there are no glob
patterns such as `"libs/*"`, so every member is named in the root manifest.

## Summary

You can now grow an Aura program past one file. A module is one file, named
by its path under `src/`, and other files reach it with `import` or `from
... import ...`. Everything in a module is private unless it's marked
`public`, which lets you keep a class's fields private and offer methods that
protect its invariants, as `Book.read_to` does for the bookmark. Imports bring
in declarations only, module constants are set once before `main` runs and
never change, and mutable state belongs to a value with an owner.

Packages carry those ideas across projects. A package is a directory with an
`Aura.toml` and a `src/`, and its name is the root of every import other
packages write. Dependencies come from a path or a git repository, the
lockfile pins exactly what was resolved, and a workspace lets several packages
share one lockfile without letting them use each other by accident. The
[Manual](/manual/packages) has the complete rules for manifests, lockfiles,
and dependency resolution, and the [Names and scopes](/manual/names-and-scopes)
page covers imports and visibility in full.

The reading list stores its books in a `list`, and the `gauge` package builds
strings by repeating characters. In the [next
chapter](/book/ch08-00-collections) we'll look closely at those two types and
at their relatives, dictionaries and sets, which are the containers most
Aura programs are built from.
