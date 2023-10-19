use std::sync::Arc;

use embedded_svc::wifi::{
    AccessPointConfiguration, AuthMethod, ClientConfiguration, Configuration,
};
use esp_idf_hal::{delay::FreeRtos, modem::Modem};
use esp_idf_svc::{
    eventloop::{EspEventLoop, System},
    nvs::{EspNvsPartition, NvsDefault},
    wifi::{config::ScanConfig, BlockingWifi, EspWifi},
};
use log::*;
use parking_lot::{Condvar, Mutex};

pub struct ConnectTask<'a> {
    modem: Modem,
    sys_loop: EspEventLoop<System>,
    nvs: Option<EspNvsPartition<NvsDefault>>,
    status: Arc<(Mutex<bool>, Condvar)>,
    ssid: &'a str,
    password: &'a str,
    tx_channel: crossbeam_channel::Sender<ScanMessage>,
    rx_channel: crossbeam_channel::Receiver<ScanMessage>,
}

impl<'a> ConnectTask<'a> {
    pub fn new(
        modem: Modem,
        sys_loop: EspEventLoop<System>,
        nvs: Option<EspNvsPartition<NvsDefault>>,
        status: Arc<(Mutex<bool>, Condvar)>,
        ssid: &'a str,
        password: &'a str,
        tx_channel: crossbeam_channel::Sender<ScanMessage>,
        rx_channel: crossbeam_channel::Receiver<ScanMessage>,
    ) -> Self {
        ConnectTask {
            modem,
            sys_loop,
            nvs,
            status,
            ssid,
            password,
            tx_channel,
            rx_channel,
        }
    }
}

fn connect(
    wifi: &mut BlockingWifi<EspWifi<'static>>,
    ssid: &str,
    password: &str,
) -> anyhow::Result<()> {
    let wifi_configuration: Configuration = Configuration::Mixed(
        ClientConfiguration::default(),
        AccessPointConfiguration {
            ssid: ssid.into(),
            ssid_hidden: false,
            auth_method: AuthMethod::WPA2Personal,
            password: password.into(),
            ..Default::default()
        },
    );

    wifi.set_configuration(&wifi_configuration)?;

    wifi.start()?;
    info!("Wifi started");

    Ok(())
}

#[derive(Debug)]
pub enum ScanMessage {
    Request,
    // TODO: find a way to wrap jojo_common::network::Ssid to work with heapless
    Response(Vec<jojo_common::network::Ssid>),
}

fn scan(
    wifi: &mut BlockingWifi<EspWifi<'static>>,
) -> anyhow::Result<(Vec<jojo_common::network::Ssid>, usize)> {
    let mut vec_ssid: Vec<jojo_common::network::Ssid> = Vec::new();

    for channel in (1..=11).rev() {
        wifi.wifi_mut().start_scan(
            &ScanConfig {
                channel: Some(channel),
                ..Default::default()
            },
            false,
        )?;

        FreeRtos::delay_ms(150);

        if let Ok(result) = wifi.wifi_mut().get_scan_result() {
            result.into_iter().for_each(|network| {
                if network.signal_strength > -50 {
                    // TODO: find a way to wrap jojo_common::network::Ssid to work with heapless
                    vec_ssid.push(network.ssid.to_string().try_into().unwrap());
                }
            });
        }
    }

    wifi.wifi_mut().stop_scan()?;

    let n_aps = vec_ssid.len();

    Ok((vec_ssid, n_aps))
}

pub fn connect_task(task: ConnectTask) {
    info!("[connect_task]:creating");

    let ConnectTask {
        modem,
        sys_loop,
        nvs,
        status,
        ssid,
        password,
        tx_channel,
        rx_channel,
    } = task;

    let (lock, cvar) = &*status;

    let mut wifi_driver = BlockingWifi::wrap(
        EspWifi::new(modem, sys_loop.clone(), nvs).unwrap(),
        sys_loop,
        // timer,
    )
    .unwrap();

    connect(&mut wifi_driver, ssid, password).unwrap();

    // Write value to mutex
    *lock.lock() = true;
    cvar.notify_all();

    info!("[connect_task]:Start channel listening");

    loop {
        if let Ok(message) = rx_channel.try_recv() {
            if let ScanMessage::Request = message {
                info!("[connect_task]:message Request");

                let (scan_result, n_aps) = scan(&mut wifi_driver).unwrap();

                info!("[connect_task]: {:?}, N: {:?}", scan_result, n_aps);

                tx_channel
                    .try_send(ScanMessage::Response(scan_result))
                    .unwrap();
            }
        }

        FreeRtos::delay_ms(100);
    }
}
