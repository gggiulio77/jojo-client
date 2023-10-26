use std::{sync::Arc, time::Duration};

use esp_idf_hal::modem::Modem;
use esp_idf_svc::{
    eventloop::{EspEventLoop, System},
    nvs::{EspNvsPartition, NvsDefault},
    timer::{EspTimerService, Task},
    wifi::{AsyncWifi, AuthMethod, ClientConfiguration, Configuration, EspWifi},
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
    ) -> Self {
        ConnectTask {
            modem,
            sys_loop,
            nvs,
            timer,
            status,
            ssid,
            password,
        }
    }
}

pub async fn connect(
    wifi: &mut AsyncWifi<EspWifi<'static>>,
    ssid: &str,
    password: &str,
) -> anyhow::Result<()> {
    let wifi_configuration: Configuration = Configuration::Client(ClientConfiguration {
        ssid: ssid.into(),
        bssid: None,
        auth_method: AuthMethod::WPA2Personal,
        password: password.into(),
        channel: None,
    });

    wifi.set_configuration(&wifi_configuration)?;

    wifi.start().await?;
    info!("Wifi started");

    wifi.connect().await?;
    info!("Wifi connected");

    wifi.wait_netif_up().await?;
    info!("Wifi netif up");

    Ok(())
}

pub fn connect_task(task: ConnectTask) {
    info!("[connect_task]:creating");

    let ConnectTask {
        modem,
        sys_loop,
        nvs,
        timer,
        status,
        ssid,
        password,
    } = task;

    let (lock, cvar) = &*status;

    let mut wifi_config = AsyncWifi::wrap(
        EspWifi::new(modem, sys_loop.clone(), nvs).unwrap(),
        sys_loop,
        timer,
    )
    .unwrap();

    block_on(connect(&mut wifi_config, ssid, password)).unwrap();

    // Write value to mutex
    *lock.lock() = true;
    cvar.notify_all();

    loop {
        std::thread::sleep(Duration::from_millis(1000));
    }
}
