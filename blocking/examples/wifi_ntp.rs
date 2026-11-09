//! # wifi_ntp: WiFi and the network time of the LilyGo `demo` sketch
//!
//! Connects to the WiFi network named in `secret.rs` (git-ignored, copy `secret.rs.example`), gets an
//! address by DHCP, asks `pool.ntp.org` for the time and stores it in the PCF8563 clock (as UTC). The
//! sketch does the same with `WiFi.begin` and `configTzTime`; the Arduino sketch is
//! `~/Src/Arduino/LilyGoT5ePaper/LilyGo/demo`. Nothing is drawn. The password is never logged.
//!
//! ## Run
//!
//! ```text
//! cp secret.rs.example secret.rs   # then edit it
//! cargo run --release --example wifi_ntp
//! ```
//!
//! ## Expected
//!
//! - The log prints `wifi_ntp: Rust, blocking`, `connected`, `got IP a.b.c.d`, `NTP time` in UTC, and
//!   `RTC set`, then the RTC time every 5 s.
//! - The NTP time matches `date -u` on a computer within 2 s. The network must be 2.4 GHz.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use defmt::{error, info, warn};
use embassy_executor::Spawner;
use embassy_net::{Config as NetConfig, Runner, StackResources, dns::DnsQueryType, udp};
use embassy_time::{Duration, Timer};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::rng::Rng;
use esp_hal::time::Rate;
use esp_println as _;
use esp_radio::wifi::{Config, ControllerConfig, Interface, sta::StationConfig};
use pcf8563::{DateTime, PCF8563};
use sntpc::{NtpContext, get_time};
use sntpc_net_embassy::UdpSocketWrapper;
use sntpc_time_embassy::EmbassyTimestampGenerator;

esp_bootloader_esp_idf::esp_app_desc!();

mod secret {
    include!("../../secret.rs");
}

/// The hardware-free part: Unix seconds to a calendar date. Same text in the async crate, and
/// tested on the host by `host-tests`.
mod ui {
    /// Pool the sketch asks first.
    pub const NTP_SERVER: &str = "pool.ntp.org";

    /// A UTC date and time.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Utc {
        pub year: u16,
        pub month: u8,
        pub day: u8,
        pub hour: u8,
        pub minute: u8,
        pub second: u8,
        /// 0 = Sunday.
        pub weekday: u8,
    }

    /// Calendar date of Unix seconds (days since 1970-01-01 to civil date, after Howard Hinnant).
    pub fn utc(unix: u64) -> Utc {
        let days = (unix / 86_400) as i64;
        let secs = (unix % 86_400) as u32;
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as u16;
        Utc {
            year,
            month,
            day,
            hour: (secs / 3600) as u8,
            minute: (secs % 3600 / 60) as u8,
            second: (secs % 60) as u8,
            weekday: ((days + 4).rem_euclid(7)) as u8,
        }
    }
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, Interface<'static>>) -> ! {
    runner.run().await
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let p = rust_lilygot5_47_s3_examples::init!();
    info!("wifi_ntp: Rust, blocking");
    // The radio wants internal RAM, see the `esp-radio` docs and `wifi_scan` in the M5 repo.
    rust_lilygot5_47_s3_examples::esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    rust_lilygot5_47_s3_examples::esp_alloc::heap_allocator!(size: 36 * 1024);
    let timg0 = esp_hal::timer::timg::TimerGroup::new(p.TIMG0);
    let sw_interrupt = esp_hal::interrupt::software::SoftwareInterruptControl::new(p.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    let Ok(i2c) = I2c::new(
        p.I2C0,
        I2cConfig::default().with_frequency(Rate::from_khz(100)),
    ) else {
        rust_lilygot5_47_s3_examples::halt();
    };
    let mut rtc = PCF8563::new(i2c.with_sda(p.GPIO18).with_scl(p.GPIO17));

    let station = StationConfig::default()
        .with_ssid(secret::WIFI_SSID)
        .with_password(secret::WIFI_PASSWORD.into());
    let Ok((mut controller, interfaces)) = esp_radio::wifi::new(
        p.WIFI,
        ControllerConfig::default().with_initial_config(Config::Station(station)),
    ) else {
        error!("wifi init failed");
        rust_lilygot5_47_s3_examples::halt();
    };
    info!("connecting");
    if controller.connect_async().await.is_err() {
        error!("connect failed: wrong name or password, or not a 2.4 GHz network");
        rust_lilygot5_47_s3_examples::halt();
    }
    info!("connected");

    let seed = u64::from(Rng::new().random()) << 32 | u64::from(Rng::new().random());
    let resources = Box::leak(Box::new(StackResources::<4>::new()));
    let (stack, runner) = embassy_net::new(
        interfaces.station,
        NetConfig::dhcpv4(Default::default()),
        resources,
        seed,
    );
    let Ok(task) = net_task(runner) else {
        error!("net task did not start");
        rust_lilygot5_47_s3_examples::halt();
    };
    spawner.spawn(task);
    stack.wait_config_up().await;
    if let Some(config) = stack.config_v4() {
        info!("got IP {}", config.address.address());
    }

    let Ok(addrs) = stack.dns_query(ui::NTP_SERVER, DnsQueryType::A).await else {
        error!("dns failed");
        rust_lilygot5_47_s3_examples::halt();
    };
    let Some(&embassy_net::IpAddress::Ipv4(server)) = addrs.first() else {
        error!("dns gave no IPv4 address");
        rust_lilygot5_47_s3_examples::halt();
    };
    info!("NTP server {}", server);

    let mut rx_meta = [udp::PacketMetadata::EMPTY; 4];
    let mut tx_meta = [udp::PacketMetadata::EMPTY; 4];
    let mut rx_buf = [0u8; 256];
    let mut tx_buf = [0u8; 256];
    let mut socket =
        udp::UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    if socket.bind(123).is_err() {
        error!("udp bind failed");
        rust_lilygot5_47_s3_examples::halt();
    }
    let socket = UdpSocketWrapper::from(socket);
    let context = NtpContext::new(EmbassyTimestampGenerator::default());
    let addr = core::net::SocketAddr::new(core::net::IpAddr::V4(server), 123);
    match get_time(addr, &socket, context).await {
        Ok(result) => {
            let t = ui::utc(result.seconds);
            info!(
                "NTP time {}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
                t.year, t.month, t.day, t.hour, t.minute, t.second
            );
            let now = DateTime {
                year: (t.year - 2000) as u8,
                month: t.month,
                weekday: t.weekday,
                day: t.day,
                hours: t.hour,
                minutes: t.minute,
                seconds: t.second,
            };
            let _ = rtc.rtc_init();
            match rtc.set_datetime(&now) {
                Ok(()) => info!("RTC set"),
                Err(_) => warn!("RTC set failed"),
            }
        }
        Err(_) => error!("NTP request failed"),
    }

    loop {
        if let Ok(t) = rtc.get_datetime() {
            info!(
                "RTC {}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
                2000 + u16::from(t.year),
                t.month,
                t.day,
                t.hours,
                t.minutes,
                t.seconds
            );
        }
        Timer::after(Duration::from_secs(5)).await;
    }
}
