use std::time::Duration;

use esp_idf_hal::gpio::{AnyIOPin, Gpio0, IOPin, Input, PinDriver, Pull};
use log::*;

pub struct ButtonTask {
    gpio_0: Gpio0,
    bt_tx: crossbeam_channel::Sender<bool>,
}

impl ButtonTask {
    pub fn new(gpio_0: Gpio0, bt_tx: crossbeam_channel::Sender<bool>) -> Self {
        ButtonTask { gpio_0, bt_tx }
    }
}

fn init_button(btn_pin: Gpio0) -> anyhow::Result<PinDriver<'static, AnyIOPin, Input>> {
    // Config pin
    let mut btn = PinDriver::input(btn_pin.downgrade())?;
    btn.set_pull(Pull::Down)?;

    return Ok(btn);
}

pub fn init_task(task: ButtonTask) {
    info!("[button_task]:creating");
    let btn = init_button(task.gpio_0).unwrap();
    let mut button_switch = false;

    loop {
        match btn.get_level() {
            esp_idf_hal::gpio::Level::High => {
                if button_switch == true {
                    button_switch = false;
                    info!("[button_task]:BUTTON RELEASE");
                }
            }
            esp_idf_hal::gpio::Level::Low => {
                if button_switch == false {
                    button_switch = true;
                    info!("[button_task]:BUTTON PRESS");
                    task.bt_tx.send(button_switch).unwrap();
                }
            }
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}
