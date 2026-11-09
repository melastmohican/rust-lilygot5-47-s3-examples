# rust-lilygot5-47-s3-examples

Examples for `epdpi` on the LilyGo T5 4.7" S3 V2.4. Sibling of `../rust-m5paperdiy-examples`,
follow its conventions. The Arduino sketches to port are in `~/Src/Arduino/LilyGoT5ePaper/`.

## Layout

- `blocking/`, `async/`, `host-tests/`: three separate Cargo projects. Never one workspace, it
  would unify the blocking and async modes of `epdpi`.
- Board support (`init!`, `lcd_bus!`, `LilygoBus`, `shift_reg`) is copied between
  `blocking/src/lib.rs` and `async/src/lib.rs`, because depending on the blocking crate would
  switch `epdpi` to blocking. Keep the two in step.
- Every port has a blocking and an async twin with the same file name as the Arduino sketch.
- Each example states its build on screen (`Rust: epdpi blocking|async`) and has a `//!` header
  with `## Run` and `## Expected`.
- READMEs describe what exists now. Planned work lives in `.agents/notes/backlog.md`.
- Docs read like a person wrote them: no em dashes, no "it's not X, it's Y".

## Hardware (V2.4, Screen-4.7-S3 2024-12-03)

Take pinouts from the V2.4 schematic, not the LilyGo wiki or the T5S3 Pro notes (those have a
TPS65185 and different pins). Pin table in `README.md`. Also on the board: I2C SDA 18 / SCL 17
(GT911 touch 0x14 or 0x5D, INT 47; PCF8563 at 0x51), SD SPI SCLK 11 / MOSI 15 / MISO 16 / CS 42,
battery ADC GPIO 14 (2:1 divider, panel power on first), user button GPIO 21. GPIO46 is also the
strobe net of the shift register, shared with GPIO0 (schematic net STR_IO0): never drive it. It
stays an input, as in the vendor library. Holding it high breaks every control line.

Shift register (74HCT4094, outputs QP0..QP7) bits: latch 0x01 (EP_LE), power_disable 0x02
(SMPS_CTRL), pos_power 0x04 (POS_CTRL), neg_power 0x08 (NEG_CTRL), stv 0x10 (EP_STV), scan_dir
0x20 (PWR_EN), mode 0x40 (EP_MODE), output_enable 0x80 (EP_OE). The supply bits may not do anything
on this revision (their series resistors are not fitted); the vendor sequence sets them anyway.

The vendor sequence is the reference that works on the board (`LilyGo-EPD47`, `ed047tc1.c`): CKV
idles low, OE and MODE are on only inside a frame, 248 clocks per row. SPH is driven by hand as in
FastEPD: high between rows, low only while a row is sent. Holding it low made 1 px strokes ghost
(a copy shifted by about a pixel, pale digits). The write strobe idles high (LCD_CK_IDLE_EDGE). The vendor
runs 10 MHz and 50 us of CKV high per row. We use 16 MHz and 22 us (`ROW_US`), the PaperDIY timing
the `epdpi` waveforms were tuned for: it gave better grays than 10 MHz / 50 us (8 Nov 2026).

Waveforms are the `epdpi` ones. M5 refresh modes belong to the PaperDIY, start with `Quality16`.
Hardware numbers (edge timing, delays) come from LilyGo-EPD47, FastEPD and epdiy. Re-derive, do
not copy (epdiy is LGPL-3.0).

## Checks (match CI)

```bash
cd blocking && cargo build --lib --examples --release && cargo fmt --all -- --check \
  && cargo clippy --lib --examples -- -D warnings
cd async    && cargo build --lib --examples --release && cargo fmt --all -- --check \
  && cargo clippy --lib --examples -- -D warnings
host-tests/run.sh && host-tests/run.sh --no-default-features
```

## Trying an unreleased `epdpi` here

Create a git-ignored `.cargo/config.toml` in the repo root:

```toml
[patch.crates-io]
epdpi = { path = "../epdpi/epdpi" }
epdpi-esp = { path = "../epdpi/epdpi-esp" }
```

For host tests use `EPDPI_PATH=../epdpi host-tests/run.sh`. Delete the override before
committing and check `git diff --stat -- '*/Cargo.lock'`.
