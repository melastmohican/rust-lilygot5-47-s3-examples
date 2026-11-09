//! Tests of the `mod ui` block of `examples/macpaint_hello.rs`: the BMP, the brush and the pen
//! path, against the assets the board uses.

use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use host_tests::macpaint_hello::*;
use tinybmp::RawBmp;

const BITMAP: &[u8] = include_bytes!("../../assets/macpaint_640x480.bmp");

/// 960 x 540 with one byte per pixel, the panel the example draws on. A pixel is 0xff white or 0
/// black. Drawing outside is clipped, as on the real framebuffer.
struct Canvas {
    pixels: Vec<u8>,
}

impl Canvas {
    fn new() -> Self {
        Self {
            pixels: vec![0xff; 960 * 540],
        }
    }

    fn black(&self) -> usize {
        self.pixels.iter().filter(|&&p| p == 0).count()
    }

    fn at(&self, x: i32, y: i32) -> u8 {
        self.pixels[(y * 960 + x) as usize]
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
                self.pixels[(p.y * 960 + p.x) as usize] =
                    if color == Gray4::BLACK { 0 } else { 0xff };
            }
        }
        Ok(())
    }
}

fn bmp() -> RawBmp<'static> {
    RawBmp::from_slice(BITMAP).expect("the BMP decodes")
}

#[test]
fn the_bmp_is_the_640_by_480_picture_of_the_sketch() {
    let bmp = bmp();
    assert_eq!(
        bmp.header().image_size,
        Size::new(IMG_W as u32, IMG_H as u32)
    );
    assert_eq!(bmp.header().bpp.bits(), 1);
}

#[test]
fn the_background_draws_every_black_pixel_and_nothing_else() {
    let mut canvas = Canvas::new();
    draw_background(&mut canvas, &bmp());
    // `image.h` of the sketch has 29787 bits set, each one black ink.
    assert_eq!(canvas.black(), 29787);
    // Outside the 640 x 480 window at (160, 30) the panel is white.
    for (x, y) in [
        (0, 0),
        (159, 300),
        (800, 300),
        (400, 29),
        (400, 510),
        (959, 539),
    ] {
        assert_eq!(canvas.at(x, y), 0xff, "({x}, {y})");
    }
}

#[test]
fn the_first_row_of_the_picture_starts_with_eight_black_pixels() {
    // image.h starts 0xFF, the BMP stores the same picture bottom up: the top left 8 pixels of the
    // window are black.
    let mut canvas = Canvas::new();
    draw_background(&mut canvas, &bmp());
    for dx in 0..8 {
        assert_eq!(canvas.at(IMG_X + dx, IMG_Y), 0, "dx {dx}");
    }
    assert_eq!(canvas.at(IMG_X + 8, IMG_Y), 0xff);
}

#[test]
fn the_drawing_area_is_white_like_the_real_macpaint() {
    let mut canvas = Canvas::new();
    draw_background(&mut canvas, &bmp());
    // Inside the empty drawing area, where the brush will write.
    for (x, y) in [(300, 100), (500, 300), (700, 400)] {
        assert_eq!(canvas.at(x, y), 0xff, "({x}, {y})");
    }
}

#[test]
fn the_pen_path_has_361_points_inside_the_window() {
    assert_eq!(POSITIONS.len(), 361);
    assert_eq!(POSITIONS[0], (263, 288));
    assert_eq!(POSITIONS[360], (627, 232));
    for (i, &(x, y)) in POSITIONS.iter().enumerate() {
        // The path is in the 800 x 480 frame, the brush must stay on the picture.
        assert!(
            (0..800).contains(&x) && (0..480).contains(&y),
            "point {i}: ({x}, {y})"
        );
    }
}

#[test]
fn the_brush_is_a_black_disc_of_radius_four() {
    let mut canvas = Canvas::new();
    draw_brush(&mut canvas, 100, 100);
    let (cx, cy) = (FRAME_X + 100, FRAME_Y + 100);
    assert_eq!(canvas.at(cx, cy), 0);
    for d in [-4, 4] {
        assert_eq!(canvas.at(cx + d, cy), 0, "edge {d}");
        assert_eq!(canvas.at(cx, cy + d), 0, "edge {d}");
    }
    assert_eq!(canvas.at(cx + 5, cy), 0xff);
    assert_eq!(canvas.at(cx, cy - 5), 0xff);
    // About pi * 4.5^2 pixels.
    let black = canvas.black();
    assert!((57..=81).contains(&black), "{black} pixels");
}

#[test]
fn the_brush_at_the_edge_is_clipped_not_a_crash() {
    let mut canvas = Canvas::new();
    draw_brush(&mut canvas, -(FRAME_X as i16), -(FRAME_Y as i16));
    assert!(canvas.black() > 0 && canvas.black() < 81);
}

#[test]
fn drawing_the_whole_path_stays_on_the_panel() {
    let mut canvas = Canvas::new();
    for &(x, y) in POSITIONS.iter() {
        draw_brush(&mut canvas, x, y);
    }
    assert!(canvas.black() > 361);
}
