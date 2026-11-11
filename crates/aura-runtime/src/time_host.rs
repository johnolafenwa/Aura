//! The host part of the `time` module.
//!
//! `std/time.au` keeps a `time.DateTime` as Unix seconds, nanoseconds, and an
//! offset in minutes, and does its calendar arithmetic in Aura. The
//! functions here read the clocks and the system time zone, and write and
//! read date and time text.

use super::*;

const NANOS_PER_SECOND: i128 = 1_000_000_000;
const SECONDS_PER_DAY: i64 = 86_400;
/// 0001-01-01T00:00:00Z and 9999-12-31T23:59:59Z as Unix seconds.
const MIN_UNIX_SECONDS: i64 = -62_135_596_800;
const MAX_UNIX_SECONDS: i64 = 253_402_300_799;

const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

fn int(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

fn tuple(element_types: Vec<Type>, elements: Vec<Value>) -> Value {
    Value::Tuple(TupleValue {
        element_types,
        elements,
    })
}

fn int_arg(args: &[Value], index: usize, name: &str) -> Result<i64> {
    match &args[index] {
        Value::Int(value) => value.as_i128().and_then(|value| i64::try_from(value).ok()),
        _ => None,
    }
    .ok_or_else(|| {
        Diagnostic::coded(
            "AU4001",
            format!("`{name}` expects argument {} to be `int64`", index + 1),
        )
    })
}

/// Days from 1970-01-01 to a proleptic Gregorian date.
pub(crate) fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month = if month <= 2 { month + 9 } else { month - 3 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The year, month, and day of a count of days from 1970-01-01.
pub(crate) fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let (year, month) = if shifted_month >= 10 {
        (year_of_era + era * 400 + 1, shifted_month - 9)
    } else {
        (year_of_era + era * 400, shifted_month + 3)
    };
    (year, month, day)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The calendar fields of an instant read at an offset.
struct Fields {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    nanos: i64,
    offset_minutes: i64,
    /// 0 for Monday to 6 for Sunday.
    weekday: usize,
    day_of_year: i64,
}

impl Fields {
    fn new(unix_seconds: i64, nanos: i64, offset_minutes: i64) -> Self {
        let local = unix_seconds.saturating_add(offset_minutes.saturating_mul(60));
        let days = local.div_euclid(SECONDS_PER_DAY);
        let of_day = local.rem_euclid(SECONDS_PER_DAY);
        let (year, month, day) = civil_from_days(days);
        Self {
            year,
            month,
            day,
            hour: of_day / 3_600,
            minute: of_day % 3_600 / 60,
            second: of_day % 60,
            nanos,
            offset_minutes,
            weekday: (days + 3).rem_euclid(7) as usize,
            day_of_year: days - days_from_civil(year, 1, 1) + 1,
        }
    }

    fn push_offset(&self, out: &mut String, colon: bool, zulu: bool) {
        use std::fmt::Write;
        if zulu && self.offset_minutes == 0 {
            out.push('Z');
            return;
        }
        let sign = if self.offset_minutes < 0 { '-' } else { '+' };
        let minutes = self.offset_minutes.abs();
        let separator = if colon { ":" } else { "" };
        let _ = write!(
            out,
            "{sign}{:02}{separator}{:02}",
            minutes / 60,
            minutes % 60
        );
    }

    fn rfc3339(&self) -> String {
        use std::fmt::Write;
        let mut out = String::with_capacity(35);
        let _ = write!(
            out,
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        );
        if self.nanos != 0 {
            if self.nanos % 1_000_000 == 0 {
                let _ = write!(out, ".{:03}", self.nanos / 1_000_000);
            } else if self.nanos % 1_000 == 0 {
                let _ = write!(out, ".{:06}", self.nanos / 1_000);
            } else {
                let _ = write!(out, ".{:09}", self.nanos);
            }
        }
        self.push_offset(&mut out, true, true);
        out
    }

    fn format(&self, pattern: &str) -> std::result::Result<String, String> {
        use std::fmt::Write;
        let mut out = String::with_capacity(pattern.len() + 16);
        let mut chars = pattern.chars();
        while let Some(ch) = chars.next() {
            if ch != '%' {
                out.push(ch);
                continue;
            }
            let Some(directive) = chars.next() else {
                return Err("the pattern ends with a lone `%`".to_string());
            };
            let _ = match directive {
                'Y' => write!(out, "{:04}", self.year),
                'y' => write!(out, "{:02}", self.year % 100),
                'm' => write!(out, "{:02}", self.month),
                'd' => write!(out, "{:02}", self.day),
                'e' => write!(out, "{:2}", self.day),
                'H' => write!(out, "{:02}", self.hour),
                'I' => write!(out, "{:02}", (self.hour + 11) % 12 + 1),
                'p' => write!(out, "{}", if self.hour < 12 { "AM" } else { "PM" }),
                'M' => write!(out, "{:02}", self.minute),
                'S' => write!(out, "{:02}", self.second),
                'f' => write!(out, "{:06}", self.nanos / 1_000),
                'j' => write!(out, "{:03}", self.day_of_year),
                'a' => write!(out, "{}", &WEEKDAYS[self.weekday][..3]),
                'A' => write!(out, "{}", WEEKDAYS[self.weekday]),
                'b' => write!(out, "{}", &MONTHS[self.month as usize - 1][..3]),
                'B' => write!(out, "{}", MONTHS[self.month as usize - 1]),
                'z' => {
                    self.push_offset(&mut out, false, false);
                    Ok(())
                }
                ':' => {
                    if chars.next() != Some('z') {
                        return Err("`%:` must be followed by `z`".to_string());
                    }
                    self.push_offset(&mut out, true, false);
                    Ok(())
                }
                '%' => {
                    out.push('%');
                    Ok(())
                }
                other => return Err(format!("unknown directive `%{other}`")),
            };
        }
        Ok(out)
    }
}

/// A cursor over the text being parsed.
struct Reader<'a> {
    rest: &'a str,
}

impl<'a> Reader<'a> {
    /// Reads from `min` to `max` ASCII digits as a number.
    fn digits(&mut self, min: usize, max: usize, what: &str) -> std::result::Result<i64, String> {
        let count = self
            .rest
            .bytes()
            .take(max)
            .take_while(u8::is_ascii_digit)
            .count();
        if count < min {
            return Err(format!("expected {what} at `{}`", self.preview()));
        }
        let (digits, rest) = self.rest.split_at(count);
        self.rest = rest;
        Ok(digits
            .bytes()
            .fold(0, |value, digit| value * 10 + i64::from(digit - b'0')))
    }

    fn preview(&self) -> String {
        self.rest.chars().take(12).collect()
    }

    fn literal(&mut self, expected: char) -> std::result::Result<(), String> {
        match self.rest.strip_prefix(expected) {
            Some(rest) => {
                self.rest = rest;
                Ok(())
            }
            None => Err(format!("expected `{expected}` at `{}`", self.preview())),
        }
    }

    /// Reads one of `names`, in full or as its first three letters,
    /// ignoring ASCII case, and returns its index.
    fn name(
        &mut self,
        names: &[&str],
        short: bool,
        what: &str,
    ) -> std::result::Result<usize, String> {
        for (index, name) in names.iter().enumerate() {
            let name = if short { &name[..3] } else { name };
            if self.rest.len() >= name.len()
                && self.rest.is_char_boundary(name.len())
                && self.rest[..name.len()].eq_ignore_ascii_case(name)
            {
                self.rest = &self.rest[name.len()..];
                return Ok(index);
            }
        }
        Err(format!("expected {what} at `{}`", self.preview()))
    }

    /// Reads a fraction of a second of one to nine digits as nanoseconds.
    fn fraction(&mut self) -> std::result::Result<i64, String> {
        let before = self.rest.len();
        let value = self.digits(1, 9, "a fraction of a second")?;
        let digits = before - self.rest.len();
        Ok(value * 10_i64.pow(9 - digits as u32))
    }

    /// Reads `+hh:mm`, `+hhmm`, or `Z` as minutes east of UTC.
    fn offset(&mut self, allow_zulu: bool) -> std::result::Result<i64, String> {
        if allow_zulu {
            if let Some(rest) = self
                .rest
                .strip_prefix('Z')
                .or_else(|| self.rest.strip_prefix('z'))
            {
                self.rest = rest;
                return Ok(0);
            }
        }
        let sign = if self.rest.starts_with('+') {
            1
        } else if self.rest.starts_with('-') {
            -1
        } else {
            return Err(format!("expected a UTC offset at `{}`", self.preview()));
        };
        self.rest = &self.rest[1..];
        let hours = self.digits(2, 2, "two offset hour digits")?;
        if let Some(rest) = self.rest.strip_prefix(':') {
            self.rest = rest;
        }
        let minutes = self.digits(2, 2, "two offset minute digits")?;
        if hours > 23 || minutes > 59 {
            return Err(format!("offset {hours:02}:{minutes:02} is out of range"));
        }
        Ok(sign * (hours * 60 + minutes))
    }
}

/// The fields a parse collected, before they are checked together.
#[derive(Default)]
struct Parsed {
    year: Option<i64>,
    month: Option<i64>,
    day: Option<i64>,
    hour: Option<i64>,
    hour12: Option<i64>,
    afternoon: Option<bool>,
    minute: Option<i64>,
    second: Option<i64>,
    nanos: i64,
    offset_minutes: i64,
}

impl Parsed {
    /// The instant the fields name: Unix seconds, nanoseconds, and offset.
    fn finish(self) -> std::result::Result<(i64, i64, i64), String> {
        let year = self.year.ok_or("the pattern reads no year")?;
        let month = self.month.unwrap_or(1);
        let day = self.day.unwrap_or(1);
        let hour = match (self.hour, self.hour12) {
            (Some(hour), _) => hour,
            (None, Some(hour12)) => {
                if !(1..=12).contains(&hour12) {
                    return Err(format!("hour {hour12} is outside 1 to 12"));
                }
                hour12 % 12 + if self.afternoon == Some(true) { 12 } else { 0 }
            }
            (None, None) => 0,
        };
        let minute = self.minute.unwrap_or(0);
        let second = self.second.unwrap_or(0);
        if !(1..=9999).contains(&year) {
            return Err(format!("year {year} is outside 1 to 9999"));
        }
        if !(1..=12).contains(&month) {
            return Err(format!("month {month} is outside 1 to 12"));
        }
        if day < 1 || day > days_in_month(year, month) {
            return Err(format!("day {day} is not in month {month} of {year}"));
        }
        if hour > 23 {
            return Err(format!("hour {hour} is outside 0 to 23"));
        }
        if minute > 59 {
            return Err(format!("minute {minute} is outside 0 to 59"));
        }
        if second > 59 {
            return Err(format!("second {second} is outside 0 to 59"));
        }
        let seconds = days_from_civil(year, month, day) * SECONDS_PER_DAY
            + hour * 3_600
            + minute * 60
            + second
            - self.offset_minutes * 60;
        if !(MIN_UNIX_SECONDS..=MAX_UNIX_SECONDS).contains(&seconds) {
            return Err("the year is outside 1 to 9999".to_string());
        }
        Ok((seconds, self.nanos, self.offset_minutes))
    }
}

fn parse_rfc3339(text: &str) -> std::result::Result<(i64, i64, i64), String> {
    let mut reader = Reader { rest: text };
    let mut parsed = Parsed {
        year: Some(reader.digits(4, 4, "four year digits")?),
        ..Parsed::default()
    };
    reader.literal('-')?;
    parsed.month = Some(reader.digits(2, 2, "two month digits")?);
    reader.literal('-')?;
    parsed.day = Some(reader.digits(2, 2, "two day digits")?);
    match reader.rest.chars().next() {
        Some('T' | 't' | ' ') => reader.rest = &reader.rest[1..],
        _ => return Err(format!("expected `T` at `{}`", reader.preview())),
    }
    parsed.hour = Some(reader.digits(2, 2, "two hour digits")?);
    reader.literal(':')?;
    parsed.minute = Some(reader.digits(2, 2, "two minute digits")?);
    reader.literal(':')?;
    parsed.second = Some(reader.digits(2, 2, "two second digits")?);
    if let Some(rest) = reader.rest.strip_prefix('.') {
        reader.rest = rest;
        parsed.nanos = reader.fraction()?;
        // Digits past the ninth are below a nanosecond and are dropped.
        reader.rest = reader
            .rest
            .trim_start_matches(|ch: char| ch.is_ascii_digit());
    }
    parsed.offset_minutes = reader.offset(true)?;
    if !reader.rest.is_empty() {
        return Err(format!("unexpected text `{}` at the end", reader.preview()));
    }
    parsed.finish()
}

fn parse_with_pattern(text: &str, pattern: &str) -> std::result::Result<(i64, i64, i64), String> {
    let mut reader = Reader { rest: text };
    let mut parsed = Parsed::default();
    let mut chars = pattern.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            if ch.is_whitespace() {
                reader.rest = reader.rest.trim_start();
            } else {
                reader.literal(ch)?;
            }
            continue;
        }
        let Some(directive) = chars.next() else {
            return Err("the pattern ends with a lone `%`".to_string());
        };
        match directive {
            'Y' => parsed.year = Some(reader.digits(4, 4, "four year digits")?),
            'y' => {
                // As POSIX reads two digits: 69 to 99 are the 1900s.
                let short = reader.digits(2, 2, "two year digits")?;
                parsed.year = Some(if short >= 69 {
                    1900 + short
                } else {
                    2000 + short
                });
            }
            'm' => parsed.month = Some(reader.digits(1, 2, "a month")?),
            'd' => parsed.day = Some(reader.digits(1, 2, "a day")?),
            'e' => {
                reader.rest = reader.rest.trim_start();
                parsed.day = Some(reader.digits(1, 2, "a day")?);
            }
            'H' => parsed.hour = Some(reader.digits(1, 2, "an hour")?),
            'I' => parsed.hour12 = Some(reader.digits(1, 2, "an hour")?),
            'p' => parsed.afternoon = Some(reader.name(&["AM", "PM"], false, "AM or PM")? == 1),
            'M' => parsed.minute = Some(reader.digits(1, 2, "a minute")?),
            'S' => parsed.second = Some(reader.digits(1, 2, "a second")?),
            'f' => parsed.nanos = reader.fraction()?,
            'a' => {
                reader.name(&WEEKDAYS, true, "a weekday name")?;
            }
            'A' => {
                reader.name(&WEEKDAYS, false, "a weekday name")?;
            }
            'b' => parsed.month = Some(reader.name(&MONTHS, true, "a month name")? as i64 + 1),
            'B' => parsed.month = Some(reader.name(&MONTHS, false, "a month name")? as i64 + 1),
            'z' => parsed.offset_minutes = reader.offset(true)?,
            ':' => {
                if chars.next() != Some('z') {
                    return Err("`%:` must be followed by `z`".to_string());
                }
                parsed.offset_minutes = reader.offset(true)?;
            }
            '%' => reader.literal('%')?,
            'j' => return Err("`%j` cannot be parsed".to_string()),
            other => return Err(format!("unknown directive `%{other}`")),
        }
    }
    if !reader.rest.is_empty() {
        return Err(format!("unexpected text `{}` at the end", reader.preview()));
    }
    parsed.finish()
}

