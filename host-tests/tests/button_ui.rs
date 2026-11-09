//! Tests of the `mod ui` block of `examples/button.rs`: the page sequence, the button debouncing
//! and the drawing of the pages with the real font.

use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use fontdue::{Font, FontSettings};
use host_tests::button::*;

const ROBOTO_BLACK: &[u8] = include_bytes!("../../assets/fonts/Roboto-Black-ASCII.ttf");

fn font() -> Font {
    Font::from_bytes(ROBOTO_BLACK, FontSettings::default()).expect("the font loads")
}

fn width(font: &Font, text: &str) -> f32 {
    text.chars()
        .map(|c| font.metrics(c, PX).advance_width)
        .sum()
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

    fn at(&self, x: i32, y: i32) -> u8 {
        self.pixels[(y * 960 + x) as usize]
    }

    fn black_in(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> usize {
        let mut n = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                if self.at(x, y) == 0 {
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
        for Pixel(p, color) in pixels {
            if (0..960).contains(&p.x) && (0..540).contains(&p.y) {
                self.pixels[(p.y * 960 + p.x) as usize] = color.luma();
            }
        }
        Ok(())
    }
}

#[test]
fn the_presses_go_through_the_pages_then_to_sleep() {
    let mut pager = Pager::new();
    assert_eq!(pager.press(), Action::Page(1));
    assert_eq!(pager.press(), Action::Page(2));
    assert_eq!(pager.press(), Action::Sleep);
    // After a wake-up the program starts again, but the sequence also wraps to the first page.
    assert_eq!(pager.press(), Action::Page(0));
    assert_eq!(pager.press(), Action::Page(1));
}

#[test]
fn a_press_counts_once_after_the_debounce_time() {
    let mut d = Debouncer::new();
    assert!(!d.update(false, 0));
    assert!(!d.update(true, 100));
    assert!(!d.update(true, 100 + DEBOUNCE_MS - 1));
    assert!(d.update(true, 100 + DEBOUNCE_MS));
    // Held down: nothing more.
    for t in 1..50 {
        assert!(!d.update(true, 120 + t * 10), "t {t}");
    }
}

#[test]
fn a_new_press_needs_a_release() {
    let mut d = Debouncer::new();
    d.update(true, 0);
    assert!(d.update(true, 30));
    assert!(!d.update(true, 500));
    assert!(!d.update(false, 510));
    assert!(!d.update(true, 600));
    assert!(d.update(true, 630));
}

#[test]
fn a_bounce_shorter_than_the_debounce_time_does_not_count() {
    let mut d = Debouncer::new();
    assert!(!d.update(true, 0));
    assert!(!d.update(true, 10));
    assert!(!d.update(false, 12));
    // The contact settles: this one counts.
    assert!(!d.update(true, 14));
    assert!(!d.update(true, 30));
    assert!(d.update(true, 14 + DEBOUNCE_MS));
}

#[test]
fn the_clock_wrapping_around_does_not_break_a_press() {
    let mut d = Debouncer::new();
    let start = u32::MAX - 5;
    assert!(!d.update(true, start));
    assert!(d.update(true, start.wrapping_add(DEBOUNCE_MS)));
}

#[test]
fn every_character_of_the_pages_is_in_the_font() {
    let font = font();
    for text in PAGES.iter().chain(["DeepSleep", "Next"].iter()) {
        for c in text.chars().filter(|c| *c != '\n') {
            assert!(font.lookup_glyph_index(c) != 0, "{c:?} is not in the font");
        }
    }
}

#[test]
fn every_line_fits_in_the_frame() {
    let font = font();
    let (_, y, w, h) = FRAME;
    for (n, page) in PAGES.iter().enumerate() {
        let lines: Vec<&str> = page.lines().collect();
        for line in &lines {
            let wide = width(&font, line);
            assert!(
                TEXT_X as f32 + wide < (w as i32 - 10) as f32,
                "page {n}: {line:?} is {wide} px wide"
            );
        }
        let last_baseline = TEXT_Y + (lines.len() as i32 - 1) * LINE;
        assert!(
            last_baseline + 12 < y + h as i32,
            "page {n} runs out of the frame"
        );
    }
}

#[test]
fn a_screen_has_the_frame_the_text_and_the_next_box() {
    let font = font();
    let mut canvas = Canvas::new();
    draw_screen(&mut canvas, &font, PAGES[0]);
    // White everywhere else, a black frame, text inside, the Next box at the bottom right.
    assert_eq!(canvas.at(5, 5), 15);
    assert_eq!(canvas.at(10, 100), 0, "left edge of the frame");
    assert_eq!(canvas.at(500, 20), 0, "top edge of the frame");
    assert!(canvas.black_in(20, 40, 940, 300) > 3000, "the text");
    let (x, y, w, h) = NEXT_BOX;
    assert_eq!(canvas.at(x, y + 30), 0, "left edge of the Next box");
    assert!(
        canvas.black_in(x + 5, y + 5, x + w as i32 - 5, y + h as i32 - 5) > 100,
        "Next"
    );
    // Nothing between the frame and the box, below the frame on the left.
    assert_eq!(canvas.black_in(0, 400, 700, 540), 0);
}

#[test]
fn the_sleep_screen_shows_the_word() {
    let font = font();
    let mut canvas = Canvas::new();
    draw_screen(&mut canvas, &font, "DeepSleep");
    assert!(canvas.black_in(20, 20, 300, 80) > 300);
    // The first page has far more ink than the sleep screen.
    let mut first = Canvas::new();
    draw_screen(&mut first, &font, PAGES[0]);
    assert!(first.black_in(20, 40, 940, 300) > 4 * canvas.black_in(20, 40, 940, 300));
}
