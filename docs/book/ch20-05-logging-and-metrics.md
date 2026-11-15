# Logging, tracing, and metrics

A program that runs for a second can explain itself with `print`. A program
that runs for a month, on a machine nobody is watching, needs to leave a
record that another program can search, and to keep a few numbers that say
how it is doing. The last three modules of our tour are the smallest in the
library: `log` and `trace` write structured records, and `metrics` counts.

## Structured log records

Let's give `courier` a function that handles one incoming envelope and logs
what it did. A log call takes a message and a dictionary of fields. The
first attempt passes the id straight in:

```aura check-fail:AU3002
import log

def handle(id: str, size: int64) -> bool:
    if size > 4096:
        log.warn("envelope too large", {"id": id, "bytes": f"{size}"})
        return false
    log.info("delivered", {"id": id, "bytes": f"{size}"})
    return true

def main():
    _ = handle("7f3a", 512)
```

```console
$ aura check src/main.au
error[AU3002]: parameter `id` is borrowed; declare it as `own str` to take ownership, or clone the value before consuming it
 --> src/main.au:5:47
  |
5 |         log.warn("envelope too large", {"id": id, "bytes": f"{size}"})
  |                                               ^
  = related src/main.au:3:12: parameter `id` is borrowed here
  = help: declare the parameter as `own str` when the function should consume it, or call `.clone()` to consume an independent copy
  = fix: replace src/main.au:5:49-5:49 with `.clone()`
```

This has nothing to do with logging and everything to do with
[Chapter 8](/book/ch08-03-dicts-and-sets): a dictionary owns its values, so
building `{"id": id}` would move `id` into the dictionary, and `handle` only
has shared access to it. The fields of a log record are a `dict[str, str]`
like any other. The fix the compiler offers is the right one. Clone the
string, and format numbers into strings with an f-string:

*Filename: src/main.au*

```aura
import log

def handle(id: str, size: int64) -> bool:
    if size > 4096:
        log.warn("envelope too large", {"id": id.clone(), "bytes": f"{size}"})
        return false
    log.info("delivered", {"id": id.clone(), "bytes": f"{size}"})
    return true

def main():
    print(handle("7f3a", 512))
    print(handle("c04d", 20480))
```

<span class="caption">Listing 20-13: Logging with a message and fields</span>

```console
$ aura run src/main.au
{"fields":{"bytes":"512","id":"7f3a"},"kind":"log","level":"info","message":"delivered"}
true
{"fields":{"bytes":"20480","id":"c04d"},"kind":"log","level":"warn","message":"envelope too large"}
false
```

Each call wrote one line of JSON. The record has four keys: `kind` is
`"log"`, `level` is the function you called (`debug`, `info`, `warn`, or
`error`), `message` is the first argument, and `fields` holds the
dictionary. The message is a fixed phrase and the variable parts go in
fields. That is the habit that makes logs searchable: "every record whose
message is `delivered` and whose `bytes` is over 1000" is a question a tool
can answer, where `"delivered 7f3a (512 bytes)"` would need a regular
expression for every phrasing.

The lines look interleaved with the program's output, but they went
somewhere else. `print` writes to standard output and log records go to
standard error, so the two can be separated when the program is started:

```console
$ aura run src/main.au 2> courier.log
true
false
$ cat courier.log
{"fields":{"bytes":"512","id":"7f3a"},"kind":"log","level":"info","message":"delivered"}
{"fields":{"bytes":"20480","id":"c04d"},"kind":"log","level":"warn","message":"envelope too large"}
```

That split is the whole delivery mechanism. A service manager, a container
runtime, or a shell redirection collects standard error, and because every
line is a JSON object, tools such as `jq` or a log pipeline can filter it
without parsing free text. One line per record also means a record is never
split across lines, even when the message contains a newline, because JSON
escapes it.

Two things you might expect are missing. A record has no timestamp. Most
collectors add their own, and when yours does not, put one in a field:
`{"at": time.now().to_rfc3339()}`. And there is no level filter. Every call
writes, including `log.debug`. If you want debug records only when asked,
wrap the call:

```aura
import log
import sys

def debug(message: str, fields: own dict[str, str]):
    if sys.env("COURIER_DEBUG") is not None:
        log.debug(message, fields)

def main():
    debug("starting", {"mode": "test"})
    print("done")
```

```console
$ aura run src/main.au
done
$ COURIER_DEBUG=1 aura run src/main.au
{"fields":{"mode":"test"},"kind":"log","level":"debug","message":"starting"}
done
```

`sys.env` returns the value of an environment variable, or `None` when it
is not set.

## Trace events

