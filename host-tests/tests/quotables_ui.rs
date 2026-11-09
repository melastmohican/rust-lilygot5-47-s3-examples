//! Tests of the `mod ui` block of `examples/quotables.rs`: ASCII folding, parsing, wrapping and drawing.

use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use fontdue::{Font, FontSettings};
use host_tests::quotables::*;

const OPEN_SANS_BOLD: &[u8] = include_bytes!("../../assets/fonts/OpenSans-Bold.ttf");
const SAMPLE: &[u8] = include_bytes!("../fixtures/quotable_random.json");

fn font() -> Font {
    Font::from_bytes(OPEN_SANS_BOLD, FontSettings::default()).expect("the font loads")
}

#[test]
fn curly_punctuation_becomes_ascii() {
    assert_eq!(ascii_fold("\u{201C}Hi\u{201D}"), "\"Hi\"");
    assert_eq!(ascii_fold("it\u{2019}s \u{2018}ok\u{2019}"), "it's 'ok'");
    assert_eq!(ascii_fold("a\u{2013}b\u{2014}c"), "a-b-c");
    assert_eq!(ascii_fold("wait\u{2026}"), "wait...");
}

#[test]
fn other_non_ascii_is_dropped() {
    assert_eq!(ascii_fold("caf\u{e9} \u{1F600}!"), "caf !");
    assert_eq!(ascii_fold("\u{2020}x"), "x");
    assert_eq!(ascii_fold("plain"), "plain");
}

#[test]
fn a_response_parses() {
    let quote = parse(SAMPLE).expect("the sample parses");
    assert_eq!(quote.text, "The only thing we have to fear is fear itself.");
    assert_eq!(quote.author, "Franklin D. Roosevelt");
}

