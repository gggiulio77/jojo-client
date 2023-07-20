use std::sync::Arc;

use crate::{led::Neopixel, wifi_otp::ScanMessage};
use embedded_svc::{
    http::server::{Connection, Handler, HandlerError, HandlerResult, Method, Middleware, Request},
    io::Write,
};
use esp_idf_hal::delay::FreeRtos;
use esp_idf_svc::http::server::{
    fn_handler, Configuration as HttpServerConfiguration, EspHttpConnection, EspHttpServer,
};
use log::*;
use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::str;

pub struct ServerTask<'a> {
    wifi_status: Arc<(Mutex<bool>, Condvar)>,
    led: Neopixel<'a>,
    tx_channel: crossbeam_channel::Sender<ScanMessage>,
    rx_channel: crossbeam_channel::Receiver<ScanMessage>,
}

impl<'a> ServerTask<'a> {
    pub fn new(
        wifi_status: Arc<(Mutex<bool>, Condvar)>,
        led: Neopixel<'a>,
        tx_channel: crossbeam_channel::Sender<ScanMessage>,
        rx_channel: crossbeam_channel::Receiver<ScanMessage>,
    ) -> Self {
        ServerTask {
            wifi_status,
            led,
            tx_channel,
            rx_channel,
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

    let body = json!({ "status": "OK"});

    response.write_all(body.to_string().as_bytes())?;

    Ok(())
}

fn scan(
    request: Request<&mut EspHttpConnection>,
    tx_channel: &crossbeam_channel::Sender<ScanMessage>,
    rx_channel: &crossbeam_channel::Receiver<ScanMessage>,
) -> Result<(), HandlerError> {
    let mut response = request.into_response(
        200,
        None,
        &[
            ("Content-Type", "application/json"),
            ("Cache-Control", "no-cache"),
        ],
    )?;

    tx_channel.try_send(ScanMessage::Request).unwrap();

    let mut buffer_scan_result: Vec<heapless::String<32>> = Vec::new();

    loop {
        if let Ok(message) = rx_channel.try_recv() {
            if let ScanMessage::Response(scan_result) = message {
                buffer_scan_result.append(&mut scan_result.clone());
                break;
            }
            FreeRtos::delay_ms(200);
        };
    }

    let body = ScanResponse {
        found_ssid: buffer_scan_result,
    };

    info!("[server_task]:Response:{:?}", body);

    response.write_all(serde_json::to_string(&body)?.as_bytes())?;

    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
struct ScanResponse {
    found_ssid: Vec<heapless::String<32>>,
}

pub fn init_task(task: ServerTask<'static>) {
    let ServerTask {
        wifi_status,
        led: _,
        tx_channel,
        rx_channel,
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
                scan(request, &tx_channel, &rx_channel)
            })),
        )
        .unwrap();

    loop {
        FreeRtos::delay_ms(1000);
    }
}
