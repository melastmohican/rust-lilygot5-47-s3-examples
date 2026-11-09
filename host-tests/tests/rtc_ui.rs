//! Tests of the `mod ui` block of `examples/rtc.rs`: weekday and year.

use host_tests::rtc::*;

#[test]
fn known_weekdays() {
    // 0 = Sunday.
    assert_eq!(weekday(26, 11, 8), 0);
    assert_eq!(weekday(26, 11, 9), 1);
    assert_eq!(weekday(0, 1, 1), 6); // Saturday 1 Jan 2000
    assert_eq!(weekday(24, 2, 29), 4); // Thursday 29 Feb 2024
    assert_eq!(weekday(99, 12, 31), 4); // Thursday 31 Dec 2099
}

#[test]
fn the_default_time_is_a_valid_date_on_a_sunday() {
    let (year, month, day, hours, minutes, seconds) = DEFAULT;
    assert_eq!(full_year(year), 2026);
    assert!((1..=12).contains(&month) && (1..=31).contains(&day));
    assert!(hours < 24 && minutes < 60 && seconds < 60);
    assert_eq!(weekday(year, month, day), 0);
}

#[test]
fn a_bad_month_does_not_panic() {
    let _ = weekday(26, 0, 1);
    let _ = weekday(26, 13, 1);
}
