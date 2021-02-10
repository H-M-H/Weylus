use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread::spawn;
use tracing::{debug, error, info, trace, warn};

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

#[derive(Clone)]
pub struct WsConfig {
    pub address: SocketAddr,
    pub access_code: Option<String>,
    #[cfg(target_os = "linux")]
    pub try_vaapi: bool,
    #[cfg(target_os = "linux")]
    pub try_nvenc: bool,
}

pub fn run(
    sender: mpsc::Sender<Ws2GuiMessage>,
    receiver: mpsc::Receiver<Gui2WsMessage>,
    config: WsConfig,
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
    spawn(move || listen_websocket(config, clients2, shutdown2, sender));
}

fn listen_websocket(
    config: WsConfig,
    clients: Arc<Mutex<HashMap<SocketAddr, Arc<Mutex<Writer<TcpStream>>>>>>,
    shutdown: Arc<AtomicBool>,
    _sender: mpsc::Sender<Ws2GuiMessage>,
) {
    let server = Server::bind(config.address);
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
            info!("Shutting down websocket: {}", config.address);
            return;
        }
        match server.accept() {
            Ok(request) => {
                let clients = clients.clone();
                let config = config.clone();
                spawn(move || handle_connection(request, clients, config));
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
    config: WsConfig,
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

    let mut ws_handler = WsHandler::new(ws_sender, &peer_addr, config.clone());

    let mut authed = config.access_code.is_none();
    let access_code = config.access_code.unwrap_or_else(|| "".into());
    for msg in ws_receiver.incoming_messages() {
        match msg {
            Ok(msg) => {
                if !authed {
                    if let OwnedMessage::Text(pw) = &msg {
                        if pw == &access_code {
                            authed = true;
                            info!("WS-Client authenticated: {}!", peer_addr);
                        } else {
                            warn!(
                                "Authentication failed: {} sent wrong access code: '{}'",
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
    max_width: usize,
    max_height: usize,
}

enum VideoCommands {
    Start(VideoConfig),
    TryGetFrame,
}

fn handle_video(receiver: mpsc::Receiver<VideoCommands>, sender: WsWriter, config: WsConfig) {
    let mut screen_capture: Option<Box<dyn ScreenCapture>> = None;
    let mut video_encoder: Option<Box<VideoEncoder>> = None;

    let mut max_width = 1920;
    let mut max_height = 1080;

    loop {
        let msg = receiver.recv();

        // stop thread once the channel is closed
        if msg.is_err() {
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
                let (width_in, height_in) = screen_capture.size();
                let scale = (max_width as f64 / width_in as f64).min(max_height as f64 / height_in as f64);
                let mut width_out = width_in;
                let mut height_out = height_in;
                if scale < 1.0 {
                    width_out = (width_out as f64 * scale) as usize;
                    height_out = (height_out as f64 * scale) as usize;
                }
                // video encoder is not setup or setup for encoding the wrong size: restart it
                if video_encoder.is_none()
                    || !video_encoder
                        .as_ref()
                        .unwrap()
                        .check_size(width_in, height_in, width_out, height_out)
                {
                    send_msg(&sender, &MessageOutbound::NewVideo);
                    let sender = sender.clone();
                    let res = VideoEncoder::new(
                        width_in,
                        height_in,
                        width_out,
                        height_out,
                        move |data| {
                            let msg = Message::binary(data);
                            if let Err(err) = sender.lock().unwrap().send_message(&msg) {
                                match err {
                                    WebSocketError::IoError(err) => {
                                        // ignore broken pipe errors as those are caused by
                                        // intentionally shutting down the websocket
                                        if err.kind() == std::io::ErrorKind::BrokenPipe {
                                            debug!("Error sending video: {}", err);
                                        } else {
                                            warn!("Error sending video: {}", err);
                                        }
                                    }
                                    _ => warn!("Error sending video: {}", err),
                                }
                            }
                        },
                        #[cfg(target_os = "linux")]
                        config.try_vaapi,
                        #[cfg(target_os = "linux")]
                        config.try_nvenc,
                    );
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
                max_width = config.max_width;
                max_height = config.max_height;
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
    fn new(sender: WsWriter, client_addr: &SocketAddr, config: WsConfig) -> Self {
        let (video_sender, video_receiver) = mpsc::channel::<VideoCommands>();
        {
            let sender = sender.clone();
            let config = config.clone();
            // offload creating the videostream to another thread to avoid blocking the thread that
            // is receiving messages from the websocket
            spawn(move || handle_video(video_receiver, sender, config));
        }

        #[cfg(target_os = "linux")]
        let mut x11ctx = X11Context::new();

        #[cfg(target_os = "linux")]
        let capturables = x11ctx.as_mut().map_or_else(Vec::new, |ctx| {
            ctx.capturables().unwrap_or_else(|_| Vec::new())
        });

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

    // Enqueue a request to send a new video frame.
    //
    // This does not do any further work in order not to block receiving messages. `handle_video`
    // is resposible to do the actual work.
    fn queue_try_send_video_frame(&mut self) {
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
                        max_width: config.max_width,
                        max_height: config.max_height,
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
                .send(VideoCommands::Start(VideoConfig {
                    max_width: config.max_width,
                    max_height: config.max_height,
                }))
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
                            trace!("Got: {:?}", &event);
                            self.process_pointer_event(&event);
                        }
                        MessageInbound::TryGetFrame => self.queue_try_send_video_frame(),
                        MessageInbound::GetCapturableList => {
                            trace!("Got: GetCapturableList");
                            self.send_capturable_list()
                        }
                        MessageInbound::Config(config) => {
                            trace!("Got: {:?}", &config);
                            self.setup(config)
                        }
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
