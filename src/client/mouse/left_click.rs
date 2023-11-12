use esp_idf_hal::gpio::{AnyIOPin, Gpio7, IOPin, Input, PinDriver, Pull};
use jojo_common::{
    button::ButtonAction,
    message::{ClientMessage, Reads},
    mouse::MouseButtonState,
};
use log::*;
use parking_lot::{Condvar, Mutex};
use std::{sync::Arc, time::Duration};

pub struct LeftClickTask {
    // TODO: add click gpio
    gpio: Gpio7,
    websocket_sender_tx: crossbeam_channel::Sender<jojo_common::message::ClientMessage>,
    wb_status: Arc<(Mutex<bool>, Condvar)>,
    button_action: ButtonAction, // TODO: think about replace this with the action
}

impl LeftClickTask {
    pub fn new(
        gpio: Gpio7, // TODO: test replace Gpio7 with AnyIOPin
        // TODO: replace with gpio7_websocket_sender_tx and websocket Message Reads
        websocket_sender_tx: crossbeam_channel::Sender<jojo_common::message::ClientMessage>,
        wb_status: Arc<(Mutex<bool>, Condvar)>,
        button_action: ButtonAction,
    ) -> Self {
        LeftClickTask {
            gpio,
            websocket_sender_tx,
            wb_status,
            button_action,
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
    button_state: jojo_common::mouse::MouseButtonState,
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
    pub fn new(init_state: ReadStates, button_state: jojo_common::mouse::MouseButtonState) -> Self {
        ReadState {
            current_state: init_state,
            button_state,
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
        info!("[gpio7_task]:state_reading");
        self.to_state(ReadStates::Reading)
    }
}

pub fn init_task(task: LeftClickTask) {
    let LeftClickTask {
        gpio,
        websocket_sender_tx,
        wb_status,
        button_action,
    } = task;

    info!("[gpio7_task]:creating");

    let click_btn = init_button(gpio).unwrap();

    let (lock, cvar) = &*wb_status;

    let mut started = lock.lock();

    if !*started {
        cvar.wait(&mut started);
    }
    drop(started);

    let mut main_state = ReadState::default();

    // TODO: we need to read what action this button trigger in device dynamically
    loop {
        match main_state.state() {
            ReadStates::Reading => {
                match button_action {
                    ButtonAction::MouseButton(ref button, state) => {
                        // This flow is for button that use the state of the button, such a mouse click
                        let button_read = if click_btn.is_high() {
                            state.to_owned()
                        } else {
                            MouseButtonState::Down
                        };

                        if main_state.button_state != button_read {
                            main_state.button_state = button_read;

                            websocket_sender_tx
                                .try_send(ClientMessage::Reads(vec![Reads::new(
                                    None,
                                    Some(vec![ButtonAction::MouseButton(
                                        button.to_owned(),
                                        button_read,
                                    )]),
                                )]))
                                .unwrap();
                        }
                    }
                    ButtonAction::KeyboardButton(ref button) => {
                        // This flow is for click actions, we don't care about the button state
                        if click_btn.is_low() {
                            websocket_sender_tx
                                .try_send(ClientMessage::Reads(vec![Reads::new(
                                    None,
                                    Some(vec![ButtonAction::KeyboardButton(button.to_owned())]),
                                )]))
                                .unwrap();
                        };
                    }
                    ButtonAction::CustomButton(_) => todo!(),
                }
                // TODO: maybe we can move this tu a button handler and generalize a STATE_CASE and a CLICK_CASE to manage different logics
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}
