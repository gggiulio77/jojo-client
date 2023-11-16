use std::{sync::Arc, time::Duration};

use esp_idf_hal::{
    adc::{self, *},
    gpio::{Gpio5, Gpio6},
};
use jojo_common::message::{ClientMessage, Reads};
use log::*;
use parking_lot::{Condvar, Mutex};

pub struct StickTask {
    adc1: ADC1,
    // TODO: replace with a generic, restricted to ADC1 GPIOs
    gpio_x: Gpio5,
    // TODO: replace with a generic, restricted to ADC1 GPIOs
    gpio_y: Gpio6,
    websocket_sender_tx: crossbeam_channel::Sender<jojo_common::message::ClientMessage>,
    wb_status: Arc<(Mutex<bool>, Condvar)>,
}

impl StickTask {
    pub fn new(
        adc1: ADC1,
        gpio_x: Gpio5,
        gpio_y: Gpio6,
        // TODO: replace with stick_websocket_sender_tx and websocket Message Reads
        websocket_sender_tx: crossbeam_channel::Sender<jojo_common::message::ClientMessage>,
        wb_status: Arc<(Mutex<bool>, Condvar)>,
    ) -> Self {
        StickTask {
            adc1,
            gpio_x,
            gpio_y,
            websocket_sender_tx,
            wb_status,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StickCalibration {
    x_b: i32,
    y_b: i32,
    x_m: f32,
    y_m: f32,
    x_zero_read: u16,
    y_zero_read: u16,
}

impl StickCalibration {
    pub fn new(x_b: i32, y_b: i32, x_m: f32, y_m: f32, x_zero_read: u16, y_zero_read: u16) -> Self {
        StickCalibration {
            x_b,
            y_b,
            x_m,
            y_m,
            x_zero_read,
            y_zero_read,
        }
    }

    pub fn calibrate(x_zero_read: u16, y_zero_read: u16) -> Self {
        StickCalibration::new(
            -20,
            -20,
            20.0 / x_zero_read as f32,
            20.0 / y_zero_read as f32,
            x_zero_read,
            y_zero_read,
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StickRead {
    x_read: u16,
    y_read: u16,
    // TODO: generalize to all mouse buttons
    calibration: StickCalibration,
}

impl StickRead {
    pub fn new(x_read: u16, y_read: u16, calibration: StickCalibration) -> Self {
        StickRead {
            x_read,
            y_read,
            calibration,
        }
    }
}

impl Into<jojo_common::mouse::MouseRead> for StickRead {
    // type Error = anyhow::Error;

    fn into(self) -> jojo_common::mouse::MouseRead {
        let StickCalibration {
            x_b,
            y_b,
            x_m,
            y_m,
            x_zero_read,
            y_zero_read,
        } = self.calibration;
        let (x_min_lim, x_max_lim) = (x_zero_read - 50, x_zero_read + 50);
        let (y_min_lim, y_max_lim) = (y_zero_read - 50, y_zero_read + 50);

        let x_read = if self.x_read > x_min_lim && self.x_read < x_max_lim {
            0
        } else {
            (x_m * self.x_read as f32).round() as i32 + x_b
        };

        let y_read = if self.y_read > y_min_lim && self.y_read < y_max_lim {
            0
        } else {
            (y_m * self.y_read as f32).round() as i32 + y_b
        };

        jojo_common::mouse::MouseRead::new(x_read, y_read)
    }
}

enum ReadStates {
    Calibrating,
    Reading(StickCalibration),
}

struct ReadState {
    current_state: ReadStates,
}

impl Default for ReadState {
    fn default() -> Self {
        ReadState::new(ReadStates::Calibrating)
    }
}

impl ReadState {
    pub fn new(init_state: ReadStates) -> Self {
        ReadState {
            current_state: init_state,
        }
    }

    pub fn state(&self) -> &ReadStates {
        &self.current_state
    }

    fn to_state(&mut self, state: ReadStates) -> &mut Self {
        self.current_state = state;
        self
    }

    pub fn _to_calibrating(&mut self) -> &Self {
        info!("[stick_task]:state_calibrating");
        self.to_state(ReadStates::Calibrating)
    }

    pub fn to_reading(&mut self, calibration: StickCalibration) -> &Self {
        info!("[stick_task]:state_reading");
        self.to_state(ReadStates::Reading(calibration))
    }
}

pub fn init_task(task: StickTask) {
    let StickTask {
        adc1,
        gpio_x,
        gpio_y,
        websocket_sender_tx,
        wb_status,
    } = task;

    info!("[stick_task]:creating");
    let mut adc_driver =
        AdcDriver::new(adc1, &adc::config::Config::new().calibration(true)).unwrap();

    let mut x_adc_channel: AdcChannelDriver<{ attenuation::DB_11 }, Gpio5> =
        AdcChannelDriver::new(gpio_x).unwrap();

    let mut y_adc_channel: AdcChannelDriver<{ attenuation::DB_11 }, Gpio6> =
        AdcChannelDriver::new(gpio_y).unwrap();

    let (lock, cvar) = &*wb_status;

    let mut started = lock.lock();

    if !*started {
        cvar.wait(&mut started);
    }
    drop(started);

    let mut main_state = ReadState::default();
    loop {
        match main_state.state() {
            ReadStates::Calibrating => {
                let (mut x_zero_total, mut y_zero_total): (u16, u16) = (0, 0);
                for _n in 0..10 {
                    x_zero_total += adc_driver.read(&mut x_adc_channel).unwrap();

                    y_zero_total += adc_driver.read(&mut y_adc_channel).unwrap();
                }

                let calibration = StickCalibration::calibrate(x_zero_total / 10, y_zero_total / 10);

                main_state.to_reading(calibration);
            }
            ReadStates::Reading(calibration) => {
                // TODO: review this approach, maybe it make sense to accumulate reads instead of sending one at a time
                let x_read = adc_driver.read(&mut x_adc_channel).unwrap();

                let y_read = adc_driver.read(&mut y_adc_channel).unwrap();

                let mut mouse_read: jojo_common::mouse::MouseRead =
                    StickRead::new(x_read, y_read, *calibration).into();

                let mouse_config = jojo_common::mouse::MouseConfig::default();

                mouse_read = jojo_common::mouse::MouseRead::new(
                    mouse_read.x_read() * i32::from(mouse_config.x_sen()),
                    mouse_read.y_read() * i32::from(mouse_config.y_sen()),
                );

                if mouse_read.x_read() != 0 || mouse_read.y_read() != 0 {
                    websocket_sender_tx
                        .try_send(ClientMessage::Reads(vec![Reads::new(
                            Some(mouse_read),
                            None,
                        )]))
                        .unwrap();
                }

                // std::thread::sleep(Duration::from_millis(20));
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
