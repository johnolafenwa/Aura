// Dates and times over generated timestamps.
//
// The same work as date_time.au, on the `chrono` crate.
use chrono::{DateTime, Datelike, Duration, FixedOffset, SecondsFormat, Timelike};

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(n) = args.get(1).and_then(|text| text.parse::<i64>().ok()) else {
        println!("usage: date_time <instants>");
        std::process::exit(2);
    };
    let mut rng = Rng { state: 59 };
    let (mut parsed_count, mut ordered, mut weekdays, mut year_days) = (0i64, 0i64, 0i64, 0i64);
    let (mut fields, mut byte_count, mut checksum) = (0i64, 0i64, 0i64);
    let layout = "%d/%b/%Y:%H:%M:%S %z";
    for _ in 0..n {
        let seconds = 631152000 + rng.next() % 1577836800;
        let millis = rng.next() % 1000;
        let offset = (rng.next() % 105 - 48) * 15;
        let zone = FixedOffset::east_opt((offset * 60) as i32).unwrap();
        let start = DateTime::from_timestamp(seconds, (millis * 1_000_000) as u32)
            .unwrap()
            .with_timezone(&zone);

        let precision = if millis == 0 {
            SecondsFormat::Secs
        } else {
            SecondsFormat::Millis
        };
        let text = start.to_rfc3339_opts(precision, true);
        byte_count += text.len() as i64;
        let parsed = DateTime::parse_from_rfc3339(&text).unwrap();
        if parsed == start && i64::from(parsed.offset().local_minus_utc()) == offset * 60 {
            parsed_count += 1;
        }

        let later = parsed
            + Duration::seconds(rng.next() % 31536000)
            + Duration::milliseconds(rng.next() % 1000);
        if parsed < later {
            ordered += 1;
        }
        weekdays += i64::from(later.weekday().number_from_monday());
        year_days += i64::from(later.ordinal());
        fields += i64::from(later.year())
            + i64::from(later.month() + later.day() + later.hour() + later.minute() + later.second());

        let line = later.format(layout).to_string();
        byte_count += line.len() as i64;
        let again = DateTime::parse_from_str(&line, layout).unwrap();
        let lost = (later - again).num_milliseconds();
        checksum = (checksum * 31 + again.timestamp() + lost) % 1000000007;
    }

    println!(
        "date_time instants={n} parsed={parsed_count} ordered={ordered} weekdays={weekdays} \
         year_days={year_days} fields={fields} bytes={byte_count} checksum={checksum}"
    );
}
