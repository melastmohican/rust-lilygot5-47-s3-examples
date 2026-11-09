//! Board support for the LilyGo T5 4.7" S3 (V2.4), async flavour of `blocking/`.
//!
//! Copied, not shared, because depending on the blocking crate would switch `epdpi` back to
//! blocking mode.
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
//! (see [`shift_reg`]). The board has no I2C power IC. GPIO46 is wired to the same strobe net as
//! GPIO0 and must stay an input: driving it holds the strobe high and garbles the control lines.
#![no_std]

extern crate alloc;

pub use embassy_time;
pub use esp_alloc;
pub use esp_hal;
pub use esp_rtos;

use embedded_hal_async::delay::DelayNs;
use esp_hal::Async;
use esp_hal::delay::Delay;
use esp_hal::dma::{DmaError, DmaTxBuf};
use esp_hal::gpio::Output;
use esp_hal::lcd_cam::lcd::i8080::{Command, I8080};

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

/// The 74HCT4094 shift register (same protocol as a 74HC595) that carries the panel control lines.
pub mod shift_reg {
    use super::*;

    /// Latch enable: clocks the source drivers' row into the outputs.
    pub const LATCH: u8 = 0x01;
    /// Active high: holds the panel supplies off.
    pub const POWER_DISABLE: u8 = 0x02;
    /// Positive rail enable.
    pub const POS_POWER: u8 = 0x04;
    /// Negative rail enable.
    pub const NEG_POWER: u8 = 0x08;
    /// Gate start pulse (SPV), active low during the frame start.
    pub const STV: u8 = 0x10;
    /// Scan direction.
    pub const SCAN_DIR: u8 = 0x20;
    /// Mode: high while a frame is being driven.
    pub const MODE: u8 = 0x40;
    /// Source output enable.
    pub const OUTPUT_ENABLE: u8 = 0x80;

    /// Burns a few dozen CPU cycles so the 74HC595 sees its setup and pulse widths. Back to
    /// back GPIO writes at full clock speed are too close together for it at 3.3 V.
    #[inline(always)]
    fn settle() {
        for _ in 0..16 {
            core::hint::spin_loop();
        }
    }

    /// Bit-banged writer with the last value cached, so single bits can change.
    pub struct ShiftReg {
        data: Output<'static>,
        clock: Output<'static>,
        store: Output<'static>,
        state: u8,
    }

    impl ShiftReg {
        /// Starts with the supplies disabled, scan direction set and STV high, as FastEPD does.
        pub fn new(data: Output<'static>, clock: Output<'static>, store: Output<'static>) -> Self {
            let mut reg = Self {
                data,
                clock,
                store,
                state: POWER_DISABLE | SCAN_DIR | STV,
            };
            reg.write();
            reg
        }

        /// Shifts the cached byte out, MSB first, and latches it.
        fn write(&mut self) {
            self.store.set_low();
            let mut byte = self.state;
            for _ in 0..8 {
                self.clock.set_low();
                self.data.set_level((byte & 0x80 != 0).into());
                byte <<= 1;
                settle();
                self.clock.set_high();
                settle();
            }
            self.store.set_high();
            settle();
        }

        /// Sets the bits in `mask` and writes.
        pub fn set(&mut self, mask: u8) {
            self.state |= mask;
            self.write();
        }

        /// Clears the bits in `mask` and writes.
        pub fn clear(&mut self, mask: u8) {
            self.state &= !mask;
            self.write();
        }
    }
}

/// Why a bus call failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LilygoBusError {
    /// The DMA transfer failed.
    Dma(DmaError),
    /// A previous failure left the bus without its peripheral.
    Lost,
    /// The scanline does not fit the DMA buffer.
    TooLong,
}

/// Time CKV stays high per row. The vendor uses 50 us at 10 MHz. 22 us at 16 MHz is the PaperDIY
/// timing the `epdpi` waveforms were tuned for, and gave better grays here.
const ROW_US: u64 = 22;

/// The panel bus: data through `LCD_CAM` and DMA, CKV on a GPIO, everything else through the
/// shift register. Frame and row sequence follow the vendor's `ed047tc1.c` (read, not copied):
/// CKV idles low, OE and MODE are on only inside a frame, SPH is high between rows and low while a row is sent (FastEPD, without it 1 px strokes ghost).
pub struct LilygoBus<D> {
    i8080: Option<I8080<'static, Async>>,
    buf: Option<DmaTxBuf>,
    ckv: Output<'static>,
    reg: shift_reg::ShiftReg,
    /// SPH (start pulse horizontal) is not the LCD chip select here: it is driven by hand, low only while a row is sent.
    _sph: Output<'static>,
    delay: D,
}

