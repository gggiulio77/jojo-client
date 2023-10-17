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
    pub socket: Option<WebSocket<TcpStream>>,
}

impl Default for WebsocketState {
    fn default() -> Self {
        WebsocketState::new(WebsocketStates::Discovery, None)
    }
}

impl WebsocketState {
    pub fn new(init_state: WebsocketStates, socket: Option<WebSocket<TcpStream>>) -> Self {
        WebsocketState {
            current_state: init_state,
            socket,
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

    pub fn to_hello(&mut self) {
        info!("[websocket_task]:state_hello");
        self.to_state(WebsocketStates::Hello);
    }

    pub fn to_connected(&mut self) {
        info!("[websocket_task]:state_connecting");
        self.to_state(WebsocketStates::Connected);
    }
}

#[derive(Debug)]
enum WebsocketStates {
    Discovery,
    Init(SocketAddr),
    Hello,
    Connected,
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

                // Save socket in state
                main_state.socket = Some(socket);

                main_state.to_hello();
            }
            WebsocketStates::Hello => {
                // TODO: send device message to server
                // TODO: replace device with flash device
                info!("[websocket_task]:Sending Device");
                let device = jojo_common::device::Device::new(
                    uuid!("340917e8-87a9-455c-9645-d08eb99162f9"),
                    "test".to_string(),
                    None,
                    vec![],
                );
                let message = jojo_common::message::ClientMessage::Device(device);

                main_state
                    .socket
                    .as_mut()
                    .unwrap()
                    .write(Message::Binary(bincode::serialize(&message).unwrap()))
                    .unwrap();

                main_state.to_connected()
            }
            WebsocketStates::Connected => {
                let socket = main_state.socket.as_mut().unwrap();
                // let test = Arc::new(parking_lot::RwLock::new(socket));
                // let test_clone = test.clone();

                // let _ = std::thread::Builder::new()
                //     .stack_size(2 * 1024)
                //     .spawn(move || loop {
                //         if let Ok(message) = test_clone.read().read() {
                //             info!("[websocket_task]:Rx: {:?}", message);
                //             message_handler(message)
                //         }
                //     })
                //     .unwrap();

                // TODO: use socket.read and send it to message handler
                // Low bandwidth mode can be achieved by not sending empty messages but affects the "flow" feel
                if let Ok(message) = socket.read() {
                    info!("[websocket_task]:Rx: {:?}", message);
                    message_handler(message)
                }
                // TODO: listen to sender_rx and send it witch socket.write
                // TODO: reads must be Reads type from common library
                if let Ok(reads) = websocket_sender_rx.try_recv() {
                    // info!("[websocket_task]:Tx : {:?}", reads);
                    socket
                        .write(Message::Binary(bincode::serialize(&reads).unwrap()))
                        .unwrap();
                }
            }
        }
    }
}

pub fn message_handler(_message: Message) {
    ()
}
