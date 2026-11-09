//! # quotables: port of the LilyGo `Quotables` sketch
//!
//! Fetches a random quote over HTTPS and shows it: the quote in quotation marks, wrapped into 800 px (at
//! most 6 lines), and the author at the bottom right. The Arduino sketch is
//! `~/Src/Arduino/LilyGoT5ePaper/Quotables`. The blocking twin is `quotables` in `blocking/`.
//!
//! Differences from the sketch: the quote marks are part of the text, on the first and last line (the sketch
//! has one opening mark on its own line above the text, and no closing mark), and the text is Roboto Black (`assets/fonts`, Apache 2.0) at 50, 37 and 25 px,
//! the em sizes of the sketch's OpenSans 24B, 18B and 12B, whose bitmap headers need the Arduino GFX
//! code. The Wi-Fi stays connected between refreshes when the board stays awake. TLS is `reqwless` over
//! `embedded-tls` with no certificate check, like the sketch's `setInsecure()`. With a USB host connected
//! the board stays awake and refreshes every 5 min, otherwise it deep-sleeps 30 min, as in the sketch.
//!
//! ## Run
//!
//! ```text
//! cp secret.rs.example secret.rs   # then edit it
//! cargo run --release --example quotables
//! ```
//!
//! ## Expected
//!
//! - The log prints `quotables: Rust, async`, `got IP`, `quote: ...`, and `USB connected: staying awake`.
//! - Landscape, 960 x 540. A quote of at most six lines in quotation marks, and `- Author` at
//!   the bottom right. A message `WiFi connection failed.` or `Could not fetch a quote.` on a failure.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::{boxed::Box, vec};
use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_net::{
    Config as NetConfig, Runner, StackResources,
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
};
use embassy_time::{Duration, Timer, with_timeout};
use esp_hal::rng::Rng;
use esp_hal::rtc_cntl::{Rtc, sleep::TimerWakeupSource};
use esp_println as _;
use esp_radio::wifi::{Config, ControllerConfig, Interface, sta::StationConfig};
use reqwless::{
    client::{HttpClient, TlsConfig, TlsVerify},
    request::Method,
};
use rust_lilygot5_47_s3_embassy_examples::epdpi::prelude::*;

esp_bootloader_esp_idf::esp_app_desc!();

mod secret {
    include!("../../secret.rs");
}

/// The font file, see `assets/NOTICE.md`.
const ROBOTO_BLACK: &[u8] = include_bytes!("../../assets/fonts/Roboto-Black-ASCII.ttf");

/// The hardware-free part: parsing, ASCII folding, wrapping and drawing. Same text in the other crate,
/// and tested on the host by `host-tests`.
mod ui {
    use alloc::{format, string::String, vec::Vec};
    use embedded_graphics::{pixelcolor::Gray4, prelude::*};
    use fontdue::Font;

    pub const URL: &str = "https://api.quotable.kurokeita.dev/api/quotes/random";
    /// Longer quotes are skipped, they would not fit.
    pub const MAX_QUOTE_LEN: usize = 160;
    pub const MAX_TRIES: u32 = 10;
    pub const DEV_REFRESH_MINUTES: u64 = 5;
    pub const SLEEP_MINUTES: u64 = 30;
    pub const W: i32 = 960;
    pub const H: i32 = 540;
    /// Em sizes of the sketch's OpenSans 24B, 18B and 12B (line heights 68, 51 and 34 over 1.36).
    pub const BIG_PX: f32 = 50.0;
    pub const MID_PX: f32 = 37.0;
    pub const SMALL_PX: f32 = 25.0;
    /// The quote box and the author, as in the sketch.
    pub const QUOTE_X: i32 = 80;
    pub const QUOTE_Y: i32 = 130;
    pub const QUOTE_W: i32 = W - 160;
    pub const MAX_LINES: usize = 6;
    pub const LINE_H: i32 = 60;
    pub const AUTHOR_RIGHT: i32 = W - 60;
    pub const AUTHOR_Y: i32 = H - 40;
    pub const MESSAGE: (i32, i32) = (60, 280);
    pub const WIFI_FAILED: &str = "WiFi connection failed.";
    pub const NO_QUOTE: &str = "Could not fetch a quote.";

