use esp_idf_svc::nvs::{EspNvs, EspNvsPartition, NvsDefault};

pub mod broadcast;
pub mod led;
pub mod websocket;
pub mod wifi_client;

pub enum AppState {
    OTP(EspNvsPartition<NvsDefault>, EspNvs<NvsDefault>),
    CLIENT(
        EspNvsPartition<NvsDefault>,
        jojo_common::network::NetworkCredentials,
        EspNvs<NvsDefault>,
    ),
}

pub const NAMESPACE: &'static str = env!("NAMESPACE");
// TODO: this cannot cannot be more than 15 characters, find a way to type it at compile time
pub const NETWORK_TAG: &'static str = "client_cred";
pub const DEVICE_TAG: &'static str = "device";
pub const WEBSOCKET_PATH: &'static str = env!("WEBSOCKET_PATH");
pub const BROADCAST_BIND_ADDRESS: &'static str = env!("BROADCAST_BIND_ADDRESS");
pub const BROADCAST_ADDRESS: &'static str = env!("BROADCAST_ADDRESS");