/// The `(bool, int64, int64, int64, str)` a parse returns to `std/time.au`.
fn parse_result(outcome: std::result::Result<(i64, i64, i64), String>) -> Value {
    let (ok, (seconds, nanos, offset), message) = match outcome {
        Ok(instant) => (true, instant, String::new()),
        Err(message) => (false, (0, 0, 0), message),
    };
    let int64 = || Type::named("int64");
    tuple(
        vec![
            Type::named("bool"),
            int64(),
            int64(),
            int64(),
            Type::named("str"),
        ],
        vec![
            Value::Bool(ok),
            int(seconds),
            int(nanos),
            int(offset),
            Value::string(message),
        ],
    )
}

/// Minutes east of UTC that the system time zone has at `unix_seconds`.
fn local_offset_minutes(unix_seconds: i64) -> i64 {
    #[cfg(unix)]
    {
        // Every instant a `DateTime` holds fits a 64-bit `time_t`.
        let instant = unix_seconds as libc::time_t;
        // SAFETY: `localtime_r` writes only the `tm` it is given and reads
        // the time zone database under libc's own lock.
        let seconds = unsafe {
            let mut fields: libc::tm = std::mem::zeroed();
            if libc::localtime_r(&instant, &mut fields).is_null() {
                return 0;
            }
            i64::from(fields.tm_gmtoff)
        };
        // Offsets with seconds, used before standard time, round down.
        seconds.div_euclid(60).clamp(-1_439, 1_439)
    }
    #[cfg(not(unix))]
    {
        let _ = unix_seconds;
        0
    }
}

