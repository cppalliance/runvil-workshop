//! Tests of the time text: fixed counts of seconds on each side of a
//! leap day, of a century year that has none, and of the end of the
//! year 9999, then the two rules for a clock outside those years.

use std::time::Duration;

use super::*;

/// The last second that RFC 3339 has a text for, `9999-12-31T23:59:59Z`.
const LAST_SECOND_OF_9999: u64 = 253_402_300_799;

#[test]
fn a_count_of_seconds_formats_as_its_utc_date_and_time() {
    let cases = [
        (0, "1970-01-01T00:00:00Z"),
        (951_782_400, "2000-02-29T00:00:00Z"),
        (951_868_799, "2000-02-29T23:59:59Z"),
        (1_000_000_000, "2001-09-09T01:46:40Z"),
        (1_709_164_800, "2024-02-29T00:00:00Z"),
        (LAST_SECOND_OF_9999, "9999-12-31T23:59:59Z"),
    ];
    for (seconds, text) in cases {
        assert_eq!(rfc3339_of_seconds(seconds), text, "{seconds} seconds");
    }
}

#[test]
fn a_century_year_that_400_does_not_divide_has_no_leap_day() {
    assert_eq!(rfc3339_of_seconds(4_107_542_399), "2100-02-28T23:59:59Z");
    assert_eq!(
        rfc3339_of_seconds(4_107_542_400),
        "2100-03-01T00:00:00Z",
        "the second after February 28 of 2100 is in March"
    );
}

#[test]
fn a_time_drops_its_part_of_a_second() {
    let time = UNIX_EPOCH + Duration::new(1_000_000_000, 999_999_900);
    assert_eq!(rfc3339_of(time), "2001-09-09T01:46:40Z");
}

#[test]
fn a_time_before_the_epoch_reads_as_the_epoch() {
    let before = [
        Duration::from_nanos(100),
        Duration::from_secs(1),
        Duration::from_hours(24 * 365),
    ];
    for earlier in before {
        let time = UNIX_EPOCH
            .checked_sub(earlier)
            .expect("the system clock has a time this far before the epoch");
        assert_eq!(
            rfc3339_of(time),
            "1970-01-01T00:00:00Z",
            "{earlier:?} before the epoch"
        );
    }
}

#[test]
fn a_year_past_9999_prints_every_digit_it_has() {
    assert_eq!(
        rfc3339_of_seconds(LAST_SECOND_OF_9999 + 1),
        "10000-01-01T00:00:00Z",
        "the second after the last one RFC 3339 has a text for"
    );
    assert_eq!(
        rfc3339_of_seconds(u64::MAX),
        "584554051223-11-09T07:00:15Z",
        "the largest count of seconds formats too"
    );
}

#[test]
fn the_current_time_has_the_form_of_the_text() {
    let now = now_rfc3339();
    let form: String = now
        .chars()
        .map(|character| {
            if character.is_ascii_digit() {
                '0'
            } else {
                character
            }
        })
        .collect();
    assert_eq!(form, "0000-00-00T00:00:00Z", "{now}");
}
