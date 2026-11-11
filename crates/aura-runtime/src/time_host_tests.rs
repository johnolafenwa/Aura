use super::*;

fn host(name: &str, args: Vec<Value>) -> Result<Value> {
    host_builtins::host_builtin_function(name).expect("a time builtin")(args, None)
}

fn elements(value: Value) -> Vec<Value> {
    match value {
        Value::Tuple(tuple) => tuple.elements,
        other => panic!("expected a tuple, found {other:?}"),
    }
}

fn text(value: &Value) -> String {
    host_string_ref_arg(std::slice::from_ref(value), 0, "test")
        .expect("a str value")
        .to_string()
}

fn instant(seconds: i64, nanos: i64, offset: i64) -> Vec<Value> {
    vec![int(seconds), int(nanos), int(offset)]
}

fn format(seconds: i64, nanos: i64, offset: i64, pattern: &str) -> (bool, String) {
    let mut args = instant(seconds, nanos, offset);
    args.push(Value::string(pattern));
    let written = text(&host("time::__aura_format", args).expect("format returns"));
    if host("time::__aura_failed", Vec::new()).expect("failed") == Value::Bool(true) {
        assert_eq!(written, "");
        (
            false,
            text(&host("time::__aura_error", Vec::new()).expect("error")),
        )
    } else {
        (true, written)
    }
}

fn rfc3339(seconds: i64, nanos: i64, offset: i64) -> String {
    text(&host("time::__aura_rfc3339", instant(seconds, nanos, offset)).expect("text"))
}

/// What a parse returned: the instant, or the message.
fn parsed(result: Value) -> std::result::Result<(Value, Value, Value), String> {
    let part = |name: &str| host(name, Vec::new()).expect("a kept part");
    if result == Value::Bool(true) {
        assert_eq!(part("time::__aura_failed"), Value::Bool(false));
        Ok((
            part("time::__aura_parsed_seconds"),
            part("time::__aura_parsed_nanos"),
            part("time::__aura_parsed_offset"),
        ))
    } else {
        assert_eq!(part("time::__aura_failed"), Value::Bool(true));
        Err(text(&part("time::__aura_error")))
    }
}

fn parse_rfc(input: &str) -> std::result::Result<(Value, Value, Value), String> {
    parsed(host("time::__aura_parse_rfc3339", vec![Value::string(input)]).expect("parse returns"))
}

fn parse(input: &str, pattern: &str) -> std::result::Result<(Value, Value, Value), String> {
    parsed(
        host(
            "time::__aura_parse",
            vec![Value::string(input), Value::string(pattern)],
        )
        .expect("parse returns"),
    )
}

fn at(seconds: i64, nanos: i64, offset: i64) -> (Value, Value, Value) {
    (int(seconds), int(nanos), int(offset))
}

// 2026-11-10T20:14:38Z, a Tuesday and day 314 of its year.
const MOMENT: i64 = 1_794_341_678;

#[test]
fn civil_dates_round_trip_across_every_supported_day() {
    assert_eq!(days_from_civil(1970, 1, 1), 0);
    assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    assert_eq!(days_from_civil(1, 1, 1), MIN_UNIX_SECONDS / SECONDS_PER_DAY);
    assert_eq!(civil_from_days(-1), (1969, 12, 31));
    let first = days_from_civil(1, 1, 1);
    let last = days_from_civil(9999, 12, 31);
    let mut expected = (1, 1, 1);
    for days in first..=last {
        assert_eq!(civil_from_days(days), expected, "day {days}");
        let (year, month, day) = expected;
        assert_eq!(days_from_civil(year, month, day), days);
        expected = if day < days_in_month(year, month) {
            (year, month, day + 1)
        } else if month < 12 {
            (year, month + 1, 1)
        } else {
            (year + 1, 1, 1)
        };
    }
    assert_eq!(expected, (10_000, 1, 1));
}

#[test]
fn rfc3339_text_shows_the_offset_and_the_shortest_fraction() {
    assert_eq!(rfc3339(MOMENT, 0, 0), "2026-11-10T20:14:38Z");
    assert_eq!(
        rfc3339(MOMENT, 250_000_000, 60),
        "2026-11-10T21:14:38.250+01:00"
    );
    assert_eq!(
        rfc3339(MOMENT, 1_000, -330),
        "2026-11-10T14:44:38.000001-05:30"
    );
    assert_eq!(rfc3339(MOMENT, 1, 0), "2026-11-10T20:14:38.000000001Z");
    assert_eq!(rfc3339(-1, 0, 0), "1969-12-31T23:59:59Z");
    assert_eq!(rfc3339(MIN_UNIX_SECONDS, 0, 0), "0001-01-01T00:00:00Z");
    assert_eq!(rfc3339(MAX_UNIX_SECONDS, 0, 0), "9999-12-31T23:59:59Z");
}