    /// A quote and its author, already folded to ASCII.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Quote {
        pub text: String,
        pub author: String,
    }

    /// The font has ASCII only: curly quotes, dashes and the ellipsis become their ASCII look, every
    /// other non-ASCII character is dropped.
    pub fn ascii_fold(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                c if c.is_ascii() => out.push(c),
                '\u{2018}' | '\u{2019}' => out.push('\''),
                '\u{201C}' | '\u{201D}' => out.push('"'),
                '\u{2013}' | '\u{2014}' => out.push('-'),
                '\u{2026}' => out.push_str("..."),
                _ => {}
            }
        }
        out
    }

    /// The quote of a response body, or `None` when the JSON is bad, a field is missing or the quote is
    /// longer than [`MAX_QUOTE_LEN`] bytes.
    pub fn parse(body: &[u8]) -> Option<Quote> {
        let value: serde_json::Value = serde_json::from_slice(body).ok()?;
        let content = value["quote"]["content"].as_str()?;
        let name = value["quote"]["author"]["name"].as_str()?;
        if content.len() > MAX_QUOTE_LEN {
            return None;
        }
        Some(Quote {
            text: ascii_fold(content),
            author: ascii_fold(name),
        })
    }

    /// Width of `text` in pixels.
    pub fn text_width(font: &Font, px: f32, text: &str) -> f32 {
        text.chars()
            .map(|c| font.metrics(c, px).advance_width)
            .sum()
    }

    /// The wrapped lines and whether text was left over.
    #[derive(Debug, PartialEq, Eq)]
    pub struct Wrapped {
        pub lines: Vec<String>,
        pub overflow: bool,
    }

    /// Word wrap like the sketch's `drawWrapped`: a word that makes the line wider than `width` starts
    /// the next line, at most `max_lines` lines, and `overflow` when words are left. The sketch drops a
    /// last word that arrives together with the last line without saying so, and so does this.
    pub fn wrap(font: &Font, px: f32, text: &str, width: f32, max_lines: usize) -> Wrapped {
        let n = text.len();
        let mut pos = 0;
        let mut current = String::new();
        let mut lines: Vec<String> = Vec::new();
        while pos < n && lines.len() < max_lines {
            let rest = &text[pos..];
            let (word, advance) = match rest.find(' ') {
                Some(space) => (&rest[..space], space + 1),
                None => (rest, rest.len()),
            };
            pos += advance;
            let trial = if current.is_empty() {
                String::from(word)
            } else {
                format!("{current} {word}")
            };
            if text_width(font, px, &trial) > width && !current.is_empty() {
                lines.push(core::mem::take(&mut current));
                current = String::from(word);
            } else {
                current = trial;
            }
        }
        let mut overflow = false;
        if lines.len() < max_lines && !current.is_empty() {
            lines.push(current);
        } else if pos < n && lines.len() >= max_lines {
            overflow = true;
        }
        Wrapped { lines, overflow }
    }

    /// Level of a glyph pixel from its coverage, black on white.
    pub fn level(coverage: u8) -> Option<Gray4> {
        if coverage == 0 {
            return None;
        }
        let steps = (u16::from(coverage) * 15 + 127) / 255;
        Some(Gray4::new(15 - steps as u8))
    }

    /// Draws one line with the left end of its baseline at `(x, baseline)`, black and smooth.
    pub fn draw_line<D: DrawTarget<Color = Gray4>>(
        target: &mut D,
        font: &Font,
        px: f32,
        x: i32,
        baseline: i32,
        text: &str,
    ) {
        let mut pen = x as f32;
        for ch in text.chars() {
            let (metrics, coverage) = font.rasterize(ch, px);
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
    }

    /// The quote screen: the mark, the wrapped quote and the author at the bottom right.
    pub fn draw_quote<D: DrawTarget<Color = Gray4>>(target: &mut D, font: &Font, quote: &Quote) {
        let _ = target.clear(Gray4::WHITE);
        // The quote marks are part of the text, so they sit on the first and last line, next to the words.
        let quoted = format!("\"{}\"", quote.text);
        let wrapped = wrap(font, BIG_PX, &quoted, QUOTE_W as f32, MAX_LINES);
        for (i, line) in wrapped.lines.iter().enumerate() {
            draw_line(
                target,
                font,
                BIG_PX,
                QUOTE_X,
                QUOTE_Y + i as i32 * LINE_H,
                line,
            );
        }
        if wrapped.overflow {
            draw_line(
                target,
                font,
                BIG_PX,
                QUOTE_X + QUOTE_W - 30,
                QUOTE_Y + (MAX_LINES as i32 - 1) * LINE_H,
                "...",
            );
        }
        let author = format!("- {}", quote.author);
        let width = text_width(font, SMALL_PX, &author) as i32;
        draw_line(
            target,
            font,
            SMALL_PX,
            AUTHOR_RIGHT - width,
            AUTHOR_Y,
            &author,
        );
    }

    /// A one line message on a white panel.
    pub fn draw_message<D: DrawTarget<Color = Gray4>>(target: &mut D, font: &Font, message: &str) {
        let _ = target.clear(Gray4::WHITE);
        draw_line(target, font, MID_PX, MESSAGE.0, MESSAGE.1, message);
    }
}

