# Time and dates

Programs ask two different questions about time, and mixing them up is the
source of a whole family of bugs. "What is the date?" is a question for the
calendar. "How long did that take?" is a question for a stopwatch. The
`time` module answers each with its own clock:

*Filename: src/main.au*

```aura
import time

def main():
    now = time.now()
    print(now)
    print(time.now_local())

    started = time.monotonic()
    mut total = 0
    for i in range(100000):
        total += i
    elapsed = time.monotonic() - started
    print(total)
    print(f"{elapsed.to_ms()} ms")
```

<span class="caption">Listing 20-4: The system clock and the monotonic clock</span>

```console
$ aura run src/main.au
2026-11-15T10:15:59.409629944Z
2026-11-15T11:15:59.409902610+01:00
4999950000
457.502039 ms
```

Your output will differ on every line but the third. `time.now()` reads the
system clock and returns a `time.DateTime` at UTC, which prints as RFC 3339
text ending in `Z`. `time.now_local()` is the same instant shown at the
offset of the machine's time zone, here one hour east of UTC. (The loop is
slow because `aura run` interprets it. A built binary finishes it in well
under a millisecond.)

The system clock can be wrong, and it can be corrected while your program
runs: by the user, or by a network time service stepping it back half a
second. Subtract two `time.now()` readings to time something and one day the
answer will be negative. `time.monotonic()` reads a clock that only moves
forward and is never adjusted. A single reading means nothing on its own,
but the difference between two is a trustworthy `Duration`. Use
`time.now()` when you need a date, and `time.monotonic()` when you need a
length of time.

## Instants and offsets

A `DateTime` holds two things: an instant, to the nanosecond, and an offset
from UTC in whole minutes. The instant is the moment itself, the same
everywhere on Earth. The offset only decides which calendar date and time of
day we read off it. The examples from here on build their instants from
fixed text and not from `time.now()`, so that your output matches ours:

*Filename: src/main.au*

```aura
import time

def run() -> Result[None, time.Error]:
    sent = try time.parse_rfc3339("2026-11-15T10:02:34Z")
    print(sent)
    print(f"{sent.year()}-{sent.month()}-{sent.day()} at {sent.hour()}:{sent.minute()}")
    print(f"weekday {sent.weekday()}, day {sent.day_of_year()} of the year")
    print(sent.unix())

    lagos = try sent.to_offset(60)
    tokyo = try sent.to_offset(9 * 60)
    print(lagos)
    print(tokyo)
    print(f"{lagos.hour()} and {tokyo.hour()}")
    print(lagos.same_instant(tokyo))
    return Result.Ok(None)

def main():
    match run():
        case Result.Ok(_):
            pass
        case Result.Err(time.Error.Invalid(message)):
            print(message)
```

<span class="caption">Listing 20-5: One instant read at three offsets</span>

```console
$ aura run src/main.au
2026-11-15T10:02:34Z
2026-11-15 at 10:2
weekday 7, day 319 of the year
1794736954
2026-11-15T11:02:34+01:00
2026-11-15T19:02:34+09:00
11 and 19
true
```

`parse_rfc3339` reads the standard timestamp format of the internet, the
one you see in JSON APIs and log files. It returns a `Result`, because text
can be malformed, and we use the `run` function with `try` so that every
fallible step shares one error path. (`main` itself can only return `int32`
or `None`.) The field methods return plain integers, which is why the minute
printed as `2` and not `02`. We will format properly in a moment. `weekday()`
counts from 1 for Monday to 7 for Sunday, and `unix()` is the count of
seconds since the start of 1970 that many systems use to exchange instants.

`to_offset` gives the same instant at another offset, in minutes east of
UTC. Lagos is at +60 and Tokyo at +540, so the hour reads 11 in one and 19
in the other. `same_instant` confirms that nothing but the reading changed.
Comparisons between `DateTime` values are methods: `same_instant`,
`is_before`, and `is_after` compare the instants and ignore the offsets.
There is no `<` or `==` on `DateTime`, so you always say which comparison
you mean.

Offsets are all Aura 0.16 knows about zones. There is no time zone database
and no `Europe/Paris`. `to_local()` and `now_local()` ask the operating
system for the machine's own offset at the instant in question, daylight
saving included, and any other zone is an offset you supply. A program that
schedules meetings across zones needs more than this module offers. A
program that stamps, compares, and prints instants has what it needs.

## Arithmetic with `Duration`

You met `Duration` as the type of timeouts, with literals such as `5s` and
`250ms`. It is an exact count of nanoseconds, and it is what connects two
instants:

