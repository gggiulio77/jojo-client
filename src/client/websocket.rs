use esp_idf_hal::delay::FreeRtos;
use log::*;
use std::{
    net::{SocketAddr, TcpStream},
    sync::Arc,
};
use uuid::uuid;

use tungstenite::{client::client_with_config, protocol::WebSocketConfig, Message, WebSocket};

pub struct WebsocketTask<'a> {
    path: &'a str,
    discovery_rx: crossbeam_channel::Receiver<SocketAddr>,
    websocket_sender_rx: crossbeam_channel::Receiver<jojo_common::message::ClientMessage>,
}

impl<'a> WebsocketTask<'a> {
    pub fn new(
        path: &'a str,
        discovery_rx: crossbeam_channel::Receiver<SocketAddr>,
        websocket_sender_rx: crossbeam_channel::Receiver<jojo_common::message::ClientMessage>,
    ) -> Self {
        WebsocketTask {
            path,
            discovery_rx,
            websocket_sender_rx,
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
                FreeRtos::delay_ms(100);
            }
            WebsocketStates::Init(server_address) => {
                info!(
                    "[websocket_task]:Connecting to: {}/{}",
                    server_address, path
                );

                let stream = TcpStream::connect(server_address).unwrap();

                // let (socket, _response) =
                //     client(&format!("ws://{}/{}", server_address, path), stream).unwrap();
                // TODO: replace uuid with device
                let (socket, _broadcast_discovery) = client_with_config(
                    &format!(
                        "ws://{}/{}/340917e8-87a9-455c-9645-d08eb99162f9",
                        server_address, path
                    ),
                    stream,
                    Some(WebSocketConfig {
                        write_buffer_size: 64,
                        max_message_size: Some(256),
                        max_write_buffer_size: 256,
                        max_frame_size: Some(256),
                        accept_unmasked_frames: true,
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
                // TODO: replace hardcoded device with value living in flash
                let device = jojo_common::device::Device::new(
                    uuid!("340917e8-87a9-455c-9645-d08eb99162f9"),
                    "test".to_string(),
                    None,
                    vec![],
                );
                let message = jojo_common::message::ClientMessage::Device(device);

                socket_tx
                    .lock()
                    .write(Message::Binary(bincode::serialize(&message).unwrap()))
                    .unwrap();

                drop(message);

                info!("[websocket_task]: init read task");

                // Task to read from websocket
                let _ = std::thread::Builder::new()
                    .stack_size(6 * 1024)
                    .spawn(move || loop {
                        // info!("[websocket_task]: reading from websocket");
                        if let Ok(message) = socket_rx.lock().read() {
                            // info!("[websocket_task]:Rx: {:?}", message);
                            message_handler(message)
                        }
                        FreeRtos::delay_ms(750);
                    })
                    .unwrap();

                info!("[websocket_task]: init write task");

                // Task to write to websocket
                let _ = std::thread::Builder::new()
                    .stack_size(6 * 1024)
                    .spawn(move || loop {
                        // info!("[websocket_task]: writing to websocket");
                        if let Ok(reads) = websocket_sender_rx.try_recv() {
                            // info!("[websocket_task]:Tx : {:?}", reads);
                            socket_tx
                                .lock()
                                .write(Message::Binary(bincode::serialize(&reads).unwrap()))
                                .unwrap();
                        } else {
                            // TODO: this is to maintain a "flow" feeling, review why this happen
                            let empty_message = jojo_common::message::ClientMessage::Reads(vec![
                                jojo_common::message::Reads::new(None, None),
                            ]);
                            socket_tx
                                .lock()
                                .write(Message::Binary(bincode::serialize(&empty_message).unwrap()))
                                .unwrap();
                        }
                    })
                    .unwrap();

                loop {
                    FreeRtos::delay_ms(1000);
                }
            }
        }
    }
}

pub fn message_handler(_message: Message) {
    ()
}
