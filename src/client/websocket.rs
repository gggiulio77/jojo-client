use esp_idf_hal::delay::Ets;
use jojo_common::device::Device;
use log::*;
use parking_lot::{Condvar, Mutex};
use std::{
    net::{SocketAddr, TcpStream},
    sync::Arc,
    time::Duration,
};

use tungstenite::{client::client_with_config, protocol::WebSocketConfig, Message, WebSocket};

pub struct WebsocketTask<'a> {
    path: &'a str,
    discovery_rx: crossbeam_channel::Receiver<SocketAddr>,
    websocket_sender_rx: crossbeam_channel::Receiver<jojo_common::message::ClientMessage>,
    status: Arc<(Mutex<bool>, Condvar)>,
    device: Device,
}

impl<'a> WebsocketTask<'a> {
    pub fn new(
        path: &'a str,
        discovery_rx: crossbeam_channel::Receiver<SocketAddr>,
        websocket_sender_rx: crossbeam_channel::Receiver<jojo_common::message::ClientMessage>,
        status: Arc<(Mutex<bool>, Condvar)>,
        device: Device,
    ) -> Self {
        WebsocketTask {
            path,
            discovery_rx,
            websocket_sender_rx,
            status,
            device,
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
                    .set_read_timeout(Some(Duration::from_millis(40)))
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
                main_state.to_connected(socket);
            }
            WebsocketStates::Connected(socket) => {
                let socket_tx = Arc::new(parking_lot::Mutex::new(socket));
                let socket_rx = socket_tx.clone();

                info!("[websocket_task]:Sending Device");

                let message = jojo_common::message::ClientMessage::Device(device);

                socket_tx
                    .lock()
                    .send(Message::Binary(bincode::serialize(&message).unwrap()))
                    .unwrap();

                drop(message);

                info!("[websocket_task]: init read task");

                // Task to read from websocket
                let _ = std::thread::Builder::new()
                    .stack_size(3 * 1024)
                    .spawn(move || loop {
                        if let Ok(message) = socket_rx.lock().read() {
                            // info!("[websocket_task]:Rx: {:?}", message);
                            message_handler(message)
                        }
                        std::thread::sleep(Duration::from_millis(300));
                    })
                    .unwrap();

                info!("[websocket_task]: init write task");

                let _ = std::thread::Builder::new()
                    .stack_size(6 * 1024)
                    .spawn(move || loop {
                        // info!("[websocket_task]: writing to websocket");
                        if let Ok(reads) = websocket_sender_rx.try_recv() {
                            // info!("[websocket_task]:Tx : {:?}", reads);
                            socket_tx
                                .lock()
                                .send(Message::Binary(bincode::serialize(&reads).unwrap()))
                                .unwrap();
                        } else {
                            // TODO: this is to maintain a "flow" feeling, review why this happen
                            let empty_message = jojo_common::message::ClientMessage::Reads(vec![
                                jojo_common::message::Reads::new(None, None),
                            ]);
                            socket_tx
                                .lock()
                                .send(Message::Binary(bincode::serialize(&empty_message).unwrap()))
                                .unwrap();
                        }
                        // std::thread::sleep(Duration::from_millis(1));
                        Ets::delay_us(250);
                    })
                    .unwrap();

                std::thread::sleep(Duration::from_millis(500));

                let (lock, cvar) = &*status;
                // Write value to mutex
                *lock.lock() = true;
                cvar.notify_all();
                drop(status);

                loop {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
}

pub fn message_handler(message: Message) {
    match message {
        Message::Binary(server_message) => {
            match bincode::deserialize::<jojo_common::message::ServerMessage>(&server_message)
                .unwrap()
            {
                jojo_common::message::ServerMessage::UpdateDevice(actions_map) => {
                    // TODO: we need to update flash with this new actions_map. We need to check if all buttons has actions to not break thins.
                    // TODO: we can create a channel to communicate with a task owner of flash to update it or maybe pass the nvs handler to this task.
                }
            }
        }
        _ => info!("[message_handler]: {:?}", message),
    }
}
