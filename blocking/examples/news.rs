//! # news: port of the LilyGo `News` sketch
//!
//! Fetches the top headlines from NewsAPI over HTTPS and shows up to four of them: a title, a rule, the date
//! and the time of the last update, then for each article a headline and a description. The Arduino sketch is
//! `~/Src/Arduino/LilyGoT5ePaper/News`. The async twin is `news` in `async/`. The network stack is async, so this twin
//! also runs on the `esp-rtos` executor, with the blocking `epdpi` calls for the panel.
//!
//! Needs a free NewsAPI key in `secret.rs` as `NEWS_API_KEY` (copy `secret.rs.example`). The key and the
//! request URL, which contains it, are never logged.
//!
//! Differences from the sketch: the text is Open Sans Bold (`assets/fonts`, OFL 1.1), the sketch's face, at 50, 25, 21 and
//! 17 px, the em sizes of its OpenSans 24B, 12B, 10B and 8B bitmaps. The boxes are a little tighter than
//! the sketch's: headline lines 26 px apart instead of 28, the description 2 px under the headline instead
//! of 4, and the separator below the box instead of through the second description line. The time comes from SNTP and is shown
//! in Pacific time (`PST8PDT`); the sketch does the same with `configTzTime`. The Wi-Fi stays connected
//! between refreshes when the board stays awake. TLS is `reqwless` over `embedded-tls` with no certificate
//! check, like the sketch's `setInsecure()`. With a USB host connected the board stays awake and refreshes
//! every 30 min (the free tier allows 100 requests a day), otherwise it deep-sleeps 60 min, as in the sketch.
//!
//! ## Run
//!
//! ```text
//! cp secret.rs.example secret.rs   # then edit it, with the WiFi and the NewsAPI key
//! cargo run --release --example news
//! ```
//!
//! ## Expected
//!
//! - The log prints `news: Rust, blocking`, `got IP`, `fetch ok, N items`, and `USB connected: staying awake`.
//! - Landscape, 960 x 540. `World News`, the date and the update time, and up to four articles. A message
//!   `WiFi connection failed.` or `Failed to fetch news.` on a failure.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::{boxed::Box, vec};
use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_net::{
    Config as NetConfig, Runner, Stack, StackResources,
    dns::{DnsQueryType, DnsSocket},
    tcp::client::{TcpClient, TcpClientState},
    udp,
};
use embassy_time::{Duration, Timer, with_timeout};
use esp_hal::rng::Rng;
use esp_hal::rtc_cntl::{Rtc, sleep::TimerWakeupSource};
use esp_println as _;
use esp_radio::wifi::{Config, ControllerConfig, Interface, sta::StationConfig};
use reqwless::{
    client::{HttpClient, TlsConfig, TlsVerify},
    request::{Method, RequestBuilder},
};
use rust_lilygot5_47_s3_examples::epdpi::prelude::*;
use sntpc::{NtpContext, get_time};
use sntpc_net_embassy::UdpSocketWrapper;
use sntpc_time_embassy::EmbassyTimestampGenerator;

esp_bootloader_esp_idf::esp_app_desc!();

mod secret {
    include!("../../secret.rs");
}

/// The font file, see `assets/NOTICE.md`.
const OPEN_SANS_BOLD: &[u8] = include_bytes!("../../assets/fonts/OpenSans-Bold.ttf");

/// The hardware-free part: parsing, ASCII folding, wrapping, the time zone and drawing. Same text in the
/// other crate, and tested on the host by `host-tests`.
mod ui {
    use alloc::{format, string::String, vec::Vec};
    use embedded_graphics::{
        pixelcolor::Gray4,
        prelude::*,
        primitives::{PrimitiveStyle, Rectangle},
    };
    use fontdue::Font;

