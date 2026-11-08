# rust-lilygot5-47-s3-examples

Hardware examples for the [`epdpi`](https://github.com/melastmohican/epdpi) e-paper driver on the
LilyGo T5 4.7" S3 (V2.4): ESP32-S3, 16 MB flash, OPI PSRAM, ED047TC1 panel, 960 by 540, 16 grays.

| Folder | What |
|---|---|
| `blocking/` | Examples on the blocking API. |
| `async/` | The same examples on the Embassy executor. |
| `host-tests/` | Tests that run on the host, in both modes. |

Each of `blocking/`, `async/` and `host-tests/` is a separate Cargo project on purpose. `epdpi` has
mutually exclusive blocking and async modes, and one Cargo workspace would unify them.

## Examples

| Example | What it shows |
|---|---|
| `display_demo` | Borders, text and a gray bar, one `Quality16` refresh. |

## Run

Needs the `esp` Rust toolchain (`espup`) and `espflash`. The drivers are the published crates
`epdpi` and `epdpi-esp`.

```text
cd blocking && cargo run --release --example display_demo
cd async && cargo run --release --example display_demo
```

## Tests

```text
host-tests/run.sh
host-tests/run.sh --no-default-features
```

## Board reference

Pins come from the V2.4 schematic and the LilyGo-EPD47 library.

| Signal | GPIO | Signal | GPIO |
|---|---|---|---|
| D0..D7 | 8, 1, 2, 3, 4, 5, 6, 7 | CKV | 38 |
| CL (write strobe) | 41 | SPH (chip select) | 40 |
| 74HC595 data / clock / store | 13 / 12 / 0 | Unused DC, held high | 46 |

STV, latch, output enable, mode, scan direction and both power rails go through the 74HC595.
There is no I2C power IC on this revision. The board bus is `LilygoBus` in `src/lib.rs` of each
crate.

## License

GPL-3.0-only.
