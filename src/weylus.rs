use std::net::SocketAddr;
use std::sync::Arc;
use tracing::error;

use crate::config::Config;
use crate::video::EncoderOptions;
use crate::web::{Web2UiMessage, WebServerConfig, WebStartUpMessage};
use crate::websocket::WeylusClientConfig;

pub struct Weylus {
    notify_shutdown: Arc<tokio::sync::Notify>,
    web_thread: Option<std::thread::JoinHandle<()>>,
}

impl Weylus {
    pub fn new() -> Self {
        Self {
            notify_shutdown: Arc::new(tokio::sync::Notify::new()),
            web_thread: None,
        }
    }

    pub fn start(
        &mut self,
        config: &Config,
        mut on_web_message: impl FnMut(Web2UiMessage) + Send + 'static,
    ) -> bool {
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

        let (sender_ui, mut receiver_ui) = tokio::sync::mpsc::channel(100);
        let (sender_startup, receiver_startup) = tokio::sync::oneshot::channel();

        let web_thread = crate::web::run(
            sender_ui,
            sender_startup,
            self.notify_shutdown.clone(),
            WebServerConfig {
                bind_addr: SocketAddr::new(config.bind_address, config.web_port),
                access_code: config.access_code.clone(),
                custom_index_html: config.custom_index_html.clone(),
                custom_access_html: config.custom_access_html.clone(),
                custom_style_css: config.custom_style_css.clone(),
                custom_lib_js: config.custom_lib_js.clone(),
                #[cfg(target_os = "linux")]
                enable_custom_input_areas: config.wayland_support,
                #[cfg(not(target_os = "linux"))]
                enable_custom_input_areas: false,
            },
            WeylusClientConfig {
                encoder_options,
                #[cfg(target_os = "linux")]
                wayland_support: config.wayland_support,
                no_gui: config.no_gui,
            },
        );

        match receiver_startup.blocking_recv() {
            Ok(WebStartUpMessage::Start) => (),
            Ok(WebStartUpMessage::Error) => {
                if web_thread.join().is_err() {
                    error!("Webserver thread panicked.");
                }
                return false;
            }
            Err(err) => {
                error!("Error communicating with webserver thread: {}", err);
                if web_thread.join().is_err() {
                    error!("Webserver thread panicked.");
                }
                return false;
            }
        }
        self.web_thread = Some(web_thread);
        std::thread::spawn(move || {
            while let Some(msg) = receiver_ui.blocking_recv() {
                on_web_message(msg);
            }
        });
        true
    }

    pub fn stop(&mut self) {
        self.notify_shutdown.notify_one();
        self.wait();
    }

    fn wait(&mut self) {
        if let Some(t) = self.web_thread.take() {
            if t.join().is_err() {
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