impl<D> LilygoBus<D> {
    /// Wraps a configured `I8080` (data pins and write strobe attached, no chip select), a DMA
    /// buffer at least one scanline long, CKV, the shift register, the parked SPH pin and a
    /// delay.
    pub fn new(
        i8080: I8080<'static, Async>,
        buf: DmaTxBuf,
        ckv: Output<'static>,
        reg: shift_reg::ShiftReg,
        sph: Output<'static>,
        delay: D,
    ) -> Self {
        Self {
            i8080: Some(i8080),
            buf: Some(buf),
            ckv,
            reg,
            _sph: sph,
            delay,
        }
    }

    /// CKV high for `high` microseconds, then low for `low`.
    async fn pulse_ckv(&mut self, high: u32, low: u32)
    where
        D: DelayNs,
    {
        self.ckv.set_high();
        self.delay.delay_us(high).await;
        self.ckv.set_low();
        self.delay.delay_us(low).await;
    }
}

impl<D: DelayNs> epdpi::EpdParallelBus for LilygoBus<D> {
    type Error = LilygoBusError;

    /// Vendor `epd_poweron` and `epd_poweroff`.
    async fn set_power(&mut self, on: bool) -> Result<(), Self::Error> {
        use shift_reg::*;
        if on {
            self.reg.set(SCAN_DIR);
            self.reg.clear(POWER_DISABLE);
            self.delay.delay_ms(1).await;
            self.reg.set(NEG_POWER);
            self.delay.delay_ms(1).await;
            self.reg.set(POS_POWER);
            self.delay.delay_ms(1).await;
            self.reg.set(STV);
            self.delay.delay_ms(1).await;
        } else {
            self.reg.clear(POS_POWER);
            self.reg.clear(NEG_POWER);
            self.reg.set(POWER_DISABLE);
            self.reg.clear(STV);
        }
        Ok(())
    }

    /// Vendor `epd_start_frame`.
    async fn start_frame(&mut self) -> Result<(), Self::Error> {
        use shift_reg::*;
        self.reg.set(MODE);
        self.pulse_ckv(1, 1).await;
        self.reg.clear(STV);
        self.delay.delay_us(1).await;
        self.ckv.set_high();
        self.reg.set(STV);
        self.delay.delay_us(10).await;
        self.ckv.set_low();
        self.delay.delay_us(10).await;
        self.pulse_ckv(10, 0).await;
        self.reg.set(OUTPUT_ENABLE);
        self.pulse_ckv(1, 1).await;
        Ok(())
    }

    /// Clocks one row out while CKV is high for `ROW_US`, as the vendor's `epd_output_row`.
    async fn write_scanline(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        let (Some(i8080), Some(mut buf)) = (self.i8080.take(), self.buf.take()) else {
            return Err(LilygoBusError::Lost);
        };
        if data.len() > buf.capacity() {
            self.i8080 = Some(i8080);
            self.buf = Some(buf);
            return Err(LilygoBusError::TooLong);
        }
        buf.fill(data);
        self._sph.set_low();
        let high = esp_hal::time::Instant::now();
        match i8080.send(Command::<u8>::None, 0, buf) {
            Ok(transfer) => {
                self.ckv.set_high();
                let (result, i8080, buf) = {
                    let mut transfer = transfer;
                    transfer.wait_for_done().await;
                    transfer.wait()
                };
                self._sph.set_high();
                self.i8080 = Some(i8080);
                self.buf = Some(buf);
                while (esp_hal::time::Instant::now() - high).as_micros() < ROW_US {}
                self.ckv.set_low();
                self.delay.delay_us(5).await;
                result.map_err(LilygoBusError::Dma)
            }
            Err((err, i8080, buf)) => {
                self.i8080 = Some(i8080);
                self.buf = Some(buf);
                Err(LilygoBusError::Dma(err))
            }
        }
    }

    async fn next_row(&mut self) -> Result<(), Self::Error> {
        self.reg.set(shift_reg::LATCH);
        self.reg.clear(shift_reg::LATCH);
        Ok(())
    }

    /// Vendor `epd_end_frame`.
    async fn end_frame(&mut self) -> Result<(), Self::Error> {
        use shift_reg::*;
        self.reg.clear(OUTPUT_ENABLE);
        self.reg.clear(MODE);
        self.pulse_ckv(1, 1).await;
        self.pulse_ckv(1, 1).await;
        Ok(())
    }
}

/// Builds a [`LilygoBus`] on the V2.4 pins, with the bus clock in MHz.
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
        let reg = $crate::shift_reg::ShiftReg::new(
            out($p.GPIO13.into(), Level::Low),
            out($p.GPIO12.into(), Level::Low),
            out($p.GPIO0.into(), Level::Low),
        );
        $crate::LilygoBus::new(
            i8080,
            buf,
            out($p.GPIO38.into(), Level::Low),
            reg,
            out($p.GPIO40.into(), Level::High),
            $crate::embassy_time::Delay,
        )
    }};
}
