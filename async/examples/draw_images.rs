//! # draw_images: port of the LilyGo-EPD47 `drawImages` sketch
//!
//! Shows three full-screen pictures one after another, 3 s each, and starts over. The Arduino
//! sketch is `~/Src/Arduino/LilyGoT5ePaper/LilyGo/drawImages`; it shows LilyGo's own pictures as
//! raw 4 bit arrays. The blocking twin is `draw_images` in
//! `blocking/`. Here `epdpi` runs in async mode on the `esp-rtos` Embassy executor.
//!
//! The pictures here are three of the author's, `assets/jimothy_painting.jpg`,
//! `assets/jimothy_sketch.jpg` and `assets/jimothy_manga.jpg`: the same raccoon as a painting, a
//! pencil drawing and a manga page, 723 x 540, centred on the panel. They are decoded with
//! `zune-jpeg` and drawn in 16 grays.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example draw_images
//! ```
//!
//! ## Expected
//!
//! - The log prints `draw_images: Rust, async`, then for each picture `image N drawn in N ms`.
//! - Landscape, 960 x 540. The panel flashes, then a raccoon appears, centred, with a white margin of
//!   about 118 px at the left and right. After 3 s the next picture: first a painting in color
//!   (gray here), then a pencil drawing, then a manga page with lettering. Then it starts over.
//! - Differences from the sketch: the pictures are different, they are JPEG files decoded on the
//!   board, and a refresh is `Quality16`, which clears first, where the sketch calls `epd_clear`
//!   and then draws.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::{error, info};
use embassy_time::{Duration, Instant, Timer};
use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use esp_println as _;
use rust_lilygot5_47_s3_embassy_examples::epdpi::prelude::*;

esp_bootloader_esp_idf::esp_app_desc!();

/// The three pictures, see `assets/NOTICE.md`.
const PICTURES: [&[u8]; 3] = [
    include_bytes!("../../assets/jimothy_painting.jpg"),
    include_bytes!("../../assets/jimothy_sketch.jpg"),
    include_bytes!("../../assets/jimothy_manga.jpg"),
];

/// The hardware-free part: decoding a JPEG to 16 grays and placing it. Same text in the async
/// crate, and tested on the host by `host-tests`.
mod ui {
    use alloc::vec::Vec;
    use embedded_graphics::{pixelcolor::Gray4, prelude::*};
    use zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
    use zune_jpeg::JpegDecoder;

    /// The panel size.
    pub const PANEL_W: i32 = 960;
    pub const PANEL_H: i32 = 540;
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

    /// Top left corner that centres `picture` on the panel. A picture larger than the panel is
    /// cut at the right and bottom edge, which draws nothing outside.
    pub fn origin(picture: &Gray) -> (i32, i32) {
        (
            ((PANEL_W - picture.width as i32) / 2).max(0),
            ((PANEL_H - picture.height as i32) / 2).max(0),
        )
    }
}

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = rust_lilygot5_47_s3_embassy_examples::init!();
    rust_lilygot5_47_s3_embassy_examples::start_rtos!(p);
    info!("draw_images: Rust, async");
    let bus = rust_lilygot5_47_s3_embassy_examples::lcd_bus!(p, 16);
    let mut prev = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut line = vec![0u8; Ed047tc1::SCANLINE_BYTES];
    let mut fb_mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let (Ok(mut display), Ok(mut fb)) = (
        EpdDisplayBuilder::<Ed047tc1>::new().build(bus, &mut prev, &mut line),
        EpdFrameBuffer::<Ed047tc1>::new(&mut fb_mem),
    ) else {
        error!("buffer allocation too small");
        rust_lilygot5_47_s3_embassy_examples::halt();
    };

    loop {
        for (index, jpeg) in PICTURES.iter().enumerate() {
            let _ = fb.clear(Gray4::WHITE);
            match ui::decode_gray(jpeg) {
                Ok(picture) => {
                    let (x, y) = ui::origin(&picture);
                    ui::draw_gray(&mut fb, &picture, x, y);
                }
                Err(_) => error!("picture {} does not decode", index + 1),
            }
            let start = Instant::now();
            if display.refresh(&fb, RefreshMode::Quality16).await.is_err() {
                error!("refresh failed");
            }
            info!(
                "image {} drawn in {} ms",
                index + 1,
                start.elapsed().as_millis()
            );
            Timer::after(Duration::from_secs(3)).await;
        }
    }
}
