use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use websocket::Message;
use websocket::OwnedMessage;

use tracing::warn;

use crate::input::pointer::PointerDevice;
use crate::protocol::NetMessage;
use crate::screen_capture::ScreenCapture;

type WsWriter = Arc<Mutex<websocket::sender::Writer<std::net::TcpStream>>>;

pub trait StreamHandler {
    fn process(&mut self, sender: WsWriter, message: &OwnedMessage);
}

pub struct PointerStreamHandler<T: PointerDevice> {
    device: T,
}

impl<T: PointerDevice> PointerStreamHandler<T> {
    pub fn new(device: T) -> Self {
        PointerStreamHandler { device: device }
    }
}

impl<Device: PointerDevice> StreamHandler for PointerStreamHandler<Device> {
    fn process(&mut self, _: WsWriter, message: &OwnedMessage) {
        match message {
            OwnedMessage::Text(s) => {
                println!("{}", &s);
                let message: Result<NetMessage, _> = serde_json::from_str(&s);
                match message {
                    Ok(message) => match message {
                        NetMessage::PointerEvent(event) => self.device.send_event(&event),
                    },
                    Err(err) => warn!("Unable to parse message: {}", err),
                }
            }
            _ => (),
        }
    }
}

pub struct ScreenStreamHandler<T: ScreenCapture> {
    screen_capture: T,
    base64_buf: Vec<u8>,
    update_interval: Duration,
    last_update: Instant,
}

impl<T: ScreenCapture> ScreenStreamHandler<T> {
    pub fn new(screen_capture: T, update_interval: Duration) -> Self {
        Self {
            screen_capture: screen_capture,
            base64_buf: Vec::<u8>::new(),
            update_interval: update_interval,
            last_update: Instant::now(),
        }
    }
}

impl<T: ScreenCapture> StreamHandler for ScreenStreamHandler<T> {
    fn process(&mut self, sender: WsWriter, message: &OwnedMessage) {
        match message {
            OwnedMessage::Text(_) => {
                let now = Instant::now();
                let interval = now - self.last_update;
                if interval < self.update_interval {
                    let msg = Message::text(format!(
                        "@{}", // prepend some none base64 character,
                        //  so clients can tell this is something different
                        (self.update_interval - interval).as_millis().to_string()
                    ));
                    if let Err(err) = sender.lock().unwrap().send_message(&msg) {
                        warn!("Error sending video: {}", err);
                    }
                    return;
                }
                let img = self.screen_capture.capture();
                let base64_size = img.len() * 4 / 3 + 4;
                if self.base64_buf.len() < base64_size {
                    self.base64_buf.resize(base64_size * 2, 0);
                }
                let base64_size =
                    base64::encode_config_slice(&img, base64::STANDARD, &mut self.base64_buf);
                let msg = Message::text(unsafe {
                    std::str::from_utf8_unchecked(&self.base64_buf[0..base64_size])
                });
                if let Err(err) = sender.lock().unwrap().send_message(&msg) {
                    warn!("Error sending video: {}", err);
                }
                self.last_update = Instant::now();
            }
            _ => (),
        }
    }
}
