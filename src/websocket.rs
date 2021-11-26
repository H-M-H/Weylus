use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::SendError;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread::spawn;
use tracing::{debug, error, info, warn};

use websocket::sender::Writer;
use websocket::server::upgrade::{sync::Buffer as WsBuffer, WsUpgrade};
use websocket::sync::Server;
use websocket::{Message, OwnedMessage, WebSocketError};

use crate::capturable::{get_capturables, Capturable, Recorder};
use crate::input::device::{InputDevice, InputDeviceType};
use crate::protocol::{
    ClientConfiguration, KeyboardEvent, MessageInbound, MessageOutbound, PointerEvent, WheelEvent,
};

use crate::cerror::CErrorCode;
use crate::video::{EncoderOptions, VideoEncoder};

type WsWriter = Arc<Mutex<websocket::sender::Writer<std::net::TcpStream>>>;
type WsClients = Arc<Mutex<HashMap<SocketAddr, Arc<Mutex<Writer<TcpStream>>>>>>;

pub enum Ws2UiMessage {
    Start,
    UInputInaccessible,
    Error(String),
}

pub enum Ui2WsMessage {
    Shutdown,
}

#[derive(Clone)]
pub struct WsConfig {
    pub address: SocketAddr,
    pub access_code: Option<String>,
    pub encoder_options: EncoderOptions,
    #[cfg(target_os = "linux")]
    pub wayland_support: bool,
}

fn log_send_error<T>(res: Result<(), SendError<T>>) {
    if let Err(err) = res {
        warn!("Websocket: Failed to send message to ui: {}", err);
    }
}

pub fn run(
    sender: mpsc::Sender<Ws2UiMessage>,
    receiver: mpsc::Receiver<Ui2WsMessage>,
    config: WsConfig,
) -> std::thread::JoinHandle<()> {
    let clients = Arc::new(Mutex::new(HashMap::<
        SocketAddr,
        Arc<Mutex<Writer<TcpStream>>>,
    >::new()));
    let clients2 = clients.clone();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown2 = shutdown.clone();

    spawn(move || match receiver.recv() {
        Err(_) | Ok(Ui2WsMessage::Shutdown) => {
            let clients = clients.lock().unwrap();
            for client in clients.values() {
                let client = client.lock().unwrap();
                if let Err(err) = client.shutdown_all() {
                    error!("Could not shutdown websocket client: {}", err);
                }
            }
            shutdown.store(true, Ordering::Relaxed);
        }
    });
    spawn(move || listen_websocket(config, clients2, shutdown2, sender))
}

