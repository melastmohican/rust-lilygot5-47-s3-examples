//! # gif_player: port of the FastEPD `gif_player` sketch
//!
//! Draws a title, then plays a 32 frame, 432 x 322, 1 bit animated GIF once, centred on the
//! panel, one partial update per frame and as fast as it can run. After 3 s the panel flashes black
//! and white and stops. The Arduino sketch is
//! `~/Src/Arduino/LilyGoT5ePaper/FastEPD/gif_player`, which decodes with AnimatedGIF. The blocking
//! twin is `gif_player` in `blocking/`. Here `epdpi` runs in async mode on the `esp-rtos` Embassy
//! executor.
//!
//! The animation is `assets/jimothy_walk.gif`, the author's raccoon sketch walking, which you can
//! open in any viewer. The sketch itself plays a pixel art city (`1bitsmallcity.h`) of unknown
//! licence, so it is not used here. The GIF is made by `tools/jimothy_walk.py`. It is decoded with
//! `tinygif`.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example gif_player
//! ```
//!
//! ## Expected
//!
//! - The log prints `gif_player: Rust, async` and, at the end, `played N frames in N ms`.
//! - Landscape, 960 x 540, white. The title `epdpi GIF Demo` near the top. A pencil drawing of a
//!   raccoon, black lines on white (432 x 322 at x 264, y 109), takes two steps: the front paw and
//!   the hind foot swing forward together, then the other pair, the body bobs, the head nods and
//!   the tail sways. It plays once, holds the last frame for 3 s, flashes black and then white,
//!   and stays blank.
//! - Differences from the sketch: the title is drawn with `u8g2-fonts` where the sketch uses a
//!   FastEPD font, and each frame is a `FastMono` update of the area that changed.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::{error, info};
use embassy_time::{Duration, Instant, Timer};
use embedded_graphics::{
    pixelcolor::{Gray4, Rgb888},
    prelude::*,
};
use esp_println as _;
use rust_lilygot5_47_s3_embassy_examples::epdpi::prelude::*;
use u8g2_fonts::{
    FontRenderer, fonts,
    types::{FontColor, HorizontalAlignment, VerticalPosition},
};

esp_bootloader_esp_idf::esp_app_desc!();

/// The animation, see `assets/NOTICE.md`.
const GIF: &[u8] = include_bytes!("../../assets/jimothy_walk.gif");

/// The hardware-free part: turning GIF colors into panel grays and placing a frame. Same text in
/// the async crate, and tested on the host by `host-tests`.
mod ui {
    use embedded_graphics::{
        pixelcolor::{Gray4, Rgb888},
        prelude::*,
    };
    use tinygif::{Frame, Gif};

    /// The panel size.
    const PANEL_W: i32 = 960;
    const PANEL_H: i32 = 540;

    /// The gray level of a GIF color: its luma, in the top 4 bits.
    pub fn gray(color: Rgb888) -> Gray4 {
        let luma =
            (u32::from(color.r()) * 299 + u32::from(color.g()) * 587 + u32::from(color.b()) * 114)
                / 1000;
        Gray4::new((luma >> 4) as u8)
    }

    /// Top left corner that centres the GIF canvas on the panel, as the sketch does.
    pub fn origin(gif: &Gif<'_, Rgb888>) -> Point {
        Point::new(
            ((PANEL_W - i32::from(gif.width())) / 2).max(0),
            ((PANEL_H - i32::from(gif.height())) / 2).max(0),
        )
    }

    /// A draw target that turns the colors of a frame into grays and moves them by `offset`.
    struct GifTarget<'a, D> {
        inner: &'a mut D,
        offset: Point,
    }

    impl<D: DrawTarget<Color = Gray4>> Dimensions for GifTarget<'_, D> {
        fn bounding_box(&self) -> embedded_graphics::primitives::Rectangle {
            self.inner.bounding_box()
        }
    }

    impl<D: DrawTarget<Color = Gray4>> DrawTarget for GifTarget<'_, D> {
        type Color = Rgb888;
        type Error = D::Error;

        fn draw_iter<I: IntoIterator<Item = Pixel<Rgb888>>>(
            &mut self,
            pixels: I,
        ) -> Result<(), Self::Error> {
            let offset = self.offset;
            self.inner.draw_iter(
                pixels
                    .into_iter()
                    .map(|Pixel(point, color)| Pixel(point + offset, gray(color))),
            )
        }
    }

    /// Draws one frame of the GIF into `target`, its canvas corner at `offset`. Pixels that the
    /// frame leaves transparent are not touched, so frames build on the ones before.
    pub fn draw_frame<D: DrawTarget<Color = Gray4>>(
        frame: &Frame<'_, Rgb888>,
        target: &mut D,
        offset: Point,
    ) {
        let mut adapter = GifTarget {
            inner: target,
            offset,
        };
        let _ = frame.draw(&mut adapter);
    }
}

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = rust_lilygot5_47_s3_embassy_examples::init!();
    rust_lilygot5_47_s3_embassy_examples::start_rtos!(p);
    info!("gif_player: Rust, async");
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
    let Ok(gif) = tinygif::Gif::<Rgb888>::from_slice(GIF) else {
        error!("the GIF does not parse");
        rust_lilygot5_47_s3_embassy_examples::halt();
    };

    // The title, then a full refresh: `fullUpdate(CLEAR_SLOW, true)`.
    let _ = fb.clear(Gray4::WHITE);
    let _ = FontRenderer::new::<fonts::u8g2_font_fub42_tf>().render_aligned(
        "epdpi GIF Demo",
        Point::new(480, 90),
        VerticalPosition::Baseline,
        HorizontalAlignment::Center,
        FontColor::Transparent(Gray4::BLACK),
        &mut fb,
    );
    if display.clear().await.is_err() || display.refresh(&fb, RefreshMode::FastMono).await.is_err()
    {
        error!("first refresh failed");
    }
    fb.clear_dirty();

    // Play the GIF once, a partial update per frame, as fast as it goes.
    let origin = ui::origin(&gif);
    let start = Instant::now();
    let mut frames = 0u32;
    for frame in gif.frames() {
        ui::draw_frame(&frame, &mut fb, origin);
        if display
            .refresh_dirty(&mut fb, RefreshMode::FastMono)
            .await
            .is_err()
        {
            error!("refresh failed");
        }
        frames += 1;
    }
    info!(
        "played {} frames in {} ms",
        frames,
        start.elapsed().as_millis()
    );

    // `clearBlack` and `clearWhite`: one clear cycle does both.
    Timer::after(Duration::from_secs(3)).await;
    if display.clear().await.is_err() {
        error!("final clear failed");
    }
    rust_lilygot5_47_s3_embassy_examples::halt();
}
