pub mod button;
pub mod led;
pub mod stick;
pub mod websocket;
pub mod wifi;

use crossbeam_channel::bounded;
use esp_idf_hal::prelude::Peripherals;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop, log::EspLogger, nvs::EspDefaultNvsPartition,
    timer::EspTaskTimerService,
};
use esp_idf_sys as _; // If using the `binstart` feature of `esp-idf-sys`, always keep this module imported
use log::*;
use parking_lot::{Condvar, Mutex};
use rgb::RGB8;
use std::sync::Arc;

use crate::{
    button::ButtonTask,
    stick::{StickRead, StickTask},
};

const SSID: &'static str = env!("SSID");
const PASSWORD: &'static str = env!("PASSWORD");
const WEBSOCKET_ADDRESS: &'static str = env!("WEBSOCKET_ADDRESS");

fn main() -> anyhow::Result<()> {
    esp_idf_sys::link_patches();

    EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();
    let sys_loop = EspSystemEventLoop::take()?;
    let timer_service = EspTaskTimerService::new()?;
    let nvs = EspDefaultNvsPartition::take()?;

    info!("Starting application");

    // TODO: find a way (if possible) to copy/clone the TxRmtDriver instance, create a task with a channel or use this as a singleton
    // another way is to use a mutex with the neopixel instance and clone it to each task
    let mut neopixel = led::Neopixel::new(peripherals.pins.gpio48, peripherals.rmt.channel0)?;

    neopixel.set(RGB8 { r: 0, g: 0, b: 0 })?;
    // Create channel
    let (bt_tx, _bt_rx) = bounded::<bool>(10);
    let (stick_tx, stick_rx) = bounded::<StickRead>(10);

    let wifi_status = Arc::new((Mutex::new(false), Condvar::new()));
    let wifi_status_wb = Arc::clone(&wifi_status);

    let _wifi_thread = std::thread::Builder::new()
        .stack_size(6 * 1024)
        .spawn(move || {
            wifi::connect_task(wifi::ConnectTask::new(
                peripherals.modem,
                sys_loop,
                Some(nvs),
                timer_service,
                wifi_status,
                SSID,
                PASSWORD,
            ))
        })?;

    let _button_thread = std::thread::Builder::new()
        .stack_size(4 * 1024)
        .spawn(|| button::init_task(ButtonTask::new(peripherals.pins.gpio0, bt_tx)))?;

    let _stick_thread = std::thread::Builder::new().stack_size(4 * 1024).spawn(|| {
        stick::init_task(StickTask::new(
            peripherals.adc1,
            peripherals.pins.gpio5,
            peripherals.pins.gpio6,
            stick_tx,
        ))
    })?;

    let _websocket_thread = std::thread::Builder::new().stack_size(8 * 1024).spawn(|| {
        websocket::init_task(websocket::WebsocketTask::new(
            WEBSOCKET_ADDRESS,
            wifi_status_wb,
            stick_rx,
        ))
    })?;

    Ok(())
}