    pub const NTP_SERVER: &str = "0.north-america.pool.ntp.org";
    /// Pacific time with daylight saving, like the sketch's `PST8PDT,M3.2.0,M11.1.0`.
    pub const TZ: &str = "PST8PDT,M3.2.0,M11.1.0";
    pub const COUNTRY: &str = "us";
    pub const MAX_ITEMS: usize = 4;
    pub const DEV_REFRESH_MINUTES: u64 = 30;
    pub const SLEEP_MINUTES: u64 = 60;
    pub const W: i32 = 960;
    /// Em sizes of the sketch's OpenSans 24B, 12B, 10B and 8B (line heights 68, 34, 28 and 23 over 1.36).
    pub const HEAD_PX: f32 = 50.0;
    pub const TITLE_PX: f32 = 25.0;
    pub const DATE_PX: f32 = 21.0;
    pub const TEXT_PX: f32 = 17.0;
    pub const MARGIN: i32 = 40;
    pub const MESSAGE: (i32, i32) = (60, 280);
    pub const WIFI_FAILED: &str = "WiFi connection failed.";
    pub const NO_NEWS: &str = "Failed to fetch news.";
    /// The sketch's gray levels as 4 bit levels: DarkGrey 0x44, LightGrey 0xBB.
    pub const DARK_GRAY: u8 = 4;
    pub const LIGHT_GRAY: u8 = 11;
    /// Where the article boxes start, their height and the line heights of title and description.
    pub const TOP: i32 = 112;
    pub const BOX_H: i32 = 105;
    pub const TITLE_LINE: i32 = 26;
    pub const TEXT_LINE: i32 = 22;
    /// Space between the last headline line and the first description line.
    pub const DESC_GAP: i32 = 2;
    /// The separator sits in the gap below a box: the sketch's `y + 99` cut through the second description line.
    pub const SEPARATOR_Y: i32 = BOX_H + 1;

