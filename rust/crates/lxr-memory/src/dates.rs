//! Dates: civil-date arithmetic and the date phrases the retain and recall steps read.
//!
//! No model and no locale: a small fixed set of English phrases. The calendar is the caller's:
//! `today` is the calendar day that contains the clock in the time zone of the caller, who gives
//! that zone as an offset from UTC (`utc_offset` in the retain and recall options). With an offset
//! of 0 the day is the UTC day. The spans that come back are always Unix seconds, UTC.

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

/// The day that is `months` calendar months before the one `now` is in. It keeps the day of
/// the month, or takes the last day of the month when that month is shorter, so `12 months
/// ago` and `1 year ago` name one day and a leap day is not a day off.
fn months_ago(now: i64, months: i64) -> Option<Span> {
    let date = date_of(now);
    let total = i64::from(date.year()) * 12 + i64::from(date.month() as u8) - 1 - months;
    let year = i32::try_from(total.div_euclid(12)).ok()?;
    let month = total.rem_euclid(12) as u8 + 1;
    let last = Month::try_from(month).ok()?.length(year);
    let day = ymd(year, month, date.day().min(last))?;
    Some(day_span(midnight_of(day)))
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

/// The first date phrase in `text`, as the span of time it names, read against `now` in the time
/// zone that is `utc_offset` seconds east of UTC (negative to the west; a value beyond a day
/// either way is cut to a day). Days, weeks and months are the ones of that zone: with an
/// offset of -18,000 (UTC-5) and a clock at 02:00 UTC on the 11th, `today` is the span that
/// starts at 05:00 UTC on the 10th. The span is in Unix seconds, UTC.
///
/// Understood: ISO dates (`2026-03-05`, `2026/03/05`, and `2026-03` for a whole month),
/// `today`, `yesterday`, `last week` (the Monday-to-Sunday week before this one), `last
/// month`, `N minutes|hours|days|weeks ago` (the day it falls in), `N months|years ago` (the
/// same day of the month that many calendar months back, or the last day of a shorter month;
/// a year is twelve months), a month name
/// with a day (`March 3`, `Mar 3rd, 2025`, `3 March`, `the 3rd of March`), and a month name
/// with a year (`March 2025`, the whole month). A month and day without a year means the
/// latest such day that is not in the future. A single day is a span of 24 hours.
pub(crate) fn parse_span(text: &str, now: i64, utc_offset: i64) -> Option<Span> {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    // The calendar arithmetic below counts days from midnight UTC. Shifting the clock by the
    // offset puts the local midnights on those boundaries; the span is shifted back at the end.
    let offset = utc_offset.clamp(-DAY, DAY);
    let local_now = now.saturating_add(offset);
    let (from, to) = (0..words.len()).find_map(|i| match_at(&words, i, local_now))?;
    Some((from - offset, to - offset))
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
    if let (Some(n), Some("ago")) = (count_word(word), next(2)) {
        match next(1) {
            Some("month" | "months") => return months_ago(now, n),
            Some("year" | "years") => return months_ago(now, n * 12),
            _ => {}
        }
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
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The phrase read in UTC, which most of these tests are about.
    fn parse_span(text: &str, now: i64) -> Option<Span> {
        super::parse_span(text, now, 0)
    }

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
    fn months_and_years_ago_count_calendar_months_and_years() {
        // 2026-03-11: twelve months and one year are the same day.
        assert_eq!(parse_span("12 months ago", now()), Some(day(2025, 3, 11)));
        assert_eq!(parse_span("1 year ago", now()), Some(day(2025, 3, 11)));
        assert_eq!(parse_span("three years ago", now()), Some(day(2023, 3, 11)));
        assert_eq!(parse_span("a month ago", now()), Some(day(2026, 2, 11)));
        assert_eq!(parse_span("2 months ago", now()), Some(day(2026, 1, 11)));
        assert_eq!(parse_span("4 months ago", now()), Some(day(2025, 11, 11)));
        // A day the earlier month lacks falls on that month's last day.
        let end_of_march = midnight(2026, 3, 31).unwrap() + 6 * 3600;
        assert_eq!(parse_span("a month ago", end_of_march), Some(day(2026, 2, 28)));
        let leap_day = midnight(2028, 2, 29).unwrap() + 6 * 3600;
        assert_eq!(parse_span("4 years ago", leap_day), Some(day(2024, 2, 29)));
        assert_eq!(parse_span("1 year ago", leap_day), Some(day(2027, 2, 28)));
        // Years of 365 days put this on 2024-02-29, a day off, because 2024 has a leap day.
        assert_eq!(parse_span("3 years ago", midnight(2027, 2, 28).unwrap()), Some(day(2024, 2, 28)));
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

    /// The local calendar day `y-m-d` in a zone `offset` seconds east of UTC, as UTC seconds.
    fn local_day(y: i32, m: u8, d: u8, offset: i64) -> Span {
        let (from, to) = day(y, m, d);
        (from - offset, to - offset)
    }

    const WEST: i64 = -5 * 3600;
    const EAST: i64 = 9 * 3600;

    #[test]
    fn today_and_yesterday_are_the_days_of_the_callers_zone() {
        // 2026-03-11 02:00 UTC: still the evening of the 10th in UTC-5, already the 11th in UTC.
        let early = midnight(2026, 3, 11).unwrap() + 2 * 3600;
        assert_eq!(super::parse_span("today", early, 0), Some(day(2026, 3, 11)));
        assert_eq!(super::parse_span("today", early, WEST), Some(local_day(2026, 3, 10, WEST)));
        assert_eq!(super::parse_span("yesterday", early, WEST), Some(local_day(2026, 3, 9, WEST)));
        // 2026-03-11 20:00 UTC: already the morning of the 12th at UTC+9.
        let late = midnight(2026, 3, 11).unwrap() + 20 * 3600;
        assert_eq!(super::parse_span("today", late, 0), Some(day(2026, 3, 11)));
        assert_eq!(super::parse_span("today", late, EAST), Some(local_day(2026, 3, 12, EAST)));
        assert_eq!(super::parse_span("yesterday", late, EAST), Some(local_day(2026, 3, 11, EAST)));
        // A day of the caller is 24 hours long and starts at the caller's midnight.
        let (from, to) = super::parse_span("today", late, EAST).unwrap();
        assert_eq!(to - from + 1, DAY);
        assert_eq!((from + EAST).rem_euclid(DAY), 0);
    }

    #[test]
    fn last_week_starts_on_the_callers_monday() {
        // Monday 2026-03-09, 02:00 UTC. In UTC the week before is 2 to 8 March. At UTC-5 it is
        // still Sunday evening, so this week began on 2 March and the week before on 23 February.
        let clock = midnight(2026, 3, 9).unwrap() + 2 * 3600;
        let week = |from: (i32, u8, u8), to: (i32, u8, u8), offset: i64| {
            (local_day(from.0, from.1, from.2, offset).0, local_day(to.0, to.1, to.2, offset).1)
        };
        assert_eq!(super::parse_span("last week", clock, 0), Some(week((2026, 3, 2), (2026, 3, 8), 0)));
        assert_eq!(super::parse_span("last week", clock, WEST), Some(week((2026, 2, 23), (2026, 3, 1), WEST)));
    }

    #[test]
    fn calendar_phrases_are_read_in_the_callers_zone() {
        let clock = midnight(2026, 3, 11).unwrap() + 2 * 3600;
        // Local time is 21:00 on 10 March at UTC-5.
        assert_eq!(super::parse_span("5 hours ago", clock, WEST), Some(local_day(2026, 3, 10, WEST)));
        assert_eq!(super::parse_span("22 hours ago", clock, WEST), Some(local_day(2026, 3, 9, WEST)));
        assert_eq!(super::parse_span("a month ago", clock, WEST), Some(local_day(2026, 2, 10, WEST)));
        assert_eq!(super::parse_span("fixed on 2026-03-05", clock, WEST), Some(local_day(2026, 3, 5, WEST)));
        assert_eq!(super::parse_span("on Mar 3rd, 2025", clock, EAST), Some(local_day(2025, 3, 3, EAST)));
        let march = super::parse_span("in 2026-03", clock, WEST).unwrap();
        assert_eq!(march.0, midnight(2026, 3, 1).unwrap() - WEST);
        assert_eq!(march.1, midnight(2026, 4, 1).unwrap() - WEST - 1);
        let last_month = super::parse_span("last month", midnight(2026, 3, 1).unwrap() + 2 * 3600, WEST).unwrap();
        assert_eq!(last_month.0, midnight(2026, 1, 1).unwrap() - WEST, "at UTC-5 it is still 28 February");
        // The latest such day that is not in the future: the 11th at 02:00 UTC is the evening of the 10th.
        assert_eq!(super::parse_span("on March 11", clock, WEST), Some(local_day(2025, 3, 11, WEST)));
        assert_eq!(super::parse_span("on March 11", clock, 0), Some(local_day(2026, 3, 11, 0)));
    }

    #[test]
    fn an_offset_of_zero_changes_nothing_and_one_beyond_a_day_is_cut() {
        let clock = now();
        for text in ["today", "yesterday", "last week", "last month", "3 days ago", "a year ago", "March 3", "2026-03-05"] {
            assert_eq!(super::parse_span(text, clock, 0), parse_span(text, clock), "{text}");
        }
        assert_eq!(super::parse_span("today", clock, 40 * DAY), super::parse_span("today", clock, DAY));
        assert_eq!(super::parse_span("today", clock, i64::MIN), super::parse_span("today", clock, -DAY));
        assert_eq!(super::parse_span("today", clock, i64::MAX), super::parse_span("today", clock, DAY));
    }

    #[test]
    fn iso_date_formats_a_timestamp() {
        assert_eq!(iso_date(midnight(2026, 3, 5).unwrap() + 3600), "2026-03-05");
        assert_eq!(iso_date(0), "1970-01-01");
        assert_eq!(iso_date(-1), "1969-12-31");
    }
}
