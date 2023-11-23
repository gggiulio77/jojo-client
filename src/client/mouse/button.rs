use esp_idf_hal::gpio::{AnyIOPin, IOPin, Input, Level, PinDriver, Pull};
use jojo_common::{
    button::{ButtonAction, ButtonMode},
    message::{ClientMessage, Reads},
    mouse::MouseButtonState,
};
use log::*;
use parking_lot::{Condvar, Mutex};
use std::{sync::Arc, time::Duration};

pub struct ButtonTask {
    // TODO: add click gpio
    gpio: AnyIOPin,
    websocket_sender_tx: crossbeam_channel::Sender<jojo_common::message::ClientMessage>,
    wb_status: Arc<(Mutex<bool>, Condvar)>,
    button_actions: Vec<ButtonAction>, // TODO: think about replace this with the action
    button_mode: ButtonMode,
    pull: Pull,
}

impl ButtonTask {
    pub fn new(
        gpio: AnyIOPin, // TODO: test replace Gpio7 with AnyIOPin
        // TODO: replace with gpio7_websocket_sender_tx and websocket Message Reads
        websocket_sender_tx: crossbeam_channel::Sender<jojo_common::message::ClientMessage>,
        wb_status: Arc<(Mutex<bool>, Condvar)>,
        button_actions: Vec<ButtonAction>,
        button_mode: ButtonMode,
        pull: Pull,
    ) -> Self {
        ButtonTask {
            gpio,
            websocket_sender_tx,
            wb_status,
            button_actions,
            button_mode,
            pull,
        }
    }
}

fn init_button(
    btn_pin: AnyIOPin,
    pull: Pull,
) -> anyhow::Result<PinDriver<'static, AnyIOPin, Input>> {
    // Config pin
    let mut btn = PinDriver::input(btn_pin.downgrade())?;
    btn.set_pull(pull)?;

    return Ok(btn);
}

#[derive(Debug, Clone, Copy)]
enum ReadStates {
    Reading,
}

#[derive(Debug, Clone, Copy)]
struct ReadState {
    current_state: ReadStates,
    button_level: Level,
}

impl Default for ReadState {
    fn default() -> Self {
        ReadState::new(ReadStates::Reading, Level::High)
    }
}

impl ReadState {
    pub fn new(init_state: ReadStates, button_level: Level) -> Self {
        ReadState {
            current_state: init_state,
            button_level,
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

// struct LevelExt(Level);
struct MouseButtonStateExt(MouseButtonState);

impl From<Level> for MouseButtonStateExt {
    fn from(value: Level) -> Self {
        match value {
            Level::High => MouseButtonStateExt(MouseButtonState::Up),
            Level::Low => MouseButtonStateExt(MouseButtonState::Down),
        }
    }
}

pub fn init_task(task: ButtonTask) {
    let ButtonTask {
        gpio,
        websocket_sender_tx,
        wb_status,
        button_actions,
        button_mode,
        pull,
    } = task;

    info!("[gpio7_task]:creating");

    let btn = init_button(gpio, pull).unwrap();

    let (lock, cvar) = &*wb_status;

    let mut started = lock.lock();

    if !*started {
        cvar.wait(&mut started);
    }
    drop(started);

    let mut main_state = ReadState::new(
        ReadStates::Reading,
        if pull == Pull::Up {
            Level::High
        } else {
            Level::Low
        },
    );

    // TODO: we need to read what action this button trigger in device dynamically
    loop {
        match main_state.state() {
            ReadStates::Reading => {
                // Reading button level
                let btn_level = btn.get_level();

                let collect: Vec<ButtonAction> = match button_mode {
                    ButtonMode::Hold => {
                        if main_state.button_level == btn_level {
                            vec![]
                        } else {
                            main_state.button_level = btn_level;

                            button_actions
                                .iter()
                                .map(|action| {
                                    // TODO: remove this clone, find a better way to populate collect
                                    if let ButtonAction::MouseButton(button, _) = action {
                                        let MouseButtonStateExt(state) =
                                            main_state.button_level.into();

                                        return ButtonAction::MouseButton(button.to_owned(), state);
                                    } else {
                                        return action.to_owned();
                                    }
                                })
                                .collect()
                        }
                    }
                    ButtonMode::Click => {
                        if main_state.button_level == btn_level {
                            vec![]
                        } else {
                            main_state.button_level = btn_level;
                            match main_state.button_level {
                                Level::Low => button_actions.clone(),
                                Level::High => vec![],
                            }
                        }
                    }
                };

                if collect.len() > 0 {
                    websocket_sender_tx
                        .try_send(ClientMessage::Reads(vec![Reads::new(None, Some(collect))]))
                        .unwrap();
                };

                std::thread::sleep(Duration::from_millis(35));
            }
        }
    }
}
