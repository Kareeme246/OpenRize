//! How rize reads times and ranges. Everything is local time; weeks start on
//! Monday, as they do in the app.

use chrono::{
    DateTime, Datelike, Days, Local, LocalResult, Months, NaiveDate, NaiveDateTime, NaiveTime,
    TimeZone, Utc, Weekday,
};

pub const HELP: &str = "a date (2026-09-01), an ISO 8601 date-time (2026-09-01T09:00, local unless it has an offset), today, yesterday, a weekday (monday), this-week, last-week, this-month or last-month";

/// A local calendar span, `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

/// The span a word names, relative to `today`.
pub fn named(word: &str, today: NaiveDate) -> Option<Span> {
    let day = |date: NaiveDate| Span {
        start: date,
        end: date + Days::new(1),
    };
    let week = |monday: NaiveDate| Span {
        start: monday,
        end: monday + Days::new(7),
    };
    let month = |first: NaiveDate| Span {
        start: first,
        end: first + Months::new(1),
    };
    let monday = today - Days::new(u64::from(today.weekday().num_days_from_monday()));
    let first = today.with_day(1)?;
    Some(match word.to_ascii_lowercase().as_str() {
        "today" => day(today),
        "yesterday" => day(today - Days::new(1)),
        "this-week" | "week" => week(monday),
        "last-week" => week(monday - Days::new(7)),
        "this-month" | "month" => month(first),
        "last-month" => month(first - Months::new(1)),
        other => {
            let weekday: Weekday = other.parse().ok()?;
            // The most recent such day, today included.
            let back =
                (7 + today.weekday().num_days_from_monday() - weekday.num_days_from_monday()) % 7;
            day(today - Days::new(u64::from(back)))
        }
    })
}

/// A point in time in epoch milliseconds. A whole day (a date or a word)
/// gives its first millisecond, or with `end` the first millisecond after it.
pub fn instant(input: &str, end: bool, today: NaiveDate) -> Result<u64, String> {
    let error = || format!("expected {HELP}, not \"{input}\"");
    let span = if let Some(span) = named(input, today) {
        Some(span)
    } else if let Ok(date) = NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        Some(Span {
            start: date,
            end: date.succ_opt().ok_or_else(error)?,
        })
    } else {
        None
    };
    let utc = match span {
        Some(span) => midnight(if end { span.end } else { span.start }).ok_or_else(error)?,
        None if !input.contains('T') => return Err(error()),
        None => {
            if let Ok(date) = DateTime::parse_from_rfc3339(input) {
                date.with_timezone(&Utc)
            } else {
                let naive = ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M"]
                    .iter()
                    .find_map(|format| NaiveDateTime::parse_from_str(input, format).ok())
                    .ok_or_else(error)?;
                local(naive).ok_or_else(|| format!("{input} does not exist in local time"))?
            }
        }
    };
    if utc.timestamp_subsec_nanos() % 1_000_000 != 0 {
        return Err(error());
    }
    u64::try_from(utc.timestamp_millis()).map_err(|_| "times must be on or after 1970-01-01".into())
}

/// `[from, to)` from the range flags: `--period`, or `--from`/`--to` (each
/// defaulting to its side of `default`), or `--last`.
pub fn range(
    period: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
    last: Option<&str>,
    default: &str,
    today: NaiveDate,
) -> Result<(u64, u64), String> {
    let (from, to) = if let Some(period) = period {
        named(period, today).ok_or_else(|| format!("unknown period \"{period}\""))?;
        (
            instant(period, false, today)?,
            instant(period, true, today)?,
        )
    } else if let Some(last) = last {
        let now = Utc::now().timestamp_millis() as u64;
        (now.saturating_sub(duration(last)?), now + 1)
    } else {
        (
            instant(from.unwrap_or(default), false, today)?,
            instant(to.unwrap_or(from.unwrap_or(default)), true, today)?,
        )
    };
    if to <= from {
        return Err("--to must be after --from".into());
    }
    Ok((from, to))
}

/// `90m`, `12h`, `7d` or `2w`, in milliseconds.
pub fn duration(input: &str) -> Result<u64, String> {
    let error = || format!("expected a duration such as 90m, 12h, 7d or 2w, not \"{input}\"");
    let split = input.len().checked_sub(1).ok_or_else(error)?;
    let (amount, unit) = input.split_at(split);
    let amount: u64 = amount.parse().map_err(|_| error())?;
    let unit_ms = match unit {
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        "w" => 7 * 86_400_000,
        _ => return Err(error()),
    };
    amount.checked_mul(unit_ms).ok_or_else(error)
}

