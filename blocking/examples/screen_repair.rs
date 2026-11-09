//! # screen_repair: port of the LilyGo-EPD47 `screen_repair` sketch
//!
//! Wipes ghosts and stuck pixels by cycling the whole panel: a clear, 20 black pulses half a
//! second apart, a clear, 40 white pulses half a second apart, and a last clear. The Arduino
//! sketch is `~/Src/Arduino/LilyGoT5ePaper/LilyGo/screen_repair`. The async twin is
//! `screen_repair` in `async/`.
//!
//! No picture is drawn, so this works on the bus directly, with `epdpi`'s drive codes.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example screen_repair
//! ```
//!
//! ## Expected
//!
//! - The log prints `screen_repair: start`, then `black pulses done`, `white pulses done` and
//!   `done in N ms`, about 40 s in all.
//! - On the panel: it flashes, turns gradually darker with each black pulse, flashes again, turns
//!   gradually lighter with each white pulse, and ends clean white.
//! - Differences from the Arduino sketch: a clear is 16 black then 16 white passes, `epdpi`'s own,
//!   where the sketch's `epd_clear` runs about 80. Rows are 50 us like the sketch. With 22 us rows
//!   the pulses were too weak: the panel ended dark gray with a ghost of the last picture.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::info;
use esp_hal::delay::Delay;
use esp_hal::main;
use esp_hal::time::Instant;
use esp_println as _;
use rust_lilygot5_47_s3_examples::epdpi::prelude::*;
use rust_lilygot5_47_s3_examples::epdpi::waveform::WaveformEngine;
use rust_lilygot5_47_s3_examples::epdpi::{DriveCode, EpdParallelBus};

esp_bootloader_esp_idf::esp_app_desc!();

type Bus = rust_lilygot5_47_s3_examples::epdpi_esp::shift_reg_bus::ShiftRegBus<Delay>;

/// One pass over the whole panel with the same drive code for every pixel.
fn pass(bus: &mut Bus, row: &mut [u8], code: DriveCode) {
    WaveformEngine::uniform_scanline::<Ed047tc1>(code, row);
    let _ = bus.start_frame();
    for _ in 0..Ed047tc1::HEIGHT {
        let _ = bus.write_scanline(&row[..Ed047tc1::SCANLINE_BYTES]);
        let _ = bus.next_row();
    }
    let _ = bus.end_frame();
}

/// Flashes black then white, like `EpdDisplay::clear`.
fn clear(bus: &mut Bus, row: &mut [u8]) {
    for _ in 0..16 {
        pass(bus, row, DriveCode::Darken);
    }
    for _ in 0..16 {
        pass(bus, row, DriveCode::Lighten);
    }
    pass(bus, row, DriveCode::Hold);
}

#[main]
fn main() -> ! {
    let p = rust_lilygot5_47_s3_examples::init!();
    let mut bus = rust_lilygot5_47_s3_examples::lcd_bus!(p, 16);
    // Repair needs hard drive: the vendor library holds each row 50 us.
    bus.set_row_us(50);
    let delay = Delay::new();
    let mut row = vec![0u8; Ed047tc1::SCANLINE_BYTES];

    info!("screen_repair: start");
    let start = Instant::now();
    let _ = bus.set_power(true);
    delay.delay_millis(10);
    clear(&mut bus, &mut row);
    for _ in 0..20 {
        pass(&mut bus, &mut row, DriveCode::Darken);
        delay.delay_millis(500);
    }
    info!("screen_repair: black pulses done");
    clear(&mut bus, &mut row);
    for _ in 0..40 {
        pass(&mut bus, &mut row, DriveCode::Lighten);
        delay.delay_millis(500);
    }
    info!("screen_repair: white pulses done");
    clear(&mut bus, &mut row);
    let _ = bus.set_power(false);
    info!(
        "screen_repair: done in {} ms",
        (Instant::now() - start).as_millis()
    );
    loop {
        delay.delay_millis(1000);
    }
}
