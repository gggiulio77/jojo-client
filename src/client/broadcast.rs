use std::{
    net::{SocketAddr, UdpSocket},
    sync::Arc,
};

use esp_idf_hal::delay::FreeRtos;
use log::*;
use parking_lot::{Condvar, Mutex};

pub struct BroadcastTask {
    wifi_status: Arc<(Mutex<bool>, Condvar)>,
    discovery_tx: crossbeam_channel::Sender<SocketAddr>,
}

impl BroadcastTask {
    pub fn new(
        wifi_status: Arc<(Mutex<bool>, Condvar)>,
        discovery_tx: crossbeam_channel::Sender<SocketAddr>,
    ) -> Self {
        BroadcastTask {
            wifi_status,
            discovery_tx,
        }
    }
}

const BIND_ADDRESS: &'static str = "0.0.0.0:0";
const BROADCAST_ADDRESS: &'static str = "192.168.0.255:11430";

pub fn init_task(task: BroadcastTask) -> anyhow::Result<()> {
    let BroadcastTask {
        wifi_status,
        discovery_tx,
    } = task;

    // TODO: convert this task to a machine state that can be re allocated, maybe two broadcast to be M:M

    let (wifi_lock, wifi_cvar) = &*wifi_status;

    let mut started = wifi_lock.lock();

    if !*started {
        wifi_cvar.wait(&mut started);
    }
    drop(started);

    // TODO: make a multicast alternative
    let socket = UdpSocket::bind(BIND_ADDRESS).unwrap();
    socket.set_broadcast(true).unwrap();

    info!("[discovery_task]: Listening to {:?}", socket.local_addr());

    let socket_arc = Arc::new(socket);
    let socket_sender = socket_arc.clone();

    let (sender_tx, sender_rx) = crossbeam_channel::unbounded::<bool>();

    let _ = std::thread::Builder::new()
        .stack_size(2 * 1024)
        .spawn(move || loop {
            if let Ok(_) = sender_rx.try_recv() {
                info!("[sender_task]: ending task");
                return;
            }
            info!("[sender_task]: sending udp packet");
            socket_sender
                .send_to("hello".as_bytes(), BROADCAST_ADDRESS)
                .unwrap();

            FreeRtos::delay_ms(1000);
        })
        .unwrap();

    let mut buffer: [u8; 512] = [0; 512];
    info!("[discovery_task]: waiting for message");

    let (n_bytes, ip_address) = socket_arc.recv_from(&mut buffer).unwrap();
    let message = std::str::from_utf8(&buffer[0..n_bytes]);
    info!("Receive: {:?}, from: {:?}", message, ip_address);

    info!("[discovery_task]: dropping sender_task");
    sender_tx.send(true).unwrap();
    drop(socket_arc);

    discovery_tx.send(ip_address).unwrap();

    info!("[discovery_task]: dropping");

    Ok(())
}
