use std::net::SocketAddr;
use std::sync::mpsc;
use tokio::sync::mpsc as mpsc_tokio;
use tracing::{error, warn};

use crate::config::Config;
use crate::video::EncoderOptions;
use crate::web::{Ui2WebMessage, Web2UiMessage};
use crate::websocket::{Ui2WsMessage, Ws2UiMessage, WsConfig};

struct Channels {
    sender_ui2ws: mpsc::Sender<Ui2WsMessage>,
    sender_ui2web: mpsc_tokio::Sender<Ui2WebMessage>,
}

pub struct Weylus {
    channels: Option<Channels>,
    ws_thread: Option<std::thread::JoinHandle<()>>,
    web_thread: Option<std::thread::JoinHandle<()>>,
}

impl Weylus {
    pub fn new() -> Self {
        Self {
            channels: None,
            ws_thread: None,
            web_thread: None,
        }
    }

    pub fn start(
        &mut self,
        config: &Config,
        mut on_web_message: impl FnMut(Web2UiMessage) + Send + 'static,
        mut on_ws_message: impl FnMut(Ws2UiMessage) + Send + 'static,
    ) -> bool {
        if self.channels.is_some() {
            return false;
        }
        let encoder_options = EncoderOptions {
            #[cfg(target_os = "linux")]
            try_vaapi: config.try_vaapi,
            #[cfg(not(target_os = "linux"))]
            try_vaapi: false,

            #[cfg(any(target_os = "linux", target_os = "windows"))]
            try_nvenc: config.try_nvenc,
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            try_nvenc: false,

            #[cfg(target_os = "macos")]
            try_videotoolbox: config.try_videotoolbox,
            #[cfg(not(target_os = "macos"))]
            try_videotoolbox: false,

            #[cfg(target_os = "windows")]
            try_mediafoundation: config.try_mediafoundation,
            #[cfg(not(target_os = "windows"))]
            try_mediafoundation: false,
        };

        let ws_config = WsConfig {
            address: SocketAddr::new(config.bind_address, config.websocket_port),
            access_code: config.access_code.clone(),
            encoder_options,
            #[cfg(target_os = "linux")]
            wayland_support: config.wayland_support,
        };

        let (sender_ui2ws, receiver_ui2ws) = mpsc::channel();
        let (sender_ui2web, receiver_ui2web) = mpsc_tokio::channel(100);

        let (sender_ws2ui, receiver_ws2ui) = mpsc::channel();
        let (sender_web2ui, receiver_web2ui) = mpsc::channel();

        let ws_thread = crate::websocket::run(sender_ws2ui, receiver_ui2ws, ws_config);
        match receiver_ws2ui.recv() {
            Ok(Ws2UiMessage::Start) => {}
            Ok(Ws2UiMessage::Error(err)) => {
                error!("Failed to start websocket server: {}", err);
                if let Err(_) = ws_thread.join() {
                    error!("Websocketserver thread panicked.");
                }
                return false;
            }
            Ok(Ws2UiMessage::UInputInaccessible) => unreachable!(),
            Err(err) => {
                error!("Error communicating with websocketserver thread: {}", err);
                if let Err(_) = ws_thread.join() {
                    error!("Websocketserver thread panicked.");
                }
                return false;
            }
        }

        let web_thread = crate::web::run(
            sender_web2ui,
            receiver_ui2web,
            &SocketAddr::new(config.bind_address, config.web_port),
            config.websocket_port,
            config.access_code.as_ref().map(|s| s.as_str()),
            config.custom_index_html.clone(),
            config.custom_access_html.clone(),
            config.custom_style_css.clone(),
            config.custom_lib_js.clone(),
        );
        match receiver_web2ui.recv() {
            Ok(Web2UiMessage::Start) => (),
            Ok(Web2UiMessage::Error(err)) => {
                error!("Webserver error: {}", err);
                if let Err(_) = web_thread.join() {
                    error!("Webserver thread panicked.");
                }

                if let Err(err) = sender_ui2ws.send(Ui2WsMessage::Shutdown) {
                    warn!(
                        "Failed to send shutdown command to websocketserver: {}",
                        err
                    );
                }
                if let Err(_) = ws_thread.join() {
                    error!("Websocketserver thread panicked.");
                }
                return false;
            }
            Err(err) => {
                error!("Error communicating with webserver thread: {}", err);
                if let Err(_) = web_thread.join() {
                    error!("Webserver thread panicked.");
                }

                if let Err(err) = sender_ui2ws.send(Ui2WsMessage::Shutdown) {
                    warn!(
                        "Failed to send shutdown command to websocketserver: {}",
                        err
                    );
                }
                if let Err(_) = ws_thread.join() {
                    error!("Websocketserver thread panicked.");
                }
                return false;
            }
        }
        self.ws_thread = Some(ws_thread);
        self.web_thread = Some(web_thread);
        self.channels = Some(Channels {
            sender_ui2ws,
            sender_ui2web,
        });
        std::thread::spawn(move || {
            for msg in receiver_web2ui.iter() {
                on_web_message(msg);
            }
        });
        std::thread::spawn(move || {
            for msg in receiver_ws2ui.iter() {
                on_ws_message(msg);
            }
        });
        true
    }

    pub fn stop(&mut self) {
        if let Some(channels) = self.channels.as_mut() {
            if let Err(err) = channels.sender_ui2ws.send(Ui2WsMessage::Shutdown) {
                warn!(
                    "Failed to send shutdown command to websocketserver: {}",
                    err
                );
            }
            if let Err(err) = channels.sender_ui2web.try_send(Ui2WebMessage::Shutdown) {
                warn!("Failed to send shutdown command to webserver: {}", err);
            }
        }
        self.wait();
        self.channels = None;
    }

    pub fn wait(&mut self) {
        if let Some(t) = self.ws_thread.take() {
            if let Err(_) = t.join() {
                error!("Websocket thread panicked.");
            }
        }
        if let Some(t) = self.web_thread.take() {
            if let Err(_) = t.join() {
                error!("Web thread panicked.");
            }
        }
    }
}

impl Drop for Weylus {
    fn drop(&mut self) {
        self.stop();
    }
}
