use std::{sync::Arc, time::Duration};

use embedded_svc::wifi::{AuthMethod, ClientConfiguration, Configuration};
use esp_idf_hal::modem::Modem;
use esp_idf_svc::{
    eventloop::{EspEventLoop, System},
    nvs::{EspNvsPartition, NvsDefault},
    timer::{EspTimerService, Task},
    wifi::{AsyncWifi, EspWifi},
};
use futures::executor::block_on;
use log::*;
use parking_lot::{Condvar, Mutex};

// TODO: make private fields with constructor
pub struct ConnectTask<'a> {
    pub modem: Modem,
    pub sys_loop: EspEventLoop<System>,
    pub nvs: Option<EspNvsPartition<NvsDefault>>,
    pub timer: EspTimerService<Task>,
    pub status: Arc<(Mutex<bool>, Condvar)>,
    pub ssid: &'a str,
    pub password: &'a str,
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