    /// An article with a headline and a description, folded to ASCII.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Item {
        pub title: String,
        pub description: String,
    }

    /// The font has ASCII only: curly quotes, dashes and the ellipsis become their ASCII look, every other
    /// non-ASCII character is dropped.
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

    /// The request URL for a key. It contains the key, so it must not be logged.
    pub fn url(key: &str) -> String {
        format!("https://newsapi.org/v2/top-headlines?country={COUNTRY}&pageSize=10&apiKey={key}")
    }

    /// Up to [`MAX_ITEMS`] articles of a response that have a title and a non-empty description, or `None`
    /// when the JSON is bad or no article qualifies.
    pub fn parse(body: &[u8]) -> Option<Vec<Item>> {
        let value: serde_json::Value = serde_json::from_slice(body).ok()?;
        let mut items = Vec::new();
        for article in value["articles"].as_array()? {
            if items.len() >= MAX_ITEMS {
                break;
            }
            let (Some(title), Some(description)) =
                (article["title"].as_str(), article["description"].as_str())
            else {
                continue;
            };
            if description.is_empty() {
                continue;
            }
            items.push(Item {
                title: ascii_fold(title),
                description: ascii_fold(description),
            });
        }
        (!items.is_empty()).then_some(items)
    }

    /// A date and time.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Clock {
        pub year: u16,
        pub month: u8,
        pub day: u8,
        pub hour: u8,
        pub minute: u8,
        pub second: u8,
    }

    /// Calendar date of Unix seconds (after Howard Hinnant's `civil_from_days`).
    pub fn clock(unix: i64) -> Clock {
        let days = unix.div_euclid(86_400);
        let secs = unix.rem_euclid(86_400) as u32;
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as u16;
        Clock {
            year,
            month,
            day,
            hour: (secs / 3600) as u8,
            minute: (secs % 3600 / 60) as u8,
            second: (secs % 60) as u8,
        }
    }

    /// The Pacific local time for UTC Unix seconds.
    pub fn local(unix_utc: i64) -> Option<Clock> {
        let zone = timezone_data::parse_posix_tz(TZ).ok()?;
        let (_name, offset, _dst) = zone.lookup(unix_utc);
        Some(clock(unix_utc + i64::from(offset)))
    }

    /// `Date: dd.mm.yyyy`, as the sketch's `dateStr`.
    pub fn date_line(local: Option<Clock>) -> String {
        match local {
            Some(t) => format!("Date: {:02}.{:02}.{:04}", t.day, t.month, t.year),
            None => String::from("Date: unknown"),
        }
    }

    /// `Last update: hh:mm`, as the sketch's `updStr`.
    pub fn update_line(local: Option<Clock>) -> String {
        match local {
            Some(t) => format!("Last update: {:02}:{:02}", t.hour, t.minute),
            None => String::from("Last update: unknown"),
        }
    }

    /// Width of `text` in pixels.
    pub fn text_width(font: &Font, px: f32, text: &str) -> f32 {
        text.chars()
            .map(|c| font.metrics(c, px).advance_width)
            .sum()
    }

    /// Word wrap like the sketch's `drawWrapped`: a word that makes the line wider than `width` starts the
    /// next line, at most `max_lines` lines. Returns the lines and whether words were left. The sketch drops
    /// a last word that arrives together with the last line without saying so, and so does this.
    pub fn wrap(
        font: &Font,
        px: f32,
        text: &str,
        width: f32,
        max_lines: usize,
    ) -> (Vec<String>, bool) {
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
        (lines, overflow)
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

    fn draw_centered<D: DrawTarget<Color = Gray4>>(
        target: &mut D,
        font: &Font,
        px: f32,
        x_center: i32,
        baseline: i32,
        text: &str,
    ) {
        let x = x_center - text_width(font, px, text) as i32 / 2;
        draw_line(target, font, px, x, baseline, text);
    }

    fn draw_right<D: DrawTarget<Color = Gray4>>(
        target: &mut D,
        font: &Font,
        px: f32,
        x_right: i32,
        baseline: i32,
        text: &str,
    ) {
        let x = x_right - text_width(font, px, text) as i32;
        draw_line(target, font, px, x, baseline, text);
    }

    fn fill<D: DrawTarget<Color = Gray4>>(
        target: &mut D,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        level: u8,
    ) {
        let _ = Rectangle::new(Point::new(x, y), Size::new(w, h))
            .into_styled(PrimitiveStyle::with_fill(Gray4::new(level)))
            .draw(target);
    }

    /// The news screen as the sketch's `drawNews`.
    pub fn draw_news<D: DrawTarget<Color = Gray4>>(
        target: &mut D,
        font: &Font,
        items: &[Item],
        date: &str,
        updated: &str,
    ) {
        let _ = target.clear(Gray4::WHITE);
        let inner = (W - 2 * MARGIN) as u32;
        draw_centered(target, font, HEAD_PX, W / 2, 48, "World News");
        fill(target, MARGIN, 62, inner, 4, DARK_GRAY);
        draw_line(target, font, DATE_PX, MARGIN, 92, date);
        draw_right(target, font, DATE_PX, W - MARGIN, 92, updated);
        fill(target, MARGIN, 102, inner, 2, 0);
        for (i, item) in items.iter().enumerate() {
            let y = TOP + i as i32 * BOX_H;
            let (title, _) = wrap(font, TITLE_PX, &item.title, inner as f32, 2);
            for (n, line) in title.iter().enumerate() {
                draw_line(
                    target,
                    font,
                    TITLE_PX,
                    MARGIN,
                    y + 24 + n as i32 * TITLE_LINE,
                    line,
                );
            }
            // The sketch's `drawWrapped` returns the baseline after the last line it drew.
            let next = y + 24 + title.len() as i32 * TITLE_LINE;
            let (text, overflow) = wrap(font, TEXT_PX, &item.description, inner as f32, 2);
            for (n, line) in text.iter().enumerate() {
                draw_line(
                    target,
                    font,
                    TEXT_PX,
                    MARGIN,
                    next + DESC_GAP + n as i32 * TEXT_LINE,
                    line,
                );
            }
            if overflow {
                draw_line(
                    target,
                    font,
                    TEXT_PX,
                    MARGIN + inner as i32 - 30,
                    next + DESC_GAP + TEXT_LINE,
                    "...",
                );
            }
            if i + 1 < items.len() {
                fill(target, MARGIN, y + SEPARATOR_Y, inner, 1, LIGHT_GRAY);
            }
        }
    }

    /// A one line message on a white panel.
    pub fn draw_message<D: DrawTarget<Color = Gray4>>(target: &mut D, font: &Font, message: &str) {
        let _ = target.clear(Gray4::WHITE);
        draw_line(target, font, 37.0, MESSAGE.0, MESSAGE.1, message);
    }
}

/// True while a USB host is attached: the SOF frame counter of the USB device runs only then, like the
/// sketch's `Serial.isConnected()`.
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

