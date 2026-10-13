//! Dates and ages: the date forms the flags accept, and the short age words in the output.

use std::time::{SystemTime, UNIX_EPOCH};

/// The current time in Unix seconds.
pub fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// The current time in nanoseconds since the Unix epoch.
pub fn now_nanos() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0)
}

/// How long ago a time was, in the short form of the status line: under 90 seconds
/// `Ns`, under 5,400 `Nm`, under 172,800 `Nh`, otherwise `Nd`. A time in the future
/// counts as now.
pub fn age_word(seconds: i64) -> String {
    let s = seconds.max(0);
    if s < 90 {
        format!("{s}s")
    } else if s < 5_400 {
        format!("{}m", s / 60)
    } else if s < 172_800 {
        format!("{}h", s / 3_600)
    } else {
        format!("{}d", s / 86_400)
    }
}

/// Days from 1970-01-01 to the given civil date (proleptic Gregorian calendar).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
            if leap {
                29
            } else {
                28
            }
        }
    }
}

/// Parses a number made only of ASCII digits, with a length limit.
fn digits(text: &str, min_len: usize, max_len: usize) -> Option<i64> {
    if text.len() < min_len || text.len() > max_len || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// A date as the flags take it, in Unix seconds (UTC).
///
/// Forms: `2026-10-12`, `2026-10-12 14:30`, `2026-10-12T14:30:05`, with an optional
/// trailing `Z`. A date without a time is midnight at its start, or with `end_of_day`
/// the last second of that day (used for `--until`). `None` when the text is not a date.
pub fn parse_date(text: &str, end_of_day: bool) -> Option<i64> {
    let text = text.trim();
    let text = text.strip_suffix('Z').or_else(|| text.strip_suffix('z')).unwrap_or(text);
    let (date, time) = match text.find(|c| c == 'T' || c == 't' || c == ' ') {
        Some(i) => (&text[..i], Some(text[i + 1..].trim())),
        None => (text, None),
    };
    let mut parts = date.split('-');
    let year = digits(parts.next()?, 4, 4)?;
    let month = digits(parts.next()?, 1, 2)?;
    let day = digits(parts.next()?, 1, 2)?;
    if parts.next().is_some() || !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    let mut seconds = days_from_civil(year, month, day) * 86_400;
    match time {
        None => {
            if end_of_day {
                seconds += 86_399;
            }
        }
        Some(clock) => {
            let mut fields = clock.split(':');
            let hour = digits(fields.next()?, 1, 2)?;
            let minute = digits(fields.next()?, 1, 2)?;
            let second = match fields.next() {
                Some(s) => digits(s, 1, 2)?,
                None => 0,
            };
            if fields.next().is_some() || hour > 23 || minute > 59 || second > 59 {
                return None;
            }
            seconds += hour * 3_600 + minute * 60 + second;
        }
    }
    Some(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn age_words_follow_the_status_line_thresholds() {
        assert_eq!(age_word(0), "0s");
        assert_eq!(age_word(89), "89s");
        assert_eq!(age_word(90), "1m");
        assert_eq!(age_word(5_399), "89m");
        assert_eq!(age_word(5_400), "1h");
        assert_eq!(age_word(172_799), "47h");
        assert_eq!(age_word(172_800), "2d");
        assert_eq!(age_word(-5), "0s");
    }

    #[test]
    fn dates_parse_to_unix_seconds() {
        assert_eq!(parse_date("1970-01-01", false), Some(0));
        assert_eq!(parse_date("1970-01-02", false), Some(86_400));
        assert_eq!(parse_date("2000-03-01", false), Some(951_868_800));
        assert_eq!(parse_date("2026-10-12T14:30:05Z", false), Some(1_791_815_405));
        assert_eq!(parse_date("2026-10-12 14:30", false), Some(1_791_815_400));
        assert_eq!(parse_date("2026-10-12", true), Some(1_791_815_400 - 14 * 3_600 - 30 * 60 + 86_399));
    }

    #[test]
    fn bad_dates_are_refused() {
        for bad in ["", "yesterday", "2026-13-01", "2026-02-30", "2026-10", "26-10-12", "2026-10-12 25:00", "2026-10-12T10"] {
            assert_eq!(parse_date(bad, false), None, "{bad}");
        }
    }
}
