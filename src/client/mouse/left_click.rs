use esp_idf_hal::{
    delay::FreeRtos,
    gpio::{AnyIOPin, Gpio7, IOPin, Input, PinDriver, Pull},
};
use jojo_common::{
    button::{Button, ButtonRead},
    message::{ClientMessage, Reads},
    mouse::{MouseButton, MouseButtonState},
};
use log::*;
use parking_lot::{Condvar, Mutex};
use std::sync::Arc;

pub struct LeftClickTask {
    // TODO: add click gpio
    gpio_click: Gpio7,
    websocket_sender_tx: crossbeam_channel::Sender<jojo_common::message::ClientMessage>,
    wifi_status: Arc<(Mutex<bool>, Condvar)>,
}

impl LeftClickTask {
    pub fn new(
        gpio_click: Gpio7,
        // TODO: replace with stick_websocket_sender_tx and websocket Message Reads
        websocket_sender_tx: crossbeam_channel::Sender<jojo_common::message::ClientMessage>,
        wifi_status: Arc<(Mutex<bool>, Condvar)>,
    ) -> Self {
        LeftClickTask {
            gpio_click,
            websocket_sender_tx,
            wifi_status,
        }
    }
}

fn init_button(btn_pin: Gpio7) -> anyhow::Result<PinDriver<'static, AnyIOPin, Input>> {
    // Config pin
    let mut btn = PinDriver::input(btn_pin.downgrade())?;
    btn.set_pull(Pull::Up)?;

    return Ok(btn);
}

#[derive(Debug, Clone, Copy)]
enum ReadStates {
    Reading,
}

#[derive(Debug, Clone, Copy)]
struct ReadState {
    current_state: ReadStates,
    left_click: jojo_common::mouse::MouseButtonState,
}

impl Default for ReadState {
    fn default() -> Self {
        ReadState::new(
            ReadStates::Reading,
            jojo_common::mouse::MouseButtonState::Up,
        )
    }
}

impl ReadState {
    pub fn new(init_state: ReadStates, left_click: jojo_common::mouse::MouseButtonState) -> Self {
        ReadState {
            current_state: init_state,
            left_click,
        }
    }

    pub fn state(&self) -> &ReadStates {
        &self.current_state
    }

    fn to_state(&mut self, state: ReadStates) -> &mut Self {
        self.current_state = state;
        self
    }

    pub fn to_reading(&mut self) -> &Self {
        info!("[stick_task]:state_reading");
        self.to_state(ReadStates::Reading)
    }
}

pub fn init_task(task: LeftClickTask) {
    let LeftClickTask {
        gpio_click,
        websocket_sender_tx,
        wifi_status,
    } = task;

    info!("[stick_task]:creating");

    let click_btn = init_button(gpio_click).unwrap();

    let (lock, cvar) = &*wifi_status;

    let mut started = lock.lock();

    if !*started {
        cvar.wait(&mut started);
    }
    drop(started);

    let mut main_state = ReadState::default();

    loop {
        match main_state.state() {
            ReadStates::Reading => {
                // TODO: review this approach, maybe it make sense to accumulate reads instead of sending one at a time
                let button_read = if click_btn.is_high() {
                    MouseButtonState::Up
                } else {
                    MouseButtonState::Down
                };

                if main_state.left_click != button_read {
                    main_state.left_click = button_read;

                    websocket_sender_tx
                        .try_send(ClientMessage::Reads(vec![Reads::new(
                            None,
                            Some(vec![ButtonRead::new(Button::MouseButton(
                                MouseButton::Left(main_state.left_click),
                            ))]),
                        )]))
                        .unwrap();
                }

                FreeRtos::delay_ms(20);
            }
        }
    }
}
