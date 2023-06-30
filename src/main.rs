pub mod button;
pub mod led;
pub mod websocket;
pub mod wifi;

use crossbeam_channel::bounded;
use esp_idf_hal::{
    gpio::{IOPin, PinDriver, Pull},
    prelude::Peripherals,
};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop, log::EspLogger, nvs::EspDefaultNvsPartition,
    timer::EspTaskTimerService,
};
use esp_idf_sys as _; // If using the `binstart` feature of `esp-idf-sys`, always keep this module imported
use log::*;
use parking_lot::{Condvar, Mutex};
use rgb::RGB8;
use std::sync::Arc;

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

    // Config pin
    let mut btn_pin = PinDriver::input(peripherals.pins.gpio0.downgrade()).unwrap();
    btn_pin.set_pull(Pull::Down).unwrap();

    // TODO: find a way (if possible) to copy/clone the TxRmtDriver instance, create a task with a channel or use this as a singleton
    // another way is to use a mutex with the neopixel instance and clone it to each task
    let mut neopixel =
        led::Neopixel::new(peripherals.pins.gpio48, peripherals.rmt.channel0).unwrap();

    neopixel.set(RGB8 { r: 0, g: 0, b: 0 }).unwrap();
    // Create channel
    let (bt_tx, bt_rx) = bounded::<bool>(10);

    let wifi_status = Arc::new((Mutex::new(false), Condvar::new()));
    let wifi_status_wb = Arc::clone(&wifi_status);

    let _wifi_thread = std::thread::Builder::new()
        .stack_size(6 * 1024)
        .spawn(move || {
            wifi::connect_task(wifi::ConnectTask {
                modem: peripherals.modem,
                sys_loop,
                nvs: Some(nvs),
                timer: timer_service,
                status: wifi_status,
                ssid: SSID,
                password: PASSWORD,
            })
        })
        .unwrap();

    let _button_thread = std::thread::Builder::new()
        .stack_size(4 * 1024)
        .spawn(|| button::init_task(btn_pin, bt_tx))
        .unwrap();

    let _websocket_thread = std::thread::Builder::new()
        .stack_size(8 * 1024)
        .spawn(|| {
            websocket::init_task(websocket::WebsocketTask {
                address: WEBSOCKET_ADDRESS,
                wifi_status: wifi_status_wb,
                bt_rx,
            })
        })
        .unwrap();

    Ok(())
}
