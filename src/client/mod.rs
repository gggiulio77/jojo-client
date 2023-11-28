use std::{net::SocketAddr, sync::Arc};

use crossbeam_channel::unbounded;
use esp_idf_hal::{
    adc::{self, AdcChannelDriver, AdcDriver},
    gpio::{AnyIOPin, Pull},
    prelude::Peripherals,
};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    nvs::{EspNvs, EspNvsPartition, NvsDefault},
    timer::EspTaskTimerService,
};
use jojo_common::device::Device;
use log::*;
use parking_lot::{Condvar, Mutex};
use rgb::RGB8;

pub mod broadcast;
pub mod mouse;
pub mod websocket;
pub mod wifi_client;

use crate::{client::broadcast::BroadcastTask, client::mouse::button::ButtonTask, led};

const WEBSOCKET_PATH: &'static str = env!("WEBSOCKET_PATH");

pub fn main(
    nvs_default: EspNvsPartition<NvsDefault>,
    network_credentials: jojo_common::network::NetworkCredentials,
    nvs_namespace: EspNvs<NvsDefault>,
    device: Device,
) -> anyhow::Result<()> {
    info!("[client_task]: init");

    let peripherals = Peripherals::take().unwrap();
    let sys_loop = EspSystemEventLoop::take()?;
    let timer_service = EspTaskTimerService::new()?;

    // TODO: find a way (if possible) to copy/clone the TxRmtDriver instance, create a task with a channel or use this as a singleton
    // another way is to use a mutex with the neopixel instance and clone it to each task
    let mut neopixel = led::Neopixel::new(peripherals.pins.gpio48, peripherals.rmt.channel0)?;

    neopixel.set(RGB8 { r: 0, g: 0, b: 0 })?;

    let wifi_status = Arc::new((Mutex::new(false), Condvar::new()));
    let wifi_status_bd = Arc::clone(&wifi_status);

    // TODO: review this condvar, maybe replace it with a websocket broadcast channel
    let wb_status = Arc::new((Mutex::new(false), Condvar::new()));
    let wb_status_cloned = Arc::clone(&wb_status);
    let wb_status_stick = Arc::clone(&wb_status);
    let wb_status_axis = Arc::clone(&wb_status);

    // TODO: review this channel, maybe replace it with two broadcast channels, for websocket and discovery
    let (discovery_tx, discovery_rx) = unbounded::<SocketAddr>();

    // Channels to send websocket messages
    let (wb_sender_tx, wb_sender_rx) = unbounded::<jojo_common::message::ClientMessage>();
    let stick_wb_sender_tx = wb_sender_tx.clone();
    let axis_wb_sender_tx = wb_sender_tx.clone();

    info!("[client_task]: creating tasks");

    let _wifi_thread = std::thread::Builder::new()
        .name("wifi_thread".into())
        .stack_size(6 * 1024)
        .spawn(move || {
            wifi_client::connect_task(wifi_client::ConnectTask::new(
                peripherals.modem,
                sys_loop,
                Some(nvs_default),
                timer_service,
                wifi_status,
                network_credentials.ssid.to_string().as_str(),
                network_credentials.password.to_string().as_str(),
            ))
        })?;

    let _broadcast_discovery = std::thread::Builder::new()
        .name("broadcast_discovery".into())
        .stack_size(6 * 1024)
        .spawn(|| broadcast::init_task(BroadcastTask::new(wifi_status_bd, discovery_tx)))?;

    let cloned_device = device.clone();

    let _websocket_thread = std::thread::Builder::new()
        .name("websocket_thread".into())
        .stack_size(22 * 1024)
        .spawn(|| {
            websocket::init_task(websocket::WebsocketTask::new(
                WEBSOCKET_PATH,
                discovery_rx,
                wb_sender_rx,
                wb_status,
                cloned_device,
            ))
        })?;

    // TODO: we need a way to know if we have a mouse or a gamepad stick or both. Maybe find a way to store it in flash or made hardcoded code to one device only.
    // let _stick_thread = std::thread::Builder::new()
    //     .name("stick_thread".into())
    //     .stack_size(8 * 1024)
    //     .spawn(|| {
    //         mouse::stick::init_task(mouse::stick::StickTask::new(
    //             peripherals.adc1,
    //             peripherals.pins.gpio5,
    //             peripherals.pins.gpio4,
    //             // TODO: replace with stick_websocket_sender_tx
    //             stick_wb_sender_tx,
    //             wb_status_stick,
    //         ))
    //     })?;

    // TODO: move all this shit to a build pattern or function
    let adc_1_driver = Arc::new(Mutex::new(
        AdcDriver::new(
            peripherals.adc1,
            &adc::config::Config::new().calibration(true),
        )
        .unwrap(),
    ));
    let adc_1_driver_clone = adc_1_driver.clone();
    let axis_wb_sender_tx_clone = axis_wb_sender_tx.clone();
    let wb_status_axis_clone = wb_status_axis.clone();

    let _axis_thread = std::thread::Builder::new()
        .name("axis1_thread".into())
        .stack_size(8 * 1024)
        .spawn(|| {
            mouse::axis::init_task(mouse::axis::AxisTask::new(
                adc_1_driver,
                AdcChannelDriver::new(peripherals.pins.gpio4).unwrap(),
                jojo_common::gamepad::Axis::Axis1,
                // TODO: replace with stick_websocket_sender_tx
                axis_wb_sender_tx,
                wb_status_axis,
            ))
        })?;

    let _axis_thread = std::thread::Builder::new()
        .name("axis2_thread".into())
        .stack_size(8 * 1024)
        .spawn(|| {
            mouse::axis::init_task(mouse::axis::AxisTask::new(
                adc_1_driver_clone,
                AdcChannelDriver::new(peripherals.pins.gpio5).unwrap(),
                jojo_common::gamepad::Axis::Axis2,
                // TODO: replace with stick_websocket_sender_tx
                axis_wb_sender_tx_clone,
                wb_status_axis_clone,
            ))
        })?;

    // TODO: find a more idiomatic way of doing this, maybe a builder pattern may help
    let mut gpios: Vec<(AnyIOPin, Pull)> = vec![
        (peripherals.pins.gpio0.into(), Pull::Down),
        (peripherals.pins.gpio6.into(), Pull::Up),
    ];

    let mut actions_map = device.actions_map().clone();
    let buttons = device.buttons().clone();

    info!("[buttons]: {:?}", buttons);
    info!("[actions]: {:?}", actions_map);

    for button in buttons {
        info!("[button]: {:?}, gpio_len: {:?}", button, gpios.len());

        let (pin, pull) = gpios.pop().expect("cannot unwrap gpio");
        let action = actions_map.remove(&button.id()).expect(
            format!(
                "cannot unwrap button.id: {:?}, actions: {:?}",
                actions_map,
                button.id()
            )
            .as_str(),
        );
        let gpio_wb_sender_tx = wb_sender_tx.clone();
        let wb_status_gpio = Arc::clone(&wb_status_cloned);
        let button_task: ButtonTask = ButtonTask::new(
            pin,
            gpio_wb_sender_tx,
            wb_status_gpio,
            action,
            button.mode().clone(),
            pull,
        );

        info!("[client_task]: creating gpio task");

        let _gpio = std::thread::Builder::new()
            .name("gpio_thread".into())
            .stack_size(6 * 1024)
            .spawn(move || mouse::button::init_task(button_task))
            .unwrap();
    }

    // let _ = std::thread::Builder::new().stack_size(4 * 1024).spawn(|| {
    //     let time = Instant::now();
    //     loop {
    //         info!("[client_task]: i'am alive bitch {:?}", time.elapsed());
    //         Ets::delay_ms(60000);
    //     }
    // })?;

    Ok(())
}
