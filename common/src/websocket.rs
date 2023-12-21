use esp_idf_hal::sys::esp_restart;
use esp_idf_svc::nvs::{EspNvs, NvsDefault};
use jojo_common::{device::Device, message::ServerMessage};
use log::*;
use parking_lot::{Condvar, Mutex};
use std::{
    net::{SocketAddr, TcpStream},
    sync::Arc,
    time::Duration,
};

use tungstenite::{client::client_with_config, protocol::WebSocketConfig, Message, WebSocket};

use crate::{DEVICE_TAG, NETWORK_TAG};

pub struct WebsocketTask<'a> {
    path: &'a str,
    discovery_rx: crossbeam_channel::Receiver<SocketAddr>,
    websocket_sender_rx: crossbeam_channel::Receiver<jojo_common::message::ClientMessage>,
    status: Arc<(Mutex<bool>, Condvar)>,
    device: Device,
    nvs_namespace: EspNvs<NvsDefault>,
}

impl<'a> WebsocketTask<'a> {
    pub fn new(
        path: &'a str,
        discovery_rx: crossbeam_channel::Receiver<SocketAddr>,
        websocket_sender_rx: crossbeam_channel::Receiver<jojo_common::message::ClientMessage>,
        status: Arc<(Mutex<bool>, Condvar)>,
        device: Device,
        nvs_namespace: EspNvs<NvsDefault>,
    ) -> Self {
        WebsocketTask {
            path,
            discovery_rx,
            websocket_sender_rx,
            status,
            device,
            nvs_namespace,
        }
    }
}

// TODO: try a crate for states machines
#[derive(Debug)]
struct WebsocketState {
    pub current_state: WebsocketStates,
}

impl Default for WebsocketState {
    fn default() -> Self {
        WebsocketState::new(WebsocketStates::Discovery)
    }
}

impl WebsocketState {
    pub fn new(init_state: WebsocketStates) -> Self {
        WebsocketState {
            current_state: init_state,
        }
    }

    fn to_state(&mut self, state: WebsocketStates) {
        self.current_state = state;
    }

    pub fn _to_discovery(&mut self) {
        info!("[websocket_task]:state_discovery");
        self.to_state(WebsocketStates::Discovery);
    }

    pub fn to_init(&mut self, ip_address: SocketAddr) {
        info!("[websocket_task]:state_init");
        self.to_state(WebsocketStates::Init(ip_address));
    }

    pub fn to_connected(&mut self, socket: WebSocket<TcpStream>) {
        info!("[websocket_task]:state_connecting");
        self.to_state(WebsocketStates::Connected(socket));
    }
}

#[derive(Debug)]
enum WebsocketStates {
    Discovery,
    Init(SocketAddr),
    Connected(WebSocket<TcpStream>),
}

