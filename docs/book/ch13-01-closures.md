# Closures

Let's start from a question we could ask of the log in the last chapter: which
requests were slow? Suppose we already have the durations, in milliseconds, in
a list. Finding the slow ones is a filter, and a list's `filter` method takes a
function that says yes or no for each element.

## Functions are values

We can hand `filter` an ordinary named function:

*Filename: slow.au*

```aura
def is_slow(ms: int64) -> bool:
    return ms > 100

def is_tiny(ms: int64) -> bool:
    return ms < 10

def main():
    durations = [12, 1840, 35, 410, 7]
    print(durations.filter(is_slow))
    check: def(ms: int64) -> bool = is_slow
    print(check(250))
    checks: list[def(ms: int64) -> bool] = [is_slow, is_tiny]
    for keep in checks:
        print(durations.filter(keep))
```

<span class="caption">Listing 13-1: Passing, storing, and calling named functions as values</span>

```console
$ aura run slow.au
[1840, 410]
true
[1840, 410]
[7]
```

Writing `is_slow` without parentheses doesn't call it. It names the function as
a value, which we can pass to `filter`, store in a binding, or put in a list.
The type of that value is written the way the function was declared, with the
body and the name left out: `def(ms: int64) -> bool`. You can use a function
type anywhere a type is allowed, including a parameter, a class field, or a
list's element type as in `checks`.

The parameter name is part of the type. That may be surprising if you come
from languages where only the parameter types matter, so let's see what
happens if we leave it out:

```aura check-fail:AU2015
def is_slow(ms: int64) -> bool:
    return ms > 100

def main():
    check: def(int64) -> bool = is_slow
    print(check(250))
```

```console
$ aura run slow.au
error[AU2015]: callable contract mismatch: positional parameter 1 differs from parameter `ms` in its name; declare `type Adapter = def(int64) -> bool` and use `Adapter(is_slow)`
 --> slow.au:5:33
  |
5 |     check: def(int64) -> bool = is_slow
  |                                 ^
  = help: preserve identical complete contracts at bare destinations; apply safe restrictions through an explicit thin alias or owned callable constructor such as `Alias(function)`
```

Aura calls the full description of how a function may be called its
*contract*: the parameter names, their modes and types, any defaults, and the
result. `is_slow` can be called as `is_slow(ms=250)`, but a `def(int64) -> bool`
could only be called positionally, so storing one as the other would quietly
throw away part of the contract. The compiler won't do that implicitly. If you
want a value without the name, the message tells you how to ask for it: define
`type Adapter = def(int64) -> bool` and write `Adapter(is_slow)`. In practice
you usually don't need to write the type at all. `check = is_slow` infers the
complete contract, names and all.

Named functions like these are *thin* function values: a pointer to code, with
nothing else attached. They are copy values, so passing one around never moves
anything, and they can go anywhere a value can go, including across tasks.

## Capturing the limit

`is_slow` has its limit, 100, written into its body. In a real tool the limit
comes from the user. A named function at module level can't see the local
bindings of `main`, so it has no way to read a `limit` chosen at run time.
This is the job of a closure:

*Filename: slow.au*

```aura
def main():
    durations = [12, 1840, 35, 410, 7]
    limit = 30
    print(durations.filter(lambda ms: ms > limit))
    print(durations.map(lambda ms: f"{ms}ms"))
    print(sorted(durations, key=lambda ms: -ms))
```

<span class="caption">Listing 13-2: Lambdas as callbacks, one of them capturing `limit`</span>

```console
$ aura run slow.au
[1840, 35, 410]
[12ms, 1840ms, 35ms, 410ms, 7ms]
[1840, 410, 35, 12, 7]
```

`lambda ms: ms > limit` is a function value with one parameter, `ms`, and a body
of one expression. The body mentions `limit`, which is not a parameter, so the
lambda *captures* it from the surrounding scope. `map` builds a new list from
each element's result, and `sorted` uses the lambda as a key, here sorting by
the negated duration to get the slowest first.

An Aura lambda's body is always exactly one expression. There are no statement
bodies and no `return`. When a callback needs more than one step, write a named
function, and pass the closure's captured values to it as arguments; we'll do
that with decorators in the next section.

## Where a lambda's types come from

The lambdas above never say that `ms` is an `int64`. They don't have to,
because each one is passed straight to a method that knows what it will pass
in. Take the lambda out of the call and give it a name, and that context is
gone:

```aura check-fail:AU2002
def main():
    limit = 30
    is_slow = lambda ms: ms > limit
    print(is_slow(150))
```

```console
$ aura check slow.au
error[AU2002]: lambda parameter types require an expected `def(...) -> ...` context
 --> slow.au:3:15
  |
3 |     is_slow = lambda ms: ms > limit
  |               ^
  = help: add a `def(...) -> ...` annotation to the immutable local, or pass the lambda directly to a callback parameter
```

