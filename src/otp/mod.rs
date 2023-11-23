use std::sync::Arc;

use crossbeam_channel::unbounded;
use esp_idf_hal::prelude::Peripherals;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    nvs::{EspNvs, EspNvsPartition, NvsDefault},
};
use log::*;
use parking_lot::{Condvar, Mutex};
use rgb::RGB8;

pub mod nvs;
pub mod server;
pub mod wifi_otp;

use crate::led;

pub fn main(
    nvs_default: EspNvsPartition<NvsDefault>,
    nvs_namespace: EspNvs<NvsDefault>,
) -> anyhow::Result<()> {
    info!("[otp_task]: init");

    let peripherals = Peripherals::take().unwrap();
    let sys_loop = EspSystemEventLoop::take()?;

    // TODO: find a way (if possible) to copy/clone the TxRmtDriver instance, create a task with a channel or use this as a singleton
    // another way is to use a mutex with the neopixel instance and clone it to each task
    let mut neopixel = led::Neopixel::new(peripherals.pins.gpio48, peripherals.rmt.channel0)?;

    neopixel.set(RGB8 { r: 0, g: 0, b: 0 })?;

    let (wifi_scan_tx, wifi_scan_rx) = unbounded::<wifi_otp::ScanMessage>();
    let (server_scan_tx, server_scan_rx) = unbounded::<wifi_otp::ScanMessage>();
    let (nvs_tx, nvs_rx) = unbounded::<jojo_common::network::NetworkCredentials>();

    let wifi_status = Arc::new((Mutex::new(false), Condvar::new()));
    let wifi_status_server = Arc::clone(&wifi_status);

    info!("[otp_task]: creating tasks");

    let _wifi_thread = std::thread::Builder::new()
        .name("wifi_thread".into())
        .stack_size(10 * 1024)
        .spawn(move || {
            wifi_otp::connect_task(wifi_otp::ConnectTask::new(
                peripherals.modem,
                sys_loop,
                Some(nvs_default),
                wifi_status,
                "AP ESP TEST",
                "Hello1234",
                server_scan_tx,
                wifi_scan_rx,
            ))
        })?;

    let _server_thread = std::thread::Builder::new()
        .name("server_thread".into())
        .stack_size(7 * 1024)
        .spawn(|| {
            server::init_task(server::ServerTask::new(
                wifi_status_server,
                wifi_scan_tx,
                server_scan_rx,
                nvs_tx,
            ))
        })
        .unwrap();

    let _nvs_thread = std::thread::Builder::new()
        .name("nvs_thread".into())
        .stack_size(7 * 1024)
        .spawn(|| nvs::init_task(nvs::NvsTask::new(nvs_namespace, nvs_rx)))
        .unwrap();

    Ok(())
}
