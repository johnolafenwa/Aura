# Time Module

The `time` module reads the clocks and works with dates and times. A
`time.DateTime` is an instant together with the offset from UTC at which
its calendar fields are read.

```aura
import time
```

## Public API

| API | Signature | Result |
| --- | --- | --- |
| `time.now` | `now() -> time.DateTime` | The current instant from the system clock, read at UTC. |
| `time.now_local` | `now_local() -> time.DateTime` | The current instant, read at the system time zone's current offset. |
| `time.monotonic` | `monotonic() -> Duration` | Time since a fixed point in the past, from a clock that never goes backward. |
| `time.utc` | `utc(year: int64, month: int64, day: int64, hour: int64 = 0, minute: int64 = 0, second: int64 = 0, nanosecond: int64 = 0) -> Result[time.DateTime, time.Error]` | The instant with these calendar fields at UTC. |
| `time.from_unix` | `from_unix(seconds: int64, nanosecond: int64 = 0) -> Result[time.DateTime, time.Error]` | The instant that many seconds after 1970-01-01T00:00:00Z. |
| `time.from_unix_ms` | `from_unix_ms(milliseconds: int64) -> Result[time.DateTime, time.Error]` | The instant that many milliseconds after 1970-01-01T00:00:00Z. |
| `time.parse_rfc3339` | `parse_rfc3339(text: str) -> Result[time.DateTime, time.Error]` | Parses RFC 3339 text and keeps its offset. |
| `time.parse` | `parse(text: str, pattern: str) -> Result[time.DateTime, time.Error]` | Parses text laid out as `pattern` directs. |

`time.DateTime` methods:

| Method | Signature | Result |
| --- | --- | --- |
| `year` `month` `day` | `year(self) -> int64` | The calendar date at the value's offset. `month` runs from 1 to 12 and `day` from 1 to 31. |
| `date` | `date(self) -> (int64, int64, int64)` | Year, month, and day together. |
| `hour` `minute` `second` | `hour(self) -> int64` | The time of day at the value's offset. |
| `nanosecond` | `nanosecond(self) -> int64` | Nanoseconds past the whole second, from 0 to 999,999,999. |
| `weekday` | `weekday(self) -> int64` | 1 for Monday to 7 for Sunday. |
| `day_of_year` | `day_of_year(self) -> int64` | 1 to 366. |
| `offset_minutes` | `offset_minutes(self) -> int64` | Minutes east of UTC. |
| `unix` | `unix(self) -> int64` | Whole seconds since 1970-01-01T00:00:00Z, rounded down. |
| `unix_ms` | `unix_ms(self) -> int64` | Whole milliseconds since 1970-01-01T00:00:00Z, rounded down. |
| `to_utc` | `to_utc(self) -> time.DateTime` | The same instant read at UTC. |
| `to_offset` | `to_offset(self, minutes: int64) -> Result[time.DateTime, time.Error]` | The same instant read `minutes` east of UTC. |
| `to_local` | `to_local(self) -> time.DateTime` | The same instant read at the offset the system time zone has at that instant. |
| `add` | `add(self, amount: Duration) -> time.DateTime` | The instant `amount` later, at the same offset. |
| `sub` | `sub(self, amount: Duration) -> time.DateTime` | The instant `amount` earlier, at the same offset. |
| `since` | `since(self, earlier: time.DateTime) -> Duration` | The time from `earlier` to this instant. Negative when `earlier` is later. |
| `is_before` `is_after` | `is_before(self, other: time.DateTime) -> bool` | Compares the instants, whatever their offsets. |
| `same_instant` | `same_instant(self, other: time.DateTime) -> bool` | Whether both are the same instant, whatever their offsets. |
| `to_rfc3339` | `to_rfc3339(self) -> str` | RFC 3339 text. |
| `format` | `format(self, pattern: str) -> Result[str, time.Error]` | Text laid out as `pattern` directs. |

## Example

<<< ../../examples/time/timestamps.au{aura}

It prints:

```text
2026-11-10T20:14:38.250Z
2026-11-10 is day 314, weekday 2
1794341678250
2026-11-11T01:44:38.250+05:30
11 1:44
true
2027-01-09T20:14:38.250Z
5184000.0
true
Saturday 09 January 2027, 08:14 PM
day 29 is not in month 2 of 2026
2026-11-10T20:14:38Z
true
true
```

## Instants And Offsets

A `DateTime` holds an instant to the nanosecond and an offset in whole
minutes. The instant is what `unix`, `since`, `is_before`, and
`same_instant` read. The offset only decides which calendar date and time of
day the field methods and the text methods show. `to_utc`, `to_offset`, and
`to_local` change the offset and never the instant.

The calendar is the Gregorian calendar, extended backward before its
adoption. Years run from 1 to 9999. A constructor or a parse returns
`time.Error.Invalid` for an instant outside that range, and `add` and `sub`
trap when their result leaves it, because a result that far away is a
programming error.

