use std::sync::Arc;

use esp_idf_hal::{
    adc::{self, *},
    delay::Ets,
    gpio::{Gpio5, Gpio6},
};
use log::*;
use parking_lot::{Condvar, Mutex};

use crate::websocket::MouseRead;

pub struct StickTask {
    adc1: ADC1,
    // TODO: replace with a generic, restricted to ADC1 GPIOs
    gpio_x: Gpio5,
    // TODO: replace with a generic, restricted to ADC1 GPIOs
    gpio_y: Gpio6,
    stick_tx: crossbeam_channel::Sender<MouseRead>,
    wifi_status: Arc<(Mutex<bool>, Condvar)>,
}

const X_B: i32 = -2;
const Y_B: i32 = -2;
const X_M: f32 = 2.0 / 1615.0;
const Y_M: f32 = 2.0 / 1592.0;

impl Into<MouseRead> for StickRead {
    fn into(self) -> MouseRead {
        let x_read = if self.x_read > 1605 && self.x_read < 1625 {
            0
        } else {
            (X_M * self.x_read as f32).round() as i32 + X_B
        };

        let y_read = if self.y_read > 1582 && self.y_read < 1602 {
            0
        } else {
            (Y_M * self.y_read as f32).round() as i32 + Y_B
        };

        MouseRead::new(x_read, y_read)
    }
}

impl StickTask {
    pub fn new(
        adc1: ADC1,
        gpio_x: Gpio5,
        gpio_y: Gpio6,
        stick_tx: crossbeam_channel::Sender<MouseRead>,
        wifi_status: Arc<(Mutex<bool>, Condvar)>,
    ) -> Self {
        StickTask {
            adc1,
            gpio_x,
            gpio_y,
            stick_tx,
            wifi_status,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StickRead {
    pub x_read: u16,
    pub y_read: u16,
}

impl StickRead {
    pub fn new(x_read: u16, y_read: u16) -> Self {
        StickRead { x_read, y_read }
    }
}

pub fn init_task(task: StickTask) {
    info!("[stick_task]:creating");
    let mut adc_driver =
        AdcDriver::new(task.adc1, &adc::config::Config::new().calibration(true)).unwrap();

    let mut x_adc_channel: AdcChannelDriver<'_, Gpio5, Atten11dB<ADC1>> =
        AdcChannelDriver::<_, Atten11dB<ADC1>>::new(task.gpio_x).unwrap();

    let mut y_adc_channel = AdcChannelDriver::<_, Atten11dB<ADC1>>::new(task.gpio_y).unwrap();

    let (lock, cvar) = &*task.wifi_status;

    let mut started = lock.lock();

    if !*started {
        cvar.wait(&mut started);
    }
    drop(started);

    loop {
        // TODO: review this approach, maybe it make sense to accumulate reads instead of sending one at a time
        let x_read = adc_driver.read(&mut x_adc_channel).unwrap();

        let y_read = adc_driver.read(&mut y_adc_channel).unwrap();

        task.stick_tx
            .try_send(StickRead::new(x_read, y_read).into())
            .unwrap();

        Ets::delay_ms(3);
    }
}
