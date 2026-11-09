//! # macpaint_hello: port of the FastEPD `macpaint_hello` sketch
//!
//! A 640 x 480 MacPaint window is drawn once, then a brush follows a recorded pen path that
//! writes "hello" in 361 steps. Each step adds a black dot and refreshes only what changed. It
//! runs once and stops, press reset to replay. The Arduino sketch is
//! `~/Src/Arduino/LilyGoT5ePaper/FastEPD/macpaint_hello`. The async twin is `macpaint_hello` in
//! `async/`.
//!
//! The picture is the sketch's `image.h` as the BMP `assets/macpaint_640x480.bmp`, which you can
//! open in any viewer. The pen path is the sketch's `positions.h`, in `mod ui`.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example macpaint_hello
//! ```
//!
//! ## Expected
//!
//! - The log prints `macpaint_hello: Rust, blocking`, then `N / 361 points` every 25 points and
//!   `done in N ms`.
//! - Landscape, 960 x 540, white. The panel clears, then the MacPaint window appears (menu bar,
//!   tool palette, an empty drawing area) at x 160, y 30. Then a dot trail writes "hel" and "lo"
//!   in the drawing area, one small update at a time. It stops with the writing visible.
//! - Differences from the sketch: the first draw is a clear and a black and white refresh where
//!   the sketch runs `fullUpdate(CLEAR_SLOW)`, the dots are `FastMono` updates, and the dot is a
//!   9 px disc from `embedded-graphics`, where FastEPD's `fillCircle` draws its own.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::{error, info};
use esp_hal::main;
use esp_hal::time::Instant;
use esp_println as _;
use rust_lilygot5_47_s3_examples::epdpi::prelude::*;

esp_bootloader_esp_idf::esp_app_desc!();

/// The sketch's `image.h` as a 1 bit BMP, see `assets/NOTICE.md`.
const BITMAP: &[u8] = include_bytes!("../../assets/macpaint_640x480.bmp");

/// The hardware-free part: the bitmap, the brush and the pen path. Same text in the async crate,
/// and tested on the host by `host-tests`.
mod ui {
    use embedded_graphics::{
        pixelcolor::Gray4,
        prelude::*,
        primitives::{Circle, PrimitiveStyle},
    };
    use tinybmp::RawBmp;

    /// Bitmap size in pixels.
    #[allow(dead_code)] // for the host tests
    pub const IMG_W: i32 = 640;
    #[allow(dead_code)] // for the host tests
    pub const IMG_H: i32 = 480;
    /// The sketch centres an 800 x 480 frame on the 960 x 540 panel.
    pub const FRAME_X: i32 = 80;
    pub const FRAME_Y: i32 = 30;
    /// The 640 x 480 bitmap sits 80 px into the frame.
    pub const IMG_X: i32 = FRAME_X + 80;
    pub const IMG_Y: i32 = FRAME_Y;
    /// The sketch's `BRUSH_RADIUS`.
    pub const BRUSH_RADIUS: u32 = 4;

