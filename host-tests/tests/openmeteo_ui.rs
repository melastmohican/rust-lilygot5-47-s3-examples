//! Tests of the `mod ui` block of `examples/openmeteo.rs`: the request, the parsing and the mappings of the
//! `OpenMeteo-Weather` sketch, against a real response of Open-Meteo.

use host_tests::openmeteo::*;

const FORECAST: &[u8] = include_bytes!("../fixtures/openmeteo_forecast.json");

/// The fixture's moment: Sunday 8 November 2026, 23:00 local time.
const SUNDAY_23: Now = Now {
    weekday: 0,
    hour: 23,
};

fn json() -> serde_json::Value {
    serde_json::from_slice(FORECAST).unwrap()
}

fn f(v: &serde_json::Value) -> f32 {
    v.as_f64().unwrap() as f32
}

#[test]
fn weather_codes_map_to_the_icons_of_the_sketch() {
    let table: &[(&[i32], Icon)] = &[
        (&[0], Icon::ClearSky),
        (&[1, 2, 3], Icon::PartlyCloudy),
        (&[45, 48], Icon::Fog),
        (
            &[51, 53, 55, 56, 57, 61, 63, 65, 66, 67, 80, 81, 82],
            Icon::Rain,
        ),
        (&[71, 73, 75, 77, 85, 86], Icon::Snow),
        (&[95, 96, 99], Icon::Storm),
        (&[4, 10, 44, 46, 60, 70, 90, 100, -1], Icon::Gray),
    ];
    for (codes, icon) in table {
        for &code in *codes {
            assert_eq!(weather_icon(code), *icon, "code {code}");
        }
    }
}

#[test]
fn weather_codes_have_the_sketchs_descriptions() {
    assert_eq!(weather_description(0), "Clear sky");
    assert_eq!(weather_description(2), "Mainly clear, partly cloudy");
    assert_eq!(weather_description(45), "Fog");
    assert_eq!(weather_description(53), "Drizzle");
    assert_eq!(weather_description(57), "Freezing drizzle");
    assert_eq!(weather_description(63), "Rain");
    assert_eq!(weather_description(67), "Freezing rain");
    assert_eq!(weather_description(73), "Snowfall");
    assert_eq!(weather_description(77), "Snow grains");
    assert_eq!(weather_description(81), "Rain showers");
    assert_eq!(weather_description(86), "Snow showers");
    assert_eq!(weather_description(95), "Thunderstorm");
    assert_eq!(weather_description(99), "Thunderstorm, hail");
    assert_eq!(weather_description(12), "Unknown condition");
}

#[test]
fn every_icon_name_is_a_file_in_assets() {
    let icons = [
        Icon::ClearSky,
        Icon::PartlyCloudy,
        Icon::Fog,
        Icon::Rain,
        Icon::Snow,
        Icon::Storm,
        Icon::Gray,
        Icon::FullBattery,
        Icon::HighBattery,
        Icon::HalfBattery,
        Icon::LowBattery,
    ];
    for icon in icons {
        let path = format!(
            "{}/../assets/icons/{}.bmp",
            env!("CARGO_MANIFEST_DIR"),
            icon.file_name()
        );
        assert!(std::path::Path::new(&path).exists(), "{path}");
    }
}

#[test]
fn battery_percent_runs_from_3_0_to_4_2_volts() {
    assert_eq!(battery_percent(4.2), 100);
    assert_eq!(battery_percent(4.5), 100);
    assert_eq!(battery_percent(3.0), 0);
    assert_eq!(battery_percent(2.0), 0);
    assert_eq!(battery_percent(3.6), 50);
    assert_eq!(battery_percent(3.9), 75);
    assert_eq!(battery_percent(4.19), 99);
}

