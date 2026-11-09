//! Tests of the `mod ui` block of `examples/wifi_ntp.rs`: Unix seconds to a UTC date.

use host_tests::wifi_ntp::*;

fn at(unix: u64) -> (u16, u8, u8, u8, u8, u8, u8) {
    let t = utc(unix);
    (
        t.year, t.month, t.day, t.hour, t.minute, t.second, t.weekday,
    )
}

#[test]
fn the_epoch_is_a_thursday() {
    assert_eq!(at(0), (1970, 1, 1, 0, 0, 0, 4));
}

#[test]
fn a_billion_seconds() {
    assert_eq!(at(1_000_000_000), (2001, 9, 9, 1, 46, 40, 0));
}

#[test]
fn a_leap_day() {
    assert_eq!(at(1_709_164_800), (2024, 2, 29, 0, 0, 0, 4));
}

#[test]
fn a_recent_date() {
    assert_eq!(at(1_793_000_000), (2026, 10, 26, 7, 33, 20, 1));
}

#[test]
fn the_century_after_2000() {
    assert_eq!(at(4_102_444_800), (2100, 1, 1, 0, 0, 0, 5));
}
