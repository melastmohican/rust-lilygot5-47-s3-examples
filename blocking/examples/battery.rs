//! # battery: the battery voltage reading of the LilyGo `demo` sketch
//!
//! Reads the battery divider on GPIO14 every 2 s and logs it. The sketch's line is
//! `analogReadMilliVolts(BATT_PIN) * 2.0 / 1000.0` with the panel power switched on first, and the
//! result clamped to 4.2 V. The Arduino sketch is `~/Src/Arduino/LilyGoT5ePaper/LilyGo/demo`. The
//! async twin is `battery` in `async/`. Nothing is drawn: the panel power is switched on for each
//! reading and off again.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example battery
//! ```
//!
//! ## Expected
//!
//! - The log prints `battery: Rust, blocking`, then every 2 s `pin_mv=N volts=V`.
//! - With no battery fitted and USB connected, the pin reads about 2107 mV and the volts show
//!   `4.20` (the reference sketch reads 2107 mV, 4.214 V before the clamp).
#![no_std]
#![no_main]

use defmt::info;
use esp_hal::analog::adc::{Adc, AdcCalCurve, AdcConfig, Attenuation};
use esp_hal::delay::Delay;
use esp_hal::main;
use esp_println as _;
use rust_lilygot5_47_s3_examples::epdpi::EpdParallelBus;

esp_bootloader_esp_idf::esp_app_desc!();

/// The hardware-free part: the divider and the clamp. Same text in the async crate, and tested on
/// the host by `host-tests`.
mod ui {
    /// The battery is read through a divider that halves it.
    pub const DIVIDER: u32 = 2;
    /// A full cell. The sketch shows no more than this.
    pub const MAX_MV: u32 = 4200;

    /// Battery millivolts from the millivolts at the pin, clamped like the sketch.
    pub fn battery_mv(pin_mv: u32) -> u32 {
        (pin_mv * DIVIDER).min(MAX_MV)
    }
}

#[main]
fn main() -> ! {
    let p = rust_lilygot5_47_s3_examples::init!();
    info!("battery: Rust, blocking");
    let mut bus = rust_lilygot5_47_s3_examples::lcd_bus!(p, 16);
    let mut config = AdcConfig::new();
    let mut pin = config.enable_pin_with_cal::<_, AdcCalCurve<_>>(p.GPIO14, Attenuation::_11dB);
    let mut adc = Adc::new(p.ADC2, config);
    let delay = Delay::new();
    loop {
        let _ = bus.set_power(true);
        delay.delay_millis(10);
        let mut pin_mv = 0;
        for _ in 0..1000 {
            if let Ok(v) = adc.read_oneshot(&mut pin) {
                pin_mv = v;
                break;
            }
        }
        let _ = bus.set_power(false);
        let mv = ui::battery_mv(u32::from(pin_mv));
        info!(
            "pin_mv={} volts={}.{:02}",
            pin_mv,
            mv / 1000,
            mv % 1000 / 10
        );
        delay.delay_millis(2000);
    }
}
