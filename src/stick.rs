use std::time::Duration;

use esp_idf_hal::{
    adc::{self, *},
    gpio::{Gpio5, Gpio6},
};
use log::*;

pub struct StickTask {
    adc1: ADC1,
    // TODO: replace with a generic, restricted to ADC1 GPIOs
    gpio_x: Gpio5,
    // TODO: replace with a generic, restricted to ADC1 GPIOs
    gpio_y: Gpio6,
    stick_tx: crossbeam_channel::Sender<StickRead>,
}

impl StickTask {
    pub fn new(
        adc1: ADC1,
        gpio_x: Gpio5,
        gpio_y: Gpio6,
        stick_tx: crossbeam_channel::Sender<StickRead>,
    ) -> Self {
        StickTask {
            adc1,
            gpio_x,
            gpio_y,
            stick_tx,
        }
    }
}

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

    loop {
        let x_read = match adc_driver.read(&mut x_adc_channel) {
            Ok(read) => {
                info!("[stick_task]:x_read {read}\n");
                read
            }
            Err(e) => {
                error!("err reading x_adc_driver: {e}\n");
                break;
            }
        };
        let y_read = match adc_driver.read(&mut y_adc_channel) {
            Ok(read) => {
                info!("[stick_task]:y_read {read}\n");
                read
            }
            Err(e) => {
                error!("err reading x_adc_driver: {e}\n");
                break;
            }
        };

        task.stick_tx.send(StickRead::new(x_read, y_read)).unwrap();

        std::thread::sleep(Duration::from_millis(500));
    }
}