Aura doesn't guess a parameter's type from how the body uses it. `ms > limit`
would work for several numeric types, and a guess that is right today can
become wrong when someone edits the body. So a lambda with parameters needs an
expected function type, either from the callback it is passed to or from an
annotation. Lambda parameters themselves can't carry type annotations; the type
always comes from outside. (A lambda with no parameters, such as
`lambda: limit * 2`, can infer its result from the body, because there is
nothing to guess about its inputs.)

Add the annotation the help line suggests:

```aura
def main():
    limit = 30
    is_slow: def(int64) -> bool = lambda ms: ms > limit
    print(is_slow(150))
    print(is_slow(20))
    print(limit)
```

```console
$ aura run slow.au
true
false
30
```

## What a capture does with its value

Notice the last line of that output. `limit` is still usable after the
closure captured it. That's because `limit` is an `int64`, a copy value, and a
lambda without a capture list copies each copy value it uses at the moment the
lambda is created. The closure holds its own 30, independent of the binding
outside.

An owned value can't be copied implicitly, so a lambda captures it by moving it
in. Here is a filter that keeps lines from one service:

```aura check-fail:AU3001
def main():
    service = "db"
    from_service: def(str) -> bool = lambda name: name == service
    print(from_service("db"))
    print(service)
```

```console
$ aura check slow.au
error[AU3001]: use of moved value `service`
 --> slow.au:5:11
  |
5 |     print(service)
  |           ^
  = related slow.au:3:59: value moved into closure here
  = help: pass shared access when ownership is not needed, or call `.clone()` at the move site when an independent value is required
  = fix: replace slow.au:3:66-3:66 with `.clone()`
```

This is the same move error as passing `service` to an `own` parameter, and the
`related` line points at the place inside the lambda where the move happened.
The closure now owns the string, so `main` can't print it.

Aura moves by default, rather than borrowing, because a closure is a value,
and values travel. A closure that borrowed `service` would be tied to the scope
where `service` lives, and it would become invalid the moment that scope ended
if it had been returned or stored somewhere. A closure that owns its captures
can go anywhere. Python makes the other choice, capturing a reference to the
variable, which is why a Python lambda created in a loop famously sees the
loop variable's *last* value. In Aura a capture is taken when the lambda is
created, so that surprise can't happen.

There are two ways to keep `service` usable. The help line offers one: clone
the string, so the closure gets its own copy. The other is to say explicitly
that the closure should borrow, with a *capture list* in square brackets after
`lambda`:

```aura
def main():
    service = "db"
    from_service: def(str) -> bool = lambda [service] name: name == service
    print(from_service("db"))
    print(from_service("api"))
    print(service)
```

<span class="caption">Listing 13-3: Borrowing a capture with an explicit capture list</span>

```console
$ aura run slow.au
true
false
db
```

A capture list names every outer binding the body uses, and the form of each
entry sets how it is captured:

- `name` borrows the binding with shared access, for as long as the closure is
  in use.
- `mut name` borrows it with mutable access, so the closure can change it.
- `own name` copies a copy value or moves an owned one, which is what a lambda
  without a list does.

Once you write a list, it must be complete. A name the body uses but the list
leaves out is an error, and so is a listed name the body never uses. That
keeps the list honest as a description of what the closure touches. A
borrowing closure is tied to the scope of what it borrows, so it can't be
stored in a long-lived place or returned; we'll see the owned alternative for
that shortly.

## Call once, call many, or call and change

What a closure does with its captures when it runs decides how often it can be
called. Aura sorts every closure into one of three *call kinds*.

A closure whose body only reads its captures is *shared*: it can be called any
number of times, through any binding. Every closure we've written so far is
shared.

A closure whose body gives an owned capture away can run only once, because
after the first call it no longer has the value. This one returns its captured
string:

```aura check-fail:AU3001
def main():
    banner = "slow requests"
    take_banner = lambda: banner
    print(take_banner())
    print(take_banner())
```

```console
$ aura check slow.au
error[AU3001]: use of moved value `take_banner`
 --> slow.au:5:11
  |
5 |     print(take_banner())
  |           ^
  = related slow.au:4:11: value moved here
  = help: pass shared access when ownership is not needed, or transfer this non-cloneable value only once
```

The first call moved the string out of the closure and handed it to `print`,
so calling the closure consumed the closure itself, and the second call is a
use after a move. This is a *consuming* closure. It is useful for work that
should happen exactly once, such as handing a finished result to its new
owner, and the compiler guarantees it does happen at most once.

The third kind changes its captures. Here a closure owns a counter and bumps
it on each call:

