use std::{sync::Arc, time::Duration};

use esp_idf_hal::{adc::*, gpio::ADCPin};
use jojo_common::message::ClientMessage;
use log::*;
use parking_lot::{Condvar, Mutex};

pub struct AxisTask<'a, P: ADCPin<Adc = ADC1>> {
    adc_driver: Arc<Mutex<AdcDriver<'a, ADC1>>>,
    // TODO: replace with a generic, restricted to ADC1 GPIOs
    axis_channel_driver: AdcChannelDriver<'a, { attenuation::DB_11 }, P>,
    axis: jojo_common::gamepad::Axis,
    websocket_sender_tx: crossbeam_channel::Sender<jojo_common::message::ClientMessage>,
    wb_status: Arc<(Mutex<bool>, Condvar)>,
}

impl<'a, P: ADCPin<Adc = ADC1>> AxisTask<'a, P> {
    pub fn new(
        adc_driver: Arc<Mutex<AdcDriver<'a, ADC1>>>,
        axis_channel_driver: AdcChannelDriver<'a, { attenuation::DB_11 }, P>,
        axis: jojo_common::gamepad::Axis,
        // TODO: replace with stick_websocket_sender_tx and websocket Message Reads
        websocket_sender_tx: crossbeam_channel::Sender<jojo_common::message::ClientMessage>,
        wb_status: Arc<(Mutex<bool>, Condvar)>,
    ) -> Self {
        AxisTask {
            adc_driver,
            axis_channel_driver,
            axis,
            websocket_sender_tx,
            wb_status,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AxisCalibration {
    b: i32,
    m: f32,
}

impl AxisCalibration {
    pub fn new(b: i32, m: f32) -> Self {
        AxisCalibration { b, m }
    }
}

// TODO: maybe replace this with a function or a trait?
#[derive(Debug, Clone, Copy)]
pub struct AxisRead {
    read: u16,
    axis: jojo_common::gamepad::Axis,
    // TODO: generalize to all mouse buttons
    calibration: AxisCalibration,
}

impl AxisRead {
    pub fn new(read: u16, axis: jojo_common::gamepad::Axis, calibration: AxisCalibration) -> Self {
        AxisRead {
            read,
            axis,
            calibration,
        }
    }
}

impl Into<jojo_common::gamepad::AxisRead> for AxisRead {
    // type Error = anyhow::Error;

    fn into(self) -> jojo_common::gamepad::AxisRead {
        let AxisCalibration { b, m } = self.calibration;

        let read = (m * self.read as f32).round() as i32 + b;

        jojo_common::gamepad::AxisRead::new(self.axis, read)
    }
}

enum ReadStates {
    Calibrating,
    Reading(AxisCalibration),
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
        info!("[axis_task]:state_calibrating");
        self.to_state(ReadStates::Calibrating)
    }

    pub fn to_reading(&mut self, calibration: AxisCalibration) -> &Self {
        info!("[axis_task]:state_reading");
        self.to_state(ReadStates::Reading(calibration))
    }
}

const COMPARE_STEP: u16 = 500;
const ADC_MAX_READ: u16 = 3000;

pub fn init_task(task: AxisTask<impl ADCPin<Adc = ADC1>>) {
    let AxisTask {
        adc_driver,
        mut axis_channel_driver,
        axis,
        websocket_sender_tx,
        wb_status,
    } = task;

    info!("[axis_task]:creating");
    // TODO: move adc driver to a Mutex static or and Arc<Mutex>, we need it in axis, hat and stick

    let (lock, cvar) = &*wb_status;

    let mut started = lock.lock();

    if !*started {
        cvar.wait(&mut started);
    }
    drop(started);

    let mut main_state = ReadState::default();
    let mut last_read: u16 = 0;

    loop {
        match main_state.state() {
            ReadStates::Calibrating => {
                let calibration = AxisCalibration::new(0, (i16::MAX / ADC_MAX_READ as i16).into());

                last_read = adc_driver.lock().read(&mut axis_channel_driver).unwrap();

                main_state.to_reading(calibration);
            }
            ReadStates::Reading(calibration) => {
                // TODO: maybe we can store the last reading in ReadState instead of mutate an external variable
                let read = adc_driver.lock().read(&mut axis_channel_driver).unwrap();

                let axis_read: jojo_common::gamepad::AxisRead =
                    AxisRead::new(read, axis, *calibration).into();
                let jojo_common::gamepad::AxisRead(_, last_axis_read) =
                    AxisRead::new(last_read, axis, *calibration).into();

                // We want to compare the difference between last and actual read to send less information. The idea es tu send steps.
                if last_axis_read.abs_diff(axis_read.1) > COMPARE_STEP.into() {
                    last_read = read;

                    websocket_sender_tx
                        .try_send(ClientMessage::AxisRead(axis_read))
                        .unwrap();
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
