use std::sync::Arc;

use bus::BusReader;
use esp_idf_hal::{
    adc::{self, *},
    delay::FreeRtos,
    gpio::{AnyIOPin, Gpio5, Gpio6, Gpio7, IOPin, Input, PinDriver, Pull},
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
    // TODO: add click gpio
    gpio_click: Gpio7,
    stick_tx: crossbeam_channel::Sender<MouseRead>,
    bt_rx: BusReader<bool>,
    wifi_status: Arc<(Mutex<bool>, Condvar)>,
}

impl StickTask {
    pub fn new(
        adc1: ADC1,
        gpio_x: Gpio5,
        gpio_y: Gpio6,
        gpio_click: Gpio7,
        stick_tx: crossbeam_channel::Sender<MouseRead>,
        bt_rx: BusReader<bool>,
        wifi_status: Arc<(Mutex<bool>, Condvar)>,
    ) -> Self {
        StickTask {
            adc1,
            gpio_x,
            gpio_y,
            gpio_click,
            stick_tx,
            bt_rx,
            wifi_status,
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
    click_read: bool,
    calibration: StickCalibration,
}

impl StickRead {
    pub fn new(x_read: u16, y_read: u16, click_read: bool, calibration: StickCalibration) -> Self {
        StickRead {
            x_read,
            y_read,
            click_read,
            calibration,
        }
    }
}

impl Into<MouseRead> for StickRead {
    // type Error = anyhow::Error;

    fn into(self) -> MouseRead {
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

        MouseRead::new(x_read, y_read, self.click_read)
    }
}

fn init_button(btn_pin: Gpio7) -> anyhow::Result<PinDriver<'static, AnyIOPin, Input>> {
    // Config pin
    let mut btn = PinDriver::input(btn_pin.downgrade())?;
    btn.set_pull(Pull::Up)?;

    return Ok(btn);
}

enum ReadStates {
    Calibrating,
    Reading(StickCalibration),
    Paused,
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

    pub fn to_calibrating(&mut self) -> &Self {
        info!("[stick_task]:state_calibrating");
        self.to_state(ReadStates::Calibrating)
    }

    pub fn to_reading(&mut self, calibration: StickCalibration) -> &Self {
        info!("[stick_task]:state_reading");
        self.to_state(ReadStates::Reading(calibration))
    }

    pub fn to_paused(&mut self) -> &Self {
        info!("[stick_task]:state_pausing");
        self.to_state(ReadStates::Paused)
    }
}

pub fn init_task(task: StickTask) {
    let StickTask {
        adc1,
        gpio_x,
        gpio_y,
        gpio_click,
        stick_tx,
        mut bt_rx,
        wifi_status,
    } = task;

    info!("[stick_task]:creating");
    let mut adc_driver =
        AdcDriver::new(adc1, &adc::config::Config::new().calibration(true)).unwrap();

    let mut x_adc_channel: AdcChannelDriver<'_, Gpio5, Atten11dB<ADC1>> =
        AdcChannelDriver::<_, Atten11dB<ADC1>>::new(gpio_x).unwrap();

    let mut y_adc_channel = AdcChannelDriver::<_, Atten11dB<ADC1>>::new(gpio_y).unwrap();

    let click_btn = init_button(gpio_click).unwrap();

    let (lock, cvar) = &*wifi_status;

    let mut started = lock.lock();

    if !*started {
        cvar.wait(&mut started);
    }
    drop(started);

    // TODO: generalize to all mouse buttons states
    let mut click_state = true;

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
            ReadStates::Reading(calibration) => 'label: {
                if let Ok(_) = bt_rx.try_recv() {
                    info!("Pausing Stick");
                    main_state.to_paused();
                    break 'label;
                }

                // TODO: review this approach, maybe it make sense to accumulate reads instead of sending one at a time
                let x_read = adc_driver.read(&mut x_adc_channel).unwrap();

                let y_read = adc_driver.read(&mut y_adc_channel).unwrap();

                let click_read = click_btn.is_high();

                let mouse_read: MouseRead =
                    StickRead::new(x_read, y_read, click_read, *calibration).into();

                let (x_read, y_read, _) = mouse_read.reads();

                if x_read != 0 || y_read != 0 || click_read != click_state {
                    click_state = click_read;

                    stick_tx.try_send(mouse_read).unwrap();
                }

                FreeRtos::delay_ms(20);
            }
            ReadStates::Paused => {
                if let Ok(_) = bt_rx.try_recv() {
                    main_state.to_calibrating();
                }
                FreeRtos::delay_ms(500);
            }
        }
    }
}