```aura check-fail:AU3003
class Counter:
    total: int64

    def bump(mut self) -> int64:
        self.total += 1
        return self.total

def main():
    counter = Counter(total=0)
    next_id = lambda [own counter]: counter.bump()
    print(next_id())
```

```console
$ aura check slow.au
error[AU3003]: a mutable-repeatable closure must be called through a mutable closure place
 --> slow.au:11:11
   |
11 |     print(next_id())
   |           ^
  = help: store the closure in a `mut` local before calling it
```

Calling `next_id` changes the counter it owns, so calling it changes
`next_id`. The rule for that is the one we know from methods: changing a value
requires a `mut` binding. Write `mut next_id = ...` and the closure keeps its
counter between calls:

```aura
class Counter:
    total: int64

    def bump(mut self) -> int64:
        self.total += 1
        return self.total

def main():
    counter = Counter(total=0)
    mut next_id = lambda [own counter]: counter.bump()
    print(next_id())
    print(next_id())
    print(next_id())
```

<span class="caption">Listing 13-4: A mutable closure that owns its state</span>

```console
$ aura run slow.au
1
2
3
```

The counter moved into the closure when we wrote `[own counter]`, and from then
on it lives in the closure's environment. A Python programmer would build this
with a class or with `nonlocal`; here it is one line, and the `mut` on the
binding tells the reader that calls have an effect.

A closure can also change a binding outside itself, through a `mut` entry in
the capture list. That is a mutable loan, and it follows the same rule as any
mutable loan: while it is live, nothing else may touch the borrowed value.

```aura check-fail:AU3002
def main():
    durations = [12, 1840, 35, 410, 7]
    mut slow: list[int64] = []
    mut keep: def(int64) -> None = lambda [mut slow] ms: slow.append(ms)
    keep(1840)
    print(slow)
    keep(410)
```

```console
$ aura check slow.au
error[AU3002]: cannot read `slow` while mutable view `keep` remains live; use the view instead
 --> slow.au:6:11
  |
6 |     print(slow)
  |           ^
  = related slow.au:4:48: mutable view `keep` starts here
  = related slow.au:7:5: last use of view `keep` keeps this loan live
```

The two `related` lines mark the span of the loan: it starts where the closure
is created and lasts until its last call on line 7. Reading `slow` in the
middle of that span would mean reading a list while something else holds the
right to change it. Finish using the closure first, and the loan ends in time:

```aura
def main():
    durations = [12, 1840, 35, 410, 7]
    mut slow: list[int64] = []
    mut keep: def(int64) -> None = lambda [mut slow] ms: slow.append(ms)
    for ms in durations:
        if ms > 100:
            keep(ms)
    print(slow)
```

```console
$ aura run slow.au
[1840, 410]
```

## Passing closures to your own functions

Built-in callback sites such as `filter`, `map`, and `sorted` accept capturing
closures directly. Your own functions are a different matter. Let's write a
`count_where` that takes a predicate:

```aura check-fail:AU2002
def count_where(values: list[int64], keep: def(int64) -> bool) -> int64:
    mut total = 0
    for value in values:
        if keep(value):
            total += 1
    return total

def main():
    durations = [12, 1840, 35, 410, 7]
    print(count_where(durations, lambda ms: ms > 100))
    limit = 30
    print(count_where(durations, lambda ms: ms > limit))
```

```console
$ aura check slow.au
error[AU2002]: argument type mismatch for function `count_where`: expected `def(int64) -> bool`, found `closure def(int64) -> bool`
 --> slow.au:12:34
   |
12 |     print(count_where(durations, lambda ms: ms > limit))
   |                                  ^
```

The first call is accepted and the second is not. The difference is the
capture. `lambda ms: ms > 100` captures nothing, so it is a thin function
value like `is_slow`. `lambda ms: ms > limit` carries an environment holding
its copy of `limit`, and a `def(...)` parameter has room for a code pointer
only. The message spells the difference as `def(int64) -> bool` versus
`closure def(int64) -> bool`.

To accept closures with captures, a function says so with an owned callable
type, `Callable[def(...) -> R]`. It is usually given a short name with `type`:

*Filename: slow.au*

```aura
type Check = Callable[def(int64) -> bool]

def count_where(values: list[int64], keep: Check) -> int64:
    mut total = 0
    for value in values:
        if keep(value):
            total += 1
    return total

def slower_than(limit: own int64) -> Check:
    return Check(lambda ms: ms > limit)

def main():
    durations = [12, 1840, 35, 410, 7]
    over_a_second = slower_than(1000)
    print(count_where(durations, over_a_second))
    print(count_where(durations, slower_than(30)))
    print(durations.filter(over_a_second))
```

