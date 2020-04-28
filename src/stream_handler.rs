use std::sync::{Arc, Mutex};
use websocket::Message;
use websocket::OwnedMessage;


use tracing::warn;

use crate::input::pointer::PointerDevice;
use crate::protocol::{NetMessage};
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
}

impl<T: ScreenCapture> ScreenStreamHandler<T> {
    pub fn new(screen_capture: T) -> Self {
        Self {
            screen_capture: screen_capture,
            base64_buf: Vec::<u8>::new(),
        }
    }
}

impl<T: ScreenCapture> StreamHandler for ScreenStreamHandler<T> {
    fn process(&mut self, sender: WsWriter, message: &OwnedMessage) {
        match message {
            OwnedMessage::Text(_) => {
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
            }
            _ => (),
        }
    }
}