#[test]
fn format_writes_every_directive() {
    let all = "%Y %y %m %d %e %H %I %p %M %S %f %j %a %A %b %B %z %:z %%";
    assert_eq!(
        format(MOMENT, 250_000_000, 60, all),
        (
            true,
            "2026 26 11 10 10 21 09 PM 14 38 250000 314 Tue Tuesday Nov November +0100 +01:00 %"
                .to_string()
        )
    );
    // 2026-03-05T00:05:09Z: single digits pad, midnight is 12 AM.
    let early = days_from_civil(2026, 3, 5) * SECONDS_PER_DAY + 309;
    assert_eq!(
        format(early, 0, -90, "%e|%I %p|%z|%:z|%a|%j"),
        (true, " 4|10 PM|-0130|-01:30|Wed|063".to_string())
    );
    assert_eq!(format(early, 0, 0, "%I %p"), (true, "12 AM".to_string()));
    assert_eq!(
        format(early, 0, 0, "day %d."),
        (true, "day 05.".to_string())
    );
    assert_eq!(
        format(early, 0, 0, "%Q"),
        (false, "unknown directive `%Q`".to_string())
    );
    assert_eq!(
        format(early, 0, 0, "100%"),
        (false, "the pattern ends with a lone `%`".to_string())
    );
    assert_eq!(
        format(early, 0, 0, "%:m"),
        (false, "`%:` must be followed by `z`".to_string())
    );
}

#[test]
fn rfc3339_parsing_accepts_the_standard_forms_and_keeps_the_offset() {
    assert_eq!(parse_rfc("2026-11-10T20:14:38Z"), Ok(at(MOMENT, 0, 0)));
    assert_eq!(parse_rfc("2026-11-10t20:14:38z"), Ok(at(MOMENT, 0, 0)));
    assert_eq!(parse_rfc("2026-11-10 20:14:38+00:00"), Ok(at(MOMENT, 0, 0)));
    assert_eq!(
        parse_rfc("2026-11-10T21:14:38.25+01:00"),
        Ok(at(MOMENT, 250_000_000, 60))
    );
    assert_eq!(
        parse_rfc("2026-11-10T14:44:38.123456789123-05:30"),
        Ok(at(MOMENT, 123_456_789, -330))
    );
    assert_eq!(
        parse_rfc("0001-01-01T00:00:00Z"),
        Ok(at(MIN_UNIX_SECONDS, 0, 0))
    );
    for (input, reason) in [
        ("", "four year digits"),
        ("2026-11-10", "expected `T`"),
        ("2026-11-10T20:14:38", "a UTC offset"),
        ("2026-11-10T20:14:38.Z", "a fraction of a second"),
        ("2026-11-10T20:14:38Zx", "unexpected text `x`"),
        ("2026-11-10T20:14:60Z", "second 60"),
        ("2026-11-10T24:00:00Z", "hour 24"),
        ("2026-11-10T20:60:00Z", "minute 60"),
        ("2026-13-10T20:14:38Z", "month 13"),
        ("2026-02-29T20:14:38Z", "day 29 is not in month 2 of 2026"),
        ("0000-01-01T00:00:00Z", "year 0"),
        ("2026-11-10T20:14:38+24:00", "offset 24:00"),
        ("2026-11-10T20:14:38+01:60", "offset 01:60"),
        ("2026/11/10T20:14:38Z", "expected `-`"),
        ("2026-11-10T20-14-38Z", "expected `:`"),
        ("0001-01-01T00:00:00+01:00", "the year is outside 1 to 9999"),
        ("9999-12-31T23:59:59-01:00", "the year is outside 1 to 9999"),
        ("2026-11-10Té0:14:38Z", "two hour digits"),
    ] {
        let message = parse_rfc(input).expect_err(input);
        assert!(message.contains(reason), "{input}: {message}");
    }
}