#[test]
fn battery_icons_switch_at_75_50_and_25_percent() {
    assert_eq!(battery_icon(100), Icon::FullBattery);
    assert_eq!(battery_icon(75), Icon::FullBattery);
    assert_eq!(battery_icon(74), Icon::HighBattery);
    assert_eq!(battery_icon(50), Icon::HighBattery);
    assert_eq!(battery_icon(49), Icon::HalfBattery);
    assert_eq!(battery_icon(25), Icon::HalfBattery);
    assert_eq!(battery_icon(24), Icon::LowBattery);
    assert_eq!(battery_icon(0), Icon::LowBattery);
}

#[test]
fn the_request_is_the_sketchs() {
    assert_eq!(
        url(LATITUDE, LONGITUDE),
        "https://api.open-meteo.com/v1/forecast?latitude=47.6101&longitude=-122.2015\
         &daily=temperature_2m_max,temperature_2m_min,weather_code\
         &hourly=temperature_2m,precipitation_probability\
         &current=temperature_2m,apparent_temperature,weather_code&timezone=auto"
    );
}

#[test]
fn hours_and_minutes_come_out_of_the_time_string() {
    assert_eq!(hour_minute("2026-11-07T06:11"), "06:11");
    assert_eq!(hour_minute("2026-11-09T00:00"), "00:00");
    assert_eq!(hour_minute("2026-11-07T06"), "??:??");
    assert_eq!(hour_minute("no time here"), "??:??");
    assert_eq!(hour_minute(""), "??:??");
}

#[test]
fn the_fixture_parses_into_the_dashboard_values() {
    let weather = parse(FORECAST, SUNDAY_23).expect("the fixture parses");
    let v = json();
    assert_eq!(weather.current_temp, f(&v["current"]["temperature_2m"]));
    assert_eq!(weather.feels_like, f(&v["current"]["apparent_temperature"]));
    assert_eq!(weather.code, 3);
    for i in 0..DAYS {
        assert_eq!(
            weather.daily_min[i],
            f(&v["daily"]["temperature_2m_min"][i])
        );
        assert_eq!(
            weather.daily_max[i],
            f(&v["daily"]["temperature_2m_max"][i])
        );
        assert_eq!(
            weather.daily_codes[i] as i64,
            v["daily"]["weather_code"][i].as_i64().unwrap()
        );
    }
    assert_eq!(weather.daily_codes, [53, 55, 63, 55, 3, 81, 80]);
}

#[test]
fn the_hourly_window_starts_at_the_current_hour() {
    let weather = parse(FORECAST, SUNDAY_23).unwrap();
    let v = json();
    for i in 0..HOURS {
        let src = 23 + i;
        assert_eq!(
            weather.hourly_temps[i],
            f(&v["hourly"]["temperature_2m"][src])
        );
        assert_eq!(
            weather.hourly_precip[i],
            f(&v["hourly"]["precipitation_probability"][src])
        );
    }
    // 23:00 on Sunday, then 00:00 to 04:00 on Monday.
    assert_eq!(
        weather.hourly_times,
        ["23:00", "00:00", "01:00", "02:00", "03:00", "04:00"]
    );
}

#[test]
fn the_day_names_start_at_the_current_weekday() {
    let sunday = parse(FORECAST, SUNDAY_23).unwrap();
    assert_eq!(
        sunday.day_names,
        ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
    );
    let friday = parse(
        FORECAST,
        Now {
            weekday: 5,
            hour: 0,
        },
    )
    .unwrap();
    assert_eq!(
        friday.day_names,
        ["Fri", "Sat", "Sun", "Mon", "Tue", "Wed", "Thu"]
    );
    let saturday = parse(
        FORECAST,
        Now {
            weekday: 6,
            hour: 0,
        },
    )
    .unwrap();
    assert_eq!(saturday.day_names[0], "Sat");
    assert_eq!(saturday.day_names[1], "Sun");
}

#[test]
fn the_last_hour_the_arrays_allow_still_parses() {
    // 168 hourly values: starting at 162 the window ends at 167.
    assert!(
        parse(
            FORECAST,
            Now {
                weekday: 0,
                hour: 162
            }
        )
        .is_some()
    );
    assert!(
        parse(
            FORECAST,
            Now {
                weekday: 0,
                hour: 163
            }
        )
        .is_none()
    );
}

