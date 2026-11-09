//! Tests of the weather icons in `assets/icons`: the 11 Lucide icons of the `OpenMeteo-Weather` sketch as
//! 48 x 48, 1 bit BMP files. `tools/icons_from_arduino.py` checks them against the sketch's C arrays when it
//! writes them; these tests check what the examples will load.

use tinybmp::{Bpp, RawBmp};

const ICON_NAMES: [&str; 11] = [
    "clear_sky",
    "partly_cloudy",
    "fog",
    "rain",
    "snow",
    "storm",
    "gray",
    "full_battery",
    "high_battery",
    "half_battery",
    "low_battery",
];

fn load(name: &str) -> Vec<u8> {
    let path = format!("{}/../assets/icons/{name}.bmp", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The ink pixels (palette index 0, black) as a 48 x 48 grid of booleans.
fn ink(name: &str) -> Vec<bool> {
    let data = load(name);
    let bmp = RawBmp::from_slice(&data).expect("the icon is a BMP");
    let mut grid = vec![false; 48 * 48];
    for pixel in bmp.pixels() {
        grid[(pixel.position.y * 48 + pixel.position.x) as usize] = pixel.color == 0;
    }
    grid
}

#[test]
fn every_icon_is_48_by_48_and_one_bit() {
    for name in ICON_NAMES {
        let data = load(name);
        let bmp = RawBmp::from_slice(&data).unwrap_or_else(|_| panic!("{name} is a BMP"));
        let header = bmp.header();
        assert_eq!(
            (header.image_size.width, header.image_size.height),
            (48, 48),
            "{name}"
        );
        assert_eq!(header.bpp, Bpp::Bits1, "{name}");
    }
}

#[test]
fn every_icon_has_ink_and_is_mostly_white() {
    for name in ICON_NAMES {
        let count = ink(name).iter().filter(|&&p| p).count();
        assert!((200..900).contains(&count), "{name} has {count} ink pixels");
    }
}

#[test]
fn the_icons_differ_except_the_two_full_batteries_of_the_sketch() {
    for (i, a) in ICON_NAMES.iter().enumerate() {
        for b in &ICON_NAMES[i + 1..] {
            let same = ink(a) == ink(b);
            let sketch_quirk = (*a, *b) == ("full_battery", "high_battery");
            assert_eq!(same, sketch_quirk, "{a} against {b}");
        }
    }
}

#[test]
fn the_sun_is_where_a_sun_should_be() {
    // The clear sky icon: rays at the top centre and a body in the middle, nothing in the corners.
    let sun = ink("clear_sky");
    let at = |x: usize, y: usize| sun[y * 48 + x];
    assert!((20..28).any(|x| at(x, 2)), "a ray at the top");
    assert!((20..28).any(|x| at(x, 45)), "a ray at the bottom");
    assert!(
        !at(0, 0) && !at(47, 0) && !at(0, 47) && !at(47, 47),
        "empty corners"
    );
    let body = (18..30)
        .flat_map(|y| (14..34).map(move |x| (x, y)))
        .filter(|&(x, y)| at(x, y))
        .count();
    assert!(body > 30, "the circle of the sun");
}
