# Case Study: A Log Analyzer

This case study walks end-to-end through a small program that turns free-form log text into a small structured report. It is deliberately modest — no regular expressions, no command-line flags, no database — because the point is to see how Aurora code grows as text enters the program and becomes typed data.

## The Input

The program will consume lines that look like this:

```
INFO api started
WARN api slow
ERROR worker failed
INFO worker recovered
```

The report it produces answers three questions: how many entries of each severity arrived, which services appeared, and how many lines could not be parsed.

## Step 1: Model The Parsed Line

A valid log entry has three pieces: a level, a service, and a message. A class with three fields says exactly that:

```python
class LogLine:
    level: String
    service: String
    message: String
```

The original text is kept in `message` so later code can print or inspect the unmodified line if it wants to.

## Step 2: Parse One Line

A line that cannot be parsed is not a crash. It is an expected, measurable absence of data. That makes `Option[LogLine]` the right return type:

```python
def parse_line(line: String) -> Option[LogLine]:
    clean = line.trim()
    parts = clean.split(" ")

    if parts.len() < 3:
        return Option.None

    level = match parts.get(0):
        case Option.Some(value):
            value
        case Option.None:
            "UNKNOWN"

    service = match parts.get(1):
        case Option.Some(value):
            value
        case Option.None:
            "unknown"

    return Option.Some(LogLine(level=level, service=service, message=clean))
```

The `parts.len() < 3` guard makes the two fallback arms unreachable, but keeping the `match` exhaustive is a cheap insurance policy. When the parser grows — a later revision might accept quoted strings or nested fields — the exhaustive shape makes the change hard to get wrong.

## Step 3: Count With A Map

Counting is a map from string to integer. The helper is deliberately small:

```python
def increment(counts: mut Map[String, int32], key: own String):
    current = match counts.get(key):
        case Option.Some(value):
            value
        case Option.None:
            0

    counts.set(key, current + 1)
```

The small ownership detail is now visible in the signature. `Map.get` borrows
`key`; `counts.set` then consumes it because the map retains the key. No clone
is needed.

## Step 4: The Report

Now pull the pieces together:

```python
lines = ["INFO api started", "WARN api slow", "ERROR worker failed", "INFO worker recovered", "badline"]

mut levels = Map[String, int32]()
mut services = Set[String]()
mut skipped = 0

for line in lines:
    match parse_line(line):
        case Option.Some(entry):
            increment(levels, entry.level.clone())
            services.insert(entry.service)
        case Option.None:
            skipped += 1

print("levels")
for entry in levels.items():
    print("  " + entry.key + ": " + entry.value.to_string())

print("services: " + services.len().to_string())
print("skipped: " + skipped.to_string())
```

The three report variables are visible at the top of the aggregation loop. There is no hidden state.

- `levels` counts known severities.
- `services` deduplicates service names.
- `skipped` counts rows that failed to parse.

When a run produces the wrong output, the fix is nearly always visible in those three bindings.

## Why This Shape Scales

Nothing in this program assumes a particular input size. If the tool later reads from a file instead of an inline list, the change is localised:

```python
import fs

text = try fs.read_to_string("app.log")
lines = text.split("\n")
```

The parser keeps returning `Option[LogLine]`. The counter keeps mutating a map owned by its caller. The set keeps owning service names. The program's structure does not move.

That is the argument for typing data at the boundary: when the input source changes, the program's core stays exactly where it was.

## Extensions To Try

The analyzer is small on purpose. A few directions you might push it:

- Add a `Map[String, int32]` that counts services as well as levels.
- Turn `level` into an enum — `LogLevel.Info`, `LogLevel.Warn`, `LogLevel.Error` — and treat unknown levels as skipped.
- Print the most frequent service by iterating over `services` and looking up counts.
- Read from standard input instead of a fixed vector, using `io.read_line()` in a loop.

Each extension should fit into the structure without rearranging it.
