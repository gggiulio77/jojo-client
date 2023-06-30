use std::{net::TcpStream, sync::Arc, time::Duration};

use embedded_websocket::{
    framer::{Framer, FramerError},
    WebSocketClient, WebSocketOptions, WebSocketSendMessageType,
};
use parking_lot::{Condvar, Mutex};

pub struct WebsocketTask<'a> {
    pub address: &'a str,
    pub wifi_status: Arc<(Mutex<bool>, Condvar)>,
    pub bt_rx: crossbeam_channel::Receiver<bool>,
}
use log::*;
use url::Url;

pub fn init_task(task: WebsocketTask) {
    let WebsocketTask {
        address,
        wifi_status,
        bt_rx,
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

    let mut cont = 0;

    loop {
        if let Ok(_) = bt_rx.try_recv() {
            cont += 1;
            let message = format!("msg n = {cont}");
            framer
                .write(
                    &mut stream,
                    WebSocketSendMessageType::Text,
                    true,
                    message.as_bytes(),
                )
                .unwrap();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
