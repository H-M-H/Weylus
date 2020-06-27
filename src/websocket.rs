use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread::spawn;
use tracing::{error, info, trace, warn};

use websocket::sender::Writer;
use websocket::server::upgrade::{sync::Buffer as WsBuffer, WsUpgrade};
use websocket::sync::Server;
use websocket::{Message, OwnedMessage, WebSocketError};

use crate::input::device::InputDevice;
use crate::protocol::{ClientConfiguration, MessageInbound, MessageOutbound, PointerEvent};
use crate::screen_capture::generic::ScreenCaptureGeneric;
#[cfg(target_os = "linux")]
use crate::screen_capture::linux::ScreenCaptureX11;
use crate::screen_capture::ScreenCapture;
#[cfg(target_os = "linux")]
use crate::x11helper::{Capturable, X11Context};

use crate::video::VideoEncoder;

type WsWriter = Arc<Mutex<websocket::sender::Writer<std::net::TcpStream>>>;

pub enum Ws2GuiMessage {}

pub enum Gui2WsMessage {
    Shutdown,
}

pub fn run(
    sender: mpsc::Sender<Ws2GuiMessage>,
    receiver: mpsc::Receiver<Gui2WsMessage>,
    ws_socket_addr: SocketAddr,
    password: Option<&str>,
) {
    let clients = Arc::new(Mutex::new(HashMap::<
        SocketAddr,
        Arc<Mutex<Writer<TcpStream>>>,
    >::new()));
    let clients2 = clients.clone();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown2 = shutdown.clone();

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
    spawn(move || listen_websocket(ws_socket_addr, pass, clients2, shutdown2, sender));
}

fn listen_websocket(
    addr: SocketAddr,
    password: Option<String>,
    clients: Arc<Mutex<HashMap<SocketAddr, Arc<Mutex<Writer<TcpStream>>>>>>,
    shutdown: Arc<AtomicBool>,
    _sender: mpsc::Sender<Ws2GuiMessage>,
) {
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
        match server.accept() {
            Ok(request) => {
                spawn(move || handle_connection(request, clients, password));
            }
            Err(_) => {
                if shutdown.load(Ordering::Relaxed) {
                    return;
                }
            }
        };
    }
}

fn handle_connection(
    request: WsUpgrade<TcpStream, Option<WsBuffer>>,
    clients: Arc<Mutex<HashMap<SocketAddr, Arc<Mutex<Writer<TcpStream>>>>>>,
    password: Option<String>,
) {
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

    {
        let mut clients = clients.lock().unwrap();
        clients.insert(peer_addr, ws_sender.clone());
    }

    let mut ws_handler = WsHandler::new(ws_sender, &peer_addr);

    let mut authed = password.is_none();
    let password = password.unwrap_or_else(|| "".into());
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
                    ws_handler.process(&msg);
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
                    WebSocketError::NoDataAvailable => (),
                    _ => warn!("Error reading message from websocket, closing ({})", err),
                }

                let mut clients = clients.lock().unwrap();
                clients.remove(&peer_addr);
                return;
            }
        }
    }
}

fn send_msg(sender: &WsWriter, msg: &MessageOutbound) {
    if let Err(err) = sender
        .lock()
        .unwrap()
        .send_message(&Message::text(serde_json::to_string(msg).unwrap()))
    {
        warn!("Failed to send message to websocket: {}", err);
    }
}

struct VideoConfig {
    #[cfg(target_os = "linux")]
    capturable: Capturable,
    #[cfg(target_os = "linux")]
    capture_cursor: bool,
    #[cfg(target_os = "linux")]
    x11_capture: bool,
}

enum VideoCommands {
    Start(VideoConfig),
    TryGetFrame,
}

