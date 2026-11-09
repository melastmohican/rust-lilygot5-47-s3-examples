//! # micro_sd: port of the FastEPD `microSD` sketch
//!
//! Mounts the microSD card on the board's TF slot over SPI, writes a line to `LILYGO.TXT` and reads
//! it back, then reads `MOCHA.PNG` from the card, decodes it and shows it centred in 16 grays. A
//! missing card, a missing file or a bad picture is shown on the panel. The Arduino sketch is
//! `~/Src/Arduino/LilyGoT5ePaper/FastEPD/microSD`. The blocking twin is `micro_sd` in
//! `blocking/`. Here `epdpi` runs in async mode on the `esp-rtos` Embassy executor. The card access
//! is the same blocking code, the display refreshes are awaited.
//!
//! The card pins come from the V2.4 schematic (the TF slot J4), and they are the sketch's: SCLK
//! GPIO11, MOSI (CMD) GPIO15, MISO (DAT0) GPIO16, CS (DAT3) GPIO42. The card detect line goes to a
//! test point only, not to a GPIO.
//!
//! ## Run
//!
//! Put a FAT32 card in the TF slot with `MOCHA.PNG` in its root (`assets/sd-card/MOCHA.PNG`), then:
//!
//! ```text
//! cargo run --release --example micro_sd
//! ```
//!
//! ## Expected
//!
//! - The log prints `micro_sd: Rust, async`, `card size: N MB`, `wrote LILYGO.TXT`, the contents
//!   of `LILYGO.TXT`, `read N bytes in N ms`, `MOCHA.PNG: 800 x 480, decoded in N ms` and
//!   `displayed MOCHA.PNG`. The file `LILYGO.TXT` on the card has one more line each run.
//! - Landscape, 960 x 540. The picture of Mocha, 800 x 480, centred, in 16 grays.
//! - No card: the panel says `No microSD card`. No `MOCHA.PNG`: `PNG not found`.
//! - Differences from the sketch: the file names are short (8.3) names, because `embedded-sdmmc`
//!   reads only those, the whole PNG is read into memory and decoded with `minipng`, and the
//!   refresh is `Quality16`, which clears first.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::{error, info};
use embassy_time::{Duration, Instant, Timer};
use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::{SdCard, TimeSource, Timestamp};
use esp_hal::delay::Delay;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::Rate;
use esp_println as _;
use rust_lilygot5_47_s3_embassy_examples::epdpi::prelude::*;

esp_bootloader_esp_idf::esp_app_desc!();

/// The hardware-free part: the card, the PNG decoding and the drawing. Same text in the async
/// crate, and tested on the host by `host-tests` against a RAM disk.
mod ui {
    use alloc::vec::Vec;
    use embedded_graphics::{
        mono_font::{MonoTextStyle, ascii::FONT_10X20},
        pixelcolor::Gray4,
        prelude::*,
        text::{Baseline, Text},
    };
    use embedded_sdmmc::{
        BlockDevice, Error, Mode, RawDirectory, TimeSource, VolumeIdx, VolumeManager,
    };
    use minipng::{BitDepth, ColorType};

    /// The picture on the card. The sketch's `/mocha800x480.png` as a short (8.3) name.
    pub const PNG_NAME: &str = "MOCHA.PNG";
    /// The sketch's log file, `/LilyGo_Log.txt`, as a short name.
    pub const LOG_NAME: &str = "LILYGO.TXT";
    pub const LOG_LINE: &[u8] = b"LilyGo T5 microSD test\r\n";
    /// The panel size.
    const PANEL_W: i32 = 960;
    const PANEL_H: i32 = 540;
    /// The largest picture taken, in bytes of decoded data. Keeps a bad header from asking for
    /// all the memory.
    const MAX_PNG_BYTES: usize = 3_000_000;

