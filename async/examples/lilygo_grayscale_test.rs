//! # lilygo_grayscale_test: port of the LilyGo-EPD47 `grayscale_test` sketch
//!
//! Draws 16 vertical bars from black on the left to white on the right, twice per cycle, and logs
//! how long each draw took. The Arduino sketch is
//! `~/Src/Arduino/LilyGoT5ePaper/LilyGo/grayscale_test`. It is not the same as `grayscale_test`
//! here, which ports the FastEPD sketch. The blocking twin is
//! `lilygo_grayscale_test` in `blocking/`. Here `epdpi` runs in async mode on the `esp-rtos`
//! Embassy executor.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example lilygo_grayscale_test
//! ```
//!
//! ## Expected
//!
//! - Landscape, 960 by 540, as in the sketch (no rotation). A cycle takes about 10 s and repeats.
//! - First draw ("white on black"): the panel goes black, then the 16 bars appear. Three seconds
//!   later the second draw ("black on white"): the panel clears to white, then the same 16 bars
//!   appear, black on the left to white on the right.
//! - The log prints `WHITE_ON_BLACK draw took N ms` and `BLACK_ON_WHITE draw took N ms`.
//! - Differences from the Arduino sketch: `epdpi` has no white-on-black draw mode, and the sketch's
//!   three black pulses are a black `Quality16` refresh here. Both draws show the same ramp. The
//!   sketch draws a mirrored image in its first draw and the result is not documented.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::{error, info};
use embassy_time::{Duration, Instant, Timer};
use embedded_graphics::{
    pixelcolor::Gray4,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use esp_println as _;
use rust_lilygot5_47_s3_embassy_examples::epdpi::prelude::*;

esp_bootloader_esp_idf::esp_app_desc!();

/// 16 bars, 60 px each, level 0 (black) on the left to level 15 (white) on the right.
fn draw_ramp<D: DrawTarget<Color = Gray4> + OriginDimensions>(fb: &mut D) {
    let h = fb.size().height;
    for level in 0..16u8 {
        let _ = Rectangle::new(Point::new(level as i32 * 60, 0), Size::new(60, h))
            .into_styled(PrimitiveStyle::with_fill(Gray4::new(level)))
            .draw(fb);
    }
}

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = rust_lilygot5_47_s3_embassy_examples::init!();
    rust_lilygot5_47_s3_embassy_examples::start_rtos!(p);
    let bus = rust_lilygot5_47_s3_embassy_examples::lcd_bus!(p, 16);
    let mut prev = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut line = vec![0u8; Ed047tc1::SCANLINE_BYTES];
    let mut ramp_mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut black_mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let (Ok(mut display), Ok(mut ramp), Ok(mut black)) = (
        EpdDisplayBuilder::<Ed047tc1>::new().build(bus, &mut prev, &mut line),
        EpdFrameBuffer::<Ed047tc1>::new(&mut ramp_mem),
        EpdFrameBuffer::<Ed047tc1>::new(&mut black_mem),
    ) else {
        error!("buffer allocation too small");
        rust_lilygot5_47_s3_embassy_examples::halt();
    };
    draw_ramp(&mut ramp);
    let _ = black.clear(Gray4::BLACK);

    loop {
        info!("WHITE_ON_BLACK drawing:");
        if display.clear().await.is_err()
            || display
                .refresh(&black, RefreshMode::Quality16)
                .await
                .is_err()
        {
            error!("refresh failed");
        }
        let start = Instant::now();
        if display
            .refresh(&ramp, RefreshMode::Quality16)
            .await
            .is_err()
        {
            error!("refresh failed");
        }
        info!(
            "WHITE_ON_BLACK draw took {} ms",
            start.elapsed().as_millis()
        );
        Timer::after(Duration::from_secs(3)).await;

        info!("BLACK_ON_WHITE drawing:");
        if display.clear().await.is_err() {
            error!("clear failed");
        }
        let start = Instant::now();
        if display
            .refresh(&ramp, RefreshMode::Quality16)
            .await
            .is_err()
        {
            error!("refresh failed");
        }
        info!(
            "BLACK_ON_WHITE draw took {} ms",
            start.elapsed().as_millis()
        );
        Timer::after(Duration::from_secs(3)).await;
    }
}