<span class="caption">Listing 13-5: Returning and passing closures as `Callable` values</span>

```console
$ aura run slow.au
1
3
[1840]
```

A `Check` holds a closure together with its environment. You make one by
calling the type with a lambda, `Check(lambda ms: ...)`, which is called
*packing*. Packing is always written out. Assigning a bare lambda to a `Check`
binding is rejected with `AU2015` and a message saying that a lambda becomes a
`Callable` only through the constructor. The reason is that a packed closure
is a different kind of value from a thin one: it owns an environment, it is
not a copy value, and it can't be cloned. Aura wants that change visible where
it happens.

Once packed, the closure can go where thin function values go: into
parameters like `keep`, out of functions like `slower_than`, into fields and
lists. `slower_than` is a function that builds functions, and each one it
returns remembers its own limit. `count_where` borrows `over_a_second` with a
shared parameter, so we can use it again afterward, and `filter` accepts it
too.

`slower_than` takes its parameter as `own int64`. Try it with a bare `limit:
int64` and the compiler explains why:

```aura check-fail:AU3002
type Check = Callable[def(int64) -> bool]

def slower_than(limit: int64) -> Check:
    return Check(lambda ms: ms > limit)

def main():
    over_a_second = slower_than(1000)
    print(over_a_second(1840))
```

```console
$ aura check slow.au
error[AU3002]: lambda cannot capture shared parameter `limit` by value
 --> slow.au:4:34
  |
4 |     return Check(lambda ms: ms > limit)
  |                                  ^
  = related slow.au:3:17: shared parameter `limit` is declared here
  = help: clone `limit` into an owned local before creating the lambda, or declare the enclosing parameter as `own int64`
```

A bare parameter is shared access to a value the caller still owns, even when
the type is a copy value. A lambda without a capture list takes its captures
by value, and Aura doesn't let it turn a borrowed parameter into a value of its
own without saying so. Taking the parameter as `own` makes `slower_than` the
owner of its argument, and then the lambda is free to keep it.

## Closures over records

Closures are at their most useful with collections of class values. Here is a
list of requests, queried three ways:

*Filename: slow.au*

```aura check-fail:AU3002
class Request:
    service: str
    ms: int64

def main():
    requests = [
        Request(service="api", ms=12),
        Request(service="db", ms=1840),
        Request(service="api", ms=410),
    ]
    slowest = sorted(requests, key=lambda request: request.ms, reverse=true)
    print(slowest.map(lambda request: request.service))
```

```console
$ aura check slow.au
error[AU3002]: cannot move non-copy field `service` out of borrowed value `request`
 --> slow.au:12:47
   |
12 |     print(slowest.map(lambda request: request.service))
   |                                               ^
  = related slow.au:12:30: `request` is borrowed here
  = help: take `request` as `own Request` when the field should be moved, or call `.clone()` on the field to return an independent value
  = fix: replace slow.au:12:54-12:54 with `.clone()`
```

`map` passes each element to the callback with shared access, because the list
still owns its elements after `map` is done. Returning `request.service` would
move the string out of an element the list still owns. The key lambda,
`request.ms`, is fine, since an `int64` copies. This is the same rule we met
with `Summary.add` in the last chapter, and the same fix applies:

```aura
class Request:
    service: str
    ms: int64

def main():
    requests = [
        Request(service="api", ms=12),
        Request(service="db", ms=1840),
        Request(service="api", ms=410),
    ]
    wanted = "api"
    from_api = requests.filter(lambda request: request.service == wanted)
    print(from_api.len())
    slowest = sorted(requests, key=lambda request: request.ms, reverse=true)
    print(slowest.map(lambda request: request.service.clone()))
    print(sum(requests.map(lambda request: request.ms)))
```

<span class="caption">Listing 13-6: Filtering, sorting, and mapping a list of class values</span>

```console
$ aura run slow.au
2
[db, api, api]
2262
```

`filter` and `map` are eager: each returns a new list right away and leaves
the original in place. `filter` gives back copies of the elements it keeps, so
the list `requests` is still whole for the `sorted` and `sum` that follow.

## Choosing between a lambda and a named function

A lambda earns its place when the behavior is short and only makes sense next
to the call that uses it, as a sort key or a filter condition usually does.
When the logic needs a statement, a name, or a test of its own, a named
function is clearer, and you can still give it captured values by passing them
as arguments from a small lambda. The [Manual](/manual/closures) has the full
rules, including how closures cross into tasks, which [Chapter 15](/book/ch15-00-concurrency)
takes up.

The pattern from Listing 13-5, a function that takes some configuration and
returns a closure built from it, is more general than it looks. If the
configuration is itself a function, the result is a wrapped version of that
function. That is a decorator, and it is where we go next.
