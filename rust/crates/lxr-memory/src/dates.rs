//! Dates: civil-date arithmetic and the date phrases the retain and recall steps read.
//!
//! No model and no locale: a small fixed set of English phrases, all in UTC.

use time::{Date, Month};

/// Seconds in a day.
pub(crate) const DAY: i64 = 86_400;
/// Julian day number of 1970-01-01.
const UNIX_JULIAN: i64 = 2_440_588;

/// A closed time span in Unix seconds.
pub(crate) type Span = (i64, i64);

/// The system clock in Unix seconds.
pub(crate) fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn date_of(ts: i64) -> Date {
    let jd = ts.div_euclid(DAY) + UNIX_JULIAN;
    i32::try_from(jd).ok().and_then(|jd| Date::from_julian_day(jd).ok()).unwrap_or(Date::MIN)
}

fn midnight_of(date: Date) -> i64 {
    (i64::from(date.to_julian_day()) - UNIX_JULIAN) * DAY
}

fn ymd(year: i32, month: u8, day: u8) -> Option<Date> {
    Date::from_calendar_date(year, Month::try_from(month).ok()?, day).ok()
}

/// Midnight UTC of the day that contains `ts`.
pub(crate) fn day_start(ts: i64) -> i64 {
    ts.div_euclid(DAY) * DAY
}

/// The day that contains `ts` as `YYYY-MM-DD`.
pub(crate) fn iso_date(ts: i64) -> String {
    let date = date_of(ts);
    format!("{:04}-{:02}-{:02}", date.year(), date.month() as u8, date.day())
}

/// Midnight UTC of a calendar day, or `None` when there is no such day.
#[cfg(test)]
pub(crate) fn midnight(year: i32, month: u8, day: u8) -> Option<i64> {
    ymd(year, month, day).map(midnight_of)
}

fn day_span(ts: i64) -> Span {
    let start = day_start(ts);
    (start, start + DAY - 1)
}

fn month_span(year: i32, month: u8) -> Option<Span> {
    let first = ymd(year, month, 1)?;
    let days = i64::from(Month::try_from(month).ok()?.length(year));
    let start = midnight_of(first);
    Some((start, start + days * DAY - 1))
}

/// The Monday-to-Sunday week before the one `now` is in.
fn last_week(now: i64) -> Span {
    let back = i64::from(date_of(now).weekday().number_days_from_monday());
    let monday = day_start(now) - back * DAY;
    (monday - 7 * DAY, monday - 1)
}

/// The calendar month before the one `now` is in.
fn last_month(now: i64) -> Option<Span> {
    let date = date_of(now);
    let month = date.month() as u8;
    if month == 1 {
        month_span(date.year() - 1, 12)
    } else {
        month_span(date.year(), month - 1)
    }
}

/// The first date phrase in `text`, as the span of time it names, read against `now`.
///
/// Understood: ISO dates (`2026-03-05`, `2026/03/05`, and `2026-03` for a whole month),
/// `today`, `yesterday`, `last week` (the Monday-to-Sunday week before this one), `last
/// month`, `N minutes|hours|days|weeks|months|years ago` (the day it falls in), a month name
/// with a day (`March 3`, `Mar 3rd, 2025`, `3 March`, `the 3rd of March`), and a month name
/// with a year (`March 2025`, the whole month). A month and day without a year means the
/// latest such day that is not in the future. A single day is a span of 24 hours.
pub(crate) fn parse_span(text: &str, now: i64) -> Option<Span> {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    (0..words.len()).find_map(|i| match_at(&words, i, now))
}

