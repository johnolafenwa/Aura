# Copies and slices

Borrowing covers most of what functions need, but sometimes you really do
want two independent values. You want to keep yesterday's scores while you
change today's, or keep a name in a list and also store it in a record. For
those cases Aura makes copies, and it makes them only when you ask. Asking
takes a different spelling for different types, and every copy has a cost
that grows with the value, so both are worth knowing before you reach for
one.

Let's start with an error that looks like it should not be one. We have a
list of names, and we want the first:

*Filename: src/main.au*

```aura check-fail:AU3005
def main():
    names = ["Ada", "Grace", "Margaret"]
    first = names[0]
    print(first)
```

<span class="caption">Listing 4-13: Binding a list element to a new name</span>

```console
$ aura check src/main.au
error[AU3005]: cannot implicitly copy `str` out of a list index; use `view value = items[index]` for shared access, `items[index].clone()` for an explicit copy, or `pop(index)` or `set(index, value)` to transfer ownership
 --> src/main.au:3:13
  |
3 |     first = names[0]
  |             ^
```

In Python, `first = names[0]` makes `first` a second name for the string in
the list, and that's that. In Aura, `first = ...` creates a new owner, and the
string already has one: the list. The compiler won't quietly duplicate the
string, and it won't quietly pull it out of the list either, because either
choice would surprise someone. So it lists the options and asks you to pick
one:

- `view value = items[index]` gives you shared access to the element where it
  sits, without copying it or taking it out.
- `items[index].clone()` makes a new string with its own owner.
- `pop(index)` or `set(index, value)` takes the element out of the list and
  gives you ownership of it, leaving the list one element shorter or holding
  a replacement.

Here are the first two side by side:

*Filename: src/main.au*

```aura
def main():
    names = ["Ada", "Grace", "Margaret"]
    first = names[0].clone()
    view also_first = names[0]
    print(first)
    print(also_first.len())
    print(names)
```

<span class="caption">Listing 4-14: Cloning an element, and viewing one in place</span>

```console
$ aura run src/main.au
Ada
3
[Ada, Grace, Margaret]
```

`first` is a new string, owned by `main`, which we could change, store, or
return without affecting the list. `also_first` is a *view*: another name for
the string inside the list, with shared access. A view costs nothing to make,
and the compiler makes sure the list doesn't change underneath it while the
view is in use. If all you need is to read the element, a view is the better
choice. Most of the time you won't even need the `view` keyword, because
using `names[0]` directly as an argument, as in `print(names[0])`, lends the
element for that one call. The [Manual](/manual/ownership-and-borrowing) has
the full rules for views, including `view mut` for changing an element in
place.

## Making copies with `.copy()` and `.clone()`

When you need a second owner, the operation that makes one depends on the
type. Lists, dictionaries, and sets have `.copy()`. Strings have `.clone()`.
The two spellings come from the compiler itself, and its diagnostics always
suggest the right one for the type in front of them. If you mix them up, it
tells you:

```aura check-fail:AU2999
def main():
    name = "Ada"
    other = name.copy()
    print(other)
```

```console
$ aura check src/main.au
error[AU2999]: unsupported method call `copy` on `str`
 --> src/main.au:3:18
  |
3 |     other = name.copy()
  |                  ^
```

A copy of a collection is fully independent. It gets its own storage, and
every element is copied or cloned into it, so changing one list never shows
up in the other:

*Filename: src/main.au*

```aura
def main():
    mut today = [70, 85, 92]
    yesterday = today.copy()
    today.append(64)
    print(today)
    print(yesterday)
```

<span class="caption">Listing 4-15: A copied list does not see changes to the original</span>

```console
$ aura run src/main.au
[70, 85, 92, 64]
[70, 85, 92]
```

If you have used Python's `list.copy()`, notice the difference. Python's copy
is *shallow*: the new list holds the same element objects as the old one, so
a list of lists or a list of objects still shares its contents, and you reach
for `copy.deepcopy` when that bites. Aura's copy goes all the way down. A list
of class instances copies each instance and each of their fields:

*Filename: src/main.au*

```aura
class Player:
    name: str
    score: int64

def main():
    team = [Player(name="Ada", score=90), Player(name="Grace", score=88)]
    mut backup = team.copy()
    backup[0].score = 10
    print(team[0].score)
    print(backup[0].score)
```

```console
$ aura run src/main.au
90
10
```

The two lists can't share anything, because each element can only have one
owner. A shallow copy would give every player two owners, which is exactly
what ownership rules out.

Your own classes don't come with a copy method. When you want one, write it,
copying each field the way its type expects:

*Filename: src/main.au*

```aura
class Player:
    name: str
    score: int64

    def copy(self) -> Player:
        return Player(name=self.name.clone(), score=self.score)

def main():
    ada = Player(name="Ada", score=90)
    mut twin = ada.copy()
    twin.score = 95
    print(ada)
    print(twin)
```

