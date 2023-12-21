use common::{AppState, NAMESPACE, NETWORK_TAG};
use esp_idf_svc::{
    log::EspLogger,
    nvs::{EspDefaultNvsPartition, EspNvs, EspNvsPartition, NvsDefault},
};

use log::*;

pub mod client;

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

    let buffer: &mut [u8] = &mut [0; 200];

    let state: AppState = match nvs_namespace.get_raw(NETWORK_TAG, buffer)? {
        Some(network_credentials) => {
            let decode: jojo_common::network::NetworkCredentials =
                bincode::deserialize(network_credentials)?;
            info!("[main_task]: Network credentials found: {:?}", decode);

            AppState::CLIENT(nvs_default, decode, nvs_namespace)
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
        AppState::CLIENT(nvs_default, network_credentials, nvs_namespace) => {
            client::main(nvs_default, network_credentials, nvs_namespace)?;
        }
    }

    Ok(())
}
