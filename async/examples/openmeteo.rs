//! # openmeteo: port of the LilyGo `OpenMeteo-Weather` sketch
//!
//! A weather dashboard from Open-Meteo (no key): the current weather, the next six hours as a graph of
//! temperature and rain chance, and six days with highs and lows, for Bellevue. The Arduino sketch is
//! `~/Src/Arduino/LilyGoT5ePaper/OpenMeteo-Weather`. The blocking twin is `openmeteo` in `blocking/`.
//!
//! Differences from the sketch: the text is Open Sans Bold (`assets/fonts`, OFL 1.1), the sketch's face, at the
//! em sizes of its bitmaps (50, 37, 25, 21 and 17 px). The icons are the sketch's Lucide icons (ISC licence) as
//! 1-bit BMP files in `assets/icons`. A missing or null value in the response is a failed fetch, where the
//! sketch shows a zero. The time comes from SNTP and is Pacific (`PST8PDT`). TLS is `reqwless` over
//! `embedded-tls` with no certificate check. With a USB host connected the board stays awake and refreshes every
//! 5 min, otherwise it deep-sleeps 60 min, as in the sketch. The Wi-Fi stays connected between refreshes.
//!
//! ## Run
//!
//! ```text
//! cp secret.rs.example secret.rs   # then edit it, with the WiFi name and password
//! cargo run --release --example openmeteo
//! ```
//!
//! ## Expected
//!
//! - The log prints `openmeteo: Rust, async`, `got IP`, `fetch ok`, `dashboard drawn` and
//!   `USB connected: staying awake`.
//! - Landscape, 960 x 540: a header with the city, the temperature and the condition and a gray info panel with
//!   the battery, the date and the refresh time; an hourly graph; a strip of six days.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::{boxed::Box, vec};
use defmt::{error, info, warn};
use embassy_executor::Spawner;
use embassy_net::{
    Config as NetConfig, Runner, Stack, StackResources,
    dns::{DnsQueryType, DnsSocket},
    tcp::client::{TcpClient, TcpClientState},
    udp,
};
use embassy_time::{Duration, Timer, with_timeout};
use esp_hal::analog::adc::{Adc, AdcCalCurve, AdcConfig, Attenuation};
use esp_hal::rng::Rng;
use esp_hal::rtc_cntl::{Rtc, sleep::TimerWakeupSource};
use esp_println as _;
use esp_radio::wifi::{Config, ControllerConfig, Interface, sta::StationConfig};
use reqwless::{
    client::{HttpClient, TlsConfig, TlsVerify},
    request::Method,
};
use rust_lilygot5_47_s3_embassy_examples::epdpi::prelude::*;
use sntpc::{NtpContext, get_time};
use sntpc_net_embassy::UdpSocketWrapper;
use sntpc_time_embassy::EmbassyTimestampGenerator;

esp_bootloader_esp_idf::esp_app_desc!();

mod secret {
    include!("../../secret.rs");
}

/// The font file, see `assets/NOTICE.md`.
const OPEN_SANS_BOLD: &[u8] = include_bytes!("../../assets/fonts/OpenSans-Bold.ttf");

/// The icon files, see `assets/NOTICE.md`.
fn icon_bytes(icon: ui::Icon) -> &'static [u8] {
    match icon {
        ui::Icon::ClearSky => include_bytes!("../../assets/icons/clear_sky.bmp"),
        ui::Icon::PartlyCloudy => include_bytes!("../../assets/icons/partly_cloudy.bmp"),
        ui::Icon::Fog => include_bytes!("../../assets/icons/fog.bmp"),
        ui::Icon::Rain => include_bytes!("../../assets/icons/rain.bmp"),
        ui::Icon::Snow => include_bytes!("../../assets/icons/snow.bmp"),
        ui::Icon::Storm => include_bytes!("../../assets/icons/storm.bmp"),
        ui::Icon::Gray => include_bytes!("../../assets/icons/gray.bmp"),
        ui::Icon::FullBattery => include_bytes!("../../assets/icons/full_battery.bmp"),
        ui::Icon::HighBattery => include_bytes!("../../assets/icons/high_battery.bmp"),
        ui::Icon::HalfBattery => include_bytes!("../../assets/icons/half_battery.bmp"),
        ui::Icon::LowBattery => include_bytes!("../../assets/icons/low_battery.bmp"),
    }
}

