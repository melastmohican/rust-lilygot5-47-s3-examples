//! # display_demo: first screen on the LilyGo T5 4.7" S3 (V2.4)
//!
//! Draws a triple border, four centered lines of text and a four cell gray bar, then refreshes
//! once with `RefreshMode::Quality16`. The async twin is `display_demo` in `async/`.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example display_demo
//! ```
//!
//! ## Expected
//!
//! - The log prints `refresh done in N ms`.
//! - Portrait, 540 by 960. White screen, three black borders 2 px apart, `LILYGO T5 4.7 S3`,
//!   `Panel: ED047TC1`, `Resolution: 540 x 960`, `16-Level Grayscale E-Paper`, a four cell bar
//!   (black, dark gray, light gray, black) and `Rust: epdpi blocking`.
//! - Not hardware tested yet: the waveform tables are the PaperDIY ones, so grays may be off.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::{error, info};
use embedded_graphics::{
    pixelcolor::Gray4,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use esp_hal::delay::Delay;
use esp_hal::main;
use esp_hal::time::Instant;
use esp_println as _;
use rust_lilygot5_47_s3_examples::epdpi::prelude::*;
use u8g2_fonts::{
    FontRenderer, fonts,
    types::{FontColor, HorizontalAlignment, VerticalPosition},
};

esp_bootloader_esp_idf::esp_app_desc!();

/// The four bar cells: black, dark gray, light gray, black.
const BAR_COLORS: [Gray4; 4] = [Gray4::new(0), Gray4::new(7), Gray4::new(13), Gray4::new(0)];

fn draw_demo<D: DrawTarget<Color = Gray4> + OriginDimensions>(fb: &mut D) {
    let Size {
        width: w,
        height: h,
    } = fb.size();
    let _ = fb.clear(Gray4::WHITE);

    let line = PrimitiveStyle::with_stroke(Gray4::BLACK, 1);
    for inset in [0u32, 2, 4] {
        let _ = Rectangle::new(
            Point::new(inset as i32, inset as i32),
            Size::new(w - 2 * inset, h - 2 * inset),
        )
        .into_styled(line)
        .draw(fb);
    }

    let big = FontRenderer::new::<fonts::u8g2_font_fub35_tf>();
    let small = FontRenderer::new::<fonts::u8g2_font_fub25_tf>();
    let centered = |font: &FontRenderer, y: i32, text: core::fmt::Arguments, fb: &mut D| {
        let _ = font.render_aligned(
            text,
            Point::new(w as i32 / 2, y),
            VerticalPosition::Center,
            HorizontalAlignment::Center,
            FontColor::Transparent(Gray4::BLACK),
            fb,
        );
    };
    centered(&big, 115, format_args!("LILYGO T5 4.7 S3"), fb);
    centered(&small, 205, format_args!("Panel: ED047TC1"), fb);
    centered(&small, 255, format_args!("Resolution: {} x {}", w, h), fb);
    centered(&small, 305, format_args!("16-Level Grayscale E-Paper"), fb);

    let (bar_x, bar_y, bar_w, bar_h) = (120u32, 370u32, w - 240, 58u32);
    let cell_w = bar_w / 4;
    for (i, color) in BAR_COLORS.iter().enumerate() {
        let x = bar_x + i as u32 * cell_w;
        let cw = if i == 3 { bar_x + bar_w - x } else { cell_w };
        let cell = Rectangle::new(Point::new(x as i32, bar_y as i32), Size::new(cw, bar_h));
        let _ = cell.into_styled(PrimitiveStyle::with_fill(*color)).draw(fb);
        let _ = cell.into_styled(line).draw(fb);
    }

    centered(
        &FontRenderer::new::<fonts::u8g2_font_fub17_tf>(),
        h as i32 - 40,
        format_args!("Rust: epdpi blocking"),
        fb,
    );
}

#[main]
fn main() -> ! {
    let p = rust_lilygot5_47_s3_examples::init!();
    let bus = rust_lilygot5_47_s3_examples::lcd_bus!(p, 10);
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

    fb.set_rotation(Rotation::Deg270);
    draw_demo(&mut fb);

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
