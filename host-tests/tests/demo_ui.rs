//! Tests of the `mod ui` block of `examples/demo.rs`: Pacific time, the line texts, the button and
//! the drawing.

use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use fontdue::{Font, FontSettings};
use host_tests::demo::*;

const ROBOTO_BLACK: &[u8] = include_bytes!("../../assets/fonts/Roboto-Black-ASCII.ttf");

fn utc(y: u16, mo: u8, d: u8, h: u8, mi: u8, s: u8) -> i64 {
    unix(&Clock {
        year: y,
        month: mo,
        day: d,
        hour: h,
        minute: mi,
        second: s,
        weekday: 0,
    })
}

fn shown(unix_utc: i64) -> String {
    time_line(Some(unix_utc), true)
}

#[test]
fn unix_and_clock_round_trip() {
    assert_eq!(utc(1970, 1, 1, 0, 0, 0), 0);
    assert_eq!(utc(2001, 9, 9, 1, 46, 40), 1_000_000_000);
    for t in [
        0,
        1_000_000_000,
        1_709_164_800,
        1_793_000_000,
        4_102_444_800,
    ] {
        let c = clock(t);
        assert_eq!(unix(&c), t);
    }
}

#[test]
fn winter_is_pst() {
    // 2026-11-09 20:00:00 UTC is 12:00:00 PST.
    assert_eq!(
        shown(utc(2026, 11, 9, 20, 0, 0)),
        "2026-11-09 12:00:00 PST, NTP ok"
    );
}

#[test]
fn summer_is_pdt() {
    // 2026-07-04 19:00:00 UTC is 12:00:00 PDT.
    assert_eq!(
        shown(utc(2026, 7, 4, 19, 0, 0)),
        "2026-07-04 12:00:00 PDT, NTP ok"
    );
}

#[test]
fn the_switch_dates_of_2026() {
    // DST starts Sunday 8 March at 2:00 PST = 10:00 UTC and ends Sunday 1 November at 2:00 PDT = 9:00 UTC.
    assert!(shown(utc(2026, 3, 8, 9, 59, 59)).contains("PST"));
    assert!(shown(utc(2026, 3, 8, 10, 0, 0)).contains("PDT"));
    assert!(shown(utc(2026, 11, 1, 8, 59, 59)).contains("PDT"));
    assert!(shown(utc(2026, 11, 1, 9, 0, 0)).contains("PST"));
}

#[test]
fn the_local_date_can_be_the_day_before() {
    // 2026-01-01 03:00 UTC is still 31 December 2025 in Pacific time.
    assert_eq!(
        shown(utc(2026, 1, 1, 3, 0, 0)),
        "2025-12-31 19:00:00 PST, NTP ok"
    );
}

#[test]
fn line_texts() {
    assert_eq!(sd_line(Some(15_931_539_456)), "SD card: 14.8 GB");
    assert_eq!(sd_line(None), "SD card: not found");
    assert_eq!(rtc_line(true), "RTC: found");
    assert_eq!(rtc_line(false), "RTC: not found");
    assert_eq!(touch_line(false), "Touch: not fitted");
    assert_eq!(touch_line(true), "Touch: found (not used)");
    assert_eq!(voltage_line(4200), "Voltage: 4.20 V");
    assert_eq!(voltage_line(3712), "Voltage: 3.71 V");
    assert_eq!(pressed_line(7), "Pressed: 7");
    assert_eq!(time_line(None, false), "Time: unknown, NTP no");
}

#[test]
fn the_battery_is_halved_back_and_clamped() {
    assert_eq!(battery_mv(1800), 3600);
    assert_eq!(battery_mv(2107), MAX_MV);
}

#[test]
fn a_short_press_counts_once() {
    let mut b = Button::new();
    assert_eq!(b.update(true, 0), Event::None);
    assert_eq!(b.update(true, DEBOUNCE_MS - 1), Event::None);
    assert_eq!(b.update(true, DEBOUNCE_MS), Event::Press);
    assert_eq!(b.update(true, 500), Event::None);
    assert_eq!(b.update(false, 600), Event::None);
    assert_eq!(b.update(true, 700), Event::None);
    assert_eq!(b.update(true, 700 + DEBOUNCE_MS), Event::Press);
}

