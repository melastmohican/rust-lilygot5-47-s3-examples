//! # rtc: the PCF8563 clock of the LilyGo `demo` sketch
//!
//! Probes the real-time clock at I2C address 0x51 (SDA GPIO18, SCL GPIO17), reads the time, sets it
//! when the clock has lost it, and logs it every 2 s. The chip is driven by the `pcf8563` crate. The
//! sketch uses SensorLib's `SensorPCF8563`; the Arduino sketch is `~/Src/Arduino/LilyGoT5ePaper/LilyGo/demo`.
//! The async twin is `rtc` in `async/`. Nothing is drawn.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example rtc
//! ```
//!
//! ## Expected
//!
//! - The log prints `rtc: Rust, blocking`, then `RTC found`, or `RTC not found` and nothing more.
//! - Every 2 s a line like `2026-11-08 12:00:06`, advancing by 2 s. A first run on a clock that lost
//!   its time starts at `2026-11-08 12:00:00`. After a chip reset with USB power kept, the time
//!   continues from where it was.
#![no_std]
#![no_main]

use defmt::{info, warn};
use esp_hal::delay::Delay;
use esp_hal::i2c::master::{Config, I2c};
use esp_hal::main;
use esp_hal::time::Rate;
use esp_println as _;
use pcf8563::{DateTime, PCF8563};

esp_bootloader_esp_idf::esp_app_desc!();

/// The hardware-free part: the default time, the weekday and the year. Same text in the async crate, and tested on
/// the host by `host-tests`.
mod ui {
    /// First year the two-digit year of the chip stands for.
    pub const YEAR_BASE: u16 = 2000;

    /// The time the clock is set to when it has none: 2026-11-08 12:00:00.
    pub const DEFAULT: (u8, u8, u8, u8, u8, u8) = (26, 11, 8, 12, 0, 0);

    /// Day of the week, 0 = Sunday, for a year 2000 to 2099 (`year` is the two-digit year).
    pub fn weekday(year: u8, month: u8, day: u8) -> u8 {
        let offset = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let mut y = i32::from(YEAR_BASE) + i32::from(year);
        if month < 3 {
            y -= 1;
        }
        let index = (month.clamp(1, 12) - 1) as usize;
        ((y + y / 4 - y / 100 + y / 400 + offset[index] + i32::from(day)) % 7) as u8
    }

    /// The full year for the chip's two-digit year.
    pub fn full_year(year: u8) -> u16 {
        YEAR_BASE + u16::from(year)
    }
}

#[main]
fn main() -> ! {
    let p = rust_lilygot5_47_s3_examples::init!();
    info!("rtc: Rust, blocking");
    let Ok(i2c) = I2c::new(
        p.I2C0,
        Config::default().with_frequency(Rate::from_khz(100)),
    ) else {
        rust_lilygot5_47_s3_examples::halt();
    };
    let i2c = i2c.with_sda(p.GPIO18).with_scl(p.GPIO17);
    let mut rtc = PCF8563::new(i2c);
    let delay = Delay::new();

    // The chip answers a read of its lost-time flag, or it is not fitted.
    let Ok(lost) = rtc.get_voltage_low_flag() else {
        warn!("RTC not found");
        rust_lilygot5_47_s3_examples::halt();
    };
    info!("RTC found");
    if lost {
        let (year, month, day, hours, minutes, seconds) = ui::DEFAULT;
        let now = DateTime {
            year,
            month,
            weekday: ui::weekday(year, month, day),
            day,
            hours,
            minutes,
            seconds,
        };
        let _ = rtc.rtc_init();
        let _ = rtc.set_datetime(&now);
        info!("RTC had no time, set 2026-11-08 12:00:00");
    }
    loop {
        match rtc.get_datetime() {
            Ok(t) => info!(
                "{}-{:02}-{:02} {:02}:{:02}:{:02}",
                ui::full_year(t.year),
                t.month,
                t.day,
                t.hours,
                t.minutes,
                t.seconds
            ),
            Err(_) => warn!("RTC read failed"),
        }
        delay.delay_millis(2000);
    }
}