<span class="caption">Listing 4-16: A hand-written `copy` method on a class</span>

```console
$ aura run src/main.au
Player(name=Ada, score=90)
Player(name=Ada, score=95)
```

`self.name.clone()` makes a new string, and `self.score` copies because it is
an `int64`. Printing a class instance shows its fields by name. (The fields
come out in alphabetical order, which is the default rendering for a class.
[Chapter 5](/book/ch05-00-classes) shows how to choose your own.)

### Some values can't be copied

A few types refuse to be copied at all. A random number generator,
`random.Rng`, is the standard example: two generators with the same internal
state would produce the same "random" numbers, which is a classic source of
subtle bugs in simulations and tests. So `Rng` has no copy, and neither does
anything that contains one:

```aura check-fail:AU3007
import random

def main():
    gens = [random.Rng(seed=1)]
    more = gens.copy()
    print(more.len())
```

```console
$ aura check src/main.au
error[AU3007]: cannot use `list.copy` because `list[random.Rng]` contains non-cloneable `random.Rng` state
 --> src/main.au:5:17
  |
5 |     more = gens.copy()
  |                 ^
  = help: move or remove the value so it has one owner, or construct an independent generator with an explicit seed
```

The help line is the design in one sentence: if you need a second generator,
make a new one with its own seed, so that the program says plainly where each
stream of numbers comes from.

## Slices are copies too

A *slice* picks out a range of a list or a string with `start:end`, the same
syntax Python uses. In Aura a slice is always a fresh owned value. It copies
the selected elements into new storage, and it never refers back to the
original:

*Filename: src/main.au*

```aura
def main():
    names = ["Ada", "Grace", "Margaret"]
    mut rest = names[1:]
    rest.append("Katherine")
    print(rest)
    print(names)
    word = "ownership"
    print(word[0:3])
    print("A🎉Z"[1:2])
```

<span class="caption">Listing 4-17: List and string slices produce new owned values</span>

```console
$ aura run src/main.au
[Grace, Margaret, Katherine]
[Ada, Grace, Margaret]
own
🎉
```

`rest` started as a copy of the last two names, and appending to it left
`names` unchanged. The string slices work the same way: `word[0:3]` is a new
three-character string. String positions count Unicode characters, not bytes,
which is why `"A🎉Z"[1:2]` gives the whole emoji rather than a piece of its
UTF-8 encoding. We'll come back to how strings are stored in
[Chapter 8](/book/ch08-00-collections).

This matches Python, where `names[1:]` also builds a new list. One Python
habit doesn't carry over, though. Python lets you assign to a slice to
replace part of a list in place. Since an Aura slice is a separate value,
assigning to one would change nothing, so the compiler refuses:

```aura check-fail:AU2005
def main():
    names = ["Ada", "Grace", "Margaret"]
    names[1:] = ["Grace"]
```

```console
$ aura check src/main.au
error[AU2005]: slice assignment is unavailable because slices are owned copies; mutate the source by index or build a new value
 --> src/main.au:3:12
  |
3 |     names[1:] = ["Grace"]
  |            ^
```

Since a slice copies its elements, slicing a list of `random.Rng` values is
rejected with the same `AU3007` error as `.copy()`.

## What copying costs

Every `.copy()`, `.clone()`, and slice allocates new memory and copies into
it, so the cost grows with the size of the value. Copying a list of ten
numbers is nothing. Copying a list of a million strings means a million
allocations. That is why Aura never copies an owned value on its own: every
copy appears in the source as a method call or a slice, where you can see it
and decide whether it is worth it.

Python makes the opposite trade. Assignment never copies, so it is always
cheap, but two names can share a value without either of them knowing, and
the cost shows up later as reference counting on every assignment and as
bugs where a change in one place appears in another.

A few habits keep copying under control in Aura:

1. Borrow first. A shared or `mut` parameter costs nothing, and it is what
   most functions want.
2. Copy close to the reason. When you do clone, do it at the line where you
   need the second owner, so a reader can see why it is there.
3. Move when you are finished. If the caller doesn't need a value after the
   call, an `own` parameter hands it over without any copying at all.

Copy values such as integers, floats, and `bool` are the exception. They are
copied on every assignment, because copying them is as cheap as reading them,
and there is no separate memory behind them to share.

## Summary

Ownership is how Aura manages memory and resources without a garbage
collector. Each value has one owner, assigning or passing an owned value moves
it unless the function only borrows it, and the value is cleaned up at the
end of its owner's scope. The compiler checks all of this before the program
runs, and its errors name both the place you used a value and the place it
moved. You can now read a signature and know what a function does with each
argument: a bare type borrows for reading, `mut` borrows for changing, and
`own` takes the value over. You also know when to reach for `.copy()`,
`.clone()`, a slice, or a view, and what each one costs.

These rules are going to show up everywhere from here on. In the next
chapter we'll use them to build our own types with classes, where fields,
constructors, and the three kinds of `self` all follow the patterns you've
learned here.
