//! Host tests for the hardware-free `mod ui` blocks of the examples. `build.rs` copies each block
//! out of its example, so what is tested is the code the board runs. Run with `./run.sh`.

extern crate alloc;

pub use ::epdpi;

/// Which `epdpi` mode the tests run in, printed by the smoke test.
pub const BUILD: &str = if cfg!(feature = "blocking") {
    "epdpi blocking"
} else {
    "epdpi async"
};

/// The G5 decoder, sprite and bouncing of `animated_smiles`.
pub mod animated_smiles {
    include!(concat!(env!("OUT_DIR"), "/animated_smiles_ui.rs"));
}

/// The bitmap, brush and pen path of `macpaint_hello`.
pub mod macpaint_hello {
    include!(concat!(env!("OUT_DIR"), "/macpaint_hello_ui.rs"));
}

/// The text drawing of `antialias_font`.
pub mod antialias_font {
    include!(concat!(env!("OUT_DIR"), "/antialias_font_ui.rs"));
}

/// The JPEG decoding and drawing of `show_jpeg`.
pub mod show_jpeg {
    include!(concat!(env!("OUT_DIR"), "/show_jpeg_ui.rs"));
}

/// The JPEG decoding and placement of `draw_images`.
pub mod draw_images {
    include!(concat!(env!("OUT_DIR"), "/draw_images_ui.rs"));
}

/// The GIF drawing of `gif_player`.
pub mod gif_player {
    include!(concat!(env!("OUT_DIR"), "/gif_player_ui.rs"));
}

/// The battery divider and clamp of `battery`.
pub mod battery {
    include!(concat!(env!("OUT_DIR"), "/battery_ui.rs"));
}

/// The calendar date of Unix seconds in `wifi_ntp`.
pub mod wifi_ntp {
    include!(concat!(env!("OUT_DIR"), "/wifi_ntp_ui.rs"));
}

/// The default time, weekday and year of `rtc`.
pub mod rtc {
    include!(concat!(env!("OUT_DIR"), "/rtc_ui.rs"));
}

/// The pages, button logic and drawing of `button`.
pub mod button {
    include!(concat!(env!("OUT_DIR"), "/button_ui.rs"));
}

/// The card access, PNG decoding and drawing of `micro_sd`.
pub mod micro_sd {
    include!(concat!(env!("OUT_DIR"), "/micro_sd_ui.rs"));
}
