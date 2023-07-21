use esp_idf_svc::{
    log::EspLogger,
    nvs::{EspDefaultNvsPartition, EspNvs, EspNvsPartition, NvsDefault},
};
use esp_idf_sys::{self as _};
use log::*;
use serde::{Deserialize, Serialize};

pub mod button;
pub mod client;
pub mod led;
pub mod otp;

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkCredentials {
    ssid: heapless::String<32>,
    password: heapless::String<64>,
}

impl NetworkCredentials {
    pub fn new(ssid: heapless::String<32>, password: heapless::String<64>) -> Self {
        NetworkCredentials { ssid, password }
    }
}

impl Default for NetworkCredentials {
    fn default() -> Self {
        let ssid = heapless::String::from("DefaultSSID");
        let password = heapless::String::from("DefaultPassword");
        NetworkCredentials::new(ssid, password)
    }
}

enum AppState {
    OTP(EspNvsPartition<NvsDefault>, EspNvs<NvsDefault>),
    CLIENT(EspNvsPartition<NvsDefault>, NetworkCredentials),
}

const NAMESPACE: &'static str = env!("NAMESPACE");
// TODO: this cannot cannot be more than 15 characters, find a way to type it at compile time
pub const NETWORK_TAG: &'static str = "client_cred";

fn main() -> anyhow::Result<()> {
    esp_idf_sys::link_patches();

    EspLogger::initialize_default();

    let nvs_default: EspNvsPartition<NvsDefault> = EspDefaultNvsPartition::take().unwrap();

    let nvs_namespace = match EspNvs::new(nvs_default.clone(), NAMESPACE, true) {
        Ok(nvs) => {
            info!("Got namespace {:?} from default partition", NAMESPACE);
            nvs
        }
        Err(e) => panic!("Could't get namespace {:?}", e),
    };

    let buffer: &mut [u8] = &mut [0; 200];

    let state: AppState = match nvs_namespace.get_raw(NETWORK_TAG, buffer)? {
        Some(network_credentials) => {
            let decode: NetworkCredentials = bincode::deserialize(network_credentials)?;
            info!("[main_task]: Network credentials found: {:?}", decode);
            AppState::CLIENT(nvs_default, decode)
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
        AppState::CLIENT(nvs_default, network_credentials) => {
            client::main(nvs_default, network_credentials)?;
        }
    }

    Ok(())
}
