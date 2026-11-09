//! Tests of the `mod ui` block of `examples/gif_player.rs`: the GIF of the sketch, its gray levels,
//! where it lands and that its frames build on each other.

use embedded_graphics::{
    pixelcolor::{Gray4, Rgb888},
    prelude::*,
};
use host_tests::gif_player::*;
use tinygif::Gif;

const GIF: &[u8] = include_bytes!("../../assets/1bitsmallcity.gif");

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

    fn drawn(&self) -> usize {
        self.pixels.iter().filter(|&&p| p != 0xee).count()
    }

    fn count(&self, test: impl Fn(u8) -> bool) -> usize {
        self.pixels
            .iter()
            .filter(|&&p| p != 0xee && test(p))
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
        for Pixel(p, color) in pixels {
            if (0..960).contains(&p.x) && (0..540).contains(&p.y) {
                self.pixels[(p.y * 960 + p.x) as usize] = color.luma();
            }
        }
        Ok(())
    }
}

fn gif() -> Gif<'static, Rgb888> {
    Gif::from_slice(GIF).expect("the GIF parses")
}

#[test]
fn the_gif_is_320_by_288_with_56_frames() {
    let gif = gif();
    assert_eq!((gif.width(), gif.height()), (320, 288));
    // The comment in the sketch's `1bitsmallcity.h` says 55, the file has 56 image blocks.
    assert_eq!(gif.frames().count(), 56);
}

#[test]
fn the_canvas_is_centred_like_the_sketch() {
    assert_eq!(origin(&gif()), Point::new(320, 126));
}

#[test]
fn a_color_becomes_its_luma_level() {
    assert_eq!(gray(Rgb888::new(0, 0, 0)), Gray4::BLACK);
    assert_eq!(gray(Rgb888::new(255, 255, 255)), Gray4::WHITE);
    assert_eq!(gray(Rgb888::new(128, 128, 128)), Gray4::new(8));
    // Green weighs more than blue: the same step is lighter.
    assert!(gray(Rgb888::new(0, 255, 0)).luma() > gray(Rgb888::new(0, 0, 255)).luma());
}

#[test]
fn the_first_frame_is_dark_lines_on_a_light_background() {
    let gif = gif();
    let frame = gif.frames().next().unwrap();
    let mut canvas = Canvas::new();
    draw_frame(&frame, &mut canvas, origin(&gif));
    let total = canvas.drawn();
    assert_eq!(total, 320 * 288, "the first frame covers the canvas");
    let light = canvas.count(|p| p >= 8);
    let dark = canvas.count(|p| p < 8);
    // A pixel art city: the background is the larger part, the lines are the rest.
    assert!(
        light > dark && dark > total / 20,
        "light {light}, dark {dark}"
    );
    // Nothing outside the canvas.
    for (x, y) in [
        (0, 0),
        (319, 300),
        (640, 300),
        (400, 125),
        (400, 414),
        (959, 539),
    ] {
        assert_eq!(canvas.at(x, y), 0xee, "({x}, {y})");
    }
}

#[test]
fn the_animation_changes_from_frame_to_frame() {
    let gif = gif();
    let origin = origin(&gif);
    let mut canvas = Canvas::new();
    let mut last = canvas.pixels.clone();
    let mut changes = 0;
    for frame in gif.frames() {
        draw_frame(&frame, &mut canvas, origin);
        if canvas.pixels != last {
            changes += 1;
        }
        last = canvas.pixels.clone();
    }
    assert!(
        changes >= 40,
        "only {changes} of 56 frames changed the picture"
    );
}

#[test]
fn a_frame_moves_by_the_offset_and_only_by_the_offset() {
    let gif = gif();
    let frame = gif.frames().next().unwrap();
    let mut a = Canvas::new();
    let mut b = Canvas::new();
    draw_frame(&frame, &mut a, Point::new(100, 50));
    draw_frame(&frame, &mut b, Point::new(110, 60));
    for (x, y) in [(100, 50), (150, 100), (419, 337)] {
        assert_eq!(a.at(x, y), b.at(x + 10, y + 10), "({x}, {y})");
    }
    assert_eq!(a.drawn(), b.drawn());
}

#[test]
fn all_frames_draw_without_a_panic_at_the_edge_too() {
    let gif = gif();
    let mut canvas = Canvas::new();
    for frame in gif.frames() {
        // Half of the canvas hangs over the right edge: the target clips.
        draw_frame(&frame, &mut canvas, Point::new(800, 400));
    }
    assert!(canvas.drawn() > 0);
}
