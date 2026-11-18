//! The time text the workspace stores: RFC 3339 in UTC to whole seconds,
//! `YYYY-MM-DDTHH:MM:SSZ`.
//!
//! [`now_rfc3339`] reads the wall clock, and [`rfc3339_of_seconds`]
//! formats a count of seconds with no clock, so a test pins exact texts.
//! Both are total: no clock reading and no count of seconds makes one
//! fail or panic. A time before the Unix epoch reads as the epoch, and
//! a year past 9999, for which RFC 3339 has no form, prints with every
//! digit it has.

use std::time::{SystemTime, UNIX_EPOCH};

/// The seconds in one day of Unix time, which counts no leap second.
const SECONDS_PER_DAY: u64 = 86_400;

/// The current time as RFC 3339 in UTC, whole seconds. It never fails:
/// a clock set before the Unix epoch reads as the epoch
/// ([`rfc3339_of`]), and a clock past the year 9999 prints its year with
/// every digit it has ([`rfc3339_of_seconds`]).
pub(crate) fn now_rfc3339() -> String {
    rfc3339_of(SystemTime::now())
}

/// `time` as RFC 3339 in UTC, with the part of a second dropped. A time
/// before the Unix epoch reads as the epoch, `1970-01-01T00:00:00Z`: the
/// text records when something happened, and a clock set that far back
/// is wrong whatever the text says.
fn rfc3339_of(time: SystemTime) -> String {
    // `duration_since` fails only for a time before the epoch.
    let since_epoch = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    rfc3339_of_seconds(since_epoch.as_secs())
}

/// The time `seconds` whole seconds after the Unix epoch, as
/// `YYYY-MM-DDTHH:MM:SSZ`. The year has four digits through
/// `9999-12-31T23:59:59Z`, which is 253,402,300,799 seconds. A later
/// time prints its year with every digit it has, as in
/// `10000-01-01T00:00:00Z`, which is no RFC 3339 text, and the largest
/// count there is still formats.
fn rfc3339_of_seconds(seconds: u64) -> String {
    let (year, month, day) = civil_from_days(seconds / SECONDS_PER_DAY);
    let second_of_day = seconds % SECONDS_PER_DAY;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        second_of_day / 3_600,
        second_of_day / 60 % 60,
        second_of_day % 60
    )
}

/// The year, the month, and the day of the month of the day `days` days
/// after 1970-01-01, in the Gregorian calendar. `days` is a count of
/// seconds divided by [`SECONDS_PER_DAY`], and no such number is large
/// enough to overflow a sum here.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    // Howard Hinnant's `civil_from_days`:
    // https://howardhinnant.github.io/date_algorithms.html#civil_from_days
    // It counts days from 0000-03-01, so a year starts in March and a
    // leap day is the last day of its year, and it works in eras of 400
    // years, which are 146,097 days each. A day at or after 1970-01-01
    // keeps every value here at or above zero, so the arithmetic is
    // unsigned.
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    // January and February belong to the year that started in March.
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
#[path = "rfc3339-tests.rs"]
mod tests;
