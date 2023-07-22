use std::net::{SocketAddr, TcpStream};

use bus::BusReader;
use esp_idf_hal::delay::FreeRtos;
use log::*;

use serde::{Deserialize, Serialize};
use tungstenite::{client, Message, WebSocket};
use url::Url;

pub struct WebsocketTask<'a> {
    address: &'a str,
    discovery_rx: crossbeam_channel::Receiver<SocketAddr>,
    stick_rx: crossbeam_channel::Receiver<MouseRead>,
    bt_rx: BusReader<bool>,
}

impl<'a> WebsocketTask<'a> {
    pub fn new(
        address: &'a str,
        discovery_rx: crossbeam_channel::Receiver<SocketAddr>,
        stick_rx: crossbeam_channel::Receiver<MouseRead>,
        bt_rx: BusReader<bool>,
    ) -> Self {
        WebsocketTask {
            address,
            discovery_rx,
            stick_rx,
            bt_rx,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
pub struct MouseRead {
    x_read: i32,
    y_read: i32,
    // TODO: generalize to all mouse buttons
    click_read: bool,
}

impl MouseRead {
    pub fn new(x_read: i32, y_read: i32, click_read: bool) -> Self {
        MouseRead {
            x_read,
            y_read,
            click_read,
        }
    }

    pub fn reads(self) -> (i32, i32, bool) {
        (self.x_read, self.y_read, self.click_read)
    }
}

// TODO: try a crate for states machines
struct WebsocketState<T> {
    pub current_state: WebsocketStates<T>,
}

impl<T> Default for WebsocketState<T> {
    fn default() -> Self {
        WebsocketState::new(WebsocketStates::Discovery)
    }
}

impl<'a, T> WebsocketState<T> {
    pub fn new(init_state: WebsocketStates<T>) -> Self {
        WebsocketState {
            current_state: init_state,
        }
    }

    fn to_state(&mut self, state: WebsocketStates<T>) {
        self.current_state = state;
    }

    pub fn to_discovery(&mut self) {
        info!("[websocket_task]:state_discovery");
        self.to_state(WebsocketStates::Discovery);
    }

    pub fn to_init(&mut self, ip_address: SocketAddr) {
        info!("[websocket_task]:state_init");
        self.to_state(WebsocketStates::Init(ip_address));
    }

    pub fn to_connected(&mut self, socket: WebSocket<T>, ip_address: SocketAddr) {
        info!("[websocket_task]:state_connecting");
        self.to_state(WebsocketStates::Connected(socket, ip_address));
    }

    pub fn to_paused(&mut self, ip_address: SocketAddr) {
        info!("[websocket_task]:state_paused");
        self.to_state(WebsocketStates::Paused(ip_address));
    }
}

enum WebsocketStates<T> {
    Discovery,
    Init(SocketAddr),
    // TODO: remove SocketAddr
    Connected(WebSocket<T>, SocketAddr),
    // TODO: remove SocketAddr
    Paused(SocketAddr),
}

pub fn init_task(task: WebsocketTask) {
    let WebsocketTask {
        address,
        discovery_rx,
        stick_rx,
        mut bt_rx,
    } = task;

    // Parsing url
    let parsed_url = Url::parse(address).unwrap();
    let _host = parsed_url.host_str().unwrap();
    let port = parsed_url.port().unwrap();

    let mut main_state: WebsocketState<TcpStream> = WebsocketState::default();

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
            WebsocketStates::Init(ip_address) => {
                info!(
                    "[websocket_task]:Connecting to: {:?}:{:?}",
                    ip_address.ip(),
                    port
                );
                let stream = TcpStream::connect(format!("{}:{}", ip_address.ip(), port)).unwrap();

                let (socket, _response) = client(&parsed_url, stream).unwrap();

                info!("[websocket_task]:Connected.");

                main_state.to_connected(socket, ip_address);
            }
            WebsocketStates::Connected(ref mut socket, ip_address) => 'label: {
                if let Ok(_) = bt_rx.try_recv() {
                    socket.write_message(Message::Close(None)).unwrap();
                    drop(socket);
                    main_state.to_paused(ip_address);
                    break 'label;
                }

                // Low bandwidth mode can be achieved by not sending empty messages but affects the "flow" feel
                let mouse_reads: Vec<MouseRead> = stick_rx.try_iter().collect();

                socket
                    .write_message(Message::Binary(bincode::serialize(&mouse_reads).unwrap()))
                    .unwrap();
            }
            WebsocketStates::Paused(ip_address) => 'label: {
                // TODO: to this to work we need to pause the ADC, for this we need to signal with stick_tx the STOP and also RESUME
                if let Ok(_) = bt_rx.try_recv() {
                    // TODO: when we have a method to restart the discovery task replace this with to_discovery
                    main_state.to_init(ip_address);
                    break 'label;
                }
                FreeRtos::delay_ms(500);
            }
        }
    }
}
