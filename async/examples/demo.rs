//! # demo: port of the LilyGo `demo` sketch
//!
//! Shows the state of the board on the panel: microSD card size, whether the clock and a touch
//! controller answer, the battery voltage, the local time (Pacific, from the PCF8563 clock that
//! WiFi and NTP set) and how often the user button was pressed. The Arduino sketch is
//! `~/Src/Arduino/LilyGoT5ePaper/LilyGo/demo`. The async twin is `demo` in `async/`; here the blocking
//! `epdpi` calls run on the `esp-rtos` executor because the network stack needs it.
//!
//! Differences from the sketch: the LilyGo logo is not shown and the emoji are words. This board has
//! no touch panel, so the corner boxes and the `Sleep` box are not drawn, and holding the user button
//! (GPIO21) for 2 s does what the `Sleep` box does: it shows `Sleep` and deep-sleeps for 30 s or until
//! the button is pressed. The three lines that change are redrawn every 10 s with a partial refresh in
//! black and white. The clock holds UTC and the line shows Pacific time (`PST8PDT`).
//!
//! ## Run
//!
//! ```text
//! cp secret.rs.example secret.rs   # then edit it
//! cargo run --release --example demo
//! ```
//!
//! ## Expected
//!
//! - The log prints `demo: Rust, async` and the wake-up cause.
//! - Landscape, 960 x 540 (no rotation). A title and the lines `SD card`, `RTC`, `Touch`, `Voltage`, the time and
//!   `Pressed`. The time line reads `NTP no` until WiFi and NTP have set the clock, then `NTP ok`.
//! - Each press of the user button raises `Pressed` by one. Holding it for 2 s shows `Sleep`, and the
//!   board wakes 30 s later or on a press.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::{boxed::Box, vec};
use core::sync::atomic::{AtomicU32, Ordering};
use defmt::{Debug2Format, error, info, warn};
use embassy_executor::Spawner;
use embassy_net::{Config as NetConfig, Runner, Stack, StackResources, dns::DnsQueryType, udp};
use embassy_time::{Duration, Instant, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::SdCard;
use esp_hal::analog::adc::{Adc, AdcCalCurve, AdcConfig, Attenuation};
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull, RtcPin};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::rng::Rng;
use esp_hal::rtc_cntl::{
    Rtc,
    sleep::{Ext1WakeupSource, TimerWakeupSource, WakeupLevel},
    wakeup_cause,
};
use esp_hal::spi::{Mode as SpiMode, master::Config as SpiConfig, master::Spi};
use esp_hal::time::Rate;
use esp_println as _;
use esp_radio::wifi::{Config, ControllerConfig, Interface, WifiController, sta::StationConfig};
use pcf8563::{DateTime, PCF8563};
use rust_lilygot5_47_s3_embassy_examples::epdpi::prelude::*;
use sntpc::{NtpContext, get_time};
use sntpc_net_embassy::UdpSocketWrapper;
use sntpc_time_embassy::EmbassyTimestampGenerator;

esp_bootloader_esp_idf::esp_app_desc!();

mod secret {
    include!("../../secret.rs");
}

/// The font file, see `assets/NOTICE.md`.
const ROBOTO_BLACK: &[u8] = include_bytes!("../../assets/fonts/Roboto-Black-ASCII.ttf");

/// The hardware-free part: time zone, line texts, button logic and drawing. Same text in the other
/// crate, and tested on the host by `host-tests`.
mod ui {
    use alloc::{format, string::String};
    use embedded_graphics::{
        pixelcolor::Gray4,
        prelude::*,
        primitives::{PrimitiveStyle, Rectangle},
    };
    use fontdue::Font;