/// `time::__aura_now`: Unix seconds and nanoseconds from the system clock.
pub fn host_time_now(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_expect_arity("time::__aura_now", &args, 0)?;
    let (seconds, nanos) = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
    {
        Ok(since) => (since.as_secs() as i64, i64::from(since.subsec_nanos())),
        // A clock set before 1970 reads as the epoch.
        Err(_) => (0, 0),
    };
    let int64 = || Type::named("int64");
    Ok(tuple(
        vec![int64(), int64()],
        vec![int(seconds.min(MAX_UNIX_SECONDS)), int(nanos)],
    ))
}

/// `time::__aura_monotonic`: time since the first reading in this process,
/// plus one hour so that a reading is never zero.
pub fn host_time_monotonic(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_expect_arity("time::__aura_monotonic", &args, 0)?;
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let elapsed = START.get_or_init(std::time::Instant::now).elapsed();
    Ok(Value::Duration(
        elapsed.as_nanos() as i128 + 3_600 * NANOS_PER_SECOND,
    ))
}

/// `time::__aura_local_offset_minutes`.
pub fn host_time_local_offset_minutes(
    args: Vec<Value>,
    _program_args: Option<&[String]>,
) -> Result<Value> {
    let name = "time::__aura_local_offset_minutes";
    host_expect_arity(name, &args, 1)?;
    Ok(int(local_offset_minutes(int_arg(&args, 0, name)?)))
}

