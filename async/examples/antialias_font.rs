//! # antialias_font: port of the FastEPD `antialias_font` sketch
//!
//! Three lines of Roboto Black: hard 1 bit text on white, smooth anti-aliased text on white, and
//! smooth text in white on a black lower half. The Arduino sketch is
//! `~/Src/Arduino/LilyGoT5ePaper/FastEPD/antialias_font`. The blocking twin is
//! `antialias_font` in `blocking/`. Here `epdpi` runs in async mode on the `esp-rtos` Embassy
//! executor.
//!
//! The text is drawn from the TrueType file `assets/fonts/Roboto-Black-ASCII.ttf` (Apache 2.0, see
//! `assets/NOTICE.md`) with the `fontdue` rasterizer, not from FastEPD's own font files.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example antialias_font
//! ```
//!
//! ## Expected
//!
//! - The log prints `antialias_font: Rust, async` and `refresh done in N ms`.
//! - Landscape, 960 x 540. Top half white: `Roboto Black 40pt` in black with hard edges, then
//!   `Anti-aliased (80pt)` in black with smooth edges. Bottom half black: `Inverted
//!   anti-aliased` in white with smooth edges. All three lines are the same size.
//! - Differences from the sketch: the sketch draws the 80 pt font at double size and averages it
//!   down to 4 gray levels, here the glyph is rendered once at the final size with the 16 levels of
//!   the panel. The sketch's bitmap fonts are Roboto Black as well. The string `40pt` and `80pt`
//!   are the sketch's own text, the size is the same on every line.
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

/// The font file, see `assets/NOTICE.md`.
const ROBOTO_BLACK: &[u8] = include_bytes!("../../assets/fonts/Roboto-Black-ASCII.ttf");

/// The hardware-free part: text drawing from a TrueType font. Same text in the async crate, and
/// tested on the host by `host-tests`.
mod ui {
    use embedded_graphics::{pixelcolor::Gray4, prelude::*};
    use fontdue::Font;

    /// Em size in pixels. The sketch's "40pt" bitmap font is 92 px high, which is about this em.
    pub const PX: f32 = 78.0;

    /// How a line of text is drawn.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Look {
        /// 1 bit text: black where the glyph covers at least half a pixel, nothing elsewhere.
        Hard,
        /// Black text on white, the glyph coverage becomes the 16 grays of the panel.
        Smooth,
        /// White text on black, the same with the grays reversed.
        SmoothOnBlack,
    }

    /// Gray level of one pixel of a glyph, or `None` where the glyph does not cover it.
    pub fn level(look: Look, coverage: u8) -> Option<Gray4> {
        if coverage == 0 {
            return None;
        }
        let steps = (u16::from(coverage) * 15 + 127) / 255;
        match look {
            Look::Hard => (coverage >= 128).then_some(Gray4::BLACK),
            Look::Smooth => Some(Gray4::new(15 - steps as u8)),
            Look::SmoothOnBlack => Some(Gray4::new(steps as u8)),
        }
    }

    /// Draws `text` with the left end of its baseline at `(x, baseline)` and returns the x where
    /// the next character would start. Pixels the glyphs do not cover are left alone.
    pub fn draw_text<D: DrawTarget<Color = Gray4>>(
        target: &mut D,
        font: &Font,
        px: f32,
        x: i32,
        baseline: i32,
        text: &str,
        look: Look,
    ) -> i32 {
        let mut pen = x as f32;
        for ch in text.chars() {
            let (metrics, coverage) = font.rasterize(ch, px);
            let left = (pen + 0.5) as i32 + metrics.xmin;
            let top = baseline - metrics.ymin - metrics.height as i32;
            let width = metrics.width.max(1);
            let pixels = coverage.iter().enumerate().filter_map(|(i, &c)| {
                let color = level(look, c)?;
                let (col, row) = ((i % width) as i32, (i / width) as i32);
                Some(Pixel(Point::new(left + col, top + row), color))
            });
            let _ = target.draw_iter(pixels);
            pen += metrics.advance_width;
        }
        (pen + 0.5) as i32
    }

    /// Width of `text` in pixels.
    #[allow(dead_code)] // for the host tests
    pub fn text_width(font: &Font, px: f32, text: &str) -> f32 {
        text.chars()
            .map(|ch| font.metrics(ch, px).advance_width)
            .sum()
    }
}

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = rust_lilygot5_47_s3_embassy_examples::init!();
    rust_lilygot5_47_s3_embassy_examples::start_rtos!(p);
    info!("antialias_font: Rust, async");
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
    let Ok(font) = fontdue::Font::from_bytes(ROBOTO_BLACK, fontdue::FontSettings::default()) else {
        error!("the font does not load");
        rust_lilygot5_47_s3_embassy_examples::halt();
    };

    let _ = fb.clear(Gray4::WHITE);
    ui::draw_text(
        &mut fb,
        &font,
        ui::PX,
        0,
        70,
        "Roboto Black 40pt",
        ui::Look::Hard,
    );
    ui::draw_text(
        &mut fb,
        &font,
        ui::PX,
        0,
        150,
        "Anti-aliased (80pt)",
        ui::Look::Smooth,
    );
    // `fillRect(0, height / 2, width, height / 2, 0)`: the lower half is black.
    let _ = Rectangle::new(Point::new(0, 270), Size::new(960, 270))
        .into_styled(PrimitiveStyle::with_fill(Gray4::BLACK))
        .draw(&mut fb);
    ui::draw_text(
        &mut fb,
        &font,
        ui::PX,
        0,
        380,
        "Inverted anti-aliased",
        ui::Look::SmoothOnBlack,
    );

    let start = Instant::now();
    if display.refresh(&fb, RefreshMode::Quality16).await.is_err() {
        error!("refresh failed");
    }
    info!("refresh done in {} ms", start.elapsed().as_millis());
    loop {
        Timer::after(Duration::from_secs(1)).await;
    }
}