    /// Pool the sketch asks first.
    pub const NTP_SERVER: &str = "pool.ntp.org";
    /// Pacific time with daylight saving: second Sunday of March to first Sunday of November.
    pub const TZ: &str = "PST8PDT,M3.2.0,M11.1.0";
    /// The status lines start here, like the sketch's `cursor_x`.
    pub const TEXT_X: i32 = 200;
    /// Baselines of the lines, the sketch steps 50 px.
    pub const TITLE_Y: i32 = 110;
    pub const SD_Y: i32 = 200;
    pub const RTC_Y: i32 = 250;
    pub const TOUCH_Y: i32 = 300;
    pub const VOLT_Y: i32 = 350;
    pub const TIME_Y: i32 = 400;
    pub const PRESS_Y: i32 = 450;
    /// The area the three changing lines are drawn in.
    pub const DYNAMIC: (i32, i32, u32, u32) = (190, 310, 700, 160);
    pub const PX: f32 = 30.0;
    pub const TITLE_PX: f32 = 46.0;
    /// A press must hold this long, in milliseconds, to count.
    pub const DEBOUNCE_MS: u32 = 20;
    /// Holding this long sleeps.
    pub const LONG_MS: u32 = 2000;
    /// The battery is read through a divider that halves it, and shown no higher than a full cell.
    pub const MAX_MV: u32 = 4200;

