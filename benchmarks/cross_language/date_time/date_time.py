# Dates and times over generated timestamps.
#
# The same work as date_time.au, on the standard library's `datetime`.
import sys
from datetime import datetime, timedelta, timezone


class Rng:
    def __init__(self, state):
        self.state = state

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def main():
    if len(sys.argv) != 2:
        print("usage: date_time <instants>")
        return 2
    n = int(sys.argv[1])
    rng = Rng(59)
    epoch = datetime(1970, 1, 1, tzinfo=timezone.utc)
    parsed_count = ordered = weekdays = year_days = fields = byte_count = checksum = 0
    layout = "%d/%b/%Y:%H:%M:%S %z"
    for _ in range(n):
        seconds = 631152000 + rng.next() % 1577836800
        millis = rng.next() % 1000
        offset = (rng.next() % 105 - 48) * 15
        zone = timezone(timedelta(minutes=offset))
        start = (epoch + timedelta(seconds=seconds, milliseconds=millis)).astimezone(zone)

        text = start.isoformat(timespec="milliseconds" if millis else "seconds")
        if offset == 0:
            text = text[:-6] + "Z"
        byte_count += len(text)
        parsed = datetime.fromisoformat(text)
        if parsed == start and parsed.utcoffset() == timedelta(minutes=offset):
            parsed_count += 1

        later = parsed + timedelta(
            seconds=rng.next() % 31536000, milliseconds=rng.next() % 1000
        )
        if parsed < later:
            ordered += 1
        weekdays += later.isoweekday()
        year_days += later.timetuple().tm_yday
        fields += (
            later.year + later.month + later.day + later.hour + later.minute + later.second
        )

        line = later.strftime(layout)
        byte_count += len(line)
        again = datetime.strptime(line, layout)
        unix = (again - epoch) // timedelta(seconds=1)
        lost = (later - again) // timedelta(milliseconds=1)
        checksum = (checksum * 31 + unix + lost) % 1000000007

    print(
        f"date_time instants={n} parsed={parsed_count} ordered={ordered} "
        f"weekdays={weekdays} year_days={year_days} fields={fields} "
        f"bytes={byte_count} checksum={checksum}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
