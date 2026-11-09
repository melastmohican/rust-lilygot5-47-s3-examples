//! # show_jpeg: port of the FastEPD `show_jpeg` sketch
//!
//! Decodes an 800 x 480 JPEG to 8 bit gray, keeps the top 4 bits of every pixel, so 16 grays and
//! no dithering, draws it at (80, 30) on a white panel and shows it with one full refresh. The
//! Arduino sketch is `~/Src/Arduino/LilyGoT5ePaper/FastEPD/show_jpeg`, which decodes with
//! JPEGDEC. The async twin is `show_jpeg` in `async/`.
//!
//! The picture is `assets/mocha800x480.jpg`, the sketch's `mocha800x480.h` as a file you can open.
//! It is decoded with `zune-jpeg`.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example show_jpeg
//! ```
//!
//! ## Expected
//!
//! - The log prints `show_jpeg: Rust, blocking`, `decoded 800 x 480 in N ms` and
//!   `refresh done in N ms`.
//! - Landscape, 960 x 540. The picture, in 16 grays, in an 800 x 480 area with a 80 px white margin
//!   left and right and 30 px above and below.
//! - Differences from the sketch: the decoder is `zune-jpeg`, and its gray is the luma of the JPEG
//!   where JPEGDEC's is its own. The refresh is `Quality16`, which clears first, where the sketch
//!   runs `fullUpdate(CLEAR_SLOW)`.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::{error, info};
use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use esp_hal::delay::Delay;
use esp_hal::main;
use esp_hal::time::Instant;
use esp_println as _;
use rust_lilygot5_47_s3_examples::epdpi::prelude::*;

esp_bootloader_esp_idf::esp_app_desc!();

/// The sketch's `mocha800x480.h` as a JPEG file, see `assets/NOTICE.md`.
const JPEG: &[u8] = include_bytes!("../../assets/mocha800x480.jpg");

/// The hardware-free part: decoding and drawing the picture. Same text in the async crate, and
/// tested on the host by `host-tests`.
mod ui {
    use alloc::vec::Vec;
    use embedded_graphics::{pixelcolor::Gray4, prelude::*};
    use zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
    use zune_jpeg::JpegDecoder;

    /// Where the picture goes. The sketch centres 800 x 480 on the 960 x 540 panel.
    pub const IMG_X: i32 = 80;
    pub const IMG_Y: i32 = 30;
    /// The largest picture taken, in pixels. Keeps a bad header from asking for all the memory.
    const MAX_PIXELS: usize = 960 * 540;

    /// Why a picture could not be decoded.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum JpegError {
        /// Not a JPEG file, or damaged.
        Decode,
        /// Larger than the panel.
        TooBig,
    }

    /// A decoded picture: 8 bit gray, `width * height` bytes, row by row.
    pub struct Gray {
        pub width: usize,
        pub height: usize,
        pub pixels: Vec<u8>,
    }

    /// Decodes `jpeg` to 8 bit gray, the luma of the picture.
    pub fn decode_gray(jpeg: &[u8]) -> Result<Gray, JpegError> {
        let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::Luma);
        let mut decoder = JpegDecoder::new_with_options(ZCursor::new(jpeg), options);
        decoder.decode_headers().map_err(|_| JpegError::Decode)?;
        let (width, height) = decoder.dimensions().ok_or(JpegError::Decode)?;
        if width == 0 || height == 0 || width * height > MAX_PIXELS {
            return Err(JpegError::TooBig);
        }
        let pixels = decoder.decode().map_err(|_| JpegError::Decode)?;
        if pixels.len() != width * height {
            return Err(JpegError::Decode);
        }
        Ok(Gray {
            width,
            height,
            pixels,
        })
    }

    /// The panel level of an 8 bit gray: its top 4 bits, as the sketch's draw callback keeps.
    pub fn level(gray: u8) -> Gray4 {
        Gray4::new(gray >> 4)
    }

    /// Draws `picture` with its top left corner at `(x, y)`.
    pub fn draw_gray<D: DrawTarget<Color = Gray4>>(target: &mut D, picture: &Gray, x: i32, y: i32) {
        let pixels = picture.pixels.iter().enumerate().map(|(i, &gray)| {
            let (col, row) = (i % picture.width, i / picture.width);
            Pixel(Point::new(x + col as i32, y + row as i32), level(gray))
        });
        let _ = target.draw_iter(pixels);
    }
}

#[main]
fn main() -> ! {
    let p = rust_lilygot5_47_s3_examples::init!();
    info!("show_jpeg: Rust, blocking");
    let bus = rust_lilygot5_47_s3_examples::lcd_bus!(p, 16);
    let mut prev = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut line = vec![0u8; Ed047tc1::SCANLINE_BYTES];
    let mut fb_mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let (Ok(mut display), Ok(mut fb)) = (
        EpdDisplayBuilder::<Ed047tc1>::new().build(bus, &mut prev, &mut line),
        EpdFrameBuffer::<Ed047tc1>::new(&mut fb_mem),
    ) else {
        error!("buffer allocation too small");
        rust_lilygot5_47_s3_examples::halt();
    };

    let _ = fb.clear(Gray4::WHITE);
    let start = Instant::now();
    match ui::decode_gray(JPEG) {
        Ok(picture) => {
            info!(
                "decoded {} x {} in {} ms",
                picture.width,
                picture.height,
                start.elapsed().as_millis()
            );
            ui::draw_gray(&mut fb, &picture, ui::IMG_X, ui::IMG_Y);
        }
        Err(_) => error!("the JPEG does not decode"),
    }

    let start = Instant::now();
    if display.refresh(&fb, RefreshMode::Quality16).is_err() {
        error!("refresh failed");
    }
    info!("refresh done in {} ms", start.elapsed().as_millis());
    let delay = Delay::new();
    loop {
        delay.delay_millis(1000);
    }
}
