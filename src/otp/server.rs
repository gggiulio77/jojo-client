use std::sync::Arc;

use esp_idf_hal::delay::FreeRtos;
use esp_idf_svc::http::Method;
use esp_idf_svc::{
    http::server::{
        fn_handler, Configuration as HttpServerConfiguration, Connection, EspHttpConnection,
        EspHttpServer, Handler, HandlerError, HandlerResult, Middleware, Request,
    },
    io::{utils, Write},
};
use log::*;
use parking_lot::{Condvar, Mutex};
use serde_json::json;

use super::wifi_otp;

pub struct ServerTask {
    wifi_status: Arc<(Mutex<bool>, Condvar)>,
    wifi_tx: crossbeam_channel::Sender<wifi_otp::ScanMessage>,
    server_rx: crossbeam_channel::Receiver<wifi_otp::ScanMessage>,
    nvs_tx: crossbeam_channel::Sender<jojo_common::network::NetworkCredentials>,
}

impl ServerTask {
    pub fn new(
        wifi_status: Arc<(Mutex<bool>, Condvar)>,
        wifi_tx: crossbeam_channel::Sender<wifi_otp::ScanMessage>,
        server_rx: crossbeam_channel::Receiver<wifi_otp::ScanMessage>,
        nvs_tx: crossbeam_channel::Sender<jojo_common::network::NetworkCredentials>,
    ) -> Self {
        ServerTask {
            wifi_status,
            wifi_tx,
            server_rx,
            nvs_tx,
        }
    }
}

pub struct ErrorMiddleware {}

impl<C> Middleware<C> for ErrorMiddleware
where
    C: Connection,
{
    fn handle<'a, H>(&'a self, connection: &'a mut C, handler: &'a H) -> HandlerResult
    where
        H: Handler<C>,
    {
        let req = Request::wrap(connection);

        info!("ErrorMiddleware called with uri: {}", req.uri());

        let connection = req.release();

        if let Err(err) = handler.handle(connection) {
            if !connection.is_response_initiated() {
                let mut resp = Request::wrap(connection).into_response(
                    500,
                    None,
                    &[("Content-type", "application/json")],
                )?;
                let error = json!({ "error": err.to_string() });
                write!(&mut resp, "{error}")?;
            } else {
                // Nothing can be done as the error happened after the response was initiated, propagate further
                return Err(err);
            }
        }

        Ok(())
    }
}

fn health(request: Request<&mut EspHttpConnection>) -> Result<(), HandlerError> {
    let mut response = request.into_response(
        200,
        None,
        &[
            ("Content-Type", "application/json"),
            ("Cache-Control", "no-cache"),
        ],
    )?;

    // TODO: make HealthResponse struct
    let body = json!({ "status": "OK"});

    response.write_all(body.to_string().as_bytes())?;

    Ok(())
}

fn scan(
    request: Request<&mut EspHttpConnection>,
    wifi_tx: &crossbeam_channel::Sender<wifi_otp::ScanMessage>,
    server_rx: &crossbeam_channel::Receiver<wifi_otp::ScanMessage>,
) -> Result<(), HandlerError> {
    let mut response = request.into_response(
        200,
        None,
        &[
            ("Content-Type", "application/json"),
            ("Cache-Control", "no-cache"),
        ],
    )?;

    wifi_tx.try_send(wifi_otp::ScanMessage::Request).unwrap();

    let mut buffer_scan_result: Vec<jojo_common::network::Ssid> = Vec::new();

    loop {
        if let Ok(message) = server_rx.try_recv() {
            if let wifi_otp::ScanMessage::Response(scan_result) = message {
                buffer_scan_result.append(&mut scan_result.clone().try_into().unwrap());
                break;
            }
            FreeRtos::delay_ms(100);
        };
    }

    let body = jojo_common::otp::ScanResponse::new(buffer_scan_result);

    info!("[server_task]:Response:{:?}", body);

    response.write_all(serde_json::to_string(&body)?.as_bytes())?;

    Ok(())
}

fn save_credentials(
    mut request: Request<&mut EspHttpConnection>,
    nvs_tx: &crossbeam_channel::Sender<jojo_common::network::NetworkCredentials>,
) -> Result<(), HandlerError> {
    let (_, mut body) = request.split();

    let mut buf = [0u8; 512];
    let bytes_read = utils::try_read_full(&mut body, &mut buf).map_err(|e| e.0)?;

    let network_credentials: jojo_common::network::NetworkCredentials =
        serde_json::from_slice(&buf[0..bytes_read])?;

    nvs_tx.send(network_credentials)?;

    // TODO: think a way to validate the nvs task has write flash, maybe a condvar or another channel

    request.into_ok_response()?;

    Ok(())
}

pub fn init_task(task: ServerTask) {
    let ServerTask {
        wifi_status,
        wifi_tx,
        server_rx,
        nvs_tx,
    } = task;

    let (lock, cvar) = &*wifi_status;

    let mut started = lock.lock();

    if !*started {
        cvar.wait(&mut started);
    }
    drop(started);

    info!("[server_task]:creating");
    let mut server = EspHttpServer::new(&HttpServerConfiguration::default()).unwrap();

    server
        .handler(
            "/health",
            Method::Get,
            ErrorMiddleware {}.compose(fn_handler(health)),
        )
        .unwrap();

    server
        .handler(
            "/scan",
            Method::Get,
            ErrorMiddleware {}.compose(fn_handler(move |request| {
                scan(request, &wifi_tx, &server_rx)
            })),
        )
        .unwrap();

    server
        .handler(
            "/save_credentials",
            Method::Post,
            ErrorMiddleware {}.compose(fn_handler(move |request| {
                save_credentials(request, &nvs_tx)
            })),
        )
        .unwrap();

    // TODO: think about adding a restart endpoint

    loop {
        FreeRtos::delay_ms(1000);
    }
}
