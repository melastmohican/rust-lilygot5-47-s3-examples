//! Tests of the `mod ui` block of `examples/news.rs`: parsing, folding, wrapping, Pacific time and drawing.

use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use fontdue::{Font, FontSettings};
use host_tests::news::*;

const OPEN_SANS_BOLD: &[u8] = include_bytes!("../../assets/fonts/OpenSans-Bold.ttf");
const SAMPLE: &[u8] = include_bytes!("../fixtures/news_headlines.json");

fn font() -> Font {
    Font::from_bytes(OPEN_SANS_BOLD, FontSettings::default()).expect("the font loads")
}

#[test]
fn the_url_carries_the_key_and_the_sketchs_query() {
    assert_eq!(
        url("KEY"),
        "https://newsapi.org/v2/top-headlines?country=us&pageSize=10&apiKey=KEY"
    );
}

#[test]
fn articles_without_a_description_are_skipped() {
    let items = parse(SAMPLE).expect("the sample parses");
    let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(
        titles,
        vec![
            "City council approves new bike lanes",
            "Storm \"warning\" issued for the coast - again",
            "Caf opens a third location downtown",
            "Local team wins the final",
        ]
    );
}

#[test]
fn at_most_four_items_are_kept() {
    let items = parse(SAMPLE).unwrap();
    assert_eq!(items.len(), MAX_ITEMS);
}

#[test]
fn text_is_folded_to_ascii() {
    let items = parse(SAMPLE).unwrap();
    assert_eq!(
        items[1].description,
        "Forecasters expect strong wind overnight... and heavy rain."
    );
    assert_eq!(ascii_fold("caf\u{e9} \u{1F600}!"), "caf !");
}