/// The local midnights that split `[from, to)` into days, weeks or months.
pub fn boundaries(from: u64, to: u64, per: &str) -> Result<Vec<u64>, String> {
    if per == "total" {
        return Ok(vec![from, to]);
    }
    let first = date_of(from);
    let mut cursor = match per {
        "day" => first,
        "week" => first - Days::new(u64::from(first.weekday().num_days_from_monday())),
        "month" => first.with_day(1).ok_or("bad date")?,
        other => {
            return Err(format!(
                "unknown period \"{other}\"; use day, week, month or total"
            ))
        }
    };
    let mut points = vec![from];
    loop {
        cursor = match per {
            "day" => cursor + Days::new(1),
            "week" => cursor + Days::new(7),
            _ => cursor + Months::new(1),
        };
        let point = midnight(cursor).ok_or("bad date")?.timestamp_millis() as u64;
        if point >= to {
            points.push(to);
            return Ok(points);
        }
        points.push(point);
    }
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// The local date an instant falls on.
pub fn date_of(ms: u64) -> NaiveDate {
    Local
        .timestamp_millis_opt(ms as i64)
        .single()
        .map(|time| time.date_naive())
        .unwrap_or_default()
}

pub fn day_start(date: NaiveDate) -> u64 {
    midnight(date).map_or(0, |time| time.timestamp_millis() as u64)
}

fn midnight(date: NaiveDate) -> Option<DateTime<Utc>> {
    local(date.and_time(NaiveTime::MIN))
}

/// A local wall-clock time in UTC. A time repeated by a DST change resolves
/// to its first occurrence; one skipped by it does not exist.
fn local(naive: NaiveDateTime) -> Option<DateTime<Utc>> {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(time) | LocalResult::Ambiguous(time, _) => {
            Some(time.with_timezone(&Utc))
        }
        LocalResult::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, day).unwrap()
    }

    #[test]
    fn instants() {
        let today = date(10);
        assert_eq!(
            instant("2026-09-01T01:00:00+01:00", false, today).unwrap(),
            instant("2026-09-01T00:00:00Z", false, today).unwrap()
        );
        assert_eq!(
            instant("2026-09-01T00:00", false, today).unwrap(),
            day_start(date(1))
        );
        assert_eq!(
            instant("2026-09-01", false, today).unwrap(),
            day_start(date(1))
        );
        assert_eq!(
            instant("2026-09-01", true, today).unwrap(),
            day_start(date(2))
        );
        assert_eq!(instant("today", false, today).unwrap(), day_start(date(10)));
        assert_eq!(
            instant("yesterday", true, today).unwrap(),
            day_start(date(10))
        );
        for value in [
            "2026-09-32",
            "09/01/2026",
            "2026-09-01T00:00:00.0001Z",
            "1969-01-01",
            "someday",
        ] {
            assert!(instant(value, false, today).is_err(), "accepted {value}");
        }
    }

    #[test]
    fn named_spans() {
        // 2026-09-10 is a Thursday.
        let today = date(10);
        assert_eq!(named("monday", today).unwrap().start, date(7));
        assert_eq!(named("thursday", today).unwrap().start, date(10));
        assert_eq!(named("friday", today).unwrap().start, date(4));
        let week = named("this-week", today).unwrap();
        assert_eq!((week.start, week.end), (date(7), date(14)));
        let last = named("last-week", today).unwrap();
        let august_31 = NaiveDate::from_ymd_opt(2026, 8, 31).unwrap();
        assert_eq!((last.start, last.end), (august_31, date(7)));
        let month = named("last-month", today).unwrap();
        assert_eq!(month.start, NaiveDate::from_ymd_opt(2026, 8, 1).unwrap());
        assert_eq!(month.end, date(1));
    }

    #[test]
    fn ranges_and_buckets() {
        let today = date(10);
        let (from, to) = range(None, Some("2026-09-01"), None, None, "today", today).unwrap();
        assert_eq!((from, to), (day_start(date(1)), day_start(date(2))));
        assert!(range(None, Some("today"), Some("yesterday"), None, "today", today).is_err());
        let (from, to) = range(Some("this-week"), None, None, None, "today", today).unwrap();
        let days = boundaries(from, to, "day").unwrap();
        assert_eq!(days.len(), 8);
        assert_eq!(boundaries(from, to, "total").unwrap(), vec![from, to]);
        assert_eq!(duration("2w").unwrap(), 14 * 86_400_000);
        assert!(duration("5y").is_err());
    }
}