pub fn init_task(task: WebsocketTask) {
    let WebsocketTask {
        path,
        discovery_rx,
        websocket_sender_rx,
        status,
        device,
        mut nvs_namespace,
    } = task;

    let mut main_state = WebsocketState::default();
    loop {
        // TODO: remove current_state access, replace with something else
        match main_state.current_state {
            WebsocketStates::Discovery => 'label: {
                if let Ok(ip_address) = discovery_rx.try_recv() {
                    main_state.to_init(ip_address);
                    break 'label;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            WebsocketStates::Init(server_address) => {
                info!(
                    "[websocket_task]:Connecting to: {}/{}/340917e8-87a9-455c-9645-d08eb99162f9",
                    server_address, path
                );

                let stream = TcpStream::connect(server_address).unwrap();
                // TODO: review this approach for non blocking reads and write to the stream. If we use it, an HandshakeError::Interrupted() is thrown
                // stream.set_nonblocking(true).unwrap();

                // This let us reduce the "blocking" while reading from websocket
                // TODO: review this value, i think a low value creates a INTERRUPTED:HANDSHAKE error
                stream
                    .set_read_timeout(Some(Duration::from_millis(25)))
                    .unwrap();

                let (socket, _broadcast_discovery) = client_with_config(
                    &format!("ws://{}/{}/{}", server_address, path, device.id()),
                    stream,
                    Some(WebSocketConfig {
                        write_buffer_size: 1024,
                        max_message_size: Some(2048),
                        max_write_buffer_size: 2048,
                        max_frame_size: Some(2048),
                        accept_unmasked_frames: false,
                        ..WebSocketConfig::default()
                    }),
                )
                .unwrap();

                std::thread::sleep(Duration::from_millis(500));

                main_state.to_connected(socket);
            }
            WebsocketStates::Connected(socket) => {
                let socket_tx = Arc::new(parking_lot::Mutex::new(socket));
                let socket_rx = socket_tx.clone();
                let clone_device = device.clone();

                {
                    info!("[websocket_task]:Sending Device");

                    let message = jojo_common::message::ClientMessage::Device(clone_device);

                    socket_tx
                        .lock()
                        .send(Message::Binary(bincode::serialize(&message).unwrap()))
                        .unwrap();
                }

                info!("[websocket_task]: init read task");

                // Task to read from websocket
                let _ = std::thread::Builder::new()
                    .name("wb_rx".into())
                    .stack_size(8 * 1024)
                    .spawn(move || loop {
                        if let Some(mut socket) = socket_rx.try_lock() {
                            if let Ok(message) = socket.read() {
                                // info!("[websocket_task]:Rx: {:?}", message);
                                message_handler(message, &device, &mut nvs_namespace);
                                socket.flush().unwrap();
                            }
                        }
                        std::thread::sleep(Duration::from_millis(2500));
                    })
                    .unwrap();

                info!("[websocket_task]: init write task");

                std::thread::Builder::new()
                    .name("wb_tx".into())
                    .stack_size(14 * 1024)
                    .spawn(move || loop {
                        if let Ok(message) = websocket_sender_rx.try_recv() {
                            if let Some(mut socket) = socket_tx.try_lock() {
                                socket
                                    .send(Message::Binary(bincode::serialize(&message).unwrap()))
                                    .unwrap()
                            }
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    })
                    .unwrap();

                std::thread::sleep(Duration::from_millis(500));

                let (lock, cvar) = &*status;
                // Write value to mutex
                *lock.lock() = true;
                cvar.notify_all();
                drop(status);

                loop {
                    std::thread::sleep(Duration::from_millis(1000));
                }
            }
        }
    }
}

pub fn message_handler(
    wb_message: Message,
    device: &Device,
    nvs_namespace: &mut EspNvs<NvsDefault>,
) {
    match wb_message {
        Message::Binary(server_message) => {
            let Ok(message) = bincode::deserialize::<ServerMessage>(&server_message) else {
                error!(
                    "[message_handler]: {:?} : cannot deserialize message",
                    server_message
                );
                return;
            };

            match message {
                ServerMessage::UpdateDevice(_, button_actions) => {
                    // TODO: we need to update flash with this new actions_map. We need to check if all buttons has actions to not break things.
                    // TODO: we can create a channel to communicate with a task owner of flash to update it or maybe pass the nvs handler to this task.
                    let mut new_device = device.clone();
                    let mut new_actions_map = device.actions_map().clone();

                    new_actions_map.extend(button_actions);
                    new_device.set_actions_map(new_actions_map);

                    info!("[message_handler::UpdateDevice]: updating flash with new device");

                    nvs_namespace
                        .set_raw(
                            DEVICE_TAG,
                            &bincode::serialize(&new_device)
                                .expect("[message_handler::UpdateDevice]: cannot serialize device"),
                        )
                        .expect(
                            "[message_handler::UpdateDevice]: a problem occur while writing flash",
                        );

                    // TODO: we need to restart because we need to create all button tasks again, maybe we can find a way to avoid it
                    // maybe we can use a channel to trigger task creation, the problem is how to erase previous tasks from memory
                    info!("[message_handler::UpdateDevice]: restarting device");

                    // TODO: find a more secure way to restart (the wifi driver sometimes not work on the restart)
                    unsafe {
                        esp_restart();
                    }
                }
                ServerMessage::RestartDevice(_) => {
                    // TODO: restart device
                    info!("[message_handler::RestartDevice]: restarting device");
                    unsafe {
                        esp_restart();
                    }
                }
                ServerMessage::ClearCredentials(_) => {
                    info!("[message_handler::ClearCredentials]: erasing network credentials");
                    nvs_namespace.remove(NETWORK_TAG).unwrap();
                    info!("[message_handler::ClearCredentials]: restarting device");
                    unsafe {
                        esp_restart();
                    }
                }
            }
        }
        _ => info!("[message_handler]: {:?}", wb_message),
    }
}
