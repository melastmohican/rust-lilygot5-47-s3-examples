//! Tests of the `mod ui` block of `examples/antialias_font.rs`: the gray levels, the placement of
//! glyphs on the baseline and the width of the three lines, with the real Roboto Black file.

use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use fontdue::{Font, FontSettings};
use host_tests::antialias_font::*;

const ROBOTO_BLACK: &[u8] = include_bytes!("../../assets/fonts/Roboto-Black-ASCII.ttf");

const LINES: [&str; 3] = [
    "Roboto Black 40pt",
    "Anti-aliased (80pt)",
    "Inverted anti-aliased",
];

fn font() -> Font {
    Font::from_bytes(ROBOTO_BLACK, FontSettings::default()).expect("the font loads")
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

    /// Rows and columns that were drawn on: `(min_x, max_x, min_y, max_y)`.
    fn bounds(&self) -> Option<(i32, i32, i32, i32)> {
        let mut b: Option<(i32, i32, i32, i32)> = None;
        for y in 0..540 {
            for x in 0..960 {
                if self.at(x, y) != 0xee {
                    b = Some(match b {
                        None => (x, x, y, y),
                        Some((a, c, d, e)) => (a.min(x), c.max(x), d.min(y), e.max(y)),
                    });
                }
            }
        }
        b
    }

    fn levels(&self) -> Vec<u8> {
        let mut v: Vec<u8> = self.pixels.iter().copied().filter(|&p| p != 0xee).collect();
        v.sort_unstable();
        v.dedup();
        v
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
fn coverage_becomes_gray_levels() {
    // Nothing is drawn where the glyph does not cover.
    for look in [Look::Hard, Look::Smooth, Look::SmoothOnBlack] {
        assert_eq!(level(look, 0), None, "{look:?}");
    }
    // Hard text is black from half coverage on.
    assert_eq!(level(Look::Hard, 127), None);
    assert_eq!(level(Look::Hard, 128), Some(Gray4::BLACK));
    assert_eq!(level(Look::Hard, 255), Some(Gray4::BLACK));
    // Smooth text on white: full coverage is black, almost none is almost white.
    assert_eq!(level(Look::Smooth, 255), Some(Gray4::BLACK));
    assert_eq!(level(Look::Smooth, 1), Some(Gray4::WHITE));
    assert_eq!(level(Look::Smooth, 128), Some(Gray4::new(7)));
    // On black it runs the other way.
    assert_eq!(level(Look::SmoothOnBlack, 255), Some(Gray4::WHITE));
    assert_eq!(level(Look::SmoothOnBlack, 128), Some(Gray4::new(8)));
}

#[test]
fn smooth_is_monotonic_in_coverage() {
    let mut last = 15;
    for c in 1..=255u8 {
        let l = level(Look::Smooth, c).unwrap().luma();
        assert!(l <= last, "coverage {c}");
        last = l;
    }
}

#[test]
fn a_capital_h_sits_on_the_baseline() {
    let font = font();
    let mut canvas = Canvas::new();
    draw_text(&mut canvas, &font, PX, 100, 300, "H", Look::Hard);
    let (_, _, top, bottom) = canvas.bounds().expect("something was drawn");
    // The bottom of an H touches the baseline, from the row above it, and it rises about the
    // cap height of Roboto, 0.71 em.
    assert_eq!(bottom, 299);
    let height = bottom - top + 1;
    assert!((50..=60).contains(&height), "cap height {height}");
}

#[test]
fn hard_text_has_one_level_and_smooth_text_has_many() {
    let font = font();
    let mut hard = Canvas::new();
    draw_text(&mut hard, &font, PX, 10, 200, "Roboto", Look::Hard);
    assert_eq!(hard.levels(), vec![0]);
    let mut smooth = Canvas::new();
    draw_text(&mut smooth, &font, PX, 10, 200, "Roboto", Look::Smooth);
    let levels = smooth.levels();
    assert!(levels.len() >= 8, "{levels:?}");
    assert_eq!(levels[0], 0, "the stems are black");
}

#[test]
fn white_on_black_text_never_draws_darker_than_it_has_to() {
    let font = font();
    let mut canvas = Canvas::new();
    draw_text(
        &mut canvas,
        &font,
        PX,
        10,
        200,
        "Roboto",
        Look::SmoothOnBlack,
    );
    let levels = canvas.levels();
    assert_eq!(*levels.last().unwrap(), 15, "the stems are white");
    assert!(levels.len() >= 8, "{levels:?}");
}

#[test]
fn the_three_lines_fit_on_the_panel() {
    let font = font();
    for line in LINES {
        let width = text_width(&font, PX, line);
        assert!(width < 960.0, "{line:?} is {width} px wide");
    }
}

#[test]
fn draw_text_returns_where_the_next_character_goes() {
    let font = font();
    let mut canvas = Canvas::new();
    let end = draw_text(&mut canvas, &font, PX, 0, 150, LINES[1], Look::Smooth);
    let width = text_width(&font, PX, LINES[1]);
    assert!(
        (end as f32 - width).abs() <= 1.0,
        "end {end}, width {width}"
    );
    // Everything drawn is left of that and the line sits between rows 60 and 180.
    let (min_x, max_x, min_y, max_y) = canvas.bounds().unwrap();
    assert!(min_x >= 0 && max_x <= end, "{min_x}..{max_x}, end {end}");
    assert!(min_y >= 60 && max_y <= 180, "rows {min_y}..{max_y}");
}

#[test]
fn text_that_runs_off_the_panel_is_clipped_not_a_crash() {
    let font = font();
    let mut canvas = Canvas::new();
    draw_text(&mut canvas, &font, PX, -40, 70, LINES[0], Look::Smooth);
    draw_text(&mut canvas, &font, PX, 900, 520, LINES[2], Look::Smooth);
    assert!(canvas.bounds().is_some());
}

#[test]
fn spaces_draw_nothing() {
    let font = font();
    let mut canvas = Canvas::new();
    let end = draw_text(&mut canvas, &font, PX, 0, 100, "   ", Look::Smooth);
    assert_eq!(canvas.bounds(), None);
    assert!(end > 0);
}
