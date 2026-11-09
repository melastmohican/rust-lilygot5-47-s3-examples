//! # button: port of the LilyGo-EPD47 `button` sketch
//!
//! Shows a page of text about the chip in a frame. Each press of the user button (GPIO21) shows the
//! next page. The fourth press shows `DeepSleep`, switches the panel off and puts the board into deep
//! sleep. A press of the button wakes it and starts over from the first page. The Arduino sketch is
//! `~/Src/Arduino/LilyGoT5ePaper/LilyGo/button`. The blocking twin is `button` in `blocking/`.
//! Here `epdpi` runs in async mode on the `esp-rtos` Embassy executor.
//!
//! Differences from the sketch: the sketch describes the original ESP32 and has `Prev` and `Next`
//! boxes for the three buttons of that board, this board has one user button, so the pages describe
//! the ESP32-S3 and only `Next` is drawn. The text is Roboto Black (`assets/fonts`, Apache 2.0)
//! drawn with `fontdue`, not FiraSans, and LilyGo's logo image is not shown. A page turn is a full
//! `Quality16` refresh, where the sketch clears the frame and writes into it.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example button
//! ```
//!
//! ## Expected
//!
//! - The log prints `button: Rust, async`, the wake-up cause, and `page N` for each page shown.
//! - Landscape, 960 x 540. A frame with the first page of text and a `Next` box at the bottom right.
//!   Each press of the user button (the one on GPIO21, not RESET or BOOT) shows the next page, about
//!   1 s after the press. The fourth press shows `DeepSleep`, then the board sleeps: the log prints
//!   `deep sleep` and the serial port disappears. A press of the button wakes it and shows page 1
//!   again, with the wake-up cause `Ext1`.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::{Debug2Format, error, info};
use embassy_time::{Duration, Instant, Timer};
use esp_hal::gpio::{Input, InputConfig, Pull, RtcPin};
use esp_hal::rtc_cntl::{
    Rtc,
    sleep::{Ext1WakeupSource, WakeupLevel},
    wakeup_cause,
};
use esp_println as _;
use rust_lilygot5_47_s3_embassy_examples::epdpi::prelude::*;

esp_bootloader_esp_idf::esp_app_desc!();

/// The font file, see `assets/NOTICE.md`.
const ROBOTO_BLACK: &[u8] = include_bytes!("../../assets/fonts/Roboto-Black-ASCII.ttf");

/// The hardware-free part: the pages, the button logic and the drawing. Same text in the async
/// crate, and tested on the host by `host-tests`.
mod ui {
    use embedded_graphics::{
        pixelcolor::Gray4,
        prelude::*,
        primitives::{PrimitiveStyle, Rectangle},
    };
    use fontdue::Font;

    /// Em size of the page text in pixels.
    pub const PX: f32 = 30.0;
    /// Distance between two lines of text.
    pub const LINE: i32 = 42;
    /// The sketch's frame, `area1`: x 10, y 20, 940 wide, `EPD_HEIGHT / 2 + 80` high.
    pub const FRAME: (i32, i32, u32, u32) = (10, 20, 940, 350);
    /// Where the first line of text starts, the sketch's `cursor_x`, `cursor_y`.
    pub const TEXT_X: i32 = 20;
    pub const TEXT_Y: i32 = 60;
    /// The `Next` box, the sketch's second box.
    pub const NEXT_BOX: (i32, i32, u32, u32) = (740, 450, 120, 60);
    /// A press must hold this long, in milliseconds, to count.
    pub const DEBOUNCE_MS: u32 = 20;

    /// The pages of text. The sketch describes the ESP32, this is the ESP32-S3 of the board.
    pub const PAGES: [&str; 3] = [
        "ESP32-S3 is a single 2.4 GHz Wi-Fi and Bluetooth LE\n\
         combo chip designed with the TSMC ultra-low-power\n\
         40 nm technology. It is designed to achieve the best\n\
         power and RF performance, showing robustness,\n\
         versatility and reliability in a wide variety of\n\
         applications and power scenarios.",
        "> Xtensa LX7 dual-core 32-bit microprocessor\n\
         > 384 KB ROM and 16 MB flash\n\
         > 512 KB SRAM and 8 MB PSRAM\n\
         > 16 KB SRAM in RTC\n\
         > Wi-Fi 802.11 b/g/n (2.4 GHz) and Bluetooth 5 LE\n\
         > USB OTG and 45 GPIOs",
        "> 16 color grayscale\n\
         > Use with 4.7\" EPDs\n\
         > High-quality font rendering\n\
         > Rust, epdpi and no_std\n\
         > about 1 s for a full frame draw",
    ];