fn handle_video(receiver: mpsc::Receiver<VideoCommands>, sender: WsWriter) {
    let mut screen_capture: Option<Box<dyn ScreenCapture>> = None;
    let mut video_encoder: Option<Box<VideoEncoder>> = None;

    loop {
        let msg = receiver.recv();

        // stop thread once the channel is closed
        if msg.is_err() {
            std::thread::sleep(std::time::Duration::from_secs(5));
            return;
        }
        let mut msg = msg.unwrap();

        // drop frames if the client is requesting frames at a higher rate than they can be
        // produced here
        if let VideoCommands::TryGetFrame = msg {
            loop {
                match receiver.try_recv() {
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                    Ok(VideoCommands::TryGetFrame) => continue,
                    Ok(tmp_msg) => {
                        msg = tmp_msg;
                        break;
                    }
                }
            }
        }
        match msg {
            VideoCommands::TryGetFrame => {
                if screen_capture.is_none() {
                    warn!("Screen capture not initalized, can not send video frame!");
                    continue;
                }
                if let Err(err) = screen_capture.as_mut().unwrap().capture() {
                    warn!("Error capturing screen: {}", err);
                    continue;
                }
                let screen_capture = screen_capture.as_ref().unwrap();
                let (width, height) = screen_capture.size();
                // video encoder is not setup or setup for encoding the wrong size: restart it
                if video_encoder.is_none()
                    || !video_encoder.as_ref().unwrap().check_size(width, height)
                {
                    send_msg(&sender, &MessageOutbound::NewVideo);
                    let sender = sender.clone();
                    let res = VideoEncoder::new(width, height, move |data| {
                        let msg = Message::binary(data);
                        if let Err(err) = sender.lock().unwrap().send_message(&msg) {
                            match err {
                                WebSocketError::IoError(err) => {
                                    // ignore broken pipe errors as those are caused by
                                    // intentionally shutting down the websocket
                                    if err.kind() == std::io::ErrorKind::BrokenPipe {
                                        trace!("Error sending video: {}", err);
                                    } else {
                                        warn!("Error sending video: {}", err);
                                    }
                                }
                                _ => warn!("Error sending video: {}", err),
                            }
                        }
                    });
                    if let Err(err) = res {
                        warn!("{}", err);
                        continue;
                    }
                    video_encoder = Some(res.unwrap());
                }
                let video_encoder = video_encoder.as_mut().unwrap();
                video_encoder.encode(screen_capture.pixel_provider());
            }
            VideoCommands::Start(config) => {
                #[cfg(target_os = "linux")]
                {
                    if config.x11_capture {
                        screen_capture = Some(Box::new(
                            ScreenCaptureX11::new(config.capturable, config.capture_cursor)
                                .unwrap(),
                        ))
                    } else {
                        screen_capture = Some(Box::new(ScreenCaptureGeneric::new()))
                    }
                }

                #[cfg(not(target_os = "linux"))]
                {
                    screen_capture = Some(Box::new(ScreenCaptureGeneric::new()));
                }
                send_msg(&sender, &MessageOutbound::ConfigOk);
            }
        }
    }
}

struct WsHandler {
    sender: WsWriter,
    client_addr: SocketAddr,
    video_sender: mpsc::Sender<VideoCommands>,
    input_device: Option<Box<dyn InputDevice>>,
    #[cfg(target_os = "linux")]
    x11ctx: Option<X11Context>,
    #[cfg(target_os = "linux")]
    capturables: Vec<Capturable>,
}

impl WsHandler {
    fn new(sender: WsWriter, client_addr: &SocketAddr) -> Self {
        let (video_sender, video_receiver) = mpsc::channel::<VideoCommands>();
        {
            let sender = sender.clone();
            spawn(move || handle_video(video_receiver, sender));
        }

        #[cfg(target_os = "linux")]
        let mut x11ctx = X11Context::new();

        #[cfg(target_os = "linux")]
        let capturables = x11ctx.as_mut().map_or_else(
            Vec::new,
            |ctx| ctx.capturables().unwrap_or_else(|_| Vec::new()),
        );

        Self {
            sender,
            client_addr: *client_addr,
            video_sender,
            input_device: None,
            #[cfg(target_os = "linux")]
            x11ctx,
            #[cfg(target_os = "linux")]
            capturables,
        }
    }