#[test]
fn pattern_parsing_reads_every_directive_it_writes() {
    assert_eq!(
        parse("10/Nov/2026:21:14:38 +0100", "%d/%b/%Y:%H:%M:%S %z"),
        Ok(at(MOMENT, 0, 60))
    );
    assert_eq!(
        parse(
            "Tuesday, November 10 26 09:14:38.250000 pm +01:00 100%",
            "%A, %B %e %y %I:%M:%S.%f %p %:z 100%%"
        ),
        Ok(at(MOMENT, 250_000_000, 60))
    );
    assert_eq!(
        parse("Tue 2026-11-10   20:14:38Z", "%a %Y-%m-%d %H:%M:%S%z"),
        Ok(at(MOMENT, 0, 0))
    );
    // Fields a pattern leaves out start the year, the month, and the day.
    let new_year = days_from_civil(2026, 1, 1) * SECONDS_PER_DAY;
    assert_eq!(parse("2026", "%Y"), Ok(at(new_year, 0, 0)));
    assert_eq!(parse("12 AM 2026", "%I %p %Y"), Ok(at(new_year, 0, 0)));
    assert_eq!(
        parse("12 PM 2026", "%I %p %Y"),
        Ok(at(new_year + 12 * 3_600, 0, 0))
    );
    assert_eq!(parse("7 2026", "%I %Y"), Ok(at(new_year + 7 * 3_600, 0, 0)));
    assert_eq!(parse("70 1 2", "%y %m %d"), Ok(at(SECONDS_PER_DAY, 0, 0)));
    assert_eq!(
        parse("2026-1-1 5", "%Y-%m-%d %f"),
        Ok(at(new_year, 500_000_000, 0))
    );
    for (input, pattern, reason) in [
        ("10", "%d", "no year"),
        ("2026 x", "%Y", "unexpected text ` x`"),
        ("2026", "%Y %", "lone `%`"),
        ("2026", "%Y%Q", "unknown directive `%Q`"),
        ("2026", "%Y%:m", "`%:` must be followed by `z`"),
        ("2026 100", "%Y %j", "`%j` cannot be parsed"),
        ("2026 13 pm", "%Y %I %p", "hour 13 is outside 1 to 12"),
        ("2026 0", "%Y %I", "hour 0 is outside 1 to 12"),
        ("2026 Foo", "%Y %b", "a month name"),
        ("2026 Foo", "%Y %A", "a weekday name"),
        ("2026 xm", "%Y %p", "AM or PM"),
        ("2026-", "%Y/", "expected `/`"),
        ("2026 99", "%Y %H", "hour 99"),
        ("26", "%Y", "four year digits"),
        ("2026 +1", "%Y %z", "two offset hour digits"),
        ("2026 0100", "%Y %z", "a UTC offset"),
        ("2026 5", "%Y %%", "expected `%`"),
    ] {
        let message = parse(input, pattern).expect_err(input);
        assert!(message.contains(reason), "{input} / {pattern}: {message}");
    }
}

#[test]
fn clocks_and_durations_cross_the_boundary_exactly() {
    let now = elements(host("time::__aura_now", Vec::new()).expect("now"));
    assert!(matches!(&now[0], Value::Int(_)));
    assert!(int_arg(&now, 0, "test").expect("seconds") > 1_700_000_000);
    assert!((0..1_000_000_000).contains(&int_arg(&now, 1, "test").expect("nanos")));

    let first = host("time::__aura_monotonic", Vec::new()).expect("monotonic");
    let second = host("time::__aura_monotonic", Vec::new()).expect("monotonic");
    let (Value::Duration(first), Value::Duration(second)) = (first, second) else {
        panic!("expected durations");
    };
    assert!(first >= 3_600 * NANOS_PER_SECOND && second >= first);

    let parts = |nanos: i128| {
        vec![
            host(
                "time::__aura_duration_seconds",
                vec![Value::Duration(nanos)],
            )
            .expect("seconds"),
            host("time::__aura_duration_nanos", vec![Value::Duration(nanos)]).expect("nanos"),
        ]
    };
    assert_eq!(parts(1_500_000_000), vec![int(1), int(500_000_000)]);
    assert_eq!(parts(-1), vec![int(-1), int(999_999_999)]);
    assert_eq!(parts(i128::MAX)[0], int(i64::MAX / 4));
    assert_eq!(
        host("time::__aura_duration", vec![int(-2), int(500_000_000)]).expect("duration"),
        Value::Duration(-1_500_000_000)
    );

    let offset = host("time::__aura_local_offset_minutes", vec![int(MOMENT)]).expect("offset");
    assert!((-1_439..=1_439).contains(&int_arg(&[offset], 0, "test").expect("minutes")));
    assert_eq!(local_offset_minutes(MOMENT), local_offset_minutes(MOMENT));
}

#[test]
fn host_calls_reject_wrong_arguments() {
    for (name, args) in [
        ("time::__aura_now", vec![int(1)]),
        ("time::__aura_monotonic", vec![int(1)]),
        ("time::__aura_local_offset_minutes", Vec::new()),
        ("time::__aura_local_offset_minutes", vec![Value::Bool(true)]),
        ("time::__aura_duration_seconds", vec![int(1)]),
        ("time::__aura_duration_seconds", Vec::new()),
        ("time::__aura_duration_nanos", vec![int(1)]),
        ("time::__aura_duration_nanos", Vec::new()),
        ("time::__aura_failed", vec![int(1)]),
        ("time::__aura_error", vec![int(1)]),
        ("time::__aura_parsed_seconds", vec![int(1)]),
        ("time::__aura_parsed_nanos", vec![int(1)]),
        ("time::__aura_parsed_offset", vec![int(1)]),
        ("time::__aura_duration", vec![int(1)]),
        ("time::__aura_duration", vec![Value::Bool(true), int(1)]),
        ("time::__aura_rfc3339", vec![int(1)]),
        ("time::__aura_rfc3339", vec![int(1), int(1), Value::Unit]),
        ("time::__aura_format", vec![int(1)]),
        ("time::__aura_format", vec![int(1), int(1), int(1), int(1)]),
        ("time::__aura_parse_rfc3339", Vec::new()),
        ("time::__aura_parse_rfc3339", vec![int(1)]),
        ("time::__aura_parse", vec![Value::string("x")]),
        ("time::__aura_parse", vec![Value::string("x"), int(1)]),
    ] {
        assert!(host(name, args).is_err(), "{name}");
    }
    assert!(time_host_function("time::__aura_nope").is_none());
}