#[test]
fn bad_responses_give_none() {
    assert!(parse(b"", SUNDAY_23).is_none());
    assert!(parse(b"not json", SUNDAY_23).is_none());
    assert!(parse(b"{}", SUNDAY_23).is_none());
    assert!(
        parse(
            br#"{"error":true,"reason":"Latitude must be in range"}"#,
            SUNDAY_23
        )
        .is_none()
    );
}

#[test]
fn a_missing_or_short_part_gives_none() {
    let mut v = json();
    v["daily"]["temperature_2m_max"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(
        parse(v.to_string().as_bytes(), SUNDAY_23).is_none(),
        "six daily highs"
    );
    let mut v = json();
    v["current"]
        .as_object_mut()
        .unwrap()
        .remove("apparent_temperature");
    assert!(
        parse(v.to_string().as_bytes(), SUNDAY_23).is_none(),
        "no feels-like"
    );
    let mut v = json();
    v.as_object_mut().unwrap().remove("hourly");
    assert!(
        parse(v.to_string().as_bytes(), SUNDAY_23).is_none(),
        "no hourly part"
    );
}

#[test]
fn a_null_value_gives_none_instead_of_zero() {
    let mut v = json();
    v["hourly"]["temperature_2m"][25] = serde_json::Value::Null;
    assert!(parse(v.to_string().as_bytes(), SUNDAY_23).is_none());
    let mut v = json();
    v["daily"]["weather_code"][3] = serde_json::Value::Null;
    assert!(parse(v.to_string().as_bytes(), SUNDAY_23).is_none());
}

#[test]
fn numbers_are_formatted_like_the_sketch() {
    assert_eq!(temp_1(7.6), "7.6 C");
    assert_eq!(temp_1(-3.04), "-3.0 C");
    assert_eq!(temp_1(12.0), "12.0 C");
    assert_eq!(temp_0(7.6), "8 C");
    assert_eq!(temp_0(7.4), "7 C");
    assert_eq!(temp_0(-2.0), "-2 C");
    assert_eq!(percent(37.0), "37%");
    assert_eq!(percent(100.0), "100%");
    assert_eq!(percent(0.0), "0%");
}

#[test]
fn the_graph_scale_widens_the_range_by_two_each_side() {
    let (p_min, p_max, range) = graph_scale(&[7.0, 8.5, 6.0, 9.0, 8.0, 7.5]);
    assert_eq!((p_min, p_max, range), (4.0, 11.0, 7.0));
    let (p_min, p_max, range) = graph_scale(&[5.0; 6]);
    assert_eq!((p_min, p_max, range), (3.0, 7.0, 4.0));
}

#[test]
fn the_strip_has_six_days_with_icons_and_whole_degrees() {
    let weather = parse(FORECAST, SUNDAY_23).unwrap();
    let days = strip(&weather);
    assert_eq!(days.len(), SHOWN_DAYS);
    assert_eq!(days[0].0, "Sun");
    assert_eq!(days[0].1, Icon::Rain); // code 53, drizzle
    assert_eq!(days[4].1, Icon::PartlyCloudy); // code 3
    assert_eq!(days[5].1, Icon::Rain); // code 81, rain showers
    for (_, _, max, min) in &days {
        assert!(max.ends_with(" C") && min.ends_with(" C"));
    }
}

// ---- drawing -------------------------------------------------------------------------------------------

use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use fontdue::{Font, FontSettings};
use tinybmp::RawBmp;

const OPEN_SANS_BOLD: &[u8] = include_bytes!("../../assets/fonts/OpenSans-Bold.ttf");

fn font() -> Font {
    Font::from_bytes(OPEN_SANS_BOLD, FontSettings::default()).expect("the font loads")
}

fn icon_file(icon: Icon) -> &'static [u8] {
    let path = format!(
        "{}/../assets/icons/{}.bmp",
        env!("CARGO_MANIFEST_DIR"),
        icon.file_name()
    );
    Box::leak(std::fs::read(path).unwrap().into_boxed_slice())
}