/// Unix seconds from the pool, or `None`.
async fn ntp_unix(stack: Stack<'static>) -> Option<i64> {
    info!("ntp: dns");
    let addrs = with_timeout(
        Duration::from_secs(10),
        stack.dns_query(ui::NTP_SERVER, DnsQueryType::A),
    )
    .await
    .ok()?
    .ok()?;
    let Some(&embassy_net::IpAddress::Ipv4(server)) = addrs.first() else {
        return None;
    };
    info!("ntp: asking {}", server);
    let mut rx_meta = [udp::PacketMetadata::EMPTY; 4];
    let mut tx_meta = [udp::PacketMetadata::EMPTY; 4];
    let mut rx_buf = [0u8; 256];
    let mut tx_buf = [0u8; 256];
    let mut socket =
        udp::UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    socket.bind(123).ok()?;
    let socket = UdpSocketWrapper::from(socket);
    let context = NtpContext::new(EmbassyTimestampGenerator::default());
    let addr = core::net::SocketAddr::new(core::net::IpAddr::V4(server), 123);
    let result = with_timeout(Duration::from_secs(10), get_time(addr, &socket, context))
        .await
        .ok()?
        .ok()?;
    i64::try_from(result.seconds).ok()
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, Interface<'static>>) -> ! {
    runner.run().await
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let p = rust_lilygot5_47_s3_examples::init!();
    info!("news: Rust, blocking");
    // The radio wants internal RAM, see `wifi_ntp`.
    rust_lilygot5_47_s3_examples::esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    rust_lilygot5_47_s3_examples::esp_alloc::heap_allocator!(size: 36 * 1024);
    let timg0 = esp_hal::timer::timg::TimerGroup::new(p.TIMG0);
    let sw_interrupt = esp_hal::interrupt::software::SoftwareInterruptControl::new(p.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

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
    let Ok(font) = fontdue::Font::from_bytes(OPEN_SANS_BOLD, fontdue::FontSettings::default())
    else {
        error!("the font does not load");
        rust_lilygot5_47_s3_examples::halt();
    };

    let station = StationConfig::default()
        .with_ssid(secret::WIFI_SSID)
        .with_password(secret::WIFI_PASSWORD.into());
    let Ok((mut controller, interfaces)) = esp_radio::wifi::new(
        p.WIFI,
        ControllerConfig::default().with_initial_config(Config::Station(station)),
    ) else {
        error!("wifi init failed");
        rust_lilygot5_47_s3_examples::halt();
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
        rust_lilygot5_47_s3_examples::halt();
    };
    spawner.spawn(task);

    // A response with ten articles is tens of KiB; the heap is the 8 MB PSRAM.
    let mut tls_read = vec![0u8; 16_640];
    let mut tls_write = vec![0u8; 4_096];
    let mut body = vec![0u8; 96 * 1024];
    let state = TcpClientState::<1, 1_024, 1_024>::new();
    let tcp = TcpClient::new(stack, &state);
    let dns = DnsSocket::new(stack);
    let tls = TlsConfig::new(seed, &mut tls_read, &mut tls_write, TlsVerify::None);
    let mut client = HttpClient::new_with_tls(&tcp, &dns, tls);
    let mut rtc = Rtc::new(p.LPWR);
    let url = ui::url(secret::NEWS_API_KEY);

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
            let now = ntp_unix(stack).await.and_then(ui::local);
            info!("time known: {}", now.is_some());
            let mut items = None;
            if let Ok(request) = client.request(Method::GET, &url).await {
                let mut request = request.headers(&[("User-Agent", "LilyGo-T5-4.7")]);
                match request.send(&mut body).await {
                    Ok(response) if response.status.0 == 200 => {
                        match response.body().read_to_end().await {
                            Ok(bytes) => {
                                items = ui::parse(bytes);
                                info!("body {} bytes", bytes.len());
                            }
                            Err(_) => error!("body failed"),
                        }
                    }
                    Ok(response) => error!("HTTP {}", response.status.0),
                    Err(_) => error!("send failed"),
                }
            } else {
                error!("request failed");
            }
            match items {
                Some(items) => {
                    info!("fetch ok, {} items", items.len());
                    for item in &items {
                        info!("  {}", item.title.as_str());
                    }
                    let date = ui::date_line(now);
                    let updated = ui::update_line(now);
                    ui::draw_news(&mut fb, &font, &items, &date, &updated);
                    shown = true;
                }
                None => {
                    error!("fetch failed");
                    ui::draw_message(&mut fb, &font, ui::NO_NEWS);
                }
            }
        }
        if display.refresh(&fb, RefreshMode::Quality16).is_err() {
            error!("refresh failed");
        }
        info!("news shown: {}", shown);

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
