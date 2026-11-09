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