#[test]
fn bad_responses_give_none() {
    assert_eq!(parse(b""), None);
    assert_eq!(parse(b"not json"), None);
    assert_eq!(parse(br#"{"status":"error","code":"apiKeyInvalid"}"#), None);
    assert_eq!(parse(br#"{"articles":[]}"#), None);
    assert_eq!(
        parse(br#"{"articles":[{"title":"x","description":null}]}"#),
        None
    );
}

fn utc(y: u16, mo: u8, d: u8, h: u8, mi: u8) -> i64 {
    // Days from civil, checked against the known value for 1970-01-01.
    let y = i64::from(y) - i64::from(mo <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (i64::from(mo) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) * 86_400 + i64::from(h) * 3600 + i64::from(mi) * 60
}

#[test]
fn the_time_lines_are_pacific() {
    // 2026-11-09 06:30 UTC is 2026-11-08 22:30 PST.
    let t = local(utc(2026, 11, 9, 6, 30));
    assert_eq!(date_line(t), "Date: 08.11.2026");
    assert_eq!(update_line(t), "Last update: 22:30");
    // 2026-07-04 19:05 UTC is 12:05 PDT.
    let t = local(utc(2026, 7, 4, 19, 5));
    assert_eq!(date_line(t), "Date: 04.07.2026");
    assert_eq!(update_line(t), "Last update: 12:05");
}

#[test]
fn unknown_time_is_shown_as_unknown() {
    assert_eq!(date_line(None), "Date: unknown");
    assert_eq!(update_line(None), "Last update: unknown");
}

#[test]
fn wrapping_fits_the_width_and_limits_the_lines() {
    let font = font();
    let width = (W - 2 * MARGIN) as f32;
    let (lines, overflow) = wrap(&font, TITLE_PX, "Short headline", width, 2);
    assert_eq!(lines, vec!["Short headline"]);
    assert!(!overflow);
    let long = "word ".repeat(60);
    let (lines, overflow) = wrap(&font, TEXT_PX, long.trim(), width, 2);
    assert_eq!(lines.len(), 2);
    assert!(overflow);
    for line in &lines {
        assert!(text_width(&font, TEXT_PX, line) <= width);
    }
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
    fn count(&self, x0: i32, y0: i32, w: i32, h: i32, pred: impl Fn(u8) -> bool) -> usize {
        (y0..y0 + h)
            .flat_map(|y| (x0..x0 + w).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                (0..960).contains(&x)
                    && (0..540).contains(&y)
                    && pred(self.pixels[(y * 960 + x) as usize])
            })
            .count()
    }
    fn dark(&self, x0: i32, y0: i32, w: i32, h: i32) -> usize {
        self.count(x0, y0, w, h, |v| v < 8)
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

fn screen() -> Canvas {
    let font = font();
    let items = parse(SAMPLE).unwrap();
    let mut c = Canvas::new();
    draw_news(
        &mut c,
        &font,
        &items,
        "Date: 08.11.2026",
        "Last update: 22:30",
    );
    c
}

#[test]
fn the_header_is_centred_with_a_rule_below() {
    let c = screen();
    let cols: Vec<i32> = (0..960).filter(|&x| c.dark(x, 8, 1, 50) > 0).collect();
    let (first, last) = (cols[0], cols[cols.len() - 1]);
    assert!(
        ((first + last) / 2 - 480).abs() <= 6,
        "World News spans {first}..{last}"
    );
    // The dark gray rule is 4 px high and as wide as the margins allow.
    assert_eq!(c.count(MARGIN, 62, 880, 4, |v| v == DARK_GRAY), 880 * 4);
    // The black rule under the date line.
    assert_eq!(c.count(MARGIN, 102, 880, 2, |v| v == 0), 880 * 2);
}

#[test]
fn date_left_and_update_right() {
    let c = screen();
    let left = c.dark(MARGIN, 72, 400, 24);
    let right = c.dark(520, 72, 400, 24);
    assert!(left > 50 && right > 50);
    let cols: Vec<i32> = (0..960).filter(|&x| c.dark(x, 72, 1, 24) > 0).collect();
    assert!((cols[0] - MARGIN).abs() <= 4, "date starts at the margin");
    assert!(
        (cols[cols.len() - 1] - (W - MARGIN)).abs() <= 6,
        "update ends at the right margin"
    );
}

#[test]
fn four_boxes_with_separators_between_them() {
    let c = screen();
    for i in 0..4 {
        let y = TOP + i * BOX_H;
        assert!(c.dark(MARGIN, y + 4, 880, 28) > 100, "box {i} has a title");
        assert!(
            c.dark(MARGIN, y + 34, 880, 40) > 50,
            "box {i} has a description"
        );
    }
    // A light gray line closes each box but the last.
    for i in 0..3 {
        let y = TOP + i * BOX_H + SEPARATOR_Y;
        assert_eq!(
            c.count(MARGIN, y, 880, 1, |v| v == LIGHT_GRAY),
            880,
            "separator {i}"
        );
    }
    let y = TOP + 3 * BOX_H + SEPARATOR_Y;
    assert_eq!(
        c.count(MARGIN, y, 880, 1, |v| v == LIGHT_GRAY),
        0,
        "no separator after the last box"
    );
}

#[test]
fn nothing_is_drawn_outside_the_margins_or_below_the_panel() {
    let c = screen();
    assert_eq!(c.dark(0, 0, MARGIN - 2, 540), 0);
    assert_eq!(c.dark(W - MARGIN + 6, 0, MARGIN, 540), 0);
}

#[test]
fn a_message_is_drawn_alone() {
    let font = font();
    let mut c = Canvas::new();
    draw_message(&mut c, &font, NO_NEWS);
    assert!(c.dark(MESSAGE.0, MESSAGE.1 - 35, 600, 45) > 100);
    assert_eq!(c.dark(0, 0, 960, 200), 0);
}

#[test]
fn full_boxes_do_not_touch_the_next_headline() {
    let font = font();
    let long_title = "headline word ".repeat(12);
    let long_text = "description word ".repeat(20);
    let items: Vec<Item> = (0..4)
        .map(|_| Item {
            title: long_title.trim().to_string(),
            description: long_text.trim().to_string(),
        })
        .collect();
    let mut c = Canvas::new();
    draw_news(&mut c, &font, &items, "d", "u");
    for i in 0..3 {
        let y = TOP + i * BOX_H;
        // Rows between the last description line and the next headline hold only the separator.
        assert_eq!(
            c.dark(MARGIN, y + BOX_H, 880, 5),
            0,
            "gap below box {i} is clear"
        );
        assert!(
            c.dark(MARGIN, y + 70, 880, 34) > 100,
            "box {i} has its second description line"
        );
    }
    // Everything fits above the bottom edge.
    let last = TOP + 3 * BOX_H;
    assert!(c.dark(MARGIN, last + 70, 880, 34) > 100);
    assert_eq!(c.dark(0, 538, 960, 2), 0);
}
