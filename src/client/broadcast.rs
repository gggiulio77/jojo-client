use std::{
    net::{SocketAddr, UdpSocket},
    sync::Arc,
    time::Duration,
};

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

const BROADCAST_BIND_ADDRESS: &'static str = env!("BROADCAST_BIND_ADDRESS");
const BROADCAST_ADDRESS: &'static str = env!("BROADCAST_ADDRESS");

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
    let socket = UdpSocket::bind(BROADCAST_BIND_ADDRESS).unwrap();
    socket.set_broadcast(true).unwrap();

    info!("[discovery_task]: Listening to {:?}", socket.local_addr());

    let socket_rx = Arc::new(parking_lot::RwLock::new(socket));
    let socket_tx = socket_rx.clone();

    let (sender_tx, sender_rx) = crossbeam_channel::unbounded::<bool>();

    let _ = std::thread::Builder::new()
        .stack_size(4 * 1024)
        .spawn(move || loop {
            if let Ok(_) = sender_rx.try_recv() {
                info!("[sender_task]: ending task");
                return;
            }
            info!("[sender_task]: sending udp packet");
            socket_tx
                .read()
                .send_to("hello".as_bytes(), BROADCAST_ADDRESS)
                .unwrap();

            std::thread::sleep(Duration::from_millis(1000));
        })
        .unwrap();

    let mut buffer: [u8; 512] = [0; 512];
    info!("[discovery_task]: waiting for message");

    let (n_bytes, server_ip) = socket_rx.read().recv_from(&mut buffer).unwrap();
    let server_port = std::str::from_utf8(&buffer[0..n_bytes])
        .unwrap()
        .parse()
        .unwrap();
    info!(
        "[discovery_task]: Receive: {:?}, from: {:?}",
        server_port, server_ip
    );

    info!("[discovery_task]: dropping sender_task");
    sender_tx.send(true).unwrap();

    discovery_tx
        .send(SocketAddr::new(server_ip.ip(), server_port))
        .unwrap();

    info!("[discovery_task]: dropping");

    Ok(())
}
