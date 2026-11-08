//! Host tests for the hardware-free code of the examples. Empty until the first `mod ui` port.

/// Which `epdpi` mode the tests run in, printed by the smoke test.
pub const BUILD: &str = if cfg!(feature = "blocking") {
    "epdpi blocking"
} else {
    "epdpi async"
};

pub use ::epdpi;