fn ink_count(icon: Icon) -> usize {
    RawBmp::from_slice(icon_file(icon))
        .unwrap()
        .pixels()
        .filter(|p| p.color == 0)
        .count()
}

/// 960 x 540, one byte per pixel with the gray level 0 to 15, 0xee where nothing was drawn.
struct Canvas {
    pixels: Vec<u8>,
}

impl Canvas {
    fn at(&self, x: i32, y: i32) -> u8 {
        self.pixels[(y * 960 + x) as usize]
    }
    fn count(&self, x0: i32, y0: i32, w: i32, h: i32, pred: impl Fn(u8) -> bool) -> usize {
        (y0..y0 + h)
            .flat_map(|y| (x0..x0 + w).map(move |x| (x, y)))
            .filter(|&(x, y)| (0..960).contains(&x) && (0..540).contains(&y) && pred(self.at(x, y)))
            .count()
    }
    fn black(&self, x0: i32, y0: i32, w: i32, h: i32) -> usize {
        self.count(x0, y0, w, h, |v| v == 0)
    }
    fn ink(&self, x0: i32, y0: i32, w: i32, h: i32) -> usize {
        self.count(x0, y0, w, h, |v| v < 8)
    }
    fn extent(&self, x0: i32, y0: i32, w: i32, h: i32) -> Option<(i32, i32)> {
        let cols: Vec<i32> = (x0..x0 + w)
            .filter(|&x| self.ink(x, y0, 1, h) > 0)
            .collect();
        Some((*cols.first()?, *cols.last()?))
    }
}

impl OriginDimensions for Canvas {
    fn size(&self) -> Size {
        Size::new(960, 540)
    }
}

impl DrawTarget for Canvas {
    type Color = Gray4;
    type Error = core::convert::Infallible;
    fn draw_iter<I: IntoIterator<Item = Pixel<Gray4>>>(
        &mut self,
        pixels: I,
    ) -> Result<(), Self::Error> {
        for Pixel(p, c) in pixels {
            if (0..960).contains(&p.x) && (0..540).contains(&p.y) {
                self.pixels[(p.y * 960 + p.x) as usize] = c.luma();
            }
        }
        Ok(())
    }
}

fn dashboard(volts: f64) -> (Canvas, Weather) {
    let weather = parse(FORECAST, SUNDAY_23).unwrap();
    let mut canvas = Canvas {
        pixels: vec![0xee; 960 * 540],
    };
    let d = Dashboard {
        weather: &weather,
        city: CITY,
        user: USER,
        date: "2026-11-08",
        time: "23:05",
        battery_volts: volts,
    };
    draw_dashboard(&mut canvas, &font(), &icon_file, &d);
    (canvas, weather)
}

#[test]
fn text_height_follows_the_sketchs_bounds() {
    let font = font();
    // Without descenders the height is the ink height above the baseline; with them it is bigger.
    let (_, plain) = text_bounds(&font, PX_10, "HE");
    let (_, descender) = text_bounds(&font, PX_10, "Hg");
    assert!(plain > 10 && descender > plain);
    let (w1, _) = text_bounds(&font, PX_10, "H");
    let (w2, _) = text_bounds(&font, PX_10, "HH");
    assert!((w2 - 2.0 * w1).abs() < 0.01);
}

#[test]
fn the_frames_and_the_info_panel() {
    let (c, _) = dashboard(4.2);
    // Header, graph and strip frames, one pixel wide.
    assert_eq!(c.black(0, 0, 960, 1), 960);
    assert_eq!(c.black(0, 129, 960, 1), 960);
    assert_eq!(c.black(0, GRAPH_Y, 960, 1), 960);
    assert_eq!(c.black(0, WEEK_Y, 960, 1), 960);
    assert_eq!(c.black(0, 539, 960, 1), 960);
    assert_eq!(c.black(0, 0, 1, 540), 540);
    assert_eq!(c.black(959, 0, 1, 540), 540);
    // The divider before the info panel, and the panel's gray.
    assert_eq!(c.black(USER_X, 0, 1, HEADER_H), HEADER_H as usize);
    assert_eq!(c.at(950, 100), PANEL_GRAY);
    assert_eq!(c.at(900, 60), PANEL_GRAY);
    assert_eq!(c.at(950, 126), PANEL_GRAY);
    assert_eq!(c.at(600, 5), WHITE);
}

