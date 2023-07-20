use std::sync::Arc;

use embedded_svc::wifi::{
    AccessPointConfiguration, AuthMethod, ClientConfiguration, Configuration,
};
use esp_idf_hal::{delay::FreeRtos, modem::Modem};
use esp_idf_svc::{
    eventloop::{EspEventLoop, System},
    nvs::{EspNvsPartition, NvsDefault},
    timer::{EspTimerService, Task},
    wifi::{BlockingWifi, EspWifi},
};
use futures::executor::block_on;
use log::*;
use parking_lot::{Condvar, Mutex};

pub struct ConnectTask<'a> {
    modem: Modem,
    sys_loop: EspEventLoop<System>,
    nvs: Option<EspNvsPartition<NvsDefault>>,
    timer: EspTimerService<Task>,
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
        timer: EspTimerService<Task>,
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
            timer,
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
    Response(Vec<heapless::String<32>>),
}

fn scan(
    wifi: &mut BlockingWifi<EspWifi<'static>>,
) -> anyhow::Result<(Vec<heapless::String<32>>, usize)> {
    let (scan_result, n_aps) = wifi.scan_n::<5>()?;

    let vec_ssid: Vec<heapless::String<32>> =
        scan_result
            .into_iter()
            .fold(Vec::new(), |mut acc, network| {
                if network.signal_strength > -50 {
                    acc.push(network.ssid);
                }

                return acc;
            });

    Ok((vec_ssid, n_aps))
}

pub fn connect_task(task: ConnectTask) {
    info!("[connect_task]:creating");

    let ConnectTask {
        modem,
        sys_loop,
        nvs,
        timer: _,
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
