use std::{net::TcpStream, sync::Arc};

use log::*;
use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Serialize};
use tungstenite::{client, Message};
use url::Url;

pub struct WebsocketTask<'a> {
    address: &'a str,
    wifi_status: Arc<(Mutex<bool>, Condvar)>,
    stick_rx: crossbeam_channel::Receiver<MouseRead>,
}

impl<'a> WebsocketTask<'a> {
    pub fn new(
        address: &'a str,
        wifi_status: Arc<(Mutex<bool>, Condvar)>,
        stick_rx: crossbeam_channel::Receiver<MouseRead>,
    ) -> Self {
        WebsocketTask {
            address,
            wifi_status,
            stick_rx,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
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
    let host = parsed_url.host_str().unwrap();
    let port = parsed_url.port().unwrap();

    info!("[websocket_task]:Connecting to: {:?}", address);
    let stream = TcpStream::connect(format!("{}:{}", host, port)).unwrap();

    let (mut socket, _response) = client(parsed_url, stream).unwrap();

    info!("[websocket_task]:Connected.");

    loop {
        // TODO: review this approach, it may consume a lot of network bandwidth and power
        // TODO:
        let mouse_reads: Vec<MouseRead> = stick_rx.try_iter().collect();

        socket
            .write_message(Message::Binary(bincode::serialize(&mouse_reads).unwrap()))
            .unwrap();
    }
}
