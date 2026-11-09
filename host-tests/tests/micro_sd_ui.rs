//! Tests of the `mod ui` block of `examples/micro_sd.rs`: the card (on a RAM disk), the log file,
//! reading whole files, decoding the PNG and drawing it.

mod support;

use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use embedded_sdmmc::{
    Block, BlockCount, BlockDevice, BlockIdx, Mode, TimeSource, Timestamp, VolumeIdx, VolumeManager,
};
use host_tests::micro_sd::*;
use std::cell::RefCell;
use std::rc::Rc;

const PNG: &[u8] = include_bytes!("../../assets/sd-card/MOCHA.PNG");

/// A RAM disk, shared so the test can put files on it before the store sees it.
#[derive(Clone)]
struct Ram(Rc<RefCell<Vec<u8>>>);

#[derive(Debug)]
struct Fault;

impl core::fmt::Display for Fault {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("fault")
    }
}

impl core::error::Error for Fault {}

impl BlockDevice for Ram {
    type Error = Fault;
    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), Fault> {
        let data = self.0.borrow();
        for (i, block) in blocks.iter_mut().enumerate() {
            let o = (start.0 as usize + i) * 512;
            block.contents.copy_from_slice(&data[o..o + 512]);
        }
        Ok(())
    }
    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), Fault> {
        let mut data = self.0.borrow_mut();
        for (i, block) in blocks.iter().enumerate() {
            let o = (start.0 as usize + i) * 512;
            data[o..o + 512].copy_from_slice(&block.contents);
        }
        Ok(())
    }
    fn num_blocks(&self) -> Result<BlockCount, Fault> {
        Ok(BlockCount((self.0.borrow().len() / 512) as u32))
    }
}

/// A card that has been taken out: every access fails.
struct NoCard;

impl BlockDevice for NoCard {
    type Error = Fault;
    fn read(&self, _: &mut [Block], _: BlockIdx) -> Result<(), Fault> {
        Err(Fault)
    }
    fn write(&self, _: &[Block], _: BlockIdx) -> Result<(), Fault> {
        Err(Fault)
    }
    fn num_blocks(&self) -> Result<BlockCount, Fault> {
        Err(Fault)
    }
}

struct Time;
impl TimeSource for Time {
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

/// A card with these files in the root.
fn card(files: &[(&str, &[u8])]) -> Store<Ram, Time> {
    let ram = Ram(Rc::new(RefCell::new(support::fat32_image())));
    let mgr = VolumeManager::new(ram.clone(), Time);
    let volume = mgr.open_raw_volume(VolumeIdx(0)).unwrap();
    let root = mgr.open_root_dir(volume).unwrap();
    for (name, data) in files {
        let file = mgr
            .open_file_in_dir(root, *name, Mode::ReadWriteCreate)
            .unwrap();
        mgr.write(file, data).unwrap();
        mgr.close_file(file).unwrap();
    }
    mgr.close_dir(root).unwrap();
    mgr.close_volume(volume).unwrap();
    Store::new(ram, Time)
}

/// 960 x 540, one byte per pixel with the gray level 0 to 15, 0xee where nothing was drawn.
struct Canvas {
    pixels: Vec<u8>,
}

impl Canvas {
    fn new() -> Self {
        Self {
            pixels: vec![0xee; 960 * 540],
        }
    }

    fn at(&self, x: i32, y: i32) -> u8 {
        self.pixels[(y * 960 + x) as usize]
    }

    fn drawn(&self) -> usize {
        self.pixels.iter().filter(|&&p| p != 0xee).count()
    }
}

impl OriginDimensions for Canvas {
    fn size(&self) -> Size {
        Size::new(960, 540)
    }
}

impl DrawTarget for Canvas {
    type Color = Gray4;
    type Error = core::convert::Infallible;

