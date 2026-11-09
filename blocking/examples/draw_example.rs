//! # draw_example: port of the LilyGo-EPD47 `drawExample` sketch
//!
//! Draws five shapes at random places, one every second, then clears the panel and starts over:
//! a horizontal line, an empty rectangle, an empty circle, a filled rectangle and a filled
//! circle. The Arduino sketch is `~/Src/Arduino/LilyGoT5ePaper/LilyGo/drawExample`. The async
//! twin is `draw_example` in `async/`.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example draw_example
//! ```
//!
//! ## Expected
//!
//! - After a first clear, a new black shape appears on the white panel about every 1.2 s, five
//!   in all. Then the panel clears and the cycle repeats with new positions.
//! - Landscape, 960 by 540, as in the sketch (no rotation).
//! - Differences from the Arduino sketch: each step is a `FastMono` update, which drives only the
//!   changed pixels, where the sketch redraws the whole gray image each time. The log prints
//!   `shape N done in M ms`.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::{error, info};
use embedded_graphics::{
    pixelcolor::Gray4,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
};
use esp_hal::delay::Delay;
use esp_hal::main;
use esp_hal::rng::Rng;
use esp_hal::time::Instant;
use esp_println as _;
use rust_lilygot5_47_s3_examples::epdpi::prelude::*;

esp_bootloader_esp_idf::esp_app_desc!();

/// A random number in `lo..hi`, like Arduino's `random(lo, hi)`.
fn random(rng: &Rng, lo: i32, hi: i32) -> i32 {
    lo + (rng.random() % (hi - lo) as u32) as i32
}

/// Draws shape number `step` (0 to 4) of the cycle, in black, like the sketch.
fn draw_shape<D: DrawTarget<Color = Gray4> + OriginDimensions>(fb: &mut D, step: u32, rng: &Rng) {
    let Size {
        width: w,
        height: h,
    } = fb.size();
    let (w, h) = (w as i32, h as i32);
    let outline = PrimitiveStyle::with_stroke(Gray4::BLACK, 1);
    let filled = PrimitiveStyle::with_fill(Gray4::BLACK);
    let _ = match step {
        0 => {
            let y = random(rng, 10, h);
            Line::new(Point::new(10, y), Point::new(w - 11, y))
                .into_styled(outline)
                .draw(fb)
        }
        1 | 3 => {
            let y = random(rng, 10, h);
            let size = Size::new(random(rng, 10, 60) as u32, random(rng, 10, 120) as u32);
            let style = if step == 1 { outline } else { filled };
            Rectangle::new(Point::new(10, y), size)
                .into_styled(style)
                .draw(fb)
        }
        2 => {
            let center = Point::new(random(rng, 10, w), random(rng, 10, h));
            Circle::with_center(center, 2 * 120 + 1)
                .into_styled(outline)
                .draw(fb)
        }
        _ => {
            let center = Point::new(random(rng, 10, w), random(rng, 10, h));
            let radius = random(rng, 10, 160) as u32;
            Circle::with_center(center, 2 * radius + 1)
                .into_styled(filled)
                .draw(fb)
        }
    };
}

#[main]
fn main() -> ! {
    let p = rust_lilygot5_47_s3_examples::init!();
    let bus = rust_lilygot5_47_s3_examples::lcd_bus!(p, 16);
    let rng = Rng::new();
    let delay = Delay::new();
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

    if display.clear().is_err() {
        error!("clear failed");
    }
    loop {
        for step in 0..5 {
            draw_shape(&mut fb, step, &rng);
            let start = Instant::now();
            match display.refresh(&fb, RefreshMode::FastMono) {
                Ok(()) => info!(
                    "shape {} done in {} ms",
                    step + 1,
                    (Instant::now() - start).as_millis()
                ),
                Err(_) => error!("refresh failed"),
            }
            delay.delay_millis(1000);
        }
        let _ = fb.clear(Gray4::WHITE);
        if display.clear().is_err() {
            error!("clear failed");
        }
    }
}
