# Case Study: A Log Analyzer

This case study builds a small program that turns free-form log text into a structured report. It leaves out regular expressions, command-line flags, and databases on purpose. The point is to watch text enter the program and become typed data.

## The Input

The program reads lines like these:

```
INFO api started
WARN api slow
ERROR worker failed
INFO worker recovered
```

The report answers three questions:

- How many entries of each severity arrived?
- Which services appeared?
- How many lines could not be parsed?

## Step 1: Model The Parsed Line

A valid log entry has three parts: a level, a service, and a message. A class with three fields says exactly that:

```aura
class LogLine:
    level: str
    service: str
    message: str
```

`message` keeps the original text, so later code can print or inspect the unmodified line.

## Step 2: Parse One Line

A line that cannot be parsed is not a crash. It is an expected, countable absence of data, so the return type is `LogLine | None`:

```aura
def parse_line(line: str) -> LogLine | None:
    clean = line.trim()
    parts = clean.split(" ")

    if parts.len() < 3:
        return None

    level = match own parts.get(0):
        case Lookup.Found(value):
            value
        case Lookup.Missing:
            "UNKNOWN"

    service = match own parts.get(1):
        case Lookup.Found(value):
            value
        case Lookup.Missing:
            "unknown"

    return LogLine(level=level, service=service, message=clean)
```

- `return None` selects the absence member of `LogLine | None`. The final `return` puts the new `LogLine` into the union without a wrapper.
- `parts.get(0)` returns `Lookup[str]`. `match own` moves the found string out of that result, so `level` owns it.
- The `parts.len() < 3` guard makes the two fallback arms unreachable. The exhaustive `match` is still cheap insurance. If the parser later accepts quoted strings or nested fields, that shape makes the change hard to get wrong.

## Step 3: Count With A dict

Counting uses a dictionary from string to integer. The helper is small:

```aura
def increment(counts: mut dict[str, int32], key: own str):
    current = match counts.get(key):
        case Lookup.Found(value):
            value
        case Lookup.Missing:
            0

    counts[key] = current + 1
```

The signature shows the ownership detail. `dict.get` borrows `key`. The indexed assignment then consumes it, because the dictionary keeps the key. No clone is needed.

## Step 4: The Report

Now put the pieces together:

```aura
lines = ["INFO api started", "WARN api slow", "ERROR worker failed", "INFO worker recovered", "badline"]

mut levels = dict[str, int32]()
mut services = set[str]()
mut skipped = 0

for line in lines:
    match own parse_line(line):
        case LogLine as entry:
            increment(levels, entry.level.clone())
            services.add(entry.service)
        case None:
            skipped += 1

print("levels")
for level, count in levels.items():
    print("  " + level + ": " + count.to_string())

print("services: " + services.len().to_string())
print("skipped: " + skipped.to_string())
```

The type pattern `LogLine as entry` selects a parsed line, and `None` selects a skipped one. `match own` consumes the parsed value. The arm can then move `entry.service` into the set and clone only the level.

The three report variables sit at the top of the loop, with no hidden state:

- `levels` counts known severities.
- `services` removes duplicate service names.
- `skipped` counts lines that failed to parse.

When a run prints the wrong output, the cause is almost always visible in those three bindings.

## Why This Shape Scales

Nothing in the program assumes an input size. Reading from a file instead of an inline list changes only the start:

```aura
import fs

text = try fs.read_to_string("app.log")
lines = text.split("\n")
```

The parser still returns `LogLine | None`. The counter still changes a dictionary its caller owns. The set still owns the service names.

That is the case for typing data at the boundary. When the input source changes, the core of the program stays where it was.

## Extensions To Try

The analyzer is small on purpose. Some ways to extend it:

- Add a `dict[str, int32]` that counts services as well as levels.
- Turn `level` into an enum, such as `LogLevel.Info`, `LogLevel.Warn`, and `LogLevel.Error`, and treat unknown levels as skipped.
- Print the most frequent service by iterating over `services` and looking up counts.
- Read from standard input with `io.read_line()` in a loop.

Each extension should fit the existing structure without rearranging it.
