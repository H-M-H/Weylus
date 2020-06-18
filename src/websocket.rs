use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread::spawn;
use std::time::Duration;
use tracing::{error, info, warn};

use websocket::sender::Writer;
use websocket::sync::Server;
use websocket::OwnedMessage;

use crate::input::mouse_device::Mouse;
#[cfg(target_os = "linux")]
use crate::input::uinput_device::GraphicTablet;
use crate::stream_handler::{PointerStreamHandler, ScreenStreamHandler, StreamHandler};

use crate::screen_capture::generic::ScreenCaptureGeneric;

#[cfg(target_os = "linux")]
use crate::screen_capture::linux::ScreenCaptureX11;
#[cfg(target_os = "linux")]
use crate::x11helper::Capturable;

pub enum Ws2GuiMessage {}

pub enum Gui2WsMessage {
    Shutdown,
}

#[cfg(target_os = "linux")]
pub fn run(
    sender: mpsc::Sender<Ws2GuiMessage>,
    receiver: mpsc::Receiver<Gui2WsMessage>,
    ws_pointer_socket_addr: SocketAddr,
    ws_video_socket_addr: SocketAddr,
    password: Option<&str>,
    screen_update_interval: Duration,
    stylus_support: bool,
    faster_capture: bool,
    capture: Capturable,
    capture_cursor: bool,
    enable_mouse: bool,
    enable_stylus: bool,
    enable_touch: bool,
) {
    let clients = Arc::new(Mutex::new(HashMap::<
        SocketAddr,
        Arc<Mutex<Writer<TcpStream>>>,
    >::new()));
    let clients2 = clients.clone();
    let clients3 = clients.clone();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown2 = shutdown.clone();
    let shutdown3 = shutdown.clone();
    let sender2 = sender.clone();
    let sender3 = sender;

    spawn(move || match receiver.recv() {
        Err(_) | Ok(Gui2WsMessage::Shutdown) => {
            let clients = clients.lock().unwrap();
            for client in clients.values() {
                let client = client.lock().unwrap();
                if let Err(err) = client.shutdown_all() {
                    error!("Could not shutdown websocket: {}", err);
                }
            }
            shutdown.store(true, Ordering::Relaxed);
        }
    });
    let pass: Option<String> = password.map(|s| s.to_string());
    {
        let capture = capture.clone();
        if stylus_support {
            spawn(move || {
                listen_websocket(
                    ws_pointer_socket_addr,
                    pass,
                    clients2,
                    shutdown2,
                    sender2,
                    move |client_addr| {
                        create_graphic_tablet_stream_handler(
                            client_addr,
                            capture.clone(),
                            enable_mouse,
                            enable_stylus,
                            enable_touch,
                        )
                    },
                )
            });
        } else {
            spawn(move || {
                listen_websocket(
                    ws_pointer_socket_addr,
                    pass,
                    clients2,
                    shutdown2,
                    sender2,
                    move |_| {
                        create_mouse_stream_handler(
                            capture.clone(),
                            enable_mouse,
                            enable_stylus,
                            enable_touch,
                        )
                    },
                )
            });
        }
    }

    let pass: Option<String> = password.map(|s| s.to_string());
    {
        if faster_capture {
            spawn(move || {
                listen_websocket(
                    ws_video_socket_addr,
                    pass,
                    clients3,
                    shutdown3,
                    sender3,
                    move |_| {
                        create_xscreen_stream_handler(
                            capture.clone(),
                            screen_update_interval,
                            capture_cursor,
                        )
                    },
                )
            });
        } else {
            spawn(move || {
                listen_websocket(
                    ws_video_socket_addr,
                    pass,
                    clients3,
                    shutdown3,
                    sender3,
                    move |_| create_screen_stream_handler(screen_update_interval),
                )
            });
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub fn run(
    sender: mpsc::Sender<Ws2GuiMessage>,
    receiver: mpsc::Receiver<Gui2WsMessage>,
    ws_pointer_socket_addr: SocketAddr,
    ws_video_socket_addr: SocketAddr,
    password: Option<&str>,
    screen_update_interval: Duration,
    enable_mouse: bool,
    enable_stylus: bool,
    enable_touch: bool,
) {
    let clients = Arc::new(Mutex::new(HashMap::<
        SocketAddr,
        Arc<Mutex<Writer<TcpStream>>>,
    >::new()));
    let clients2 = clients.clone();
    let clients3 = clients.clone();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown2 = shutdown.clone();
    let shutdown3 = shutdown.clone();
    let sender2 = sender.clone();
    let sender3 = sender.clone();

    spawn(move || loop {
        match receiver.recv() {
            Err(_) | Ok(Gui2WsMessage::Shutdown) => {
                let clients = clients.lock().unwrap();
                for client in clients.values() {
                    let client = client.lock().unwrap();
                    if let Err(err) = client.shutdown_all() {
                        error!("Could not shutdown websocket: {}", err);
                    }
                }
                shutdown.store(true, Ordering::Relaxed);
                return;
            }
        }
    });
    let pass: Option<String> = password.map_or(None, |s| Some(s.to_string()));

    spawn(move || {
        listen_websocket(
            ws_pointer_socket_addr,
            pass,
            clients2,
            shutdown2,
            sender2,
            |_| create_mouse_stream_handler(enable_mouse, enable_stylus, enable_touch),
        )
    });

    let pass: Option<String> = password.map_or(None, |s| Some(s.to_string()));

    spawn(move || {
        listen_websocket(
            ws_video_socket_addr,
            pass,
            clients3,
            shutdown3,
            sender3,
            move |_| create_screen_stream_handler(screen_update_interval),
        )
    });
}

#[cfg(target_os = "linux")]
fn create_graphic_tablet_stream_handler(
    client_addr: &SocketAddr,
    capture: Capturable,
    enable_mouse: bool,
    enable_stylus: bool,
    enable_touch: bool,
) -> Result<PointerStreamHandler<GraphicTablet>, Box<dyn std::error::Error>> {
    Ok(PointerStreamHandler::new(GraphicTablet::new(
        capture,
        client_addr.to_string(),
        enable_mouse,
        enable_stylus,
        enable_touch,
    )?))
}

#[cfg(target_os = "linux")]
fn create_mouse_stream_handler(
    capture: Capturable,
    enable_mouse: bool,
    enable_stylus: bool,
    enable_touch: bool,
) -> Result<PointerStreamHandler<Mouse>, Box<dyn std::error::Error>> {
    Ok(PointerStreamHandler::new(Mouse::new(
        capture,
        enable_mouse,
        enable_stylus,
        enable_touch,
    )))
}

#[cfg(not(target_os = "linux"))]
fn create_mouse_stream_handler(
    enable_mouse: bool,
    enable_stylus: bool,
    enable_touch: bool,
) -> Result<PointerStreamHandler<Mouse>, Box<dyn std::error::Error>> {
    Ok(PointerStreamHandler::new(Mouse::new(
        enable_mouse,
        enable_stylus,
        enable_touch,
    )))
}

#[cfg(target_os = "linux")]
fn create_xscreen_stream_handler(
    capture: Capturable,
    update_interval: Duration,
    capture_cursor: bool,
) -> Result<ScreenStreamHandler<ScreenCaptureX11>, Box<dyn std::error::Error>> {
    Ok(ScreenStreamHandler::new(
        ScreenCaptureX11::new(capture, capture_cursor)?,
        update_interval,
    ))
}

fn create_screen_stream_handler(
    update_interval: Duration,
) -> Result<ScreenStreamHandler<ScreenCaptureGeneric>, Box<dyn std::error::Error>> {
    Ok(ScreenStreamHandler::new(
        ScreenCaptureGeneric::new(),
        update_interval,
    ))
}

fn listen_websocket<T, F>(
    addr: SocketAddr,
    password: Option<String>,
    clients: Arc<Mutex<HashMap<SocketAddr, Arc<Mutex<Writer<TcpStream>>>>>>,
    shutdown: Arc<AtomicBool>,
    _sender: mpsc::Sender<Ws2GuiMessage>,
    create_stream_handler: F,
) where
    T: StreamHandler,
    F: Fn(&SocketAddr) -> Result<T, Box<dyn std::error::Error>> + Send + 'static + Clone,
{
    let server = Server::bind(addr);
    if let Err(err) = server {
        error!("Failed binding to socket: {}", err);
        return;
    }
    let mut server = server.unwrap();
    if let Err(err) = server.set_nonblocking(true) {
        warn!(
            "Could not set websocket to non-blocking, graceful shutdown may be impossible now: {}",
            err
        );
    }

    loop {
        std::thread::sleep(std::time::Duration::from_millis(10));
        if shutdown.load(Ordering::Relaxed) {
            info!("Shutting down websocket: {}", addr);
            return;
        }
        let clients = clients.clone();
        let password = password.clone();
        let create_stream_handler = create_stream_handler.clone();
        match server.accept() {
            Ok(request) => {
                spawn(move || {
                    let client = request.accept();
                    if let Err((_, err)) = client {
                        warn!("Failed to accept client: {}", err);
                        return;
                    }
                    let client = client.unwrap();
                    if let Err(err) = client.set_nonblocking(false) {
                        warn!("Failed to set client to blocking mode: {}", err);
                    }
                    let peer_addr = client.peer_addr();
                    if let Err(err) = peer_addr {
                        warn!("Failed to retrieve client address: {}", err);
                        return;
                    }
                    let peer_addr = peer_addr.unwrap();
                    let client = client.split();
                    if let Err(err) = client {
                        warn!("Failed to setup connection: {}", err);
                        return;
                    }
                    let (mut ws_receiver, ws_sender) = client.unwrap();

                    let ws_sender = Arc::new(Mutex::new(ws_sender));

                    let stream_handler = create_stream_handler(&peer_addr);
                    if let Err(err) = stream_handler {
                        error!("Failed to create stream handler: {}", err);
                        return;
                    }

                    {
                        let mut clients = clients.lock().unwrap();
                        clients.insert(peer_addr, ws_sender.clone());
                    }

                    let mut authed = password.is_none();
                    let password = password.unwrap_or_else(|| "".into());
                    let mut stream_handler = stream_handler.unwrap();
                    for msg in ws_receiver.incoming_messages() {
                        match msg {
                            Ok(msg) => {
                                if !authed {
                                    if let OwnedMessage::Text(pw) = &msg {
                                        if pw == &password {
                                            authed = true;
                                        } else {
                                            warn!(
                                                "Authentication failed: {} sent wrong password: '{}'",
                                                peer_addr, pw
                                            );
                                            let mut clients = clients.lock().unwrap();
                                            clients.remove(&peer_addr);
                                            return;
                                        }
                                    }
                                } else {
                                    stream_handler.process(ws_sender.clone(), &msg);
                                }
                                if msg.is_close() {
                                    let mut clients = clients.lock().unwrap();
                                    clients.remove(&peer_addr);
                                    return;
                                }
                            }
                            Err(err) => {
                                match err {
                                    // this happens on calling shutdown, no need to log this
                                    websocket::WebSocketError::NoDataAvailable => (),
                                    _ => warn!(
                                        "Error reading message from websocket, closing ({})",
                                        err
                                    ),
                                }

                                let mut clients = clients.lock().unwrap();
                                clients.remove(&peer_addr);
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