/// `time::__aura_duration_parts`: a Duration as whole seconds, rounded
/// down, and the nanoseconds past them.
pub fn host_time_duration_parts(
    args: Vec<Value>,
    _program_args: Option<&[String]>,
) -> Result<Value> {
    let name = "time::__aura_duration_parts";
    host_expect_arity(name, &args, 1)?;
    let Value::Duration(nanos) = &args[0] else {
        return Err(Diagnostic::coded(
            "AU4001",
            format!("`{name}` expects a `Duration`"),
        ));
    };
    let seconds = nanos
        .div_euclid(NANOS_PER_SECOND)
        .clamp(i128::from(i64::MIN / 4), i128::from(i64::MAX / 4)) as i64;
    let int64 = || Type::named("int64");
    Ok(tuple(
        vec![int64(), int64()],
        vec![int(seconds), int(nanos.rem_euclid(NANOS_PER_SECOND) as i64)],
    ))
}

/// `time::__aura_duration`: the Duration of `seconds` and `nanos`.
pub fn host_time_duration(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "time::__aura_duration";
    host_expect_arity(name, &args, 2)?;
    let seconds = i128::from(int_arg(&args, 0, name)?);
    let nanos = i128::from(int_arg(&args, 1, name)?);
    Ok(Value::Duration(seconds * NANOS_PER_SECOND + nanos))
}

