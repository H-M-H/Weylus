use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread::spawn;
use tracing::{info, warn};

use websocket::sender::Writer;
use websocket::sync::Server;

use crate::input::mouse_device::Mouse;
#[cfg(target_os = "linux")]
use crate::input::uinput_device::GraphicTablet;
use crate::stream_handler::{PointerStreamHandler, ScreenStreamHandler, StreamHandler};

use crate::screen_capture::generic::ScreenCaptureGeneric;
#[cfg(target_os = "linux")]
use crate::screen_capture::linux::ScreenCaptureX11;

pub enum Ws2GuiMessage {}
pub enum Gui2WsMessage {
    Shutdown,
}

pub fn run(
    sender: mpsc::Sender<Ws2GuiMessage>,
    receiver: mpsc::Receiver<Gui2WsMessage>,
    ws_pointer_socket_addr: SocketAddr,
    ws_video_socket_addr: SocketAddr,
    password: Option<&str>,
) -> Result<(), String> {
    let clients = Arc::new(Mutex::new(HashMap::<
        SocketAddr,
        Arc<Mutex<Writer<TcpStream>>>,
    >::new()));
    let clients2 = clients.clone();
    let clients3 = clients.clone();
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown2 = shutdown.clone();
    let shutdown3 = shutdown.clone();
    spawn(move || loop {
        match receiver.recv() {
            Err(_) | Ok(Gui2WsMessage::Shutdown) => {
                let clients = clients.lock().unwrap();
                for client in clients.values() {
                    let client = client.lock().unwrap();
                    client.shutdown_all();
                }
                shutdown.store(true, Ordering::Relaxed);
            }
        }
    });
    spawn(move || {
        listen_websocket(
            ws_pointer_socket_addr,
            clients2,
            shutdown2,
            tx,
            &create_pointer_stream_handler,
        )
    });
    match rx.recv() {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(err)) => Err(err.to_string()),
        Err(err) => Err(err.to_string()),
    }
    .unwrap();

    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    spawn(move || {
        listen_websocket(
            ws_video_socket_addr,
            clients3,
            shutdown3,
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

#[cfg(target_os = "linux")]
fn create_pointer_stream_handler() -> PointerStreamHandler<GraphicTablet> {
    PointerStreamHandler::new(GraphicTablet::new().unwrap())
}

#[cfg(not(target_os = "linux"))]
fn create_pointer_stream_handler() -> PointerStreamHandler<Mouse> {
    PointerStreamHandler::new(Mouse::new())
}

#[cfg(target_os = "linux")]
fn create_screen_stream_handler() -> ScreenStreamHandler<ScreenCaptureX11> {
    ScreenStreamHandler::new(ScreenCaptureX11::new().unwrap())
}

#[cfg(not(target_os = "linux"))]
fn create_screen_stream_handler() -> ScreenStreamHandler<ScreenCaptureGeneric> {
    ScreenStreamHandler::new(ScreenCaptureGeneric::new())
}

fn listen_websocket<T, F>(
    addr: SocketAddr,
    clients: Arc<Mutex<HashMap<SocketAddr, Arc<Mutex<Writer<TcpStream>>>>>>,
    shutdown: Arc<AtomicBool>,
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

    let mut server = server.unwrap();
    server.set_nonblocking(true).unwrap();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(10));
        if shutdown.load(Ordering::Relaxed) {
            return;
        }
        let clients = clients.clone();
        match server.accept() {
            Ok(request) => {
                spawn(move || {
                    let client = request.accept().unwrap();
                    let peer_addr = client.peer_addr().unwrap();
                    let (mut receiver, sender) = client.split().unwrap();

                    let sender = Arc::new(Mutex::new(sender));

                    {
                        let mut clients = clients.lock().unwrap();
                        clients.insert(peer_addr, sender.clone());
                    }

                    let mut stream_handler = create_stream_handler();
                    for msg in receiver.incoming_messages() {
                        match msg {
                            Ok(msg) => {
                                stream_handler.process(sender.clone(), &msg);
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
            Err(_) => {
                if shutdown.load(Ordering::Relaxed) {
                    return;
                }
            }
        };
    }
}
