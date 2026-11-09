//! Tests of the `mod ui` block of `examples/battery.rs`: the divider and the clamp.

use host_tests::battery::*;

#[test]
fn the_divider_doubles_the_pin_voltage() {
    assert_eq!(battery_mv(1800), 3600);
}

#[test]
fn a_full_cell_is_clamped_to_4_2_volts() {
    assert_eq!(battery_mv(2107), MAX_MV);
    assert_eq!(battery_mv(2100), 4200);
}

#[test]
fn zero_stays_zero() {
    assert_eq!(battery_mv(0), 0);
}
