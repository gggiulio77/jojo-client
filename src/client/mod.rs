use std::sync::Arc;

use bus::Bus;
use crossbeam_channel::unbounded;
use esp_idf_hal::prelude::Peripherals;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    nvs::{EspNvsPartition, NvsDefault},
    timer::EspTaskTimerService,
};
use log::*;
use parking_lot::{Condvar, Mutex};
use rgb::RGB8;

pub mod stick;
pub mod websocket;
pub mod wifi_client;

use crate::{button, led, NetworkCredentials};

const WEBSOCKET_ADDRESS: &'static str = env!("WEBSOCKET_ADDRESS");

pub fn main(
    nvs_default: EspNvsPartition<NvsDefault>,
    network_credentials: NetworkCredentials,
) -> anyhow::Result<()> {
    info!("[client_task]: init");

    let peripherals = Peripherals::take().unwrap();
    let sys_loop = EspSystemEventLoop::take()?;
    let timer_service = EspTaskTimerService::new()?;

    // TODO: find a way (if possible) to copy/clone the TxRmtDriver instance, create a task with a channel or use this as a singleton
    // another way is to use a mutex with the neopixel instance and clone it to each task
    let mut neopixel = led::Neopixel::new(peripherals.pins.gpio48, peripherals.rmt.channel0)?;

    neopixel.set(RGB8 { r: 0, g: 0, b: 0 })?;
    // Create channel
    let mut bt_bus = Bus::<bool>::new(10);
    let bt_wb_rx = bt_bus.add_rx();
    let bt_stick_rx = bt_bus.add_rx();

    let (stick_tx, stick_rx) = unbounded::<websocket::MouseRead>();

    let wifi_status = Arc::new((Mutex::new(false), Condvar::new()));
    let wifi_status_wb = Arc::clone(&wifi_status);
    let wifi_status_stick = Arc::clone(&wifi_status);

    info!("[client_task]: creating tasks");

    let _wifi_thread = std::thread::Builder::new()
        .stack_size(6 * 1024)
        .spawn(move || {
            wifi_client::connect_task(wifi_client::ConnectTask::new(
                peripherals.modem,
                sys_loop,
                Some(nvs_default),
                timer_service,
                wifi_status,
                network_credentials.ssid.as_str(),
                network_credentials.password.as_str(),
            ))
        })?;

    let _button_thread = std::thread::Builder::new()
        .stack_size(4 * 1024)
        .spawn(|| button::init_task(button::ButtonTask::new(peripherals.pins.gpio0, bt_bus)))?;

    let _stick_thread = std::thread::Builder::new().stack_size(6 * 1024).spawn(|| {
        stick::init_task(stick::StickTask::new(
            peripherals.adc1,
            peripherals.pins.gpio5,
            peripherals.pins.gpio6,
            peripherals.pins.gpio7,
            stick_tx,
            bt_wb_rx,
            wifi_status_stick,
        ))
    })?;

    let _websocket_thread = std::thread::Builder::new().stack_size(3 * 4096).spawn(|| {
        websocket::init_task(websocket::WebsocketTask::new(
            WEBSOCKET_ADDRESS,
            wifi_status_wb,
            stick_rx,
            bt_stick_rx,
        ))
    })?;

    Ok(())
}
