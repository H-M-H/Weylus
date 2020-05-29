use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use websocket::Message;
use websocket::OwnedMessage;

use tracing::{trace, warn};

use crate::input::device::InputDevice;
use crate::protocol::NetMessage;
use crate::screen_capture::ScreenCapture;

use crate::video::VideoEncoder;

type WsWriter = Arc<Mutex<websocket::sender::Writer<std::net::TcpStream>>>;

pub trait StreamHandler {
    fn process(&mut self, sender: WsWriter, message: &OwnedMessage);
}

pub struct PointerStreamHandler<T: InputDevice> {
    device: T,
}

impl<T: InputDevice> PointerStreamHandler<T> {
    pub fn new(device: T) -> Self {
        PointerStreamHandler { device }
    }
}

impl<Device: InputDevice> StreamHandler for PointerStreamHandler<Device> {
    fn process(&mut self, _: WsWriter, message: &OwnedMessage) {
        match message {
            OwnedMessage::Text(s) => {
                trace!("Pointerevent: {}", &s);
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
    video_encoder: Option<Box<VideoEncoder>>,
    update_interval: Duration,
    last_update: Instant,
}

impl<T: ScreenCapture> ScreenStreamHandler<T> {
    pub fn new(screen_capture: T, update_interval: Duration) -> Self {
        Self {
            screen_capture,
            video_encoder: None,
            update_interval,
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
                self.screen_capture.capture();
                let (width, height) = self.screen_capture.size();
                // video encoder is not setup or setup for encoding the wrong size: restart it
                if self.video_encoder.is_none()
                    || !self
                        .video_encoder
                        .as_ref()
                        .unwrap()
                        .check_size(width, height)
                {
                    if let Err(err) = sender.lock().unwrap().send_message(&Message::text("new")) {
                        warn!("Error sending video: {}", err);
                    }
                    self.video_encoder = Some(
                        VideoEncoder::new(width, height, move |data| {
                            let msg = Message::binary(data);
                            if let Err(err) = sender.lock().unwrap().send_message(&msg) {
                                warn!("Error sending video: {}", err);
                            }
                        })
                        .unwrap(),
                    )
                }
                let video_encoder = self.video_encoder.as_mut().unwrap();
                let screen_capture = RefCell::new(&mut self.screen_capture);
                video_encoder.encode(|y, u, v, y_linesize, u_linesize, v_linesize| {
                    screen_capture
                        .borrow_mut()
                        .fill_yuv(y, u, v, y_linesize, u_linesize, v_linesize)
                });
                self.last_update = Instant::now();
            }
            _ => (),
        }
    }
}