#[test]
fn the_weather_icon_is_drawn_twice_as_big() {
    let (c, weather) = dashboard(4.2);
    let icon = weather_icon(weather.code);
    assert_eq!(icon, Icon::PartlyCloudy);
    // Each source pixel becomes 2 x 2 pixels inside (15, 17, 96, 96).
    assert_eq!(c.black(15, 17, 96, 96), 4 * ink_count(icon));
    assert_eq!(
        c.ink(112, 17, 20, 96),
        0,
        "nothing between the icon and the city"
    );
}

#[test]
fn the_header_texts_are_where_the_sketch_puts_them() {
    let (c, _) = dashboard(4.2);
    let (left, _) = c.extent(130, 12, 200, 40).expect("the city");
    assert!(
        (left - 140).abs() <= 6,
        "the city starts at x=140, got {left}"
    );
    assert!(c.ink(140, 62, 190, 40) > 100, "the temperature");
    assert!(c.ink(340, 70, 330, 28) > 100, "the condition");
    assert!(c.ink(340, 100, 330, 20) > 50, "feels like");
    assert!(c.ink(440, 2, 230, 66) == 0, "nothing above the condition");
}

#[test]
fn the_battery_shows_only_above_2_5_volts() {
    let full = ink_count(Icon::FullBattery);
    let (c, _) = dashboard(4.2);
    assert_eq!(c.black(USER_X + 10, 4, 48, 48), full);
    assert!(c.ink(USER_X + 65, 16, 100, 28) > 20, "the percent text");
    let (c, _) = dashboard(3.6); // 50 %, the high icon
    assert_eq!(
        c.black(USER_X + 10, 4, 48, 48),
        ink_count(Icon::HighBattery)
    );
    let (c, _) = dashboard(3.1); // 8 %, the low icon
    assert_eq!(c.black(USER_X + 10, 4, 48, 48), ink_count(Icon::LowBattery));
    for volts in [0.0, 2.5] {
        let (c, _) = dashboard(volts);
        assert_eq!(c.ink(USER_X + 10, 4, 150, 48), 0, "no battery at {volts} V");
    }
    let (c, _) = dashboard(2.51);
    assert!(c.ink(USER_X + 10, 4, 48, 48) > 0);
}

#[test]
fn the_info_panel_text_is_smooth_against_the_gray() {
    let (c, _) = dashboard(4.2);
    for (y, name) in [(64, "date"), (86, "refresh"), (108, "user")] {
        assert!(c.ink(USER_X + 10, y, 260, 20) > 30, "{name} is drawn");
    }
    // Text on the panel is blended with the panel's gray, so no pixel there is lighter than the panel.
    assert_eq!(
        c.count(USER_X + 5, 62, 270, 60, |v| v > PANEL_GRAY && v != 0xee),
        0
    );
}