    fn draw_iter<I: IntoIterator<Item = Pixel<Gray4>>>(
        &mut self,
        pixels: I,
    ) -> Result<(), Self::Error> {
        for Pixel(p, color) in pixels {
            if (0..960).contains(&p.x) && (0..540).contains(&p.y) {
                self.pixels[(p.y * 960 + p.x) as usize] = color.luma();
            }
        }
        Ok(())
    }
}

#[test]
fn the_card_reports_its_size() {
    let mut store = card(&[]);
    assert_eq!(store.card_bytes(), Some(81_920 * 512));
    let mut gone = Store::new(NoCard, Time);
    assert_eq!(gone.card_bytes(), None);
}

#[test]
fn the_log_gets_a_line_each_time_and_reads_back() {
    let mut store = card(&[]);
    assert_eq!(store.read_log(), Err(StoreError::NotFound));
    store.append_log().unwrap();
    assert_eq!(store.read_log().unwrap(), LOG_LINE);
    store.append_log().unwrap();
    let text = store.read_log().unwrap();
    assert_eq!(text.len(), 2 * LOG_LINE.len());
    assert!(text.starts_with(LOG_LINE) && text.ends_with(LOG_LINE));
}

#[test]
fn a_whole_file_comes_back_byte_for_byte_across_clusters() {
    let data: Vec<u8> = (0..300_000u32).map(|i| (i * 7 + i / 251) as u8).collect();
    let mut store = card(&[("BIG.BIN", &data)]);
    assert_eq!(store.read_file("BIG.BIN").unwrap(), data);
}

#[test]
fn a_missing_file_is_not_found_and_a_missing_card_is_a_mount_error() {
    let mut store = card(&[("OTHER.TXT", b"x")]);
    assert_eq!(store.read_file(PNG_NAME), Err(StoreError::NotFound));
    let mut gone = Store::new(NoCard, Time);
    assert_eq!(gone.read_file(PNG_NAME), Err(StoreError::Mount));
    assert_eq!(gone.append_log(), Err(StoreError::Mount));
}

#[test]
fn the_png_goes_through_the_card_and_decodes() {
    let mut store = card(&[(PNG_NAME, PNG)]);
    let data = store.read_file(PNG_NAME).unwrap();
    assert_eq!(data, PNG);
    let picture = decode_png(&data).expect("the PNG decodes");
    assert_eq!((picture.width, picture.height), (800, 480));
    assert_eq!(picture.pixels.len(), 800 * 480);
    // A photo: most of the 16 levels are used.
    let mut seen = [false; 16];
    for &g in &picture.pixels {
        seen[(g >> 4) as usize] = true;
    }
    assert!(seen.iter().filter(|&&s| s).count() >= 12);
}

#[test]
fn the_picture_is_centred_and_leaves_the_margin_alone() {
    let picture = decode_png(PNG).unwrap();
    assert_eq!(origin(&picture), (80, 30));
    let (x, y) = origin(&picture);
    let mut canvas = Canvas::new();
    draw_gray(&mut canvas, &picture, x, y);
    assert_eq!(canvas.drawn(), 800 * 480);
    assert_eq!(canvas.at(80, 30), picture.pixels[0] >> 4);
    assert_eq!(canvas.at(879, 509), picture.pixels[800 * 480 - 1] >> 4);
    for (px, py) in [(79, 200), (880, 200), (400, 29), (400, 510), (0, 0)] {
        assert_eq!(canvas.at(px, py), 0xee, "({px}, {py})");
    }
}

#[test]
fn a_level_is_the_top_four_bits() {
    assert_eq!(level(0), Gray4::new(0));
    assert_eq!(level(0x7f), Gray4::new(7));
    assert_eq!(level(255), Gray4::WHITE);
}

#[test]
fn bad_pictures_are_errors_not_panics() {
    assert_eq!(decode_png(&[]).err(), Some(ImageError::Png));
    assert_eq!(decode_png(b"not a png at all").err(), Some(ImageError::Png));
    assert!(decode_png(&PNG[..2000]).is_err());
}

#[test]
fn the_status_screen_has_a_title_and_a_detail() {
    let mut canvas = Canvas::new();
    draw_status(&mut canvas, "No microSD card", "Insert a FAT32 card");
    let ink = |y0: i32, y1: i32| {
        (y0..y1)
            .flat_map(|y| (0..960).map(move |x| (x, y)))
            .filter(|&(x, y)| canvas.at(x, y) == 0)
            .count()
    };
    assert!(ink(240, 262) > 100, "title");
    assert!(ink(280, 302) > 100, "detail");
    assert_eq!(ink(0, 230), 0);
    assert_eq!(canvas.at(5, 5), 15);
}