```aura
import time

def is_fresh(sent: time.DateTime, now: time.DateTime, max_age: Duration) -> bool:
    age = now.since(sent)
    return age >= 0s and age <= max_age

def run() -> Result[None, time.Error]:
    sent = try time.parse_rfc3339("2026-11-15T10:02:34Z")
    later = sent.add(Duration.minutes(90))
    print(later)
    print(later.since(sent))
    print(later.since(sent).to_seconds())
    print(sent.since(later))

    print(is_fresh(sent, sent.add(4m), 5m))
    print(is_fresh(sent, later, 5m))
    print(sent.add(Duration.minutes(60 * 24 * 30)))
    return Result.Ok(None)

def main():
    match run():
        case Result.Ok(_):
            pass
        case Result.Err(time.Error.Invalid(message)):
            print(message)
```

```console
$ aura run src/main.au
2026-11-15T11:32:34Z
5400000ms
5400.0
-5400000ms
true
false
2026-12-15T10:02:34Z
```

`add` and `sub` move an instant by a `Duration`, and `later.since(earlier)`
is the `Duration` between two instants, negative when the argument is the
later one. Durations compare with the ordinary operators, so `is_fresh` is
two comparisons. Notice that it takes `now` as a parameter and does not call
`time.now()` itself. A function that reads the clock is hard to test,
because its answer changes every time. A function that is handed the time
can be tested with any time you like, and only the outermost caller needs to
know what time it really is. We will write `courier` this way.

Duration literals come in `ms`, `s`, and `m`. For longer spans multiply:
`Duration.minutes(60 * 24 * 30)` is thirty days, and adding it carried us
correctly across the end of November.

Thirty days is not "one month", though, and the module has no way to add a
month. Months have different lengths, so "a month after January 31" has no
single answer. February 31 does not exist, and whether you want February
28, March 3, or an error depends on whether you are billing a customer or
scheduling a backup. Aura makes you say which. You read the calendar
fields, change them, and build a new instant with `time.utc`, which checks
that the date exists:

```aura
import time

def month_later(day: time.DateTime) -> Result[time.DateTime, time.Error]:
    (year, month, date) = day.date()
    if month == 12:
        return time.utc(year + 1, 1, date)
    return time.utc(year, month + 1, date)

def run() -> Result[None, time.Error]:
    print(month_later(try time.utc(2026, 1, 15)))
    print(month_later(try time.utc(2026, 1, 31)))
    print(time.utc(2028, 2, 29))
    return Result.Ok(None)

def main():
    match run():
        case Result.Ok(_):
            pass
        case Result.Err(time.Error.Invalid(message)):
            print(message)
```

```console
$ aura run src/main.au
Result.Ok(2026-02-15T00:00:00Z)
Result.Err(time.Error.Invalid(day 31 is not in month 2 of 2026))
Result.Ok(2028-02-29T00:00:00Z)
```

`time.utc(year, month, day)` builds an instant at midnight UTC, with
optional `hour`, `minute`, `second`, and `nanosecond` arguments. A date that
does not exist is an `Err` you can handle, and 2028 is a leap year, so its
February 29 is fine.

## Formatting and parsing

RFC 3339 is the right format between programs. People and old log files
want other layouts, and for those `format` and `parse` take a pattern. A
directive such as `%Y` stands for a field, and every other character stands
for itself:

```aura
import time

def run() -> Result[None, time.Error]:
    sent = try time.parse_rfc3339("2026-11-15T10:02:34Z")
    lagos = try sent.to_offset(60)
    print(try sent.format("%A %d %B %Y, %I:%M %p"))
    print(try lagos.format("%Y-%m-%d %H:%M %:z"))

    logged = try time.parse("15/Nov/2026:11:02:34 +0100", "%d/%b/%Y:%H:%M:%S %z")
    print(logged)
    print(logged.same_instant(sent))

    print(time.parse("2026-11-15", "%Y-%m-%d"))
    print(time.parse("15 November", "%d %B"))
    print(time.parse_rfc3339("2026-11-15 10:02:34"))
    return Result.Ok(None)

def main():
    match run():
        case Result.Ok(_):
            pass
        case Result.Err(time.Error.Invalid(message)):
            print(message)
```

<span class="caption">Listing 20-6: Writing and reading instants with patterns</span>

```console
$ aura run src/main.au
Sunday 15 November 2026, 10:02 AM
2026-11-15 11:02 +01:00
2026-11-15T11:02:34+01:00
true
Result.Ok(2026-11-15T00:00:00Z)
Result.Err(time.Error.Invalid(the pattern reads no year))
Result.Err(time.Error.Invalid(expected a UTC offset at ``))
```

The directives are the ones `strftime` uses in C and Python: `%Y` for the
year, `%m` and `%d` for month and day, `%H`, `%M`, and `%S` for the time,
`%B` and `%A` for the names of the month and the weekday, `%z` for the
offset. Names are always in English. The pattern on the fifth line of `run`
is the timestamp layout of web server access logs, and the instant it reads
is the one we started with, written an hour east.

