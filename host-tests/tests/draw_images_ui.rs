//! Tests of the `mod ui` block of `examples/draw_images.rs`: decoding the three pictures,
//! centring them and the margins.

use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use host_tests::draw_images::*;

const PICTURES: [(&str, &[u8]); 3] = [
    (
        "painting",
        include_bytes!("../../assets/jimothy_painting.jpg"),
    ),
    ("sketch", include_bytes!("../../assets/jimothy_sketch.jpg")),
    ("manga", include_bytes!("../../assets/jimothy_manga.jpg")),
];

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

fn gray(width: usize, height: usize) -> Gray {
    Gray {
        width,
        height,
        pixels: vec![0x80; width * height],
    }
}

#[test]
fn the_three_pictures_decode_to_723_by_540() {
    for (name, jpeg) in PICTURES {
        let picture = decode_gray(jpeg).unwrap_or_else(|_| panic!("{name} decodes"));
        assert_eq!((picture.width, picture.height), (723, 540), "{name}");
        assert_eq!(picture.pixels.len(), 723 * 540, "{name}");
    }
}

#[test]
fn the_pictures_are_different_and_have_many_grays() {
    let mut sums = Vec::new();
    for (name, jpeg) in PICTURES {
        let picture = decode_gray(jpeg).unwrap();
        let mut seen = [false; 16];
        for &g in &picture.pixels {
            seen[(g >> 4) as usize] = true;
        }
        let levels = seen.iter().filter(|&&s| s).count();
        assert!(levels >= 8, "{name}: only {levels} levels");
        sums.push(picture.pixels.iter().map(|&p| u64::from(p)).sum::<u64>());
    }
    assert!(
        sums[0] != sums[1] && sums[1] != sums[2] && sums[0] != sums[2],
        "{sums:?}"
    );
}

#[test]
fn a_picture_is_centred_on_the_panel() {
    assert_eq!(origin(&gray(723, 540)), (118, 0));
    assert_eq!(origin(&gray(800, 480)), (80, 30));
    assert_eq!(origin(&gray(960, 540)), (0, 0));
    // Larger than the panel: start at the corner, the rest is cut off.
    assert_eq!(origin(&gray(1200, 700)), (0, 0));
}

#[test]
fn each_picture_leaves_the_side_margins_alone() {
    for (name, jpeg) in PICTURES {
        let picture = decode_gray(jpeg).unwrap();
        let (x0, y0) = origin(&picture);
        let mut canvas = Canvas::new();
        draw_gray(&mut canvas, &picture, x0, y0);
        assert_eq!(canvas.drawn(), 723 * 540, "{name}");
        for x in [0, 50, 117, 841, 900, 959] {
            for y in [0, 270, 539] {
                assert_eq!(canvas.at(x, y), 0xee, "{name} ({x}, {y})");
            }
        }
        // The corners of the picture are its first and last pixels.
        assert_eq!(canvas.at(118, 0), picture.pixels[0] >> 4, "{name}");
        assert_eq!(
            canvas.at(118 + 722, 539),
            picture.pixels[723 * 540 - 1] >> 4,
            "{name}"
        );
    }
}

#[test]
fn bad_input_is_an_error_not_a_panic() {
    assert_eq!(decode_gray(&[]).err(), Some(JpegError::Decode));
    assert_eq!(decode_gray(b"not a jpeg").err(), Some(JpegError::Decode));
    assert!(decode_gray(&PICTURES[0].1[..400]).is_err());
}

#[test]
fn a_level_is_the_top_four_bits() {
    assert_eq!(level(0), Gray4::new(0));
    assert_eq!(level(0x7f), Gray4::new(7));
    assert_eq!(level(255), Gray4::WHITE);
}