    /// What a press of the button asks for.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Action {
        /// Show the text page with this index.
        Page(usize),
        /// Show `DeepSleep` and go to sleep.
        Sleep,
    }

    /// Which page comes next, like the sketch's `state`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Pager {
        state: usize,
    }

    impl Pager {
        /// Starts on the first page, which is shown without a press.
        pub fn new() -> Self {
            Self { state: 0 }
        }

        /// The button was pressed: pages 2 and 3, then sleep. After the sleep action it would wrap
        /// to the first page, the board is not awake to see it.
        pub fn press(&mut self) -> Action {
            self.state = (self.state + 1) % (PAGES.len() + 1);
            if self.state < PAGES.len() {
                Action::Page(self.state)
            } else {
                Action::Sleep
            }
        }
    }

    /// Turns the raw level of a button into presses: a press counts once the level has been
    /// "pressed" for [`DEBOUNCE_MS`], and the next one needs a release first.
    #[derive(Debug, Clone, Copy)]
    pub struct Debouncer {
        down: bool,
        since: Option<u32>,
        counted: bool,
    }

    impl Debouncer {
        pub fn new() -> Self {
            Self {
                down: false,
                since: None,
                counted: false,
            }
        }

        /// Feeds the raw level (true while the button is pressed) at `now` milliseconds. Returns
        /// true once per press.
        pub fn update(&mut self, pressed: bool, now: u32) -> bool {
            if !pressed {
                self.down = false;
                self.since = None;
                self.counted = false;
                return false;
            }
            if !self.down {
                self.down = true;
                self.since = Some(now);
            }
            let held = now.wrapping_sub(self.since.unwrap_or(now));
            if !self.counted && held >= DEBOUNCE_MS {
                self.counted = true;
                return true;
            }
            false
        }
    }

    /// Black on white gray level of a glyph pixel from its coverage.
    pub fn level(coverage: u8) -> Option<Gray4> {
        if coverage == 0 {
            return None;
        }
        let steps = (u16::from(coverage) * 15 + 127) / 255;
        Some(Gray4::new(15 - steps as u8))
    }

    /// Draws one line with the left end of its baseline at `(x, baseline)`, black and smooth, and
    /// returns the x where the next character would start.
    pub fn draw_line<D: DrawTarget<Color = Gray4>>(
        target: &mut D,
        font: &Font,
        x: i32,
        baseline: i32,
        text: &str,
    ) -> i32 {
        let mut pen = x as f32;
        for ch in text.chars() {
            let (metrics, coverage) = font.rasterize(ch, PX);
            let left = (pen + 0.5) as i32 + metrics.xmin;
            let top = baseline - metrics.ymin - metrics.height as i32;
            let width = metrics.width.max(1);
            let pixels = coverage.iter().enumerate().filter_map(|(i, &c)| {
                let color = level(c)?;
                let (col, row) = ((i % width) as i32, (i / width) as i32);
                Some(Pixel(Point::new(left + col, top + row), color))
            });
            let _ = target.draw_iter(pixels);
            pen += metrics.advance_width;
        }
        (pen + 0.5) as i32
    }

    /// Draws `text`, one `draw_line` per line, the first baseline at `(x, y)`.
    pub fn draw_text<D: DrawTarget<Color = Gray4>>(
        target: &mut D,
        font: &Font,
        x: i32,
        y: i32,
        text: &str,
    ) {
        for (i, line) in text.lines().enumerate() {
            draw_line(target, font, x, y + i as i32 * LINE, line);
        }
    }

    /// Draws a whole screen: white, the frame, the text and the `Next` box.
    pub fn draw_screen<D: DrawTarget<Color = Gray4>>(target: &mut D, font: &Font, text: &str) {
        let _ = target.clear(Gray4::WHITE);
        let outline = PrimitiveStyle::with_stroke(Gray4::BLACK, 2);
        let (x, y, w, h) = FRAME;
        let _ = Rectangle::new(Point::new(x, y), Size::new(w, h))
            .into_styled(outline)
            .draw(target);
        draw_text(target, font, TEXT_X, TEXT_Y, text);
        let (x, y, w, h) = NEXT_BOX;
        let _ = Rectangle::new(Point::new(x, y), Size::new(w, h))
            .into_styled(outline)
            .draw(target);
        draw_line(target, font, x + 15, y + 40, "Next");
    }
}

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = rust_lilygot5_47_s3_embassy_examples::init!();
    rust_lilygot5_47_s3_embassy_examples::start_rtos!(p);
    info!("button: Rust, async");
    info!("wake-up cause: {}", Debug2Format(&wakeup_cause()));
    let mut pin = p.GPIO21;
    let button = Input::new(pin.reborrow(), InputConfig::default().with_pull(Pull::Up));
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

    info!("page 1");
    ui::draw_screen(&mut fb, &font, ui::PAGES[0]);
    if display.refresh(&fb, RefreshMode::Quality16).await.is_err() {
        error!("refresh failed");
    }

    let start = Instant::now();
    let mut pager = ui::Pager::new();
    let mut debounce = ui::Debouncer::new();
    loop {
        let now = start.elapsed().as_millis() as u32;
        if debounce.update(button.is_low(), now) {
            match pager.press() {
                ui::Action::Page(index) => {
                    info!("page {}", index + 1);
                    ui::draw_screen(&mut fb, &font, ui::PAGES[index]);
                    if display.refresh(&fb, RefreshMode::Quality16).await.is_err() {
                        error!("refresh failed");
                    }
                }
                ui::Action::Sleep => {
                    info!("deep sleep");
                    ui::draw_screen(&mut fb, &font, "DeepSleep");
                    if display.refresh(&fb, RefreshMode::Quality16).await.is_err() {
                        error!("refresh failed");
                    }
                    Timer::after(Duration::from_secs(1)).await;
                    break;
                }
            }
        }
        Timer::after(Duration::from_millis(2)).await;
    }

    // The panel is already off after its last refresh. Wake on the button, low, like the sketch. The
    // input driver is not used any more, so the pin can be handed to the wake-up source.
    let mut rtc = Rtc::new(p.LPWR);
    let mut wake_pins: [&mut dyn RtcPin; 1] = [&mut pin];
    let ext1 = Ext1WakeupSource::new(&mut wake_pins, WakeupLevel::Low);
    rtc.sleep_deep(&[&ext1]);
}