#[test]
fn bad_responses_give_none() {
    assert_eq!(parse(b""), None);
    assert_eq!(parse(b"not json"), None);
    assert_eq!(parse(b"{}"), None);
    assert_eq!(parse(br#"{"quote":{"content":"x"}}"#), None);
    assert_eq!(parse(br#"{"quote":{"author":{"name":"A"}}}"#), None);
    assert_eq!(
        parse(br#"{"quote":{"content":5,"author":{"name":"A"}}}"#),
        None
    );
}

#[test]
fn the_length_limit_counts_bytes_before_folding() {
    let exact = "a".repeat(MAX_QUOTE_LEN);
    let json = format!(r#"{{"quote":{{"content":"{exact}","author":{{"name":"A"}}}}}}"#);
    assert!(parse(json.as_bytes()).is_some());
    let long = "a".repeat(MAX_QUOTE_LEN + 1);
    let json = format!(r#"{{"quote":{{"content":"{long}","author":{{"name":"A"}}}}}}"#);
    assert_eq!(parse(json.as_bytes()), None);
    // 80 curly quotes are 240 bytes, over the limit, although they fold to 80 characters.
    let curly = "\u{2019}".repeat(80);
    let json = format!(r#"{{"quote":{{"content":"{curly}","author":{{"name":"A"}}}}}}"#);
    assert_eq!(parse(json.as_bytes()), None);
}

#[test]
fn text_that_fits_is_one_line() {
    let font = font();
    let wrapped = wrap(&font, BIG_PX, "Short quote", QUOTE_W as f32, MAX_LINES);
    assert_eq!(wrapped.lines, vec!["Short quote"]);
    assert!(!wrapped.overflow);
}

#[test]
fn long_text_wraps_inside_the_width() {
    let font = font();
    let text = "If one is estranged from oneself, then one is estranged from others too.";
    let wrapped = wrap(&font, BIG_PX, text, QUOTE_W as f32, MAX_LINES);
    assert!(wrapped.lines.len() > 1);
    assert!(!wrapped.overflow);
    for line in &wrapped.lines {
        assert!(
            text_width(&font, BIG_PX, line) <= QUOTE_W as f32,
            "{line} fits"
        );
    }
    assert_eq!(wrapped.lines.join(" "), text);
}

#[test]
fn too_much_text_stops_at_the_last_line_with_overflow() {
    let font = font();
    let text = "word ".repeat(80);
    let wrapped = wrap(&font, BIG_PX, text.trim(), QUOTE_W as f32, MAX_LINES);
    assert_eq!(wrapped.lines.len(), MAX_LINES);
    assert!(wrapped.overflow);
}

#[test]
fn a_single_long_word_stays_on_one_line() {
    let font = font();
    let word = "x".repeat(80);
    let wrapped = wrap(&font, BIG_PX, &word, QUOTE_W as f32, MAX_LINES);
    assert_eq!(wrapped.lines, vec![word]);
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
    fn dark(&self, x0: i32, y0: i32, w: i32, h: i32) -> usize {
        (y0..y0 + h)
            .flat_map(|y| (x0..x0 + w).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                (0..960).contains(&x)
                    && (0..540).contains(&y)
                    && self.pixels[(y * 960 + x) as usize] < 8
            })
            .count()
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

fn ink_extent(c: &Canvas, y0: i32, y1: i32) -> Option<(i32, i32)> {
    let cols: Vec<i32> = (0..960)
        .filter(|&x| c.dark(x, y0, 1, y1 - y0) > 0)
        .collect();
    Some((*cols.first()?, *cols.last()?))
}

#[test]
fn the_quote_screen_has_mark_quote_and_author_in_place() {
    let font = font();
    let mut c = Canvas::new();
    let quote = parse(SAMPLE).unwrap();
    draw_quote(&mut c, &font, &quote);
    assert_eq!(
        c.dark(0, 0, 960, QUOTE_Y - 45),
        0,
        "nothing above the first line"
    );
    assert!(
        c.dark(QUOTE_X, QUOTE_Y - 40, QUOTE_W, 40) > 100,
        "the first line is drawn"
    );
    // The author ends at the right margin and sits at the bottom.
    let (_, right) = ink_extent(&c, AUTHOR_Y - 25, AUTHOR_Y + 8).expect("the author is drawn");
    assert!(
        (right - AUTHOR_RIGHT).abs() <= 6,
        "author ends near {AUTHOR_RIGHT}, got {right}"
    );
    // Nothing is drawn beyond the margins of the quote box.
    let (left, right) = ink_extent(&c, QUOTE_Y - 40, AUTHOR_Y - 35).unwrap();
    assert!(left >= QUOTE_X - 4 && right <= QUOTE_X + QUOTE_W + 8);
}

#[test]
fn six_long_lines_end_with_dots() {
    let font = font();
    let mut c = Canvas::new();
    let quote = Quote {
        text: "word ".repeat(40).trim().to_string(),
        author: "A".into(),
    };
    draw_quote(&mut c, &font, &quote);
    let last = QUOTE_Y + (MAX_LINES as i32 - 1) * LINE_H;
    assert!(
        c.dark(QUOTE_X, last - 40, QUOTE_W, 44) > 100,
        "the sixth line is drawn"
    );
    assert_eq!(
        c.dark(QUOTE_X, last + 20, QUOTE_W, 30),
        0,
        "no seventh line"
    );
}

#[test]
fn a_message_is_drawn_alone() {
    let font = font();
    let mut c = Canvas::new();
    draw_message(&mut c, &font, WIFI_FAILED);
    assert!(c.dark(MESSAGE.0, MESSAGE.1 - 35, 600, 45) > 100);
    assert_eq!(c.dark(0, 0, 960, 200), 0);
}

#[test]
fn the_quote_marks_sit_next_to_the_words() {
    let font = font();
    let mut c = Canvas::new();
    let quote = Quote {
        text: "Short".into(),
        author: "A".into(),
    };
    draw_quote(&mut c, &font, &quote);
    // One line: the opening mark starts the line and the closing mark ends it, no gap.
    let (left, right) = ink_extent(&c, QUOTE_Y - 45, QUOTE_Y + 10).unwrap();
    let end = QUOTE_X + text_width(&font, BIG_PX, "\"Short\"") as i32;
    assert!(
        (left - QUOTE_X).abs() <= 8,
        "the line starts at the box edge, got {left}"
    );
    assert!(
        (right - end).abs() <= 8,
        "the line ends with the closing mark at {end}, got {right}"
    );
    assert_eq!(
        c.dark(0, 0, 960, QUOTE_Y - 45),
        0,
        "no mark on a line above"
    );
}

#[test]
fn the_quoted_text_wraps_with_the_marks_on_the_ends() {
    let font = font();
    let text = "If one is estranged from oneself, then one is estranged from others too.";
    let wrapped = wrap(
        &font,
        BIG_PX,
        &format!("\"{text}\""),
        QUOTE_W as f32,
        MAX_LINES,
    );
    assert!(wrapped.lines.first().unwrap().starts_with('"'));
    assert!(wrapped.lines.last().unwrap().ends_with('"'));
    assert!(!wrapped.overflow);
}

#[test]
fn a_cut_off_quote_has_dots_and_no_closing_mark() {
    let font = font();
    let mut c = Canvas::new();
    let quote = Quote {
        text: "word ".repeat(40).trim().to_string(),
        author: "A".into(),
    };
    draw_quote(&mut c, &font, &quote);
    let last = QUOTE_Y + (MAX_LINES as i32 - 1) * LINE_H;
    // The dots sit at the right end of the sixth line.
    assert!(c.dark(QUOTE_X + QUOTE_W - 30, last - 15, 40, 20) > 5);
    // Right of the dots nothing else is drawn.
    assert_eq!(c.dark(QUOTE_X + QUOTE_W + 12, last - 45, 80, 50), 0);
}