    /// A UTC or local date and time.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Clock {
        pub year: u16,
        pub month: u8,
        pub day: u8,
        pub hour: u8,
        pub minute: u8,
        pub second: u8,
        /// 0 = Sunday.
        pub weekday: u8,
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
            weekday: (days + 4).rem_euclid(7) as u8,
        }
    }

    /// Unix seconds of a UTC date and time.
    pub fn unix(c: &Clock) -> i64 {
        let y = i64::from(c.year) - i64::from(c.month <= 2);
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let mp = (i64::from(c.month) + 9) % 12;
        let doy = (153 * mp + 2) / 5 + i64::from(c.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        (era * 146_097 + doe - 719_468) * 86_400
            + i64::from(c.hour) * 3600
            + i64::from(c.minute) * 60
            + i64::from(c.second)
    }

    /// The local time for UTC Unix seconds, and the zone's short name (`PST` or `PDT`).
    pub fn local(unix_utc: i64) -> Option<(Clock, &'static str)> {
        let zone = timezone_data::parse_posix_tz(TZ).ok()?;
        let (name, offset, _dst) = zone.lookup(unix_utc);
        Some((clock(unix_utc + i64::from(offset)), name))
    }

    /// Battery millivolts from the millivolts at the pin.
    pub fn battery_mv(pin_mv: u32) -> u32 {
        (pin_mv * 2).min(MAX_MV)
    }

    pub fn sd_line(bytes: Option<u64>) -> String {
        match bytes {
            Some(b) => {
                let tenths = b * 10 / (1 << 30);
                format!("SD card: {}.{} GB", tenths / 10, tenths % 10)
            }
            None => String::from("SD card: not found"),
        }
    }

    pub fn rtc_line(found: bool) -> String {
        String::from(if found {
            "RTC: found"
        } else {
            "RTC: not found"
        })
    }

    pub fn touch_line(found: bool) -> String {
        String::from(if found {
            "Touch: found (not used)"
        } else {
            "Touch: not fitted"
        })
    }

    pub fn voltage_line(mv: u32) -> String {
        format!("Voltage: {}.{:02} V", mv / 1000, mv % 1000 / 10)
    }

    /// The time line: the local time and whether NTP has set the clock.
    pub fn time_line(unix_utc: Option<i64>, synced: bool) -> String {
        let ntp = if synced { "NTP ok" } else { "NTP no" };
        match unix_utc.and_then(local) {
            Some((t, zone)) => format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {}, {}",
                t.year, t.month, t.day, t.hour, t.minute, t.second, zone, ntp
            ),
            None => format!("Time: unknown, {}", ntp),
        }
    }

    pub fn pressed_line(count: u32) -> String {
        format!("Pressed: {}", count)
    }

    /// What the button did.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Event {
        None,
        /// A press of at least [`DEBOUNCE_MS`], reported once.
        Press,
        /// The press is still held after [`LONG_MS`], reported once.
        Long,
    }

    /// Turns the raw level of the button into [`Event`]s.
    #[derive(Debug, Clone, Copy)]
    pub struct Button {
        since: Option<u32>,
        counted: bool,
        long: bool,
    }

    impl Button {
        pub fn new() -> Self {
            Self {
                since: None,
                counted: false,
                long: false,
            }
        }

        /// Feeds the raw level (true while pressed) at `now` milliseconds.
        pub fn update(&mut self, pressed: bool, now: u32) -> Event {
            if !pressed {
                *self = Self::new();
                return Event::None;
            }
            let since = *self.since.get_or_insert(now);
            let held = now.wrapping_sub(since);
            if !self.counted && held >= DEBOUNCE_MS {
                self.counted = true;
                return Event::Press;
            }
            if !self.long && held >= LONG_MS {
                self.long = true;
                return Event::Long;
            }
            Event::None
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

    /// Width of `text` in pixels.
    pub fn text_width(font: &Font, px: f32, text: &str) -> f32 {
        text.chars()
            .map(|c| font.metrics(c, px).advance_width)
            .sum()
    }

    /// The first full draw: title and the lines that do not change.
    pub fn draw_static<D: DrawTarget<Color = Gray4>>(
        target: &mut D,
        font: &Font,
        sd: &str,
        rtc: &str,
        touch: &str,
    ) {
        let _ = target.clear(Gray4::WHITE);
        draw_line(target, font, TITLE_PX, TEXT_X, TITLE_Y, "LILYGO T5 4.7 S3");
        draw_line(target, font, PX, TEXT_X, SD_Y, sd);
        draw_line(target, font, PX, TEXT_X, RTC_Y, rtc);
        draw_line(target, font, PX, TEXT_X, TOUCH_Y, touch);
    }

    /// Clears the changing area and draws the three lines in it.
    pub fn draw_dynamic<D: DrawTarget<Color = Gray4>>(
        target: &mut D,
        font: &Font,
        volt: &str,
        time: &str,
        pressed: &str,
    ) {
        let (x, y, w, h) = DYNAMIC;
        let _ = Rectangle::new(Point::new(x, y), Size::new(w, h))
            .into_styled(PrimitiveStyle::with_fill(Gray4::WHITE))
            .draw(target);
        draw_line(target, font, PX, TEXT_X, VOLT_Y, volt);
        draw_line(target, font, PX, TEXT_X, TIME_Y, time);
        draw_line(target, font, PX, TEXT_X, PRESS_Y, pressed);
    }

    /// The `Sleep` screen: the word centered on a white panel.
    pub fn draw_sleep<D: DrawTarget<Color = Gray4> + OriginDimensions>(
        target: &mut D,
        font: &Font,
    ) {
        let Size { width, height } = target.size();
        let _ = target.clear(Gray4::WHITE);
        let w = text_width(font, TITLE_PX, "Sleep");
        draw_line(
            target,
            font,
            TITLE_PX,
            (width as f32 / 2.0 - w / 2.0) as i32,
            height as i32 / 2,
            "Sleep",
        );
    }
}

/// Unix seconds from NTP, 0 until the first answer. A `u32` holds Unix seconds until 2106.
static NTP_UNIX: AtomicU32 = AtomicU32::new(0);

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, Interface<'static>>) -> ! {
    runner.run().await
}