Parsing fills in what the pattern does not mention: a missing time is
midnight, and a missing offset is UTC. It will not guess a year, so `"15
November"` is rejected. The last line is a reminder that `parse_rfc3339`
insists on an offset. A timestamp without one does not name an instant, and
half the bugs in date handling come from pretending that it does. The
[Time Module](/manual/time#patterns) page lists every directive.

## Envelopes that expire

Back to `courier`. An envelope from Listing 20-3 verifies forever, so
anyone who captures one can replay it. We will put the sealing time inside
the signed data and refuse an envelope that is too old. The time goes on a
line of its own before the message, and because it sits inside what we
sign, nobody can change it without breaking the signature.

*Filename: src/main.au*

```aura
import bytes
import gzip
import time

enum OpenError:
    Malformed
    BadSignature
    TooLarge(limit: int64)
    Expired(age: Duration)

def seal(secret: list[uint8], message: str, now: time.DateTime) -> str:
    stamped = f"{now.to_rfc3339()}\n{message}"
    packed = gzip.compress(stamped.to_bytes())
    signature = bytes.hmac_sha256(secret, packed)
    return f"{bytes.base64_encode(packed)}.{bytes.hex_encode(signature)}"

def unpack(packed: list[uint8]) -> Result[str, OpenError]:
    match gzip.decompress(packed, max_bytes=4096):
        case Result.Ok(plain):
            return str.from_bytes(plain).map_err(lambda _: OpenError.Malformed)
        case Result.Err(gzip.Error.OutputTooLarge(limit)):
            return Result.Err(OpenError.TooLarge(limit))
        case Result.Err(gzip.Error.InvalidData(_)):
            return Result.Err(OpenError.Malformed)

def check_age(stamped: str, now: time.DateTime, max_age: Duration) -> Result[str, OpenError]:
    pair = stamped.split_once("\n")
    if pair is None:
        return Result.Err(OpenError.Malformed)
    stamp, message = pair
    sent = try time.parse_rfc3339(stamp).map_err(lambda _: OpenError.Malformed)
    age = now.since(sent)
    if age < 0s or age > max_age:
        return Result.Err(OpenError.Expired(age))
    return Result.Ok(message)

def unseal(secret: list[uint8], envelope: str, now: time.DateTime, max_age: Duration) -> Result[str, OpenError]:
    parts = envelope.split(".")
    if parts.len() != 2:
        return Result.Err(OpenError.Malformed)
    packed = try bytes.base64_decode(parts[0]).map_err(lambda _: OpenError.Malformed)
    signature = try bytes.hex_decode(parts[1]).map_err(lambda _: OpenError.Malformed)
    if not bytes.constant_time_eq(bytes.hmac_sha256(secret, packed), signature):
        return Result.Err(OpenError.BadSignature)
    return check_age(try unpack(packed), now, max_age)

def run() -> Result[None, time.Error]:
    secret = "correct horse battery staple".to_bytes()
    sealed_at = try time.utc(2026, 11, 15, hour=10, minute=2, second=34)
    envelope = seal(secret, "deploy finished: 14 services healthy", sealed_at)

    print(unseal(secret, envelope, sealed_at.add(90s), 5m))
    print(unseal(secret, envelope, sealed_at.add(Duration.minutes(60 * 24 * 7)), 5m))
    print(unseal(secret, envelope, sealed_at.sub(30s), 5m))
    return Result.Ok(None)

def main():
    match run():
        case Result.Ok(_):
            pass
        case Result.Err(time.Error.Invalid(message)):
            print(message)
```

<span class="caption">Listing 20-7: `courier`, version two: envelopes carry their sealing time</span>

```console
$ aura run src/main.au
Result.Ok(deploy finished: 14 services healthy)
Result.Err(OpenError.Expired(604800000ms))
Result.Err(OpenError.Expired(-30000ms))
```

`seal` and `unseal` now take the current time as a parameter, so `run` can
play three scenes with one envelope: opened ninety seconds after sealing,
opened a week later, and opened by a machine whose clock is thirty seconds
behind the sender's. The first is accepted. The other two are refused, and
the error carries the age so that a log line can say how stale the envelope
was.

The third case deserves a second look. An envelope "from the future" is not
an attack. It is clock skew, which every pair of real machines has. A strict
`age < 0s` check will reject honest messages whenever the receiver's clock
runs slightly behind. Real protocols allow a small tolerance in both
directions, and you would relax the test to something like
`age < Duration.seconds(-60)`. We keep the strict version here because it is
easier to read. In the real tool, the outermost caller passes `time.now()`
for `now`, and that is the only place the clock is read.

Envelopes can now expire, but within their five minutes they can still be
delivered twice. To detect that, each needs an identity. Before we get to
random identifiers, though, let's look at the log `courier` keeps, because
reading it back calls for the next module.
