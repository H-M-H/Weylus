use log::{info, warn};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread::spawn;

use websocket::sync::Server;
use websocket::OwnedMessage;
use websocket::Message;

use crate::input::mouse_device::Mouse;
use crate::input::pointer::PointerDevice;
use crate::input::uinput_device::GraphicTablet;
use crate::stream_handler::{PointerStreamHandler, ScreenStreamHandler, StreamHandler};

pub fn run(addr: &str) -> Result<(), String> {
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let addr = addr.to_string();
    spawn(|| listen_websocket(addr, tx, &create_pointer_stream_handler));
    match rx.recv() {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(err)) => Err(err.to_string()),
        Err(err) => Err(err.to_string()),
    };

    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    spawn(|| {
        listen_websocket(
            "0.0.0.0:9002".to_string(),
            tx,
            &create_screen_stream_handler,
        )
    });
    match rx.recv() {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(err)) => Err(err.to_string()),
        Err(err) => Err(err.to_string()),
    }
}

fn create_pointer_stream_handler() -> PointerStreamHandler<GraphicTablet> {
    PointerStreamHandler::new(GraphicTablet::new().unwrap())
}

fn create_screen_stream_handler() -> ScreenStreamHandler {
    ScreenStreamHandler::new()
}


fn listen_websocket<T, F>(
    addr: String,
    tx: mpsc::Sender<Result<(), String>>,
    create_stream_handler: &'static F,
) where
    T: StreamHandler,
    F: Fn() -> T + Sync,
{
    let server = Server::bind(addr);
    if let Err(err) = server {
        tx.send(Err(err.to_string()))
            .expect("Could not report back to calling thread, aborting!");
        return;
    }
    tx.send(Ok(()))
        .expect("Could not report back to calling thread, aborting!");

    let server = server.unwrap();
    for request in server.filter_map(Result::ok) {
        spawn(move || {
            let client = request.accept().unwrap();
            let (mut receiver, mut sender) = client.split().unwrap();
            let mut stream_handler = create_stream_handler();
            for msg in receiver.incoming_messages() {
                match msg {
                    Ok(msg) => {
                        stream_handler.process(&mut sender, &msg);
                        if msg.is_close() {
                            return;
                        }
                    }
                    Err(err) => {
                        warn!("Error reading message from websocket, closing ({})", err);
                        return;
                    }
                }
            }
        });
    }
}