`trace.event(name, fields)` writes the same kind of line with `kind` set to
`"trace"`. Use it for the milestones of a program's life as opposed to
things that happened to a particular item: started, loaded configuration,
began shutting down.

```aura
import trace

def main():
    trace.event("courier.start", {"version": "2", "mode": "receive"})
    trace.event("courier.stop", {"reason": "end of input"})
```

```console
$ aura run src/main.au
{"fields":{"mode":"receive","version":"2"},"kind":"trace","level":"event","message":"courier.start"}
{"fields":{"reason":"end of input"},"kind":"trace","level":"event","message":"courier.stop"}
```

Keeping the two kinds apart lets a reader pull out the timeline of the
process with one filter on `kind`. That is all `trace` is in Aura 0.16: a
single event. There are no spans with a start and an end, and no trace ids
that follow a request between services. To time a step, read
`time.monotonic()` before and after and put the elapsed milliseconds in a
field.

## Counting with metrics

Logs describe individual events. Often the question is about totals: how
many envelopes, how many refused. The `metrics` module keeps named `int64`
counters for the whole process:

*Filename: src/main.au*

```aura
import log
import metrics

def handle(id: str, size: int64) -> bool:
    metrics.increment("envelopes", 1)
    if size > 4096:
        metrics.increment("rejected", 1)
        log.warn("envelope too large", {"id": id.clone(), "bytes": f"{size}"})
        return false
    metrics.increment("bytes", size)
    return true

def worker(name: str, count: int64) -> int64:
    for i in range(count):
        _ = handle(f"{name}-{i}", 100)
    return count

def main():
    _ = handle("7f3a", 512)
    _ = handle("c04d", 20480)
    with TaskGroup() as group:
        _ = group.start(worker, "east", 2)
        _ = group.start(worker, "west", 2)

    print(f"envelopes: {metrics.get("envelopes")}")
    print(f"rejected:  {metrics.get("rejected")}")
    print(f"bytes:     {metrics.get("bytes")}")
    print(f"replays:   {metrics.get("replays")}")
```

<span class="caption">Listing 20-14: Counters shared by every task in the process</span>

```console
$ aura run src/main.au 2> courier.log
envelopes: 6
rejected:  1
bytes:     912
replays:   0
```

`metrics.increment(name, amount)` adds to a counter, creating it at zero on
first use. `metrics.get(name)` reads it, and a counter that was never
incremented reads as zero, which is why `replays` printed `0` without
being declared anywhere. `metrics.reset()` clears them all, which is mostly
useful at the start of a test.

The counters belong to the process, not to a task. `main` handled two
envelopes itself and two worker tasks handled two each, and all six landed
in the same `envelopes` counter. This is one of the very few pieces of
state in Aura that tasks share without a queue, and it is safe because each
`increment` and each `get` is atomic on its own. A sequence is not: if two
tasks each do `get` and then `increment` based on what they read, they can
interleave. Keep to plain increments and you will not notice.

The amount can be negative, so a counter can also track something that goes
up and down, such as envelopes currently being processed. Pushing a counter
past the range of `int64` is a trap, like any integer overflow in Aura.

What `metrics` does not have is a way out of the process. There is no
exporter, no HTTP endpoint that a monitoring system could scrape, and no
labels on a counter. In Aura 0.16 you publish the numbers yourself. The
simplest ways are the two we have already seen: print a summary when the
program ends, as Listing 20-14 does, or write the counters as fields of a
log record every minute and let the log pipeline do the rest. A service
that already has the HTTP server from
[Chapter 16](/book/ch16-03-networking) can answer a `/metrics` path with
them.

## Summary

That completes the tour, and `courier`. Its envelopes are compressed with
`gzip`, signed with `bytes.hmac_sha256`, and carried as base64 and hex. They
are stamped with a `time.DateTime` and refused when they are stale. Each has
an id from `random.secure_bytes`, the tool's log can be taken apart with a
`regex.Regex`, and a running copy reports on itself through `log`, `trace`,
and `metrics`.

The modules share the habits of the rest of the library. What can fail on
bad input returns a `Result` with an error that says where the input went
wrong: a byte offset, a field, a position in a pattern. What can only fail
through a programming mistake traps. Decoders are strict, sizes are
bounded, and the choice with security consequences (which kind of random
number, which comparison) is spelled out in the name of the function you
call. They are small modules, and we have tried to be plain about where
each one stops.

The Manual has a page for each: [bytes](/manual/bytes),
[gzip](/manual/gzip), [time](/manual/time), [regex](/manual/regex),
[randomness](/manual/randomness), and the
[control-plane modules](/manual/control-plane) for `log`, `trace`,
`metrics`, and `sys`.