/// True while a USB host is attached: the SOF frame counter of the USB device runs only then, like
/// the sketch's `Serial.isConnected()`.
async fn usb_connected() -> bool {
    let first = esp_hal::peripherals::USB_DEVICE::regs()
        .fram_num()
        .read()
        .bits();
    Timer::after(Duration::from_millis(3)).await;
    let second = esp_hal::peripherals::USB_DEVICE::regs()
        .fram_num()
        .read()
        .bits();
    first != second
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, Interface<'static>>) -> ! {
    runner.run().await
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let p = rust_lilygot5_47_s3_embassy_examples::init!();
    info!("quotables: Rust, async");
    // The radio wants internal RAM, see `wifi_ntp`.
    rust_lilygot5_47_s3_embassy_examples::esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    rust_lilygot5_47_s3_embassy_examples::esp_alloc::heap_allocator!(size: 36 * 1024);
    rust_lilygot5_47_s3_embassy_examples::start_rtos!(p);

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

    let station = StationConfig::default()
        .with_ssid(secret::WIFI_SSID)
        .with_password(secret::WIFI_PASSWORD.into());
    let Ok((mut controller, interfaces)) = esp_radio::wifi::new(
        p.WIFI,
        ControllerConfig::default().with_initial_config(Config::Station(station)),
    ) else {
        error!("wifi init failed");
        rust_lilygot5_47_s3_embassy_examples::halt();
    };
    let rng = Rng::new();
    let seed = u64::from(rng.random()) << 32 | u64::from(rng.random());
    let resources = Box::leak(Box::new(StackResources::<6>::new()));
    let (stack, runner) = embassy_net::new(
        interfaces.station,
        NetConfig::dhcpv4(Default::default()),
        resources,
        seed,
    );
    let Ok(task) = net_task(runner) else {
        error!("net task did not start");
        rust_lilygot5_47_s3_embassy_examples::halt();
    };
    spawner.spawn(task);

    let mut tls_read = vec![0u8; 16_640];
    let mut tls_write = vec![0u8; 4_096];
    let mut body = vec![0u8; 8_192];
    let state = TcpClientState::<1, 1_024, 1_024>::new();
    let tcp = TcpClient::new(stack, &state);
    let dns = DnsSocket::new(stack);
    let tls = TlsConfig::new(seed, &mut tls_read, &mut tls_write, TlsVerify::None);
    let mut client = HttpClient::new_with_tls(&tcp, &dns, tls);
    let mut rtc = Rtc::new(p.LPWR);

    loop {
        let connected = matches!(
            with_timeout(Duration::from_secs(30), controller.connect_async()).await,
            Ok(Ok(_))
        );
        let mut shown = false;
        if !connected {
            error!("wifi connection failed");
            ui::draw_message(&mut fb, &font, ui::WIFI_FAILED);
        } else {
            stack.wait_config_up().await;
            if let Some(config) = stack.config_v4() {
                info!("got IP {}", config.address.address());
            }
            let mut quote = None;
            for attempt in 0..ui::MAX_TRIES {
                if attempt > 0 {
                    Timer::after(Duration::from_secs(1)).await;
                }
                let Ok(mut request) = client.request(Method::GET, ui::URL).await else {
                    error!("request failed");
                    continue;
                };
                let Ok(response) = request.send(&mut body).await else {
                    error!("send failed");
                    continue;
                };
                if response.status.0 != 200 {
                    error!("HTTP {}", response.status.0);
                    continue;
                }
                let Ok(bytes) = response.body().read_to_end().await else {
                    error!("body failed");
                    continue;
                };
                quote = ui::parse(bytes);
                if quote.is_some() {
                    break;
                }
                info!("quote skipped (bad or too long), {} bytes", bytes.len());
            }
            match quote {
                Some(quote) => {
                    info!("quote: {} / {}", quote.text.as_str(), quote.author.as_str());
                    ui::draw_quote(&mut fb, &font, &quote);
                    shown = true;
                }
                None => ui::draw_message(&mut fb, &font, ui::NO_QUOTE),
            }
        }
        if display.refresh(&fb, RefreshMode::Quality16).await.is_err() {
            error!("refresh failed");
        }
        info!("quote shown: {}", shown);

        if usb_connected().await {
            info!(
                "USB connected: staying awake, next refresh in {} min",
                ui::DEV_REFRESH_MINUTES
            );
            Timer::after(Duration::from_secs(ui::DEV_REFRESH_MINUTES * 60)).await;
        } else {
            info!("deep sleep for {} min", ui::SLEEP_MINUTES);
            let timer =
                TimerWakeupSource::new(core::time::Duration::from_secs(ui::SLEEP_MINUTES * 60));
            rtc.sleep_deep(&[&timer]);
        }
    }
}
