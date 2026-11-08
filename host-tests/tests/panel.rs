use host_tests::epdpi::prelude::*;

/// The ED047TC1 is 960 by 540 at 4 bits per pixel, which the DMA buffer sizing relies on.
#[test]
fn ed047tc1_buffer_sizes() {
    assert_eq!(Ed047tc1::FB_BYTES, 960 * 540 / 2);
    assert!(Ed047tc1::SCANLINE_BYTES <= 256, "the lcd_bus! DMA buffer holds 256 bytes");
}
