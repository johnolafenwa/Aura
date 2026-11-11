# ADR-0079: Dates and times

- Status: Accepted
- Date: 2026-11-10
- Builds on: ADR-0057, ADR-0078

## Context

Aura had `Duration`, `sleep`, and two numbers, `sys.unix_time_ms()` and
`sys.monotonic_time_ms()`. A program could not turn an instant into a date,
read a timestamp from a log or an API reply, or write one. The HTTP client
and the reference agent that follow in this work need both.

## Decision

### One type: an instant with an offset

`time.DateTime` is an instant, as Unix seconds and nanoseconds, with the
offset from UTC, in whole minutes, at which its calendar fields are read.
It is a `copy class` of three `int64` fields.

- Comparison and arithmetic read only the instant. `to_utc`, `to_offset`,
  and `to_local` change only the offset.
- There is no separate "naive" date and time without an offset. Mixing the
  two is the usual source of time bugs, and every constructor here can name
  an offset: `time.utc` is UTC by name, and a parse without an offset
  directive reads at UTC and says so.
- Years run from 1 to 9999 on the proleptic Gregorian calendar, the range
  RFC 3339 can write. Constructors and parsers return `time.Error.Invalid`
  outside it. `add` and `sub` trap, because the inputs were valid and only
  a programming error reaches year 10,000.
- No leap seconds: Unix time has none.

### Fixed offsets and the system zone, no zone database

`to_local` asks the operating system (`localtime_r`) for the offset its
zone has at the instant, so daylight saving time is right for that instant.
The result is a fixed offset. Named zones would need the IANA database in
every binary (about 400 KiB compressed) or on every host, and its rules
change several times a year. That is left for a later decision.

### Arithmetic is exact and takes a `Duration`

`add`, `sub`, and `since` work in nanoseconds with the existing `Duration`.
Calendar arithmetic ("one month later") is not provided: its edge cases
(January 31 plus a month) have no single right answer, and `time.utc`
already reports a date that does not exist.

### Text

`to_rfc3339` and `parse_rfc3339` cover the format APIs and logs use.
`format` and `parse` take `strftime`-style directives, the notation Python
and C programmers know, limited to the directives that do not depend on a
locale. Names are English. An unknown directive is an error value from both
functions, not a silent literal.

### Implementation

`DateTime` and its calendar arithmetic are Aura source
(`crates/aura-compiler/std/time.au`). Field reads such as `year()` are
integer arithmetic that generated code runs inline, with no host call. The
clocks, the system zone, and text formatting and parsing are host functions
named `time.__aura_*`. They take and return integers and text, so the host
never builds a `DateTime`.

The date algorithms exist twice, in Aura for the field methods and in Rust
for text. A runtime test walks every day from year 1 to 9999 through the
Rust pair, and the benchmark and the example compare the two through text.

`sys.unix_time_ms()` and `sys.monotonic_time_ms()` stay. They are the
cheapest way to get a number, and removing them buys nothing.

## Consequences

- `date_time`, the 33rd cross-language program, runs 200,000 instants
  through RFC 3339 text, arithmetic, field reads, and a log layout: Aura
  0.34 s, Python's `datetime` 4.0 s, Rust's `chrono` 0.18 s on the 4-core
  cloud host. A first version returned the date from one helper as a tuple
  and took 1.15 s: a tuple of integers is a boxed runtime value, so each
  field method now computes its own integer.
- The host functions that write and read text take the instant as three
  integer words and return one word. A parse returns whether it worked, and
  the host keeps the instant or the failure for the calls that follow:
  `__aura_parsed_seconds`, `__aura_parsed_nanos`, `__aura_parsed_offset`,
  and `__aura_error`. `std/time.au` reads them in the statements right
  after the parse, with no point between where a task can be suspended, so
  the kept outcome is per thread and never seen by another task. With
  boxed integer arguments and tuple results the program took 0.77 s.
- No new dependency. The module adds about 600 lines of Rust and 300 of
  Aura.
- `public copy class` was misread as a module constant named `copy`. The
  parser now reads it as a class. `regex.Regex` and `time.DateTime` are the
  first public copy classes in the standard library.

## Not decided here

- Named time zones and a zone database.
- Date-only and time-of-day types, and calendar durations.
- Locale-dependent names and directives.