/// Connects, asks the pool for the time once and hands it to the main loop through [`NTP_UNIX`].
#[embassy_executor::task]
async fn ntp_task(mut controller: WifiController<'static>, stack: Stack<'static>) {
    if controller.connect_async().await.is_err() {
        error!("wifi connect failed");
        return;
    }
    info!("wifi connected");
    stack.wait_config_up().await;
    let Ok(addrs) = stack.dns_query(ui::NTP_SERVER, DnsQueryType::A).await else {
        error!("dns failed");
        return;
    };
    let Some(&embassy_net::IpAddress::Ipv4(server)) = addrs.first() else {
        error!("dns gave no IPv4 address");
        return;
    };
    let mut rx_meta = [udp::PacketMetadata::EMPTY; 4];
    let mut tx_meta = [udp::PacketMetadata::EMPTY; 4];
    let mut rx_buf = [0u8; 256];
    let mut tx_buf = [0u8; 256];
    let mut socket =
        udp::UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    if socket.bind(123).is_err() {
        error!("udp bind failed");
        return;
    }
    let socket = UdpSocketWrapper::from(socket);
    let context = NtpContext::new(EmbassyTimestampGenerator::default());
    let addr = core::net::SocketAddr::new(core::net::IpAddr::V4(server), 123);
    match get_time(addr, &socket, context).await {
        Ok(result) => {
            info!("NTP time {}", result.seconds);
            NTP_UNIX.store(result.seconds as u32, Ordering::Release);
        }
        Err(_) => error!("NTP request failed"),
    }
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let p = rust_lilygot5_47_s3_embassy_examples::init!();
    info!("demo: Rust, async");
    info!("wake-up cause: {}", Debug2Format(&wakeup_cause()));
    // The radio wants internal RAM, see the `esp-radio` docs and `wifi_scan` in the M5 repo.
    rust_lilygot5_47_s3_embassy_examples::esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    rust_lilygot5_47_s3_embassy_examples::esp_alloc::heap_allocator!(size: 36 * 1024);
    rust_lilygot5_47_s3_embassy_examples::start_rtos!(p);

    let mut button_pin = p.GPIO21;
    let button = Input::new(
        button_pin.reborrow(),
        InputConfig::default().with_pull(Pull::Up),
    );
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

    // microSD on SPI: SCLK 11, MOSI 15, MISO 16, CS 42, as in `micro_sd`.
    let spi = Spi::new(
        p.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(10))
            .with_mode(SpiMode::_0),
    )
    .map(|spi| {
        spi.with_sck(p.GPIO11)
            .with_miso(p.GPIO16)
            .with_mosi(p.GPIO15)
    });
    let Ok(spi) = spi else {
        error!("spi config failed");
        rust_lilygot5_47_s3_embassy_examples::halt();
    };
    let cs = Output::new(p.GPIO42, Level::High, OutputConfig::default());
    let device = ExclusiveDevice::new_no_delay(spi, cs).unwrap_or_else(|e| match e {});
    let card = SdCard::new(device, Delay::new());
    let card_bytes = card.num_bytes().ok();
    info!("card bytes: {}", card_bytes);

    // I2C: the clock at 0x51, and the touch controller at 0x14 or 0x5D if there is one.
    let Ok(i2c) = I2c::new(
        p.I2C0,
        I2cConfig::default().with_frequency(Rate::from_khz(100)),
    ) else {
        error!("i2c config failed");
        rust_lilygot5_47_s3_embassy_examples::halt();
    };
    let mut i2c = i2c.with_sda(p.GPIO18).with_scl(p.GPIO17);
    let touch_found = [0x14u8, 0x5D].iter().any(|&a| i2c.write(a, &[]).is_ok());
    let mut rtc = PCF8563::new(i2c);
    let rtc_found = rtc.get_voltage_low_flag().is_ok();
    info!("rtc found: {}, touch found: {}", rtc_found, touch_found);

    // Battery divider on GPIO14.
    let mut adc_config = AdcConfig::new();
    let mut adc_pin =
        adc_config.enable_pin_with_cal::<_, AdcCalCurve<_>>(p.GPIO14, Attenuation::_11dB);
    let mut adc = Adc::new(p.ADC2, adc_config);

    // WiFi and NTP in the background.
    let station = StationConfig::default()
        .with_ssid(secret::WIFI_SSID)
        .with_password(secret::WIFI_PASSWORD.into());
    match esp_radio::wifi::new(
        p.WIFI,
        ControllerConfig::default().with_initial_config(Config::Station(station)),
    ) {
        Ok((controller, interfaces)) => {
            let seed = u64::from(Rng::new().random()) << 32 | u64::from(Rng::new().random());
            let resources = Box::leak(Box::new(StackResources::<4>::new()));
            let (stack, runner) = embassy_net::new(
                interfaces.station,
                NetConfig::dhcpv4(Default::default()),
                resources,
                seed,
            );
            if let (Ok(net), Ok(ntp)) = (net_task(runner), ntp_task(controller, stack)) {
                spawner.spawn(net);
                spawner.spawn(ntp);
            }
        }
        Err(_) => warn!("wifi init failed"),
    }

    let sd = ui::sd_line(card_bytes);
    let rtc_text = ui::rtc_line(rtc_found);
    let touch = ui::touch_line(touch_found);
    ui::draw_static(&mut fb, &font, &sd, &rtc_text, &touch);
    ui::draw_dynamic(
        &mut fb,
        &font,
        &ui::voltage_line(0),
        &ui::time_line(None, false),
        &ui::pressed_line(0),
    );
    if display.refresh(&fb, RefreshMode::Quality16).await.is_err() {
        error!("refresh failed");
    }
    fb.clear_dirty();
    info!("first draw done");

    let start = Instant::now();
    let mut button_state = ui::Button::new();
    let mut presses = 0u32;
    let mut synced = false;
    let mut next_update = 0u32;
    loop {
        let now = (Instant::now() - start).as_millis() as u32;
        let mut redraw = now >= next_update;
        match button_state.update(button.is_low(), now) {
            ui::Event::Press => {
                presses += 1;
                info!("pressed {}", presses);
                redraw = true;
            }
            ui::Event::Long => {
                info!("deep sleep");
                break;
            }
            ui::Event::None => {}
        }
        let ntp = NTP_UNIX.swap(0, Ordering::AcqRel);
        if ntp != 0 {
            let t = ui::clock(i64::from(ntp));
            let now_utc = DateTime {
                year: (t.year - 2000) as u8,
                month: t.month,
                weekday: t.weekday,
                day: t.day,
                hours: t.hour,
                minutes: t.minute,
                seconds: t.second,
            };
            let _ = rtc.rtc_init();
            if rtc.set_datetime(&now_utc).is_ok() {
                synced = true;
                info!("RTC set from NTP");
            }
            redraw = true;
        }
        if redraw {
            next_update = now + 10_000;
            let mut pin_mv = 0;
            for _ in 0..1000 {
                if let Ok(v) = adc.read_oneshot(&mut adc_pin) {
                    pin_mv = v;
                    break;
                }
            }
            let unix = rtc.get_datetime().ok().map(|t| {
                ui::unix(&ui::Clock {
                    year: 2000 + u16::from(t.year),
                    month: t.month,
                    day: t.day,
                    hour: t.hours,
                    minute: t.minutes,
                    second: t.seconds,
                    weekday: t.weekday,
                })
            });
            ui::draw_dynamic(
                &mut fb,
                &font,
                &ui::voltage_line(ui::battery_mv(u32::from(pin_mv))),
                &ui::time_line(unix.filter(|_| rtc_found), synced),
                &ui::pressed_line(presses),
            );
            if display
                .refresh_dirty(&mut fb, RefreshMode::FastMono)
                .await
                .is_err()
            {
                error!("refresh failed");
            }
        }
        Timer::after(Duration::from_millis(5)).await;
    }

    ui::draw_sleep(&mut fb, &font);
    if display.refresh(&fb, RefreshMode::Quality16).await.is_err() {
        error!("refresh failed");
    }
    Timer::after(Duration::from_millis(1000)).await;
    // Wake after 30 s or on a press of the button (low), like the sketch.
    let mut rtc_sleep = Rtc::new(p.LPWR);
    let mut wake_pins: [&mut dyn RtcPin; 1] = [&mut button_pin];
    let ext1 = Ext1WakeupSource::new(&mut wake_pins, WakeupLevel::Low);
    let timer = TimerWakeupSource::new(core::time::Duration::from_secs(30));
    rtc_sleep.sleep_deep(&[&ext1, &timer]);
}