#[test]
fn the_six_day_strip() {
    let (c, weather) = dashboard(4.2);
    let day_w = 160;
    for i in 0..6 {
        let x = i * day_w;
        if i > 0 {
            assert_eq!(
                c.count(x, WEEK_Y + 10, 1, 160, |v| v == LIGHT_GRAY),
                160,
                "separator {i}"
            );
        }
        // The name is centred in its column.
        let (l, r) = c
            .extent(x + 2, WEEK_Y + 10, day_w - 4, 30)
            .expect("the day name");
        assert!(
            ((l + r) / 2 - (x + 80)).abs() <= 8,
            "name {i} spans {l}..{r}"
        );
        // The icon at 1x.
        let icon = weather_icon(weather.daily_codes[i as usize]);
        assert_eq!(
            c.black(x + 14, WEEK_Y + 55, 48, 48),
            ink_count(icon),
            "icon {i}"
        );
        // The triangles: black up for the high, gray down for the low.
        let (ax, ay) = (x + 85, WEEK_Y + 62);
        assert_eq!(c.at(ax, ay + 6), 0, "high triangle {i}");
        assert_eq!(c.at(ax, ay + 40 + 3), GRAY, "low triangle {i}");
        assert!(c.ink(ax + 14, ay - 8, 60, 20) > 20, "high text {i}");
    }
    assert_eq!(
        c.black(0, WEEK_Y + 10, 1, 160),
        160,
        "no separator before the first day, only the frame"
    );
}

#[test]
fn the_graph_axes_labels_and_title() {
    let (c, _) = dashboard(4.2);
    // The y axis from graph_y + 20 to the bottom, the x axis to graph_x + graph_w + 20.
    assert_eq!(c.black(110, 185, 1, 151), 151);
    assert_eq!(c.black(110, 335, 781, 1), 781);
    assert!(c.ink(10, 136, 400, 24) > 100, "the title");
    let (_, right) = c.extent(40, 180, 68, 12).expect("the top label");
    assert!(
        (right - 102).abs() <= 6,
        "labels end at chart_left - 8, got {right}"
    );
    for y in [323, 252, 183] {
        assert!(c.ink(40, y, 68, 20) > 10, "y label near {y}");
    }
}

#[test]
fn the_graph_bars_and_the_line_follow_the_data() {
    let (c, weather) = dashboard(4.2);
    let (p_min, _, range) = graph_scale(&weather.hourly_temps);
    for i in 0..HOURS {
        let x = (110.0 + i as f32 * 148.0) as i32;
        let bar_h = ((weather.hourly_precip[i] / 100.0) * 150.0) as i32;
        let y = 335 - bar_h;
        if bar_h > 10 {
            assert_eq!(
                c.count(x + 5, y + 10, 14, bar_h - 10, |v| v == LIGHT_GRAY),
                (14 * (bar_h - 10)) as usize,
                "bar {i}"
            );
        }
        assert!(c.ink(x + 5, y - 16, 60, 18) > 10, "percent label {i}");
        assert!(c.ink(x + 3, 341, 60, 20) > 20, "time label {i}");
        let ty = (335.0 - ((weather.hourly_temps[i] - p_min) / range) * 150.0) as i32;
        assert_eq!(c.at(x, ty), 0, "the line passes through point {i}");
        assert_eq!(c.at(x, ty + 1), 0, "and is two pixels thick at point {i}");
    }
}

#[test]
fn nothing_is_drawn_outside_the_frames() {
    let (c, _) = dashboard(4.2);
    assert_eq!(
        c.count(0, 0, 960, 540, |v| v == 0xee),
        0,
        "every pixel is drawn, the page starts white"
    );
}

#[test]
fn a_message_has_two_lines() {
    let mut c = Canvas {
        pixels: vec![0xee; 960 * 540],
    };
    draw_message(&mut c, &font(), WIFI_FAILED.0, WIFI_FAILED.1);
    assert!(c.ink(60, 220, 600, 50) > 100);
    assert!(c.ink(60, 280, 700, 40) > 100);
    assert_eq!(c.ink(0, 0, 960, 200), 0);
}

#[test]
fn local_time_is_pacific_with_the_right_weekday() {
    // 2026-11-09 06:30 UTC is Sunday 2026-11-08 22:30 PST.
    let unix = 1_794_205_800;
    let t = local(unix).unwrap();
    assert_eq!(
        (t.date().as_str(), t.time().as_str(), t.weekday),
        ("2026-11-08", "22:30", 0)
    );
    assert_eq!(
        t.now(),
        Now {
            weekday: 0,
            hour: 22
        }
    );
}