    fn send_msg(&self, msg: &MessageOutbound) {
        send_msg(&self.sender, msg)
    }

    fn send_video_frame(&mut self) {
        self.video_sender.send(VideoCommands::TryGetFrame).unwrap();
    }

    fn process_pointer_event(&mut self, event: &PointerEvent) {
        if self.input_device.is_some() {
            self.input_device.as_mut().unwrap().send_event(&event)
        } else {
            warn!("Pointer device is not initalized, can not process PointerEvent!");
        }
    }

    fn send_capturable_list(&mut self) {
        let mut windows = Vec::<String>::new();
        #[cfg(not(target_os = "linux"))]
        {
            windows.push("Desktop".to_string());
        }

        #[cfg(target_os = "linux")]
        {
            if let Some(x11ctx) = self.x11ctx.as_mut() {
                let capturables = x11ctx.capturables();
                match capturables {
                    Ok(capturables) => {
                        capturables.iter().for_each(|c| {
                            windows.push(c.name());
                        });
                        self.capturables = capturables;
                    }
                    Err(err) => warn!("Failed to get list of capturables: {}", err),
                }
            } else {
                windows.push("Desktop".to_string());
            }
        }
        self.send_msg(&MessageOutbound::CapturableList(windows));
    }

    fn setup(&mut self, config: ClientConfiguration) {
        #[cfg(target_os = "linux")]
        {
            if config.capturable_id < self.capturables.len() {
                let capturable = self.capturables[if config.faster_capture {
                    config.capturable_id
                } else {
                    // can only capture desktop if capturing with ScreenCaptureGeneric
                    0
                }]
                .clone();
                if config.stylus_support {
                    let device = crate::input::uinput_device::GraphicTablet::new(
                        capturable.clone(),
                        self.client_addr.to_string(),
                    );
                    if let Err(err) = device {
                        error!("Failed to create uinput device: {}", err);
                        self.send_msg(&MessageOutbound::ConfigError(
                            "Failed to create uinput device!".to_string(),
                        ));
                        return;
                    }
                    self.input_device = Some(Box::new(device.unwrap()))
                } else {
                    self.input_device = Some(Box::new(crate::input::mouse_device::Mouse::new(
                        capturable.clone(),
                    )))
                }

                self.video_sender
                    .send(VideoCommands::Start(VideoConfig {
                        capturable,
                        capture_cursor: config.capture_cursor,
                        x11_capture: config.faster_capture,
                    }))
                    .unwrap();
            } else {
                error!("Got invalid id for capturable: {}", config.capturable_id);
                self.send_msg(&MessageOutbound::ConfigError(
                    "Invalid id for capturable!".to_string(),
                ));
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            self.input_device = Some(Box::new(crate::input::mouse_device::Mouse::new()));
            self.video_sender
                .send(VideoCommands::Start(VideoConfig {}))
                .unwrap();
        }
    }

    fn process(&mut self, message: &OwnedMessage) {
        match message {
            OwnedMessage::Text(s) => {
                let message: Result<MessageInbound, _> = serde_json::from_str(&s);
                match message {
                    Ok(message) => match message {
                        MessageInbound::PointerEvent(event) => {
                            self.process_pointer_event(&event);
                        }
                        MessageInbound::TryGetFrame => self.send_video_frame(),
                        MessageInbound::GetCapturableList => self.send_capturable_list(),
                        MessageInbound::Config(config) => self.setup(config),
                    },
                    Err(err) => {
                        warn!("Unable to parse message: {} ({})", s, err);
                        self.send_msg(&MessageOutbound::Error(
                            "Failed to parse message!".to_string(),
                        ));
                    }
                }
            }
            _ => (),
        }
    }
}
