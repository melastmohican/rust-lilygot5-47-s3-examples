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
