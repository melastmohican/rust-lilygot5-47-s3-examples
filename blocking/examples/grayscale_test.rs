//! # grayscale_test: port of the FastEPD `grayscale_test` sketch
//!
//! Draws 16 swatches, gray level 0 (black) to 15 (white), each 60 by 300 px, with a black outline
//! and the level number underneath, then refreshes once with `RefreshMode::Quality16`. The Arduino
//! sketch is `~/Src/Arduino/LilyGoT5ePaper/FastEPD/grayscale_test`. The async twin is
//! `grayscale_test` in `async/`.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example grayscale_test
//! ```
//!
//! ## Expected
//!
//! - The log prints `refresh done in N ms`.
//! - Landscape, 960 by 540, as in the sketch (no rotation). A row of 16 swatches from black on the
//!   left to white on the right, each one a visibly different gray, and the numbers 0 to 15
//!   under them.
//! - Differences from the Arduino sketch: the numbers use the 10x20 `embedded-graphics` font, not
//!   FastEPD's 12x16 one. The grays come from `epdpi`'s waveform table, not FastEPD's.
//!   The last gray level (15) is the same white as the background, so its swatch is only
//!   visible through the outline.
//! - Known limit on this board: black and the digits are slightly paler than in the Arduino sketch.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::{format, vec};
use defmt::{error, info};
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::Gray4,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
};
use esp_hal::delay::Delay;
use esp_hal::main;
use esp_hal::time::Instant;
use esp_println as _;
use rust_lilygot5_47_s3_examples::epdpi::prelude::*;

esp_bootloader_esp_idf::esp_app_desc!();

const SWATCH_W: i32 = 60;
const SWATCH_H: u32 = 300;

fn draw_swatches<D: DrawTarget<Color = Gray4>>(fb: &mut D) {
    let _ = fb.clear(Gray4::WHITE);
    for level in 0..16u8 {
        let x = level as i32 * SWATCH_W;
        let _ = Rectangle::new(Point::new(x, 0), Size::new(SWATCH_W as u32, SWATCH_H))
            .into_styled(PrimitiveStyle::with_fill(Gray4::new(level)))
            .draw(fb);
    }
    let _ = Rectangle::new(Point::zero(), Size::new(16 * SWATCH_W as u32, SWATCH_H))
        .into_styled(PrimitiveStyle::with_stroke(Gray4::BLACK, 1))
        .draw(fb);
    let style = MonoTextStyle::new(&FONT_10X20, Gray4::BLACK);
    for level in 0..16u8 {
        let label = format!("{}", level);
        let x = level as i32 * SWATCH_W + (SWATCH_W - 10 * label.len() as i32) / 2;
        let _ = Text::with_baseline(
            &label,
            Point::new(x, SWATCH_H as i32 + 12),
            style,
            Baseline::Top,
        )
        .draw(fb);
    }
}

#[main]
fn main() -> ! {
    let p = rust_lilygot5_47_s3_examples::init!();
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

    draw_swatches(&mut fb);

    let start = Instant::now();
    match display.refresh(&fb, RefreshMode::Quality16) {
        Ok(()) => info!(
            "refresh done in {} ms",
            (Instant::now() - start).as_millis()
        ),
        Err(_) => error!("refresh failed"),
    }
    let delay = Delay::new();
    loop {
        delay.delay_millis(1000);
    }
}