/// The hardware-free part: the request, the parsing of the response and the mappings of the sketch. Same text
/// in the other crate, and tested on the host by `host-tests`.
mod ui {
    use alloc::{format, string::String, vec::Vec};
    use embedded_graphics::{
        pixelcolor::Gray4,
        prelude::*,
        primitives::{Line, PrimitiveStyle, Rectangle, Triangle},
    };
    use fontdue::Font;
    use tinybmp::RawBmp;

    /// The sketch's location: city name, latitude and longitude, metric units.
    pub const CITY: &str = "Bellevue";
    pub const USER: &str = "melastmohican";
    pub const LATITUDE: f32 = 47.6101;
    pub const LONGITUDE: f32 = -122.2015;
    pub const TEMP_LABEL: &str = " C";
    pub const NTP_SERVER: &str = "0.north-america.pool.ntp.org";
    /// Pacific time with daylight saving, like the sketch's `PST8PDT,M3.2.0,M11.1.0`.
    pub const TZ: &str = "PST8PDT,M3.2.0,M11.1.0";
    pub const DEV_REFRESH_MINUTES: u64 = 5;
    pub const SLEEP_MINUTES: u64 = 60;
    /// Days and hours the sketch reads out of the response.
    pub const DAYS: usize = 7;
    pub const SHOWN_DAYS: usize = 6;
    pub const HOURS: usize = 6;

