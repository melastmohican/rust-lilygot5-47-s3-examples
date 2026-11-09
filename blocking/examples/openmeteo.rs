//! # openmeteo: port of the LilyGo `OpenMeteo-Weather` sketch (work in progress)
//!
//! This file only holds the weather logic so far (`mod ui`, tested on the host by `host-tests`); the
//! network, the drawing and the screen come with `port-openmeteo-weather`. The Arduino sketch is
//! `~/Src/Arduino/LilyGoT5ePaper/OpenMeteo-Weather`. The async twin is `openmeteo` in `async/`.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example openmeteo
//! ```
//!
//! ## Expected
//!
//! - The log prints `openmeteo: Rust, blocking` and stops. Nothing is drawn yet.
#![no_std]
#![no_main]

extern crate alloc;

use defmt::info;
use esp_println as _;

esp_bootloader_esp_idf::esp_app_desc!();

/// The hardware-free part: the request, the parsing of the response and the mappings of the sketch. Same text
/// in the other crate, and tested on the host by `host-tests`.
#[allow(dead_code)]
mod ui {
    use alloc::{format, string::String, vec::Vec};

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
}

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = rust_lilygot5_47_s3_examples::init!();
    let timg0 = esp_hal::timer::timg::TimerGroup::new(p.TIMG0);
    let sw_interrupt = esp_hal::interrupt::software::SoftwareInterruptControl::new(p.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);
    info!("openmeteo: Rust, blocking");
    rust_lilygot5_47_s3_examples::halt();
}