fn match_at(words: &[String], i: usize, now: i64) -> Option<Span> {
    let word = words[i].as_str();
    let next = |n: usize| words.get(i + n).map(String::as_str);
    if let Some(span) = iso_span(word) {
        return Some(span);
    }
    match word {
        "yesterday" => return Some(day_span(now - DAY)),
        "today" => return Some(day_span(now)),
        "last" => match next(1) {
            Some("week") => return Some(last_week(now)),
            Some("month") => {
                if let Some(span) = last_month(now) {
                    return Some(span);
                }
            }
            _ => {}
        },
        _ => {}
    }
    if let (Some(n), Some(unit), Some("ago")) = (count_word(word), next(1).and_then(unit_seconds), next(2)) {
        return Some(day_span(now - n * unit));
    }
    if let Some(month) = month_number(word) {
        if let Some(second) = next(1) {
            if let Some(day) = day_number(second) {
                return day_of(month, day, next(2).and_then(year_number), now);
            }
            if let Some(year) = year_number(second) {
                return month_span(year, month);
            }
        }
    }
    if let Some(day) = day_number(word) {
        let at = if next(1) == Some("of") { 2 } else { 1 };
        if let Some(month) = next(at).and_then(month_number) {
            return day_of(month, day, next(at + 1).and_then(year_number), now);
        }
    }
    None
}

/// `2026-03-05` or `2026/03/05` is a day, `2026-03` a month.
fn iso_span(word: &str) -> Option<Span> {
    let b = word.as_bytes();
    if !word.is_ascii() || b.len() < 7 {
        return None;
    }
    let digits = |from: usize, to: usize| b[from..to].iter().all(u8::is_ascii_digit);
    if !digits(0, 4) || !matches!(b[4], b'-' | b'/') || !digits(5, 7) {
        return None;
    }
    let number = |from: usize, to: usize| word[from..to].parse::<u32>().ok();
    let year = i32::try_from(number(0, 4)?).ok()?;
    let month = u8::try_from(number(5, 7)?).ok()?;
    if b.len() == 7 {
        return month_span(year, month);
    }
    if b.len() >= 10 && b[7] == b[4] && digits(8, 10) && (b.len() == 10 || !b[10].is_ascii_digit()) {
        let day = u8::try_from(number(8, 10)?).ok()?;
        return ymd(year, month, day).map(|d| day_span(midnight_of(d)));
    }
    None
}

/// A day of a month, in the given year or in the latest year that is not in the future.
fn day_of(month: u8, day: u8, year: Option<i32>, now: i64) -> Option<Span> {
    let year = match year {
        Some(year) => year,
        None => {
            let current = date_of(now).year();
            if midnight_of(ymd(current, month, day)?) > now {
                current - 1
            } else {
                current
            }
        }
    };
    ymd(year, month, day).map(|d| day_span(midnight_of(d)))
}

fn month_number(word: &str) -> Option<u8> {
    Some(match word {
        "january" | "jan" => 1,
        "february" | "feb" => 2,
        "march" | "mar" => 3,
        "april" | "apr" => 4,
        "may" => 5,
        "june" | "jun" => 6,
        "july" | "jul" => 7,
        "august" | "aug" => 8,
        "september" | "sep" | "sept" => 9,
        "october" | "oct" => 10,
        "november" | "nov" => 11,
        "december" | "dec" => 12,
        _ => return None,
    })
}

/// `3`, `03`, `3rd`: a day number from 1 to 31.
fn day_number(word: &str) -> Option<u8> {
    let digits = word.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let suffix = &word[digits.len()..];
    if digits.is_empty() || digits.len() > 2 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !matches!(suffix, "" | "st" | "nd" | "rd" | "th") {
        return None;
    }
    digits.parse::<u8>().ok().filter(|d| (1..=31).contains(d))
}

fn year_number(word: &str) -> Option<i32> {
    if word.len() == 4 && word.bytes().all(|b| b.is_ascii_digit()) {
        word.parse::<i32>().ok().filter(|y| (1900..=2100).contains(y))
    } else {
        None
    }
}

/// A count written as a number or a small number word.
fn count_word(word: &str) -> Option<i64> {
    Some(match word {
        "a" | "an" | "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        "eleven" => 11,
        "twelve" => 12,
        _ => {
            if word.len() <= 4 && !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit()) {
                return word.parse::<i64>().ok();
            }
            return None;
        }
    })
}

