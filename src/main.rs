use std::collections::HashMap;

use esp_idf_svc::{
    log::EspLogger,
    nvs::{EspDefaultNvsPartition, EspNvs, EspNvsPartition, NvsDefault},
};
use jojo_common::{
    button::{Button, ButtonAction, ButtonMode},
    device::Device,
    keyboard::KeyboardButton,
    mouse::{MouseButton, MouseButtonState, MouseConfig},
};
use log::*;

pub mod button;
pub mod client;
pub mod led;
pub mod otp;

enum AppState {
    OTP(EspNvsPartition<NvsDefault>, EspNvs<NvsDefault>),
    CLIENT(
        EspNvsPartition<NvsDefault>,
        jojo_common::network::NetworkCredentials,
        EspNvs<NvsDefault>,
        Device,
    ),
}

const NAMESPACE: &'static str = env!("NAMESPACE");
// TODO: this cannot cannot be more than 15 characters, find a way to type it at compile time
pub const NETWORK_TAG: &'static str = "client_cred";
pub const DEVICE_TAG: &'static str = "device";

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();

    EspLogger::initialize_default();

    let nvs_default: EspNvsPartition<NvsDefault> = EspDefaultNvsPartition::take().unwrap();

    let mut nvs_namespace = match EspNvs::new(nvs_default.clone(), NAMESPACE, true) {
        Ok(nvs) => {
            info!("Got namespace {:?} from default partition", NAMESPACE);
            nvs
        }
        Err(e) => panic!("Could't get namespace {:?}", e),
    };

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

    let buffer: &mut [u8] = &mut [0; 200];

    let state: AppState = match nvs_namespace.get_raw(NETWORK_TAG, buffer)? {
        Some(network_credentials) => {
            let decode: jojo_common::network::NetworkCredentials =
                bincode::deserialize(network_credentials)?;
            info!("[main_task]: Network credentials found: {:?}", decode);

            AppState::CLIENT(nvs_default, decode, nvs_namespace, main_device)
        }
        None => {
            info!("[main_task]: Network credentials not found");
            AppState::OTP(nvs_default, nvs_namespace)
        }
    };

    // TODO: think a way to reset client credentials (go from CLIENT -> OTP)
    match state {
        AppState::OTP(nvs_default, nvs_namespace) => {
            otp::main(nvs_default, nvs_namespace)?;
        }
        AppState::CLIENT(nvs_default, network_credentials, nvs_namespace, device) => {
            client::main(nvs_default, network_credentials, nvs_namespace, device)?;
        }
    }

    Ok(())
}
