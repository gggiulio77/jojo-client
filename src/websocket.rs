use std::{net::TcpStream, sync::Arc, time::Duration};

use embedded_websocket::{
    framer::{Framer, FramerError},
    WebSocketClient, WebSocketOptions, WebSocketSendMessageType,
};
use log::*;
use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::stick::StickRead;

pub struct WebsocketTask<'a> {
    address: &'a str,
    wifi_status: Arc<(Mutex<bool>, Condvar)>,
    stick_rx: crossbeam_channel::Receiver<StickRead>,
}

impl<'a> WebsocketTask<'a> {
    pub fn new(
        address: &'a str,
        wifi_status: Arc<(Mutex<bool>, Condvar)>,
        stick_rx: crossbeam_channel::Receiver<StickRead>,
    ) -> Self {
        WebsocketTask {
            address,
            wifi_status,
            stick_rx,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct MouseRead {
    x_read: i32,
    y_read: i32,
}

impl MouseRead {
    pub fn new(x_read: i32, y_read: i32) -> Self {
        MouseRead { x_read, y_read }
    }
}

pub fn init_task(task: WebsocketTask) {
    let WebsocketTask {
        address,
        wifi_status,
        stick_rx,
    } = task;

    let (lock, cvar) = &*wifi_status;

    let mut started = lock.lock();

    if !*started {
        cvar.wait(&mut started);
    }
    drop(started);

    // Parsing url
    let parsed_url = Url::parse(address).unwrap();
    let scheme = parsed_url.scheme();
    let host = parsed_url.host_str().unwrap();
    let port = parsed_url.port().unwrap();
    let path = parsed_url.path();
    let origin = format!("{}://{}:{}", scheme, host, port);

    info!("[websocket_task]:Connecting to: {}{}", origin, path);
    let mut stream = TcpStream::connect(format!("{}:{}", host, port))
        .map_err(FramerError::Io)
        .unwrap();
    info!("[websocket_task]:Connected.");

    // initiate a websocket opening handshake
    let options = WebSocketOptions {
        path,
        host,
        origin: &origin,
        sub_protocols: None,
        additional_headers: None,
    };

    // Buffers
    let mut read_buf = [0; 1024];
    let mut read_cursor = 0;
    let mut write_buf = [0; 1024];
    let mut _frame_buf = [0; 1024];
    let mut client = WebSocketClient::new_client(rand::thread_rng());

    let mut framer = Framer::new(&mut read_buf, &mut read_cursor, &mut write_buf, &mut client);

    framer.connect(&mut stream, &options).unwrap();

    loop {
        if let Ok(_read) = stick_rx.try_recv() {
            // TODO: transform StickRead to MouseRead
            let test_read = MouseRead::new(100, 100);
            let message = serde_json::to_string(&test_read).unwrap();
            info!("[websocket_task]:{:?}", message);
            framer
                .write(
                    &mut stream,
                    WebSocketSendMessageType::Text,
                    true,
                    message.as_bytes(),
                )
                .unwrap();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
