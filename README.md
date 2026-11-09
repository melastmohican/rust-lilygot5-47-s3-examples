# rust-lilygot5-47-s3-examples

Hardware examples for the [`epdpi`](https://github.com/melastmohican/epdpi) e-paper driver on the
LilyGo T5 4.7" S3 (V2.4): ESP32-S3, 16 MB flash, OPI PSRAM, ED047TC1 panel, 960 by 540, 16 grays.

| Folder | What |
|---|---|
| `blocking/` | Examples on the blocking API. |
| `async/` | The same examples on the Embassy executor. |
| `host-tests/` | Tests that run on the host, in both modes: the `animated_smiles` G5 decoder and bounce logic. |

Each of `blocking/`, `async/` and `host-tests/` is a separate Cargo project on purpose. `epdpi` has
mutually exclusive blocking and async modes, and one Cargo workspace would unify them.

## Examples

| Example | What it shows |
|---|---|
| `display_demo` | Borders, text and a gray bar, one `Quality16` refresh. |
| `grayscale_test` | Port of the FastEPD sketch: 16 gray swatches with their level numbers. Digits come out slightly paler than the Arduino sketch. |
| `micro_sd` | Port of the FastEPD sketch: mounts the microSD card over SPI, appends to a log file, reads `MOCHA.PNG` and shows it. Card logic tested on the host against a RAM disk. |
| `button` | Port of the LilyGo sketch: three text pages, a press of the user button (GPIO21) turns the page, the fourth press deep-sleeps, a press wakes it. Has host tests. |
| `battery` | Part of the LilyGo `demo` sketch: reads the battery divider on GPIO14 with the panel power on and logs the volts every 2 s. Nothing is drawn. Has host tests. |
| `rtc` | Part of the LilyGo `demo` sketch: probes the PCF8563 clock (I2C 0x51, SDA 18, SCL 17), sets it if it lost its time, and logs it every 2 s. Uses the `pcf8563` crate. Has host tests. |
| `wifi_ntp` | Part of the LilyGo `demo` sketch: connects to the WiFi named in `secret.rs` (copy `secret.rs.example`, git-ignored), asks `pool.ntp.org` for the time and stores it in the clock as UTC. Runs on the `esp-rtos` executor in both crates. Has host tests. |
| `demo` | Port of the LilyGo `demo` sketch: SD card size, RTC and touch probes, battery volts, Pacific time from the RTC (set over WiFi and NTP), a press counter, and a 2 s button hold that deep-sleeps for 30 s. The GT911 touch part (corner boxes, `Sleep` box) is written with the `gt911` crate but not verified on hardware: the board it was built on has no touch panel, so only the `Touch: probe failed` path has run. Has host tests. |
| `https_get` | The HTTPS GET the WiFi sketches use: connects with `secret.rs`, then fetches the Quotables and Open-Meteo URLs with `reqwless` over `embedded-tls` (TLS 1.3, no certificate check, like `setInsecure()`) and logs status, length and the start of each body. Nothing is drawn. |
| `quotables` | Port of the LilyGo `Quotables` sketch: fetches a random quote over HTTPS, folds it to ASCII, wraps it in quotation marks into six lines at most, and shows it with the author. Stays awake and refreshes every 5 min with USB attached, deep-sleeps 30 min otherwise. Has host tests. |n| `gif_player` | Port of the FastEPD sketch: a title, then a 36 frame GIF of Muybridge's walking horse (public domain) played once, a partial update per frame. Animation is `assets/muybridge_horse_walking.gif`, made by `tools/dither_gif.py`. |
| `news` | Port of the LilyGo `News` sketch: fetches NewsAPI top headlines over HTTPS (key in the git-ignored `secret.rs`), takes the time from SNTP (Pacific), and shows the date, the update time and up to four headlines with descriptions in Open Sans Bold. Stays awake and refreshes every 30 min with USB attached, deep-sleeps 60 min otherwise. Has host tests. |n| `draw_images` | Port of the LilyGo sketch: three full-screen pictures (a painting, a pencil drawing, a manga page) 3 s each in a loop. |
| `openmeteo` | Port of the LilyGo `OpenMeteo-Weather` sketch: fetches the Open-Meteo forecast over HTTPS (no key), takes the time from SNTP (Pacific) and the battery from the ADC, and draws the dashboard: current weather and feels-like, battery, date, last refresh, an hourly temperature graph with rain chance, and a 6-day strip with icons (`assets/icons/`). Stays awake with USB (refresh every 5 minutes), otherwise deep sleeps 60 minutes. |
| `show_jpeg` | Port of the FastEPD sketch: an 800x480 JPEG decoded with `zune-jpeg` and shown in 16 grays. Picture is `assets/mocha800x480.jpg`. |
| `antialias_font` | Port of the FastEPD sketch: three lines of Roboto Black, hard, smooth, and smooth white on black, from a TTF with `fontdue`. |
| `macpaint_hello` | Port of the FastEPD sketch: a MacPaint window, then a brush writes "hello" in 361 partial updates. Picture is `assets/macpaint_640x480.bmp`. |
| `animated_smiles` | Port of the FastEPD sketch: 8 bouncing smileys, 250 partial updates, then a clear. Has host tests. |
| `lilygo_grayscale_test` | Port of the LilyGo vendor sketch: 16 gradient bars drawn twice per cycle, draw times in the log. |
| `screen_repair` | Port of the LilyGo sketch: a clear, 20 black pulses, a clear, 40 white pulses, a clear. Wipes ghosts. |
| `draw_example` | Port of the LilyGo `drawExample`: five random shapes, one per second, then a clear, in a loop. |

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
| Shift register data / clock / strobe | 13 / 12 / 0 | GPIO46 | tied to the strobe, leave as input |

STV, latch, output enable and mode go through a 74HCT4094 shift register (same protocol as a
74HC595). There is no I2C power IC on this revision. GPIO46 is wired to the same strobe net as
GPIO0, so it must never be driven: holding it high keeps the strobe high and garbles the control
lines. The board bus is `LilygoBus` in `src/lib.rs` of each
crate.

## License

GPL-3.0-only.