fn unit_seconds(word: &str) -> Option<i64> {
    Some(match word {
        "minute" | "minutes" => 60,
        "hour" | "hours" => 3600,
        "day" | "days" => DAY,
        "week" | "weeks" => 7 * DAY,
        "month" | "months" => 30 * DAY,
        "year" | "years" => 365 * DAY,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wednesday 2026-03-11, 12:00 UTC.
    fn now() -> i64 {
        midnight(2026, 3, 11).unwrap() + 12 * 3600
    }

    fn day(y: i32, m: u8, d: u8) -> Span {
        let start = midnight(y, m, d).unwrap();
        (start, start + DAY - 1)
    }

    #[test]
    fn iso_dates_and_months() {
        assert_eq!(parse_span("fixed on 2026-03-05 by hand", now()), Some(day(2026, 3, 5)));
        assert_eq!(parse_span("2025/12/31.", now()), Some(day(2025, 12, 31)));
        assert_eq!(parse_span("at 2026-03-05T10:00:00Z", now()), Some(day(2026, 3, 5)));
        let feb = parse_span("in 2024-02 we shipped", now()).unwrap();
        assert_eq!(feb.0, midnight(2024, 2, 1).unwrap());
        assert_eq!(feb.1, midnight(2024, 3, 1).unwrap() - 1);
        assert_eq!(parse_span("2026-13-45", now()), None);
    }

    #[test]
    fn today_and_yesterday() {
        assert_eq!(parse_span("It broke yesterday.", now()), Some(day(2026, 3, 10)));
        assert_eq!(parse_span("Today we merged it", now()), Some(day(2026, 3, 11)));
    }

    #[test]
    fn last_week_is_the_week_before_this_one() {
        assert_eq!(parse_span("what happened last week?", now()), Some((midnight(2026, 3, 2).unwrap(), midnight(2026, 3, 9).unwrap() - 1)));
    }

    #[test]
    fn last_month_wraps_over_the_new_year() {
        let january = midnight(2026, 1, 20).unwrap();
        let span = parse_span("last month", january).unwrap();
        assert_eq!(span, (midnight(2025, 12, 1).unwrap(), midnight(2026, 1, 1).unwrap() - 1));
    }

    #[test]
    fn n_days_ago_and_friends() {
        assert_eq!(parse_span("3 days ago the cache broke", now()), Some(day(2026, 3, 8)));
        assert_eq!(parse_span("two weeks ago", now()), Some(day(2026, 2, 25)));
        assert_eq!(parse_span("a day ago", now()), Some(day(2026, 3, 10)));
        assert_eq!(parse_span("5 hours ago", now()), Some(day(2026, 3, 11)));
        assert_eq!(parse_span("14 days later", now()), None);
    }

    #[test]
    fn month_names_with_a_day() {
        assert_eq!(parse_span("we met on March 3", now()), Some(day(2026, 3, 3)));
        assert_eq!(parse_span("on Mar 3rd, 2025 we met", now()), Some(day(2025, 3, 3)));
        assert_eq!(parse_span("on 3 March 2024", now()), Some(day(2024, 3, 3)));
        assert_eq!(parse_span("the 3rd of March", now()), Some(day(2026, 3, 3)));
    }

    #[test]
    fn a_month_and_day_in_the_future_means_last_year() {
        assert_eq!(parse_span("on December 25", now()), Some(day(2025, 12, 25)));
        assert_eq!(parse_span("on February 30", now()), None);
    }

    #[test]
    fn month_and_year_is_the_whole_month() {
        let span = parse_span("shipped in March 2025", now()).unwrap();
        assert_eq!(span, (midnight(2025, 3, 1).unwrap(), midnight(2025, 4, 1).unwrap() - 1));
        let leap = parse_span("february 2024", now()).unwrap();
        assert_eq!(leap.1 - leap.0 + 1, 29 * DAY);
    }

    #[test]
    fn text_without_a_date_has_none() {
        assert_eq!(parse_span("the loader reads a file", now()), None);
        assert_eq!(parse_span("may the force be with you", now()), None);
        assert_eq!(parse_span("", now()), None);
    }

    #[test]
    fn the_earliest_phrase_wins() {
        assert_eq!(parse_span("yesterday, not 2026-01-02", now()), Some(day(2026, 3, 10)));
    }

    #[test]
    fn iso_date_formats_a_timestamp() {
        assert_eq!(iso_date(midnight(2026, 3, 5).unwrap() + 3600), "2026-03-05");
        assert_eq!(iso_date(0), "1970-01-01");
        assert_eq!(iso_date(-1), "1969-12-31");
    }
}