    /// Why the card could not give a file.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum StoreError {
        /// No card, or no FAT volume on it.
        Mount,
        NotFound,
        NoMemory,
        Io,
    }

    fn io<E: core::error::Error>(error: Error<E>) -> StoreError {
        match error {
            Error::NotFound => StoreError::NotFound,
            Error::DeviceError(_) | Error::NoSuchVolume | Error::FormatError(_) => {
                StoreError::Mount
            }
            _ => StoreError::Io,
        }
    }

    /// The card, with the operations the sketch uses.
    pub struct Store<D: BlockDevice, T: TimeSource> {
        mgr: VolumeManager<D, T>,
    }

    impl<D: BlockDevice, T: TimeSource> Store<D, T> {
        pub fn new(device: D, time: T) -> Self {
            Self {
                mgr: VolumeManager::new(device, time),
            }
        }

        /// Size of the card in bytes, `None` when it does not answer.
        pub fn card_bytes(&mut self) -> Option<u64> {
            self.mgr
                .device(|card| card.num_blocks())
                .ok()
                .map(|blocks| u64::from(blocks.0) * 512)
        }

        /// Runs `f` on the root directory of the first volume and closes both afterwards.
        fn with_root<R>(
            &mut self,
            f: impl FnOnce(&mut VolumeManager<D, T>, RawDirectory) -> Result<R, StoreError>,
        ) -> Result<R, StoreError> {
            let volume = self
                .mgr
                .open_raw_volume(VolumeIdx(0))
                .map_err(|_| StoreError::Mount)?;
            let dir = match self.mgr.open_root_dir(volume) {
                Ok(dir) => dir,
                Err(_) => {
                    let _ = self.mgr.close_volume(volume);
                    return Err(StoreError::Mount);
                }
            };
            let result = f(&mut self.mgr, dir);
            let _ = self.mgr.close_dir(dir);
            let _ = self.mgr.close_volume(volume);
            result
        }

        /// The sketch's log step: appends a line to the log file.
        pub fn append_log(&mut self) -> Result<(), StoreError> {
            self.with_root(|mgr, dir| {
                let file = mgr
                    .open_file_in_dir(dir, LOG_NAME, Mode::ReadWriteCreateOrAppend)
                    .map_err(io)?;
                let written = mgr.write(file, LOG_LINE).map_err(io);
                let closed = mgr.close_file(file).map_err(io);
                written.and(closed)
            })
        }

        /// The whole log file.
        pub fn read_log(&mut self) -> Result<Vec<u8>, StoreError> {
            self.read_file(LOG_NAME)
        }

        /// A file from the root, whole.
        pub fn read_file(&mut self, name: &str) -> Result<Vec<u8>, StoreError> {
            self.with_root(|mgr, dir| {
                let file = mgr
                    .open_file_in_dir(dir, name, Mode::ReadOnly)
                    .map_err(io)?;
                let result = (|| {
                    let length = mgr.file_length(file).map_err(io)? as usize;
                    let mut data = Vec::new();
                    data.try_reserve_exact(length)
                        .map_err(|_| StoreError::NoMemory)?;
                    data.resize(length, 0);
                    let mut have = 0;
                    while have < length {
                        match mgr.read(file, &mut data[have..]).map_err(io)? {
                            0 => return Err(StoreError::Io),
                            n => have += n,
                        }
                    }
                    Ok(data)
                })();
                let _ = mgr.close_file(file);
                result
            })
        }
    }

    /// Why a picture could not be decoded.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum ImageError {
        /// Not a PNG, or damaged.
        Png,
        /// A bit depth other than 8, which this example does not read.
        Unsupported,
        /// Larger than the panel can show or the memory allows.
        TooBig,
    }

    /// A decoded picture: 8 bit gray, `width * height` bytes, row by row.
    pub struct Gray {
        pub width: usize,
        pub height: usize,
        pub pixels: Vec<u8>,
    }

    /// 8 bit luma of a color over white, with the sketch's weights, `77 / 150 / 29`.
    fn luma_over_white(r: u8, g: u8, b: u8, a: u8) -> u8 {
        let luma = (u32::from(r) * 77 + u32::from(g) * 150 + u32::from(b) * 29) >> 8;
        let a = u32::from(a);
        ((luma * a + 255 * (255 - a) + 127) / 255).min(255) as u8
    }

    /// Decodes an 8 bit PNG of any color type to 8 bit gray.
    pub fn decode_png(bytes: &[u8]) -> Result<Gray, ImageError> {
        let header = minipng::decode_png_header(bytes).map_err(|_| ImageError::Png)?;
        if header.required_bytes() > MAX_PNG_BYTES {
            return Err(ImageError::TooBig);
        }
        let mut scratch = Vec::new();
        scratch
            .try_reserve_exact(header.required_bytes())
            .map_err(|_| ImageError::TooBig)?;
        scratch.resize(header.required_bytes(), 0u8);
        let image = minipng::decode_png(bytes, &mut scratch).map_err(|_| ImageError::Png)?;
        if image.bit_depth() != BitDepth::Eight {
            return Err(ImageError::Unsupported);
        }
        let (width, height) = (image.width() as usize, image.height() as usize);
        let color = image.color_type();
        let per_row = image.bytes_per_row();
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(width * height)
            .map_err(|_| ImageError::TooBig)?;
        for row in image.pixels().chunks_exact(per_row).take(height) {
            for x in 0..width {
                pixels.push(match color {
                    ColorType::Gray => row[x],
                    ColorType::GrayAlpha => {
                        luma_over_white(row[x * 2], row[x * 2], row[x * 2], row[x * 2 + 1])
                    }
                    ColorType::Rgb => {
                        luma_over_white(row[x * 3], row[x * 3 + 1], row[x * 3 + 2], 255)
                    }
                    ColorType::Rgba => {
                        luma_over_white(row[x * 4], row[x * 4 + 1], row[x * 4 + 2], row[x * 4 + 3])
                    }
                    ColorType::Indexed => {
                        let [r, g, b, a] = image.palette(row[x]);
                        luma_over_white(r, g, b, a)
                    }
                });
            }
        }
        Ok(Gray {
            width,
            height,
            pixels,
        })
    }

    /// The panel level of an 8 bit gray: its top 4 bits, as the sketch keeps (`gray8 >> 4`).
    pub fn level(gray: u8) -> Gray4 {
        Gray4::new(gray >> 4)
    }

    /// Top left corner that centres `picture` on the panel, as the sketch does.
    pub fn origin(picture: &Gray) -> (i32, i32) {
        (
            ((PANEL_W - picture.width as i32) / 2).max(0),
            ((PANEL_H - picture.height as i32) / 2).max(0),
        )
    }

    /// Draws `picture` with its top left corner at `(x, y)`.
    pub fn draw_gray<D: DrawTarget<Color = Gray4>>(target: &mut D, picture: &Gray, x: i32, y: i32) {
        let pixels = picture.pixels.iter().enumerate().map(|(i, &gray)| {
            let (col, row) = (i % picture.width, i / picture.width);
            Pixel(Point::new(x + col as i32, y + row as i32), level(gray))
        });
        let _ = target.draw_iter(pixels);
    }

    /// The sketch's `showStatus`: a white screen with a title and a line of detail.
    pub fn draw_status<D: DrawTarget<Color = Gray4>>(target: &mut D, title: &str, detail: &str) {
        let _ = target.clear(Gray4::WHITE);
        let style = MonoTextStyle::new(&FONT_10X20, Gray4::BLACK);
        let _ = Text::with_baseline(title, Point::new(60, 240), style, Baseline::Top).draw(target);
        let _ = Text::with_baseline(detail, Point::new(60, 280), style, Baseline::Top).draw(target);
    }
}

