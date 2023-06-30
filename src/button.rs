use std::time::Duration;

use esp_idf_hal::gpio::{AnyIOPin, Input, PinDriver};
use log::*;

pub fn init_task(btn_pin: PinDriver<AnyIOPin, Input>, tx: crossbeam_channel::Sender<bool>) {
    info!("[button_task]:creating");
    let mut button_switch = false;

    loop {
        match btn_pin.get_level() {
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
                    tx.send(button_switch).unwrap();
                }
            }
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}
