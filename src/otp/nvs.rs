use esp_idf_hal::delay::FreeRtos;
use esp_idf_svc::nvs::{EspNvs, NvsDefault};
use log::*;

use crate::NETWORK_TAG;

// TODO: think about make this task more generic to be use by otp and client
pub struct NvsTask {
    nvs_namespace: EspNvs<NvsDefault>,
    nvs_rx: crossbeam_channel::Receiver<jojo_common::network::NetworkCredentials>,
}

impl NvsTask {
    pub fn new(
        nvs_namespace: EspNvs<NvsDefault>,
        nvs_rx: crossbeam_channel::Receiver<jojo_common::network::NetworkCredentials>,
    ) -> Self {
        NvsTask {
            nvs_namespace,
            nvs_rx,
        }
    }
}

pub fn init_task(task: NvsTask) {
    let NvsTask {
        mut nvs_namespace,
        nvs_rx,
    } = task;

    info!("[nvs_task]: creating");

    loop {
        if let Ok(network_credentials) = nvs_rx.try_recv() {
            info!("[nvs_task]: writing flash {:?}", network_credentials);

            match nvs_namespace.set_raw(
                NETWORK_TAG,
                &bincode::serialize(&network_credentials).unwrap(),
            ) {
                Ok(_) => {
                    info!("[nvs_task]: flash write OK");
                }
                Err(err) => panic!("{:?}", err),
            };
        }

        FreeRtos::delay_ms(500);
    }
}
