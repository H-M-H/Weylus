use websocket::ws::message::Message as MessageTrait;
use websocket::Message;
use websocket::OwnedMessage;

use image::{DynamicImage, ImageOutputFormat};

use log::warn;

use crate::input::pointer::PointerDevice;
use crate::protocol::{NetMessage, PointerEvent};
use crate::screen_capture::ScreenCapture;

type WsWriter = websocket::sender::Writer<std::net::TcpStream>;

pub trait StreamHandler {
    fn process(&mut self, sender: &mut WsWriter, message: &OwnedMessage);
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
    fn process(&mut self, _: &mut WsWriter, message: &OwnedMessage) {
        match message {
            OwnedMessage::Text(s) => {
                println!("{}", &s);
                let message: Result<NetMessage, _> = serde_json::from_str(&s);
                match message {
                    Ok(message) => match message {
                        NetMessage::PointerEvent(event) => self.device.send_event(&event),
                        NetMessage::ClientConfig(config) => self.device.set_client_config(config),
                    },
                    Err(err) => warn!("Unable to parse message: {}", err),
                }
            }
            _ => (),
        }
    }
}

pub struct ScreenStreamHandler {
    screen_capture: ScreenCapture,
    base64_buf: Vec<u8>,
}

impl ScreenStreamHandler {
    pub fn new() -> Self {
        Self {
            screen_capture: ScreenCapture::new().unwrap(),
            base64_buf: Vec::<u8>::new(),
        }
    }
}

impl StreamHandler for ScreenStreamHandler {
    fn process(&mut self, sender: &mut WsWriter, message: &OwnedMessage) {
        match message {
            OwnedMessage::Text(s) => {
                println!("{}", &s);
                let img = self.screen_capture.capture();
                let base64_size = img.len() * 4 / 3 + 4;
                if self.base64_buf.len() < base64_size {
                    self.base64_buf.resize(base64_size * 2, 0);
                }
                //let bitmap = autopilot::bitmap::capture_screen().unwrap();
                //let mut buf = vec![];
                //let img: DynamicImage = bitmap.image;
                //img.write_to(&mut buf, ImageOutputFormat::PNG).unwrap();
                let base64_size =
                    base64::encode_config_slice(&img, base64::STANDARD, &mut self.base64_buf);
                let msg = Message::text(unsafe {
                    std::str::from_utf8_unchecked(&self.base64_buf[0..base64_size])
                });
                if let Err(err) = sender.send_message(&msg) {
                    warn!("Error sending video: {}", err);
                }
            }
            _ => (),
        }
    }
}
