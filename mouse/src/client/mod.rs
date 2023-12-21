use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use anyhow::Result;
use common::{broadcast, led, websocket, wifi_client, DEVICE_TAG, WEBSOCKET_PATH};
use crossbeam_channel::unbounded;
use esp_idf_hal::{
    gpio::{AnyIOPin, Pull},
    prelude::Peripherals,
};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    nvs::{EspNvs, EspNvsPartition, NvsDefault},
    timer::EspTaskTimerService,
};
use jojo_common::{
    button::{Button, ButtonAction, ButtonMode},
    device::Device,
    keyboard::KeyboardButton,
    mouse::{MouseButton, MouseButtonState, MouseConfig},
};
use log::*;
use parking_lot::{Condvar, Mutex};
use rgb::RGB8;

pub mod peripherals;

use crate::client::peripherals::button::ButtonTask;

pub fn main(
    nvs_default: EspNvsPartition<NvsDefault>,
    network_credentials: jojo_common::network::NetworkCredentials,
    mut nvs_namespace: EspNvs<NvsDefault>,
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

    // TODO: review this channel, maybe replace it with two broadcast channels, for websocket and discovery
    let (discovery_tx, discovery_rx) = unbounded::<SocketAddr>();

    // Channels to send websocket messages
    let (wb_sender_tx, wb_sender_rx) = unbounded::<jojo_common::message::ClientMessage>();
    let stick_wb_sender_tx = wb_sender_tx.clone();

    info!("[client_task]: getting device");

    let device = get_device(&mut nvs_namespace)?;

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
        .spawn(|| {
            broadcast::init_task(broadcast::BroadcastTask::new(wifi_status_bd, discovery_tx))
        })?;

    let cloned_device = device.clone();

    let _websocket_thread = std::thread::Builder::new()
        .name("websocket_thread".into())
        .stack_size(24 * 1024)
        .spawn(|| {
            websocket::init_task(websocket::WebsocketTask::new(
                WEBSOCKET_PATH,
                discovery_rx,
                wb_sender_rx,
                wb_status,
                cloned_device,
                nvs_namespace,
            ))
        })?;

    // TODO: we need a way to know if we have a mouse or a gamepad stick or both. Maybe find a way to store it in flash or made hardcoded code to one device only.
    let _stick_thread = std::thread::Builder::new()
        .name("stick_thread".into())
        .stack_size(8 * 1024)
        .spawn(|| {
            peripherals::stick::init_task(peripherals::stick::StickTask::new(
                peripherals.adc1,
                peripherals.pins.gpio5,
                peripherals.pins.gpio4,
                // TODO: replace with stick_websocket_sender_tx
                stick_wb_sender_tx,
                wb_status_stick,
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
            .spawn(move || peripherals::button::init_task(button_task))
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

fn get_device(nvs_namespace: &mut EspNvs<NvsDefault>) -> Result<Device> {
    let buffer: &mut [u8] = &mut [0; 500];

    // nvs_namespace.remove(NETWORK_TAG).unwrap();

    // TODO: replace this with something more elegant
    let main_device;

    // nvs_namespace.remove(DEVICE_TAG).unwrap();

    if let Some(device) = nvs_namespace.get_raw(DEVICE_TAG, buffer)? {
        main_device = bincode::deserialize(device)?;
        info!("[main_task]: device found in flash {:?}", main_device);
    } else {
        // TODO: create device and store it
        let gpio6 = uuid::Uuid::new_v4();
        let gpio6_button = Button::new(gpio6, String::from("button_0"), ButtonMode::Hold);
        let mut actions = HashMap::from([(
            gpio6,
            vec![ButtonAction::MouseButton(
                MouseButton::Left,
                MouseButtonState::Up,
            )],
        )]);

        let gpio0 = uuid::Uuid::new_v4();
        let gpio0_button = Button::new(gpio0, String::from("button_1"), ButtonMode::Click);
        actions.insert(
            gpio0,
            vec![
                ButtonAction::MouseButton(MouseButton::Left, MouseButtonState::Down),
                ButtonAction::MouseButton(MouseButton::Left, MouseButtonState::Up),
                ButtonAction::KeyboardButton(KeyboardButton::SequenceDsl(
                    "{+CTRL}a{-CTRL}".to_string(),
                )),
                ButtonAction::MouseButton(MouseButton::Right, MouseButtonState::Down),
                ButtonAction::MouseButton(MouseButton::Right, MouseButtonState::Up),
            ],
        );

        let device = Device::new(
            uuid::Uuid::new_v4(),
            String::from("device_1"),
            Some(MouseConfig::new(1, -1)),
            vec![gpio6_button, gpio0_button],
            actions,
        );

        info!("[main_task]: saving device in flash {:?}", device);

        nvs_namespace
            .set_raw(DEVICE_TAG, &bincode::serialize(&device)?)
            .unwrap();

        main_device = device;
    };

    Ok(main_device)
}
