use std::{net::SocketAddr, sync::Arc};

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

pub mod broadcast;
pub mod mouse;
pub mod websocket;
pub mod wifi_client;

use crate::{button, client::broadcast::BroadcastTask, led, NetworkCredentials};

const WEBSOCKET_PATH: &'static str = env!("WEBSOCKET_PATH");

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

    let wifi_status = Arc::new((Mutex::new(false), Condvar::new()));
    let wifi_status_bd = Arc::clone(&wifi_status);
    // TODO: review this condvar, maybe replace it with a websocket broadcast channel
    let wifi_status_stick = Arc::clone(&wifi_status);
    let wifi_status_left_click = Arc::clone(&wifi_status);

    // TODO: review this channel, maybe replace it with two broadcast channels, for websocket and discovery
    let (discovery_tx, discovery_rx) = unbounded::<SocketAddr>();

    // Channels to send websocket messages
    let (wb_sender_tx, wb_sender_rx) = unbounded::<jojo_common::message::ClientMessage>();
    let stick_wb_sender_tx = wb_sender_tx.clone();
    let left_click_wb_sender_tx = wb_sender_tx.clone();

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

    let _broadcast_discovery = std::thread::Builder::new()
        .stack_size(4 * 1024)
        .spawn(|| broadcast::init_task(BroadcastTask::new(wifi_status_bd, discovery_tx)))?;

    let _button_thread = std::thread::Builder::new()
        .stack_size(4 * 1024)
        .spawn(|| button::init_task(button::ButtonTask::new(peripherals.pins.gpio0, bt_bus)))?;

    let _stick_thread = std::thread::Builder::new().stack_size(6 * 1024).spawn(|| {
        mouse::stick::init_task(mouse::stick::StickTask::new(
            peripherals.adc1,
            peripherals.pins.gpio5,
            peripherals.pins.gpio6,
            // TODO: replace with stick_websocket_sender_tx
            stick_wb_sender_tx,
            wifi_status_stick,
        ))
    })?;

    let _left_click_thread = std::thread::Builder::new().stack_size(6 * 1024).spawn(|| {
        mouse::left_click::init_task(mouse::left_click::LeftClickTask::new(
            peripherals.pins.gpio7,
            // TODO: replace with stick_websocket_sender_tx
            left_click_wb_sender_tx,
            wifi_status_left_click,
        ))
    })?;

    let _websocket_thread = std::thread::Builder::new()
        .stack_size(20 * 1024)
        .spawn(|| {
            websocket::init_task(websocket::WebsocketTask::new(
                WEBSOCKET_PATH,
                discovery_rx,
                wb_sender_rx,
            ))
        })?;

    Ok(())
}