There are no leap seconds. Every day has 86,400 seconds, as Unix time
counts, so second 60 is rejected.

## Arithmetic

`add` and `sub` take a `Duration`, an exact count of nanoseconds, so
`moment.add(Duration.minutes(90))` is exactly 5,400 seconds later.
`later.since(earlier)` returns the `Duration` between two instants.

There is no "one month later". Months and years have different lengths, so
that is a calendar rule a program states for itself: read the fields, change
the month, and build the result with `time.utc`, which reports a day the new
month does not have.

## The System Time Zone

`now_local` and `to_local` ask the operating system for the offset its time
zone has at the instant in question, so daylight saving time is applied for
that instant. The result is a fixed offset. Arithmetic on it keeps that
offset: adding six months to a summer time does not switch to the winter
offset. Call `to_local` on the result to read it at the offset in force
then.

Named time zones such as `Europe/Paris` are not provided. A program reads
other zones with `to_offset`.

## RFC 3339 Text

`to_rfc3339` writes `2026-11-10T20:14:38Z`. A zero offset is `Z` and any
other is `+hh:mm` or `-hh:mm`. A fraction of a second is written only when
it is not zero, with 3, 6, or 9 digits, whichever is the shortest that is
exact.

`parse_rfc3339` reads the same form. It also accepts a lowercase `t` or `z`
and a space in place of `T`. The offset is required. A fraction may have any
number of digits, and digits past the ninth are dropped.

## Patterns

`format` and `parse` follow a pattern in which a directive stands for a
field and any other character stands for itself.

| Directive | Field | Written as |
| --- | --- | --- |
| `%Y` | Year | Four digits, `2026`. |
| `%y` | Year in its century | Two digits, `26`. Parsing reads 69 to 99 as the 1900s and 00 to 68 as the 2000s. |
| `%m` | Month | Two digits, `01` to `12`. |
| `%b` `%B` | Month name | `Nov`, `November`. |
| `%d` | Day | Two digits, `01` to `31`. |
| `%e` | Day | Padded with a space, ` 5`. |
| `%j` | Day of the year | Three digits, `001` to `366`. Cannot be parsed. |
| `%a` `%A` | Weekday name | `Tue`, `Tuesday`. |
| `%H` | Hour | Two digits, `00` to `23`. |
| `%I` | Hour on a 12-hour clock | Two digits, `01` to `12`. |
| `%p` | Half of the day | `AM` or `PM`. |
| `%M` | Minute | Two digits. |
| `%S` | Second | Two digits. |
| `%f` | Fraction of a second | Six digits, microseconds. Parsing reads one to nine digits. |
| `%z` | Offset | `+0100`. |
| `%:z` | Offset | `+01:00`. |
| `%%` | A percent sign | `%`. |

Names are English. A pattern with an unknown directive returns
`time.Error.Invalid` from both functions.

When parsing:

- Numeric fields other than `%Y` and `%y` accept one or two digits, so `%d`
  reads both `5` and `05`.
- Names match in any letter case.
- A space in the pattern matches any amount of whitespace, including none.
- `%z` and `%:z` each read `+0100`, `+01:00`, and `Z`.
- The pattern must read a year. A month or day it does not read is 1, and a
  time field it does not read is 0. Without an offset directive the fields
  are read at UTC.
- A weekday name is read and not checked against the date.
- The whole text must be used.

## Measuring Time

`time.monotonic()` reads a clock that only moves forward and is not changed
when the system clock is set. Subtract two readings to measure an interval:

```aura
import time

def main():
    started = time.monotonic()
    mut total = 0
    for i in range(1000):
        total += i
    elapsed = time.monotonic() - started
    print(total)
    print(elapsed.to_ms() >= 0.0)
```

The program prints `499500` and `true`. A single reading has no meaning on
its own and cannot be compared between processes.

## Error Model

| Variant | Meaning |
| --- | --- |
| `Invalid(message: str)` | A field is out of range, a date does not exist, text does not match the expected form, a pattern has an unknown directive, or the instant is outside the years 1 to 9999. `message` describes the fault for a person and is not a stable format. |

## Backend Support

The MIR runtime and the direct native backend share one implementation, so
results match. Calendar arithmetic is Aura code over three integers, and a
`DateTime` is passed and stored like any small class.

`to_local` and `now_local` use the offset 0 on a platform where the system
time zone cannot be read.

## Limits

- No named time zones and no time zone database. Offsets are fixed.
- No date-only or time-of-day-only types, and no calendar durations such as
  months.
- No leap seconds.
- Month and weekday names are English.
- `sys.unix_time_ms()` and `sys.monotonic_time_ms()` remain for programs
  that only need a number.

Design record: [ADR-0079: Dates and times](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0079-dates-and-times.md).