    /// The pen path of the sketch's `positions.h`, 361 points in its 800 x 480 frame.
    #[rustfmt::skip]
    pub const POSITIONS: [(i16, i16); 361] = [
        (263, 288), (267, 285), (272, 284), (274, 281), (278, 278), (281, 275), (285, 273), (289, 270),
        (292, 267), (294, 265), (299, 262), (301, 259), (304, 257), (308, 253), (311, 250), (313, 247),
        (316, 244), (319, 240), (321, 238), (324, 234), (327, 231), (330, 227), (332, 223), (334, 220),
        (335, 216), (336, 212), (338, 208), (339, 203), (339, 199), (338, 194), (336, 190), (335, 186),
        (332, 184), (328, 181), (324, 181), (320, 184), (317, 186), (316, 190), (315, 194), (313, 199),
        (313, 203), (312, 208), (311, 212), (311, 216), (311, 220), (309, 224), (309, 228), (308, 232),
        (308, 238), (308, 242), (307, 246), (307, 250), (305, 254), (305, 258), (305, 262), (304, 267),
        (304, 271), (303, 275), (303, 280), (301, 284), (300, 288), (300, 292), (300, 289), (301, 285),
        (301, 281), (303, 275), (304, 271), (304, 267), (305, 263), (307, 259), (308, 255), (309, 251),
        (312, 247), (315, 244), (317, 242), (321, 240), (326, 240), (331, 240), (335, 240), (338, 243),
        (342, 246), (343, 250), (344, 254), (344, 258), (344, 262), (343, 266), (343, 271), (343, 275),
        (343, 280), (343, 284), (344, 288), (348, 290), (353, 292), (357, 292), (361, 292), (365, 290),
        (369, 289), (373, 288), (377, 286), (381, 285), (385, 284), (389, 281), (392, 278), (396, 275),
        (398, 273), (401, 270), (404, 267), (407, 263), (408, 259), (409, 255), (409, 251), (408, 247),
        (407, 243), (404, 239), (401, 236), (397, 236), (392, 236), (389, 238), (385, 242), (382, 244),
        (381, 248), (378, 253), (378, 257), (377, 261), (377, 265), (377, 269), (378, 273), (381, 277),
        (382, 281), (386, 284), (389, 286), (393, 289), (397, 290), (401, 292), (405, 292), (409, 292),
        (413, 292), (417, 292), (421, 290), (425, 288), (429, 286), (434, 284), (436, 281), (439, 278),
        (443, 275), (446, 271), (448, 269), (451, 265), (454, 262), (455, 258), (458, 254), (461, 251),
        (462, 247), (465, 243), (466, 239), (467, 235), (470, 231), (471, 227), (473, 223), (474, 219),
        (475, 215), (475, 211), (477, 207), (477, 203), (477, 199), (477, 194), (477, 189), (475, 186),
        (473, 182), (469, 181), (465, 182), (461, 185), (458, 188), (455, 192), (452, 196), (451, 199),
        (450, 203), (448, 207), (447, 212), (447, 216), (446, 220), (446, 224), (444, 228), (444, 232),
        (444, 236), (444, 242), (444, 246), (444, 250), (444, 254), (444, 258), (444, 262), (444, 267),
        (446, 271), (446, 275), (447, 280), (450, 284), (452, 286), (455, 289), (459, 292), (463, 293),
        (467, 293), (471, 293), (475, 292), (479, 290), (483, 288), (488, 286), (490, 284), (494, 281),
        (497, 277), (500, 274), (502, 270), (505, 267), (508, 263), (509, 261), (512, 257), (515, 253),
        (516, 248), (519, 246), (520, 242), (523, 238), (524, 234), (525, 230), (527, 226), (528, 221),
        (529, 217), (531, 213), (532, 209), (532, 205), (533, 201), (533, 197), (533, 192), (532, 188),
        (529, 185), (525, 184), (521, 184), (519, 186), (516, 190), (513, 193), (510, 197), (509, 201),
        (508, 205), (505, 209), (504, 213), (504, 217), (502, 221), (501, 226), (501, 230), (500, 234),
        (500, 239), (500, 243), (500, 247), (500, 251), (500, 255), (501, 259), (501, 265), (502, 269),
        (504, 273), (505, 277), (506, 281), (509, 284), (512, 288), (515, 290), (519, 292), (523, 292),
        (528, 292), (532, 292), (536, 289), (539, 288), (543, 285), (546, 281), (548, 278), (551, 274),
        (552, 270), (554, 267), (556, 263), (558, 259), (559, 255), (560, 251), (563, 247), (566, 244),
        (570, 242), (573, 239), (577, 238), (581, 239), (585, 240), (589, 242), (593, 244), (596, 248),
        (597, 253), (598, 257), (600, 261), (600, 265), (600, 269), (598, 273), (598, 277), (596, 281),
        (594, 285), (591, 288), (587, 290), (583, 293), (579, 293), (575, 293), (571, 293), (567, 290),
        (564, 288), (562, 284), (560, 281), (559, 277), (558, 271), (558, 267), (558, 263), (559, 259),
        (560, 255), (562, 251), (564, 247), (567, 244), (570, 242), (574, 239), (578, 238), (582, 238),
        (586, 239), (591, 240), (596, 240), (600, 242), (604, 242), (608, 242), (612, 242), (616, 239),
        (620, 238), (623, 235), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232),
        (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232),
        (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232),
        (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232), (627, 232),
        (627, 232),
    ];

    /// Clears `target` to white and draws the black pixels of `bmp` at [`IMG_X`], [`IMG_Y`]. Black
    /// is palette entry 0, which `assets/macpaint_640x480.bmp` uses for ink.
    pub fn draw_background<D: DrawTarget<Color = Gray4>>(target: &mut D, bmp: &RawBmp<'_>) {
        let _ = target.clear(Gray4::WHITE);
        let ink = bmp.pixels().filter(|p| p.color == 0).map(|p| {
            Pixel(
                Point::new(IMG_X + p.position.x, IMG_Y + p.position.y),
                Gray4::BLACK,
            )
        });
        let _ = target.draw_iter(ink);
    }

    /// Draws the brush, a black disc of `BRUSH_RADIUS`, at a point of the pen path.
    pub fn draw_brush<D: DrawTarget<Color = Gray4>>(target: &mut D, x: i16, y: i16) {
        let center = Point::new(FRAME_X + i32::from(x), FRAME_Y + i32::from(y));
        let _ = Circle::with_center(center, 2 * BRUSH_RADIUS + 1)
            .into_styled(PrimitiveStyle::with_fill(Gray4::BLACK))
            .draw(target);
    }
}

#[main]
fn main() -> ! {
    let p = rust_lilygot5_47_s3_examples::init!();
    info!("macpaint_hello: Rust, blocking");
    let bus = rust_lilygot5_47_s3_examples::lcd_bus!(p, 16);
    let mut prev = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut line = vec![0u8; Ed047tc1::SCANLINE_BYTES];
    let mut fb_mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let (Ok(mut display), Ok(mut fb)) = (
        EpdDisplayBuilder::<Ed047tc1>::new().build(bus, &mut prev, &mut line),
        EpdFrameBuffer::<Ed047tc1>::new(&mut fb_mem),
    ) else {
        error!("buffer allocation too small");
        rust_lilygot5_47_s3_examples::halt();
    };

    // `drawBackground()`: the panel clears, then the whole picture is shown.
    let start = Instant::now();
    let Ok(bmp) = tinybmp::RawBmp::from_slice(BITMAP) else {
        error!("the bitmap does not decode");
        rust_lilygot5_47_s3_examples::halt();
    };
    ui::draw_background(&mut fb, &bmp);
    if display.clear().is_err() || display.refresh(&fb, RefreshMode::FastMono).is_err() {
        error!("background failed");
    }
    fb.clear_dirty();
    info!("background drawn in {} ms", start.elapsed().as_millis());

    // The pen path: one dot and one small update per point.
    let total = ui::POSITIONS.len();
    let start = Instant::now();
    for (index, &(x, y)) in ui::POSITIONS.iter().enumerate() {
        ui::draw_brush(&mut fb, x, y);
        if display
            .refresh_dirty(&mut fb, RefreshMode::FastMono)
            .is_err()
        {
            error!("refresh failed");
        }
        if (index + 1) % 25 == 0 {
            info!("{} / {} points", index + 1, total);
        }
    }
    info!("done in {} ms", start.elapsed().as_millis());
    rust_lilygot5_47_s3_examples::halt();
}