    /// The icons of the sketch, the file names in `assets/icons` without `.bmp`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Icon {
        ClearSky,
        PartlyCloudy,
        Fog,
        Rain,
        Snow,
        Storm,
        Gray,
        FullBattery,
        HighBattery,
        HalfBattery,
        LowBattery,
    }

    impl Icon {
        /// The file name in `assets/icons`, without `.bmp` (the host tests check the files exist).
        #[allow(dead_code)]
        pub fn file_name(self) -> &'static str {
            match self {
                Icon::ClearSky => "clear_sky",
                Icon::PartlyCloudy => "partly_cloudy",
                Icon::Fog => "fog",
                Icon::Rain => "rain",
                Icon::Snow => "snow",
                Icon::Storm => "storm",
                Icon::Gray => "gray",
                Icon::FullBattery => "full_battery",
                Icon::HighBattery => "high_battery",
                Icon::HalfBattery => "half_battery",
                Icon::LowBattery => "low_battery",
            }
        }
    }

    /// The sketch's `weatherIcon`: a WMO weather code to an icon.
    pub fn weather_icon(code: i32) -> Icon {
        match code {
            0 => Icon::ClearSky,
            1..=3 => Icon::PartlyCloudy,
            45 | 48 => Icon::Fog,
            51 | 53 | 55 | 56 | 57 | 61 | 63 | 65 | 66 | 67 | 80 | 81 | 82 => Icon::Rain,
            71 | 73 | 75 | 77 | 85 | 86 => Icon::Snow,
            95 | 96 | 99 => Icon::Storm,
            _ => Icon::Gray,
        }
    }

    /// The sketch's `weatherDescription`.
    pub fn weather_description(code: i32) -> &'static str {
        match code {
            0 => "Clear sky",
            1..=3 => "Mainly clear, partly cloudy",
            45 | 48 => "Fog",
            51 | 53 | 55 => "Drizzle",
            56 | 57 => "Freezing drizzle",
            61 | 63 | 65 => "Rain",
            66 | 67 => "Freezing rain",
            71 | 73 | 75 => "Snowfall",
            77 => "Snow grains",
            80..=82 => "Rain showers",
            85 | 86 => "Snow showers",
            95 => "Thunderstorm",
            96 | 99 => "Thunderstorm, hail",
            _ => "Unknown condition",
        }
    }

    /// The sketch's `batteryPercent`: 3.0 V is 0 % and 4.2 V is 100 %. A `double` like the sketch's, so
    /// 3.6 V is 50 % and not 49 %.
    pub fn battery_percent(volts: f64) -> i32 {
        if volts >= 4.2 {
            100
        } else if volts <= 3.0 {
            0
        } else {
            (((volts - 3.0) / 1.2) * 100.0) as i32
        }
    }

    /// The sketch's `batteryIcon`. Its full and high icons are the same picture.
    pub fn battery_icon(percent: i32) -> Icon {
        if percent >= 75 {
            Icon::FullBattery
        } else if percent >= 50 {
            Icon::HighBattery
        } else if percent >= 25 {
            Icon::HalfBattery
        } else {
            Icon::LowBattery
        }
    }

    /// The request URL of the sketch, metric units, for a place.
    pub fn url(latitude: f32, longitude: f32) -> String {
        format!(
            "https://api.open-meteo.com/v1/forecast?latitude={latitude:.4}&longitude={longitude:.4}\
             &daily=temperature_2m_max,temperature_2m_min,weather_code\
             &hourly=temperature_2m,precipitation_probability\
             &current=temperature_2m,apparent_temperature,weather_code&timezone=auto"
        )
    }

    /// `hh:mm` out of `2026-11-07T06:11`, or `??:??`, as the sketch's `sunTime`.
    pub fn hour_minute(iso: &str) -> String {
        match iso.find('T') {
            Some(i) if iso.len() > i + 5 => String::from(&iso[i + 1..i + 6]),
            _ => String::from("??:??"),
        }
    }

    /// When the dashboard is made, in local time: the weekday (0 = Sunday) names the days and the hour
    /// is where the hourly window starts.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Now {
        pub weekday: u8,
        pub hour: u8,
    }

    /// What the dashboard shows.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Weather {
        pub current_temp: f32,
        pub feels_like: f32,
        pub code: i32,
        pub daily_min: [f32; DAYS],
        pub daily_max: [f32; DAYS],
        pub daily_codes: [i32; DAYS],
        pub day_names: [&'static str; DAYS],
        pub hourly_temps: [f32; HOURS],
        pub hourly_precip: [f32; HOURS],
        pub hourly_times: [String; HOURS],
    }

    const DAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

    fn number(value: &serde_json::Value) -> Option<f32> {
        value.as_f64().map(|v| v as f32)
    }

    /// Reads the response like the sketch's `fetchWeather`. `None` when the JSON is bad, a field is missing,
    /// an array is too short for the days or the hours the sketch reads, or a value is not a number. (The
    /// sketch would show zeros for those.)
    pub fn parse(body: &[u8], now: Now) -> Option<Weather> {
        let v: serde_json::Value = serde_json::from_slice(body).ok()?;
        let current = &v["current"];
        let daily = &v["daily"];
        let hourly = &v["hourly"];
        let mut weather = Weather {
            current_temp: number(&current["temperature_2m"])?,
            feels_like: number(&current["apparent_temperature"])?,
            code: i32::try_from(current["weather_code"].as_i64()?).ok()?,
            daily_min: [0.0; DAYS],
            daily_max: [0.0; DAYS],
            daily_codes: [0; DAYS],
            day_names: DAY_NAMES[..DAYS.min(7)].try_into().ok()?,
            hourly_temps: [0.0; HOURS],
            hourly_precip: [0.0; HOURS],
            hourly_times: core::array::from_fn(|_| String::new()),
        };
        for i in 0..DAYS {
            weather.daily_min[i] = number(daily["temperature_2m_min"].get(i)?)?;
            weather.daily_max[i] = number(daily["temperature_2m_max"].get(i)?)?;
            weather.daily_codes[i] = i32::try_from(daily["weather_code"].get(i)?.as_i64()?).ok()?;
            weather.day_names[i] = DAY_NAMES[(usize::from(now.weekday) + i) % 7];
        }
        for i in 0..HOURS {
            let src = usize::from(now.hour) + i;
            weather.hourly_temps[i] = number(hourly["temperature_2m"].get(src)?)?;
            weather.hourly_precip[i] = number(hourly["precipitation_probability"].get(src)?)?;
            weather.hourly_times[i] = hour_minute(hourly["time"].get(src)?.as_str()?);
        }
        Some(weather)
    }

    /// `String(x, 1)` of the sketch plus the unit, for the current and the feels-like temperature.
    pub fn temp_1(value: f32) -> String {
        format!("{value:.1}{TEMP_LABEL}")
    }

    /// `String(x, 0)` plus the unit, for the daily highs and lows.
    pub fn temp_0(value: f32) -> String {
        format!("{value:.0}{TEMP_LABEL}")
    }

    /// The precipitation label above a bar.
    pub fn percent(value: f32) -> String {
        format!("{value:.0}%")
    }

    /// The hourly graph's scale: the temperatures' minimum and maximum widened by 2, and the span (1 when it
    /// would be 0), as in the sketch's `drawGraph`.
    pub fn graph_scale(temps: &[f32]) -> (f32, f32, f32) {
        let mut t_min = 1000.0f32;
        let mut t_max = -1000.0f32;
        for &t in temps {
            t_min = t_min.min(t);
            t_max = t_max.max(t);
        }
        let (p_min, p_max) = (t_min - 2.0, t_max + 2.0);
        let range = if p_max - p_min == 0.0 {
            1.0
        } else {
            p_max - p_min
        };
        (p_min, p_max, range)
    }

    /// The days the strip shows, as `(name, icon, max, min)`.
    pub fn strip(weather: &Weather) -> Vec<(&'static str, Icon, String, String)> {
        (0..SHOWN_DAYS)
            .map(|i| {
                (
                    weather.day_names[i],
                    weather_icon(weather.daily_codes[i]),
                    temp_0(weather.daily_max[i]),
                    temp_0(weather.daily_min[i]),
                )
            })
            .collect()
    }
    /// A local date and time.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Clock {
        pub year: u16,
        pub month: u8,
        pub day: u8,
        pub hour: u8,
        pub minute: u8,
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
            weekday: (days + 4).rem_euclid(7) as u8,
        }
    }

    /// The Pacific local time for UTC Unix seconds.
    pub fn local(unix_utc: i64) -> Option<Clock> {
        let zone = timezone_data::parse_posix_tz(TZ).ok()?;
        let (_name, offset, _dst) = zone.lookup(unix_utc);
        Some(clock(unix_utc + i64::from(offset)))
    }

    impl Clock {
        /// The weekday and hour the parsing needs.
        pub fn now(&self) -> Now {
            Now {
                weekday: self.weekday,
                hour: self.hour,
            }
        }
        /// The sketch's `%Y-%m-%d`.
        pub fn date(&self) -> String {
            format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
        }
        /// The sketch's `%H:%M`.
        pub fn time(&self) -> String {
            format!("{:02}:{:02}", self.hour, self.minute)
        }
    }

    /// The sketch's gray levels as 4 bit levels: Black, LightGrey 0xBB, Grey 0x88 and the info panel 0xDD.
    pub const BLACK: u8 = 0;
    pub const LIGHT_GRAY: u8 = 11;
    pub const GRAY: u8 = 8;
    pub const PANEL_GRAY: u8 = 13;
    pub const WHITE: u8 = 15;
    /// The layout of the sketch, 960 x 540.
    pub const SCREEN_W: i32 = 960;
    pub const SCREEN_H: i32 = 540;
    pub const HEADER_H: i32 = 130;
    pub const USER_X: i32 = 680;
    pub const GRAPH_Y: i32 = HEADER_H;
    pub const GRAPH_H: i32 = 230;
    pub const WEEK_Y: i32 = GRAPH_Y + GRAPH_H;
    pub const WEEK_H: i32 = SCREEN_H - WEEK_Y;
    /// Em sizes of the sketch's OpenSans 24B, 18B, 12B, 10B and 8B (line heights 68, 51, 34, 28 and 23
    /// over 1.36).
    pub const PX_24: f32 = 50.0;
    pub const PX_18: f32 = 37.0;
    pub const PX_12: f32 = 25.0;
    pub const PX_10: f32 = 21.0;
    pub const PX_8: f32 = 17.0;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Align {
        Left,
        Right,
        Center,
    }

    /// Width and ink height of a string, like the sketch's `get_text_bounds`: the width is the advance, the
    /// height is from the highest to the lowest ink, with the baseline included.
    pub fn text_bounds(font: &Font, px: f32, text: &str) -> (f32, i32) {
        let mut width = 0.0;
        let (mut top, mut bottom) = (0i32, 0i32);
        for c in text.chars() {
            let m = font.metrics(c, px);
            width += m.advance_width;
            if m.width > 0 && m.height > 0 {
                top = top.max(m.ymin + m.height as i32);
                bottom = bottom.min(m.ymin);
            }
        }
        (width, top - bottom)
    }

    /// The sketch's `drawString`: `y` is the top of the text, `x` the left, right or centre.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_string<D: DrawTarget<Color = Gray4>>(
        target: &mut D,
        font: &Font,
        px: f32,
        x: i32,
        y: i32,
        text: &str,
        align: Align,
        background: u8,
    ) {
        let (width, height) = text_bounds(font, px, text);
        let left = match align {
            Align::Left => x,
            Align::Right => x - width as i32,
            Align::Center => x - width as i32 / 2,
        };
        let baseline = y + height;
        let mut pen = left as f32;
        for ch in text.chars() {
            let (m, coverage) = font.rasterize(ch, px);
            let gx = (pen + 0.5) as i32 + m.xmin;
            let gy = baseline - m.ymin - m.height as i32;
            let w = m.width.max(1);
            let pixels = coverage.iter().enumerate().filter_map(|(i, &c)| {
                if c == 0 {
                    return None;
                }
                // Black text anti-aliased against `background`, like the sketch's `drawStringColor`.
                let level = (u32::from(background) * (255 - u32::from(c)) + 127) / 255;
                Some(Pixel(
                    Point::new(gx + (i % w) as i32, gy + (i / w) as i32),
                    Gray4::new(level as u8),
                ))
            });
            let _ = target.draw_iter(pixels);
            pen += m.advance_width;
        }
    }

    fn line<D: DrawTarget<Color = Gray4>>(t: &mut D, x0: i32, y0: i32, x1: i32, y1: i32, c: u8) {
        let _ = Line::new(Point::new(x0, y0), Point::new(x1, y1))
            .into_styled(PrimitiveStyle::with_stroke(Gray4::new(c), 1))
            .draw(t);
    }

    fn rect<D: DrawTarget<Color = Gray4>>(t: &mut D, x: i32, y: i32, w: i32, h: i32, c: u8) {
        let _ = Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
            .into_styled(PrimitiveStyle::with_stroke(Gray4::new(c), 1))
            .draw(t);
    }

    fn fill<D: DrawTarget<Color = Gray4>>(t: &mut D, x: i32, y: i32, w: i32, h: i32, c: u8) {
        let _ = Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
            .into_styled(PrimitiveStyle::with_fill(Gray4::new(c)))
            .draw(t);
    }

    fn triangle<D: DrawTarget<Color = Gray4>>(t: &mut D, p: [(i32, i32); 3], c: u8) {
        let _ = Triangle::new(
            Point::new(p[0].0, p[0].1),
            Point::new(p[1].0, p[1].1),
            Point::new(p[2].0, p[2].1),
        )
        .into_styled(PrimitiveStyle::with_fill(Gray4::new(c)))
        .draw(t);
    }

    /// The sketch's `drawBitmap`: the ink of a 1-bit BMP, black, each pixel `scale` pixels square.
    pub fn draw_icon<D: DrawTarget<Color = Gray4>>(
        target: &mut D,
        bmp: &[u8],
        x: i32,
        y: i32,
        scale: i32,
    ) {
        let Ok(bmp) = RawBmp::from_slice(bmp) else {
            return;
        };
        for pixel in bmp.pixels() {
            if pixel.color == 0 {
                fill(
                    target,
                    x + pixel.position.x * scale,
                    y + pixel.position.y * scale,
                    scale,
                    scale,
                    BLACK,
                );
            }
        }
    }

    /// Everything the dashboard shows.
    pub struct Dashboard<'a> {
        pub weather: &'a Weather,
        pub city: &'a str,
        pub user: &'a str,
        pub date: &'a str,
        pub time: &'a str,
        pub battery_volts: f64,
    }

    /// The hourly graph, as the sketch's `drawGraph`.
    fn draw_graph<D: DrawTarget<Color = Gray4>>(target: &mut D, font: &Font, w: &Weather) {
        let (graph_x, graph_w, margin_x, margin_y) = (90, 780, 20, 20);
        let graph_y = GRAPH_Y + 35;
        let graph_h = GRAPH_H - 40;
        let chart_left = graph_x + margin_x;
        let chart_bottom = graph_y + graph_h - margin_y;
        let plot_h = graph_h - 2 * margin_y;
        draw_string(
            target,
            font,
            PX_10,
            10,
            GRAPH_Y + 6,
            "Hourly temperature and precipitation",
            Align::Left,
            WHITE,
        );
        let (p_min, p_max, range) = graph_scale(&w.hourly_temps);
        let x_step = (graph_w - 2 * margin_x) as f32 / 5.0;
        line(
            target,
            chart_left,
            graph_y + margin_y,
            chart_left,
            chart_bottom,
            BLACK,
        );
        line(
            target,
            chart_left,
            chart_bottom,
            graph_x + graph_w + 20,
            chart_bottom,
            BLACK,
        );
        let label = |v: f32| format!("{v:.1}{TEMP_LABEL}");
        for (value, y) in [
            (p_min, chart_bottom - 12),
            ((p_min + p_max) / 2.0, chart_bottom - plot_h / 2 - 8),
            (p_max, graph_y + margin_y - 2),
        ] {
            draw_string(
                target,
                font,
                PX_8,
                chart_left - 8,
                y,
                &label(value),
                Align::Right,
                WHITE,
            );
        }
        // Precipitation probability bars, 0 to 100 %.
        for i in 0..HOURS {
            let x = (chart_left as f32 + i as f32 * x_step) as i32;
            let bar_h = ((w.hourly_precip[i] / 100.0) * plot_h as f32) as i32;
            let y = chart_bottom - bar_h;
            if bar_h > 10 {
                fill(target, x + 5, y + 10, 14, bar_h - 10, LIGHT_GRAY);
            }
            draw_string(
                target,
                font,
                PX_8,
                x + 5,
                y - 16,
                &percent(w.hourly_precip[i]),
                Align::Left,
                WHITE,
            );
        }
        // The temperature line, two pixels thick.
        let point = |i: usize| {
            let x = (chart_left as f32 + i as f32 * x_step) as i32;
            let y = (chart_bottom as f32 - ((w.hourly_temps[i] - p_min) / range) * plot_h as f32)
                as i32;
            (x, y)
        };
        for i in 0..HOURS - 1 {
            let ((x1, y1), (x2, y2)) = (point(i), point(i + 1));
            line(target, x1, y1, x2, y2, BLACK);
            line(target, x1, y1 + 1, x2, y2 + 1, BLACK);
        }
        for i in 0..HOURS {
            let x = (chart_left as f32 + i as f32 * x_step + 3.0) as i32;
            draw_string(
                target,
                font,
                PX_8,
                x,
                chart_bottom + 6,
                &w.hourly_times[i],
                Align::Left,
                WHITE,
            );
        }
    }

    /// The whole dashboard, as the sketch's `drawDashboard`.
    pub fn draw_dashboard<D: DrawTarget<Color = Gray4>>(
        target: &mut D,
        font: &Font,
        icons: &dyn Fn(Icon) -> &'static [u8],
        d: &Dashboard<'_>,
    ) {
        let w = d.weather;
        let _ = target.clear(Gray4::new(WHITE));
        // Frames and the light gray info panel.
        rect(target, 0, 0, SCREEN_W, HEADER_H, BLACK);
        fill(
            target,
            USER_X + 1,
            1,
            SCREEN_W - USER_X - 2,
            HEADER_H - 2,
            PANEL_GRAY,
        );
        line(target, USER_X, 0, USER_X, HEADER_H, BLACK);
        rect(target, 0, GRAPH_Y, SCREEN_W, GRAPH_H, BLACK);
        rect(target, 0, WEEK_Y, SCREEN_W, WEEK_H, BLACK);

        // Section 1: city, temperature, condition. The icon is drawn 2x, 96 px.
        draw_icon(target, icons(weather_icon(w.code)), 15, 17, 2);
        draw_string(target, font, PX_24, 140, 12, d.city, Align::Left, WHITE);
        draw_string(
            target,
            font,
            PX_18,
            140,
            62,
            &temp_1(w.current_temp),
            Align::Left,
            WHITE,
        );
        draw_string(
            target,
            font,
            PX_12,
            340,
            70,
            weather_description(w.code),
            Align::Left,
            WHITE,
        );
        let feels = format!("Feels like {}", temp_1(w.feels_like));
        draw_string(target, font, PX_8, 340, 100, &feels, Align::Left, WHITE);

        // Section 2: battery, date, user, on the light gray panel.
        let y = 12;
        if d.battery_volts > 2.5 {
            let percent = battery_percent(d.battery_volts);
            draw_icon(target, icons(battery_icon(percent)), USER_X + 10, 4, 1);
            draw_string(
                target,
                font,
                PX_10,
                USER_X + 65,
                16,
                &format!("{percent}%"),
                Align::Left,
                PANEL_GRAY,
            );
        }
        draw_string(
            target,
            font,
            PX_10,
            USER_X + 10,
            y + 52,
            d.date,
            Align::Left,
            PANEL_GRAY,
        );
        let refresh = format!("Last refresh: {}", d.time);
        draw_string(
            target,
            font,
            PX_10,
            USER_X + 10,
            y + 74,
            &refresh,
            Align::Left,
            PANEL_GRAY,
        );
        draw_string(
            target,
            font,
            PX_10,
            USER_X + 10,
            y + 96,
            d.user,
            Align::Left,
            PANEL_GRAY,
        );

        // Section 3: six days.
        let day_w = SCREEN_W / SHOWN_DAYS as i32;
        for (i, (name, icon, max, min)) in strip(w).into_iter().enumerate() {
            let x = i as i32 * day_w;
            if i > 0 {
                line(target, x, WEEK_Y + 10, x, SCREEN_H - 10, LIGHT_GRAY);
            }
            draw_string(
                target,
                font,
                PX_12,
                x + day_w / 2,
                WEEK_Y + 10,
                name,
                Align::Center,
                WHITE,
            );
            draw_icon(target, icons(icon), x + 14, WEEK_Y + 55, 1);
            let (ax, mut ay) = (x + 85, WEEK_Y + 62);
            triangle(
                target,
                [(ax, ay), (ax - 6, ay + 10), (ax + 6, ay + 10)],
                BLACK,
            );
            draw_string(
                target,
                font,
                PX_10,
                ax + 14,
                ay - 8,
                &max,
                Align::Left,
                WHITE,
            );
            ay += 40;
            triangle(target, [(ax, ay + 10), (ax - 6, ay), (ax + 6, ay)], GRAY);
            draw_string(
                target,
                font,
                PX_10,
                ax + 14,
                ay - 8,
                &min,
                Align::Left,
                WHITE,
            );
        }

        // Section 4: the hourly graph.
        draw_graph(target, font, w);
    }

    /// The sketch's `showMessage`: two lines on a white panel.
    pub fn draw_message<D: DrawTarget<Color = Gray4>>(
        target: &mut D,
        font: &Font,
        line1: &str,
        line2: &str,
    ) {
        let _ = target.clear(Gray4::new(WHITE));
        draw_string(target, font, PX_24, 60, 220, line1, Align::Left, WHITE);
        draw_string(target, font, PX_18, 60, 280, line2, Align::Left, WHITE);
    }

    pub const WIFI_FAILED: (&str, &str) =
        ("WiFi connection failed.", "Check credentials or try again.");
    pub const HTTP_FAILED: (&str, &str) = ("HTTP request failed.", "Check location or try again.");
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
    let p = rust_lilygot5_47_s3_embassy_examples::init!();
    info!("openmeteo: Rust, async");
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
    let Ok(font) = fontdue::Font::from_bytes(OPEN_SANS_BOLD, fontdue::FontSettings::default())
    else {
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

    // The forecast is about 5.4 KB; the heap is the 8 MB PSRAM.
    let mut tls_read = vec![0u8; 16_640];
    let mut tls_write = vec![0u8; 4_096];
    let mut body = vec![0u8; 16 * 1024];
    let state = TcpClientState::<1, 1_024, 1_024>::new();
    let tcp = TcpClient::new(stack, &state);
    let dns = DnsSocket::new(stack);
    let tls = TlsConfig::new(seed, &mut tls_read, &mut tls_write, TlsVerify::None);
    let mut client = HttpClient::new_with_tls(&tcp, &dns, tls);
    let mut rtc = Rtc::new(p.LPWR);
    // The battery divider on GPIO14, as in `battery` and `demo`.
    let mut adc_config = AdcConfig::new();
    let mut adc_pin =
        adc_config.enable_pin_with_cal::<_, AdcCalCurve<_>>(p.GPIO14, Attenuation::_11dB);
    let mut adc = Adc::new(p.ADC2, adc_config);
    let url = ui::url(ui::LATITUDE, ui::LONGITUDE);

    loop {
        let connected = matches!(
            with_timeout(Duration::from_secs(30), controller.connect_async()).await,
            Ok(Ok(_))
        );
        let mut shown = false;
        if !connected {
            error!("wifi connection failed");
            ui::draw_message(&mut fb, &font, ui::WIFI_FAILED.0, ui::WIFI_FAILED.1);
        } else {
            stack.wait_config_up().await;
            if let Some(config) = stack.config_v4() {
                info!("got IP {}", config.address.address());
            }
            let now = ntp_unix(stack).await.and_then(ui::local);
            info!("time known: {}", now.is_some());
            if now.is_none() {
                warn!("no time: the dashboard shows hour 0 and no date");
            }
            // The sketch reads the battery after the time, with the Wi-Fi up.
            let mut pin_mv = 0u16;
            for _ in 0..1000 {
                if let Ok(v) = adc.read_oneshot(&mut adc_pin) {
                    pin_mv = v;
                    break;
                }
            }
            let battery_volts = f64::from(pin_mv) * 2.0 / 1000.0;
            info!("battery pin {} mV", pin_mv);

            let mut weather = None;
            if let Ok(mut request) = client.request(Method::GET, &url).await {
                match request.send(&mut body).await {
                    Ok(response) if response.status.0 == 200 => {
                        match response.body().read_to_end().await {
                            Ok(bytes) => {
                                info!("body {} bytes", bytes.len());
                                let at = now.map(|t| t.now()).unwrap_or(ui::Now {
                                    weekday: 0,
                                    hour: 0,
                                });
                                weather = ui::parse(bytes, at);
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
            match weather {
                Some(weather) => {
                    info!(
                        "fetch ok, {} C now, code {}",
                        weather.current_temp, weather.code
                    );
                    let (date, time) = match now {
                        Some(t) => (t.date(), t.time()),
                        None => (
                            alloc::string::String::from("unknown"),
                            alloc::string::String::from("--:--"),
                        ),
                    };
                    let dashboard = ui::Dashboard {
                        weather: &weather,
                        city: ui::CITY,
                        user: ui::USER,
                        date: &date,
                        time: &time,
                        battery_volts,
                    };
                    ui::draw_dashboard(&mut fb, &font, &icon_bytes, &dashboard);
                    shown = true;
                }
                None => {
                    error!("fetch failed");
                    ui::draw_message(&mut fb, &font, ui::HTTP_FAILED.0, ui::HTTP_FAILED.1);
                }
            }
        }
        if display.refresh(&fb, RefreshMode::Quality16).await.is_err() {
            error!("refresh failed");
        }
        info!("dashboard drawn: {}", shown);

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