#[test]
fn a_bounce_does_not_count() {
    let mut b = Button::new();
    assert_eq!(b.update(true, 0), Event::None);
    assert_eq!(b.update(false, 5), Event::None);
    assert_eq!(b.update(true, 8), Event::None);
    assert_eq!(b.update(true, 20), Event::None);
}

#[test]
fn holding_sleeps_once() {
    let mut b = Button::new();
    assert_eq!(b.update(true, 0), Event::None);
    assert_eq!(b.update(true, DEBOUNCE_MS), Event::Press);
    assert_eq!(b.update(true, LONG_MS - 1), Event::None);
    assert_eq!(b.update(true, LONG_MS), Event::Long);
    assert_eq!(b.update(true, LONG_MS + 100), Event::None);
}

/// 960 x 540, one byte per pixel with the gray level 0 to 15, 0xee where nothing was drawn.
struct Canvas {
    pixels: Vec<u8>,
}

impl Canvas {
    fn new() -> Self {
        Self {
            pixels: vec![0xee; 960 * 540],
        }
    }
    fn dark_in(&self, x0: i32, y0: i32, w: i32, h: i32) -> usize {
        let mut n = 0;
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                let v = self.pixels[(y * 960 + x) as usize];
                if v < 8 {
                    n += 1;
                }
            }
        }
        n
    }
}

impl OriginDimensions for Canvas {
    fn size(&self) -> Size {
        Size::new(960, 540)
    }
}

impl DrawTarget for Canvas {
    type Color = Gray4;
    type Error = core::convert::Infallible;
    fn draw_iter<I: IntoIterator<Item = Pixel<Gray4>>>(
        &mut self,
        pixels: I,
    ) -> Result<(), Self::Error> {
        for Pixel(p, c) in pixels {
            if (0..960).contains(&p.x) && (0..540).contains(&p.y) {
                self.pixels[(p.y * 960 + p.x) as usize] = c.luma();
            }
        }
        Ok(())
    }
}

fn font() -> Font {
    Font::from_bytes(ROBOTO_BLACK, FontSettings::default()).expect("the font loads")
}

#[test]
fn the_static_screen_has_title_and_three_lines() {
    let font = font();
    let mut c = Canvas::new();
    draw_static(
        &mut c,
        &font,
        "SD card: 14.8 GB",
        "RTC: found",
        "Touch: not fitted",
    );
    for (y, name) in [
        (TITLE_Y, "title"),
        (SD_Y, "sd"),
        (RTC_Y, "rtc"),
        (TOUCH_Y, "touch"),
    ] {
        assert!(
            c.dark_in(TEXT_X, y - 30, 400, 30) > 30,
            "{name} line is drawn"
        );
    }
    assert_eq!(
        c.dark_in(TEXT_X, VOLT_Y - 30, 400, 30),
        0,
        "dynamic lines are not drawn yet"
    );
}

#[test]
fn the_dynamic_lines_replace_the_old_text() {
    let font = font();
    let mut c = Canvas::new();
    draw_dynamic(
        &mut c,
        &font,
        "Voltage: 4.20 V",
        "2026-11-09 12:00:00 PST, NTP ok",
        "Pressed: 1",
    );
    let first = c.dark_in(TEXT_X, VOLT_Y - 30, 600, 30);
    assert!(first > 30);
    draw_dynamic(&mut c, &font, "Voltage: 4.20 V", "x", "Pressed: 2");
    assert!(
        c.dark_in(TEXT_X, TIME_Y - 30, 600, 30) < 400,
        "the long time text is gone"
    );
    // Everything drawn stays inside the changing area.
    let (x, y, w, h) = DYNAMIC;
    assert!(x <= TEXT_X && y <= VOLT_Y - 30 && y + h as i32 >= PRESS_Y);
    assert!(w > 400);
}

#[test]
fn the_sleep_screen_is_centered() {
    let font = font();
    let mut c = Canvas::new();
    draw_sleep(&mut c, &font);
    let columns: Vec<i32> = (0..960)
        .filter(|&x| c.dark_in(x, 200, 1, 120) > 0)
        .collect();
    let (first, last) = (columns[0], columns[columns.len() - 1]);
    assert!(
        ((first + last) / 2 - 480).abs() <= 3,
        "ink spans {first}..{last}"
    );
    assert_eq!(c.dark_in(0, 0, 960, 150), 0);
}