/// No clock on the board, so files get a fixed date.
struct FixedTime;

impl TimeSource for FixedTime {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 56,
            zero_indexed_month: 10,
            zero_indexed_day: 8,
            hours: 12,
            minutes: 0,
            seconds: 0,
        }
    }
}

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = rust_lilygot5_47_s3_embassy_examples::init!();
    rust_lilygot5_47_s3_embassy_examples::start_rtos!(p);
    info!("micro_sd: Rust, async");
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

    // The card is on SPI: SCLK 11, MOSI (CMD) 15, MISO (DAT0) 16, CS (DAT3) 42, from the V2.4
    // schematic. The sketch runs it at 10 MHz.
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
    // Infallible for an `Output` chip select.
    let device = ExclusiveDevice::new_no_delay(spi, cs).unwrap_or_else(|e| match e {});
    let mut store = ui::Store::new(SdCard::new(device, Delay::new()), FixedTime);

    macro_rules! status {
        ($title:expr, $detail:expr) => {{
            ui::draw_status(&mut fb, $title, $detail);
            if display.refresh(&fb, RefreshMode::Quality16).await.is_err() {
                error!("refresh failed");
            }
        }};
    }

    let Some(bytes) = store.card_bytes() else {
        error!("no card answered");
        status!("No microSD card", "Insert a FAT32 card");
        rust_lilygot5_47_s3_embassy_examples::halt();
    };
    info!("card size: {} MB", bytes / (1024 * 1024));

    // The sketch's log step: append a line, then read the file back.
    match store.append_log() {
        Ok(()) => info!("wrote {}", ui::LOG_NAME),
        Err(_) => error!("failed to write {}", ui::LOG_NAME),
    }
    match store.read_log() {
        Ok(text) => info!(
            "contents of {}: {} bytes, {}",
            ui::LOG_NAME,
            text.len(),
            core::str::from_utf8(&text).unwrap_or("(not text)")
        ),
        Err(_) => error!("failed to read {}", ui::LOG_NAME),
    }

    // The picture.
    let start = Instant::now();
    let data = match store.read_file(ui::PNG_NAME) {
        Ok(data) => data,
        Err(ui::StoreError::NotFound) => {
            error!("{} not found", ui::PNG_NAME);
            status!("PNG not found", ui::PNG_NAME);
            rust_lilygot5_47_s3_embassy_examples::halt();
        }
        Err(_) => {
            error!("could not read {}", ui::PNG_NAME);
            status!("microSD mount failed", "Check card / FAT32");
            rust_lilygot5_47_s3_embassy_examples::halt();
        }
    };
    info!(
        "read {} bytes in {} ms",
        data.len(),
        start.elapsed().as_millis()
    );
    let start = Instant::now();
    let Ok(picture) = ui::decode_png(&data) else {
        error!("PNG decode failed");
        status!("PNG decode failed", ui::PNG_NAME);
        rust_lilygot5_47_s3_embassy_examples::halt();
    };
    info!(
        "{}: {} x {}, decoded in {} ms",
        ui::PNG_NAME,
        picture.width,
        picture.height,
        start.elapsed().as_millis()
    );
    drop(data);
    let (x, y) = ui::origin(&picture);
    let _ = fb.clear(Gray4::WHITE);
    ui::draw_gray(&mut fb, &picture, x, y);
    if display.refresh(&fb, RefreshMode::Quality16).await.is_err() {
        error!("refresh failed");
    }
    info!("displayed {}", ui::PNG_NAME);
    loop {
        Timer::after(Duration::from_secs(1)).await;
    }
}