/// `time::__aura_rfc3339`.
pub fn host_time_rfc3339(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "time::__aura_rfc3339";
    host_expect_arity(name, &args, 3)?;
    let fields = Fields::new(
        int_arg(&args, 0, name)?,
        int_arg(&args, 1, name)?,
        int_arg(&args, 2, name)?,
    );
    Ok(Value::string(fields.rfc3339()))
}

/// `time::__aura_format`: `(true, text)`, or `(false, reason)` for a
/// pattern that cannot be followed.
pub fn host_time_format(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "time::__aura_format";
    host_expect_arity(name, &args, 4)?;
    let fields = Fields::new(
        int_arg(&args, 0, name)?,
        int_arg(&args, 1, name)?,
        int_arg(&args, 2, name)?,
    );
    let (ok, text) = match fields.format(host_string_ref_arg(&args, 3, name)?) {
        Ok(text) => (true, text),
        Err(message) => (false, message),
    };
    Ok(tuple(
        vec![Type::named("bool"), Type::named("str")],
        vec![Value::Bool(ok), Value::string(text)],
    ))
}

/// `time::__aura_parse_rfc3339`.
pub fn host_time_parse_rfc3339(
    args: Vec<Value>,
    _program_args: Option<&[String]>,
) -> Result<Value> {
    let name = "time::__aura_parse_rfc3339";
    host_expect_arity(name, &args, 1)?;
    Ok(parse_result(parse_rfc3339(host_string_ref_arg(
        &args, 0, name,
    )?)))
}

/// `time::__aura_parse`.
pub fn host_time_parse(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "time::__aura_parse";
    host_expect_arity(name, &args, 2)?;
    Ok(parse_result(parse_with_pattern(
        host_string_ref_arg(&args, 0, name)?,
        host_string_ref_arg(&args, 1, name)?,
    )))
}

/// The interpreter's entry for a `time` host builtin.
pub fn time_host_function(name: &str) -> Option<host_builtins::HostBuiltinFn> {
    Some(match name {
        "time::__aura_now" => host_time_now,
        "time::__aura_monotonic" => host_time_monotonic,
        "time::__aura_local_offset_minutes" => host_time_local_offset_minutes,
        "time::__aura_duration_parts" => host_time_duration_parts,
        "time::__aura_duration" => host_time_duration,
        "time::__aura_rfc3339" => host_time_rfc3339,
        "time::__aura_format" => host_time_format,
        "time::__aura_parse_rfc3339" => host_time_parse_rfc3339,
        "time::__aura_parse" => host_time_parse,
        _ => return None,
    })
}

#[cfg(test)]
#[path = "time_host_tests.rs"]
mod tests;
