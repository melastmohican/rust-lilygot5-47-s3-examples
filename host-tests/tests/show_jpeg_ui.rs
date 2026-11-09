//! Tests of the `mod ui` block of `examples/show_jpeg.rs`: decoding the picture of the sketch,
//! the 4 bit levels and where it lands on the panel.

use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use host_tests::show_jpeg::*;

const JPEG: &[u8] = include_bytes!("../../assets/mocha800x480.jpg");

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
fn the_picture_is_800_by_480() {
    let picture = decode_gray(JPEG).expect("the JPEG decodes");
    assert_eq!((picture.width, picture.height), (800, 480));
    assert_eq!(picture.pixels.len(), 800 * 480);
}

#[test]
fn the_picture_is_a_photograph_not_a_flat_fill() {
    let picture = decode_gray(JPEG).unwrap();
    let mut seen = [false; 16];
    for &gray in &picture.pixels {
        seen[(gray >> 4) as usize] = true;
    }
    let levels = seen.iter().filter(|&&s| s).count();
    assert!(levels >= 12, "only {levels} of the 16 levels are used");
}

#[test]
fn a_level_is_the_top_four_bits() {
    assert_eq!(level(0), Gray4::new(0));
    assert_eq!(level(15), Gray4::new(0));
    assert_eq!(level(16), Gray4::new(1));
    assert_eq!(level(0x7f), Gray4::new(7));
    assert_eq!(level(0xf0), Gray4::new(15));
    assert_eq!(level(255), Gray4::WHITE);
}

#[test]
fn the_picture_lands_at_80_30_and_nowhere_else() {
    let picture = decode_gray(JPEG).unwrap();
    let mut canvas = Canvas::new();
    draw_gray(&mut canvas, &picture, IMG_X, IMG_Y);
    assert_eq!(canvas.drawn(), 800 * 480);
    // Every pixel of the picture is its level, at the corners and in the middle.
    for (col, row) in [
        (0, 0),
        (799, 0),
        (0, 479),
        (799, 479),
        (400, 240),
        (123, 77),
    ] {
        let expected = picture.pixels[row * 800 + col] >> 4;
        assert_eq!(
            canvas.at(80 + col as i32, 30 + row as i32),
            expected,
            "({col}, {row})"
        );
    }
    // The margin stays as it was.
    for (x, y) in [
        (79, 100),
        (880, 100),
        (400, 29),
        (400, 510),
        (0, 0),
        (959, 539),
    ] {
        assert_eq!(canvas.at(x, y), 0xee, "({x}, {y})");
    }
}

#[test]
fn bad_input_is_an_error_not_a_panic() {
    assert_eq!(decode_gray(&[]).err(), Some(JpegError::Decode));
    assert_eq!(
        decode_gray(b"not a jpeg at all").err(),
        Some(JpegError::Decode)
    );
    // The first bytes of the real file, cut off before the picture data.
    assert!(decode_gray(&JPEG[..600]).is_err());
}

#[test]
fn a_damaged_file_does_not_crash() {
    let mut damaged = JPEG.to_vec();
    for byte in damaged.iter_mut().skip(1000).step_by(97) {
        *byte ^= 0x55;
    }
    // Either a picture or an error, but no panic.
    let _ = decode_gray(&damaged);
}