fn listen_websocket(
    config: WsConfig,
    clients: WsClients,
    shutdown: Arc<AtomicBool>,
    sender: mpsc::Sender<Ws2UiMessage>,
) {
    let server = Server::bind(config.address);
    if let Err(err) = server {
        log_send_error(sender.send(Ws2UiMessage::Error(format!(
            "Failed binding to socket: {}",
            err
        ))));
        return;
    }
    let mut server = server.unwrap();
    if let Err(err) = server.set_nonblocking(true) {
        warn!(
            "Could not set websocket to non-blocking, graceful shutdown may be impossible now: {}",
            err
        );
    }

    log_send_error(sender.send(Ws2UiMessage::Start));

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
                let sender = sender.clone();
                spawn(move || handle_connection(request, clients, config, sender));
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
    clients: WsClients,
    config: WsConfig,
    gui_sender: mpsc::Sender<Ws2UiMessage>,
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
        info!(address = ?peer_addr, "Client connected.");
    }

    let mut ws_handler = WsHandler::new(ws_sender, config.clone(), gui_sender, peer_addr);

    let mut authed = config.access_code.is_none();
    let access_code = config.access_code.unwrap_or_else(|| "".into());
    for msg in ws_receiver.incoming_messages() {
        match msg {
            Ok(msg) => {
                if !authed {
                    if let OwnedMessage::Text(pw) = &msg {
                        if pw == &access_code {
                            authed = true;
                            info!(address = ?peer_addr, "WS-Client authenticated!");
                        } else {
                            warn!(
                                address = ?peer_addr,
                                access_code = %pw,
                                "Authentication failed, wrong access code",
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
                    info!(address = ?peer_addr, "Client disconnected.");
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
    capturable: Box<dyn Capturable>,
    capture_cursor: bool,
    max_width: usize,
    max_height: usize,
}

enum VideoCommands {
    Start(VideoConfig),
    TryGetFrame,
}

fn handle_video(receiver: mpsc::Receiver<VideoCommands>, sender: WsWriter, config: WsConfig) {
    let mut recorder: Option<Box<dyn Recorder>> = None;
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
                if recorder.is_none() {
                    warn!("Screen capture not initalized, can not send video frame!");
                    continue;
                }
                let pixel_data = recorder.as_mut().unwrap().capture();
                if let Err(err) = pixel_data {
                    warn!("Error capturing screen: {}", err);
                    continue;
                }
                let pixel_data = pixel_data.unwrap();
                let (width_in, height_in) = pixel_data.size();
                let scale =
                    (max_width as f64 / width_in as f64).min(max_height as f64 / height_in as f64);
                // limit video to 4K
                let scale_max = (3840.0 / width_in as f64).min(2160.0 / height_in as f64);
                let scale = scale.min(scale_max);
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
                        config.encoder_options,
                    );
                    if let Err(err) = res {
                        warn!("{}", err);
                        continue;
                    }
                    video_encoder = Some(res.unwrap());
                }
                let video_encoder = video_encoder.as_mut().unwrap();
                video_encoder.encode(pixel_data);
            }
            VideoCommands::Start(config) => {
                #[allow(unused_assignments)]
                {
                    // gstpipewire can not handle setting a pipeline's state to Null after another
                    // pipeline has been created and its state has been set to Play.
                    // This line makes sure that there always is only a single recorder and thus
                    // single pipeline in this thread by forcing rust to call the destructor of the
                    // current pipeline here, right before creating a new pipeline.
                    // See: https://gitlab.freedesktop.org/pipewire/pipewire/-/issues/986
                    //
                    // This shouldn't affect other Recorder trait objects.
                    recorder = None;
                }
                match config.capturable.recorder(config.capture_cursor) {
                    Ok(r) => {
                        recorder = Some(r);
                        max_width = config.max_width;
                        max_height = config.max_height;
                        send_msg(&sender, &MessageOutbound::ConfigOk);
                    }
                    Err(err) => {
                        warn!("Failed to init screen cast: {}!", err);
                        send_msg(
                            &sender,
                            &MessageOutbound::Error("Failed to init screen cast!".into()),
                        )
                    }
                }
            }
        }
    }
}

struct WsHandler {
    sender: WsWriter,
    video_sender: mpsc::Sender<VideoCommands>,
    input_device: Option<Box<dyn InputDevice>>,
    capturables: Vec<Box<dyn Capturable>>,
    gui_sender: mpsc::Sender<Ws2UiMessage>,
    ws_config: WsConfig,
    #[cfg(target_os = "linux")]
    capture_cursor: bool,
    client_name: Option<String>,
    client_address: SocketAddr,
}

impl WsHandler {
    fn new(
        sender: WsWriter,
        config: WsConfig,
        gui_sender: mpsc::Sender<Ws2UiMessage>,
        client_address: SocketAddr,
    ) -> Self {
        let (video_sender, video_receiver) = mpsc::channel::<VideoCommands>();
        {
            let sender = sender.clone();
            let config = config.clone();
            // offload creating the videostream to another thread to avoid blocking the thread that
            // is receiving messages from the websocket
            spawn(move || handle_video(video_receiver, sender, config));
        }

        Self {
            sender,
            video_sender,
            input_device: None,
            capturables: vec![],
            gui_sender,
            ws_config: config,
            #[cfg(target_os = "linux")]
            capture_cursor: false,
            client_name: None,
            client_address,
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

    fn process_wheel_event(&mut self, event: &WheelEvent) {
        if self.input_device.is_some() {
            self.input_device.as_mut().unwrap().send_wheel_event(&event)
        } else {
            warn!("Input device is not initalized, can not process WheelEvent!");
        }
    }

    fn process_pointer_event(&mut self, event: &PointerEvent) {
        if self.input_device.is_some() {
            self.input_device
                .as_mut()
                .unwrap()
                .send_pointer_event(&event)
        } else {
            warn!("Input device is not initalized, can not process PointerEvent!");
        }
    }

    fn process_keyboard_event(&mut self, event: &KeyboardEvent) {
        if self.input_device.is_some() {
            self.input_device
                .as_mut()
                .unwrap()
                .send_keyboard_event(&event)
        } else {
            warn!("Input device is not initalized, can not process KeyboardEvent!");
        }
    }

    fn send_capturable_list(&mut self) {
        let mut windows = Vec::<String>::new();
        self.capturables = get_capturables(
            #[cfg(target_os = "linux")]
            self.ws_config.wayland_support,
            #[cfg(target_os = "linux")]
            self.capture_cursor,
        );
        self.capturables.iter().for_each(|c| {
            windows.push(c.name());
        });
        self.send_msg(&MessageOutbound::CapturableList(windows));
    }

    fn setup(&mut self, config: ClientConfiguration) {
        let client_name_changed = if self.client_name != config.client_name {
            self.client_name = config.client_name;
            true
        } else {
            false
        };
        if config.capturable_id < self.capturables.len() {
            let capturable = self.capturables[config.capturable_id].clone();

            #[cfg(target_os = "linux")]
            {
                self.capture_cursor = config.capture_cursor;
            }

            #[cfg(target_os = "linux")]
            if config.uinput_support {
                if self.input_device.as_ref().map_or(true, |d| {
                    client_name_changed || d.device_type() != InputDeviceType::UInputDevice
                }) {
                    let device = crate::input::uinput_device::UInputDevice::new(
                        capturable.clone(),
                        &self.client_name,
                    );
                    if let Err(err) = device {
                        error!("Failed to create uinput device: {}", err);
                        if let CErrorCode::UInputNotAccessible = err.to_enum() {
                            if let Err(err) = self.gui_sender.send(Ws2UiMessage::UInputInaccessible)
                            {
                                warn!("Failed to send message to gui thread: {}!", err);
                            }
                        }
                        self.send_msg(&MessageOutbound::ConfigError(
                            "Failed to create uinput device!".to_string(),
                        ));
                        return;
                    }
                    self.input_device = Some(Box::new(device.unwrap()))
                } else if let Some(d) = self.input_device.as_mut() {
                    d.set_capturable(capturable.clone());
                }
            } else if self.input_device.as_ref().map_or(true, |d| {
                d.device_type() != InputDeviceType::AutoPilotDevice
            }) {
                self.input_device = Some(Box::new(
                    crate::input::autopilot_device::AutoPilotDevice::new(capturable.clone()),
                ));
            } else if let Some(d) = self.input_device.as_mut() {
                d.set_capturable(capturable.clone());
            }

            #[cfg(target_os = "macos")]
            if self.input_device.is_none() {
                self.input_device = Some(Box::new(
                    crate::input::autopilot_device::AutoPilotDevice::new(capturable.clone()),
                ));
            } else {
                self.input_device
                    .as_mut()
                    .map(|d| d.set_capturable(capturable.clone()));
            }
            #[cfg(target_os = "windows")]
            if self.input_device.is_none() {
                self.input_device = Some(Box::new(
                    crate::input::autopilot_device_win::WindowsInput::new(capturable.clone()),
                ));
            } else {
                self.input_device
                    .as_mut()
                    .map(|d| d.set_capturable(capturable.clone()));
            }

            self.video_sender
                .send(VideoCommands::Start(VideoConfig {
                    capturable,
                    capture_cursor: config.capture_cursor,
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

    fn process(&mut self, message: &OwnedMessage) {
        match message {
            OwnedMessage::Text(s) => {
                let message: Result<MessageInbound, _> = serde_json::from_str(&s);
                match message {
                    Ok(message) => {
                        if let MessageInbound::TryGetFrame = message {
                        } else {
                            debug!(
                                client_message = %s,
                                address = ?self.client_address,
                                "Got message from client."
                            );
                        }
                        match message {
                            MessageInbound::WheelEvent(event) => {
                                self.process_wheel_event(&event);
                            }
                            MessageInbound::PointerEvent(event) => {
                                self.process_pointer_event(&event);
                            }
                            MessageInbound::KeyboardEvent(event) => {
                                self.process_keyboard_event(&event);
                            }
                            MessageInbound::TryGetFrame => self.queue_try_send_video_frame(),
                            MessageInbound::GetCapturableList => self.send_capturable_list(),
                            MessageInbound::Config(config) => self.setup(config),
                        }
                    }
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
