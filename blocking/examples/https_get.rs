//! # https_get: the HTTPS GET that the LilyGo WiFi sketches use
//!
//! Connects to the WiFi network in `secret.rs` (git-ignored, copy `secret.rs.example`), then makes an
//! HTTPS GET to the Quotables and Open-Meteo URLs of the Arduino sketches and logs the status, the
//! length and the start of each body. The Arduino sketches use `WiFiClientSecure` with `setInsecure()`
//! and `HTTPClient`; here it is `reqwless` over `embedded-tls` (TLS 1.3) with no certificate check, the
//! same trade. Nothing is drawn. The password is never logged.
//! The async twin is `https_get` in `async/`. The network stack is async, so this twin also runs on
//! the `esp-rtos` executor, as `wifi_ntp` does; it has no display code for the blocking `epdpi` API to differ in.
//!
//! ## Run
//!
//! ```text
//! cp secret.rs.example secret.rs   # then edit it
//! cargo run --release --example https_get
//! ```
//!
//! ## Expected
//!
//! - The log prints `https_get: Rust, blocking`, `got IP`, then for each URL `GET <host>`, `status 200`,
//!   `bytes N`, the start of the body, and the free heap before and after.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::{boxed::Box, vec};
use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_net::{
    Config as NetConfig, Runner, StackResources,
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
};
use embassy_time::{Duration, Timer};
use esp_hal::rng::Rng;
use esp_println as _;
use esp_radio::wifi::{Config, ControllerConfig, Interface, sta::StationConfig};
use reqwless::{
    client::{HttpClient, TlsConfig, TlsVerify},
    request::Method,
};

esp_bootloader_esp_idf::esp_app_desc!();

mod secret {
    include!("../../secret.rs");
}

/// The URLs of `Quotables` and `OpenMeteo-Weather`.
const URLS: [&str; 2] = [
    "https://api.quotable.kurokeita.dev/api/quotes/random",
    "https://api.open-meteo.com/v1/forecast?latitude=47.6101&longitude=-122.2015&current=temperature_2m",
];

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, Interface<'static>>) -> ! {
    runner.run().await
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let p = rust_lilygot5_47_s3_examples::init!();
    info!("https_get: Rust, blocking");
    // The radio wants internal RAM, see `wifi_ntp`.
    rust_lilygot5_47_s3_examples::esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    rust_lilygot5_47_s3_examples::esp_alloc::heap_allocator!(size: 36 * 1024);
    let timg0 = esp_hal::timer::timg::TimerGroup::new(p.TIMG0);
    let sw_interrupt = esp_hal::interrupt::software::SoftwareInterruptControl::new(p.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

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
    if controller.connect_async().await.is_err() {
        error!("connect failed: wrong name or password, or not a 2.4 GHz network");
        rust_lilygot5_47_s3_examples::halt();
    }
    let rng = Rng::new();
    let seed = u64::from(rng.random()) << 32 | u64::from(rng.random());
    let resources = Box::leak(Box::new(StackResources::<6>::new()));
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

    // `embedded-tls` wants a whole TLS record (16 KiB) to read into.
    let mut tls_read = vec![0u8; 16_640];
    let mut tls_write = vec![0u8; 4_096];
    let mut body = vec![0u8; 8_192];
    let state = TcpClientState::<1, 1_024, 1_024>::new();
    let tcp = TcpClient::new(stack, &state);
    let dns = DnsSocket::new(stack);
    let tls = TlsConfig::new(seed, &mut tls_read, &mut tls_write, TlsVerify::None);
    let mut client = HttpClient::new_with_tls(&tcp, &dns, tls);

    for url in URLS {
        let heap = &rust_lilygot5_47_s3_examples::esp_alloc::HEAP;
        info!("GET {} (free heap {})", url, heap.free());
        let mut request = match client.request(Method::GET, url).await {
            Ok(request) => request,
            Err(e) => {
                error!("request failed: {}", e);
                continue;
            }
        };
        let response = match request.send(&mut body).await {
            Ok(response) => response,
            Err(e) => {
                error!("send failed: {}", e);
                continue;
            }
        };
        info!("status {}", response.status.0);
        match response.body().read_to_end().await {
            Ok(bytes) => {
                info!("bytes {}", bytes.len());
                let head = &bytes[..bytes.len().min(200)];
                info!("{=str}", core::str::from_utf8(head).unwrap_or("(not text)"));
            }
            Err(e) => error!("body failed: {}", e),
        }
        info!("free heap after {}", heap.free());
    }
    info!("https_get: done");
    loop {
        Timer::after(Duration::from_secs(60)).await;
    }
}
