//! Board support for the LilyGo T5 4.7" S3 (V2.4), async flavour of `blocking/`.
//!
//! Copied, not shared, because depending on the blocking crate would switch `epdpi` back to
//! blocking mode. The bus itself lives in `epdpi-esp`.
//!
//! Pin map from the LilyGo-EPD47 `ed047tc1.h` (ESP32-S3 block) and the V2.4 schematic, power and
//! row sequence from FastEPD's LilyGo callbacks (both read, not copied).
//!
//! | Signal | GPIO | Signal | GPIO |
//! |---|---|---|---|
//! | D0..D7 | 8, 1, 2, 3, 4, 5, 6, 7 | CKV | 38 |
//! | CL (write strobe) | 41 | SPH (chip select) | 40 |
//! | Shift register data / clock / strobe | 13 / 12 / 0 | Strobe, tied to GPIO0 | 46 (never driven) |
//!
//! STV, latch, output enable, mode, scan direction and both power rails sit behind the 74HC595
//! (the bus is `epdpi_esp::shift_reg_bus::ShiftRegBus`). The board has no I2C power IC. GPIO46 is wired to the same strobe net as
//! GPIO0 and must stay an input: driving it holds the strobe high and garbles the control lines.
#![no_std]

extern crate alloc;

pub use embassy_time;
pub use esp_alloc;
pub use esp_hal;
pub use esp_rtos;

use esp_hal::delay::Delay;

pub use epdpi;
pub use epdpi_esp;

/// Parks the CPU after an unrecoverable setup error, so the log stays readable.
pub fn halt() -> ! {
    let delay = Delay::new();
    loop {
        delay.delay_millis(1000);
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("{}", defmt::Display2Format(info));
    loop {}
}

/// Initializes the chip with PSRAM mapped into the heap and returns the peripherals.
///
/// The two 4 bits per pixel buffers are 259,200 bytes each, so PSRAM is required.
#[macro_export]
macro_rules! init {
    () => {{
        use $crate::esp_alloc::{HEAP, HeapRegion, MemoryCapability};
        use $crate::esp_hal::psram::{Psram, PsramConfig};
        let config = $crate::esp_hal::Config::default()
            .with_cpu_clock($crate::esp_hal::clock::CpuClock::max());
        let peripherals = $crate::esp_hal::init(config);
        let psram = Psram::new(
            unsafe { peripherals.PSRAM.clone_unchecked() },
            PsramConfig::default(),
        );
        let (start, size) = psram.raw_parts();
        unsafe {
            HEAP.add_region(HeapRegion::new(
                start,
                size,
                MemoryCapability::External.into(),
            ))
        };
        peripherals
    }};
}

/// Starts the `esp-rtos` Embassy scheduler on TIMG0 and a software interrupt.
#[macro_export]
macro_rules! start_rtos {
    ($p:expr) => {{
        let timg0 = $crate::esp_hal::timer::timg::TimerGroup::new($p.TIMG0);
        let sw_interrupt =
            $crate::esp_hal::interrupt::software::SoftwareInterruptControl::new($p.SW_INTERRUPT);
        $crate::esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);
    }};
}

/// Builds an `epdpi_esp::shift_reg_bus::ShiftRegBus` on the V2.4 pins, with the bus clock in MHz.
///
/// Uses `DMA_CH0` and `LCD_CAM`. CL is GPIO41, driven by the LCD peripheral. SPH (GPIO40) is
/// parked low.
#[macro_export]
macro_rules! lcd_bus {
    ($p:expr, $mhz:expr) => {{
        use $crate::esp_hal::dma_tx_buffer;
        use $crate::esp_hal::gpio::{Level, Output, OutputConfig};
        use $crate::esp_hal::lcd_cam::LcdCam;
        use $crate::esp_hal::lcd_cam::lcd::i8080::{Config, I8080};
        use $crate::esp_hal::lcd_cam::lcd::{ClockMode, Phase, Polarity};
        use $crate::esp_hal::time::Rate;
        let out = |pin: $crate::esp_hal::gpio::AnyPin<'static>, level| {
            Output::new(pin, level, OutputConfig::default())
        };
        let lcd_cam = LcdCam::new($p.LCD_CAM).into_async();
        // The write strobe idles high, as in the vendor library (LCD_CK_IDLE_EDGE = 1). With the
        // esp-hal default (idle low) the data changes on the sampling edge, which breaks every
        // pixel that differs from its neighbours.
        let config = Config::default()
            .with_frequency(Rate::from_mhz($mhz))
            .with_clock_mode(ClockMode {
                polarity: Polarity::IdleHigh,
                phase: Phase::ShiftLow,
            });
        let Ok(i8080) = I8080::new(lcd_cam.lcd, $p.DMA_CH0, config) else {
            panic!("LCD_CAM config rejected");
        };
        let i8080 = i8080
            .with_wrx($p.GPIO41)
            .with_data0($p.GPIO8)
            .with_data1($p.GPIO1)
            .with_data2($p.GPIO2)
            .with_data3($p.GPIO3)
            .with_data4($p.GPIO4)
            .with_data5($p.GPIO5)
            .with_data6($p.GPIO6)
            .with_data7($p.GPIO7);
        let Ok(buf) = dma_tx_buffer!(256) else {
            panic!("DMA buffer allocation failed");
        };
        let reg = $crate::epdpi_esp::shift_reg_bus::register(
            out($p.GPIO13.into(), Level::Low),
            out($p.GPIO12.into(), Level::Low),
            out($p.GPIO0.into(), Level::Low),
        );
        $crate::epdpi_esp::shift_reg_bus::ShiftRegBus::new(
            i8080,
            buf,
            out($p.GPIO38.into(), Level::Low),
            reg,
            out($p.GPIO40.into(), Level::High),
            $crate::embassy_time::Delay,
        )
    }};
}
