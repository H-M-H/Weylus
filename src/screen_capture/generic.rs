use image::{DynamicImage, ImageOutputFormat};

use crate::screen_capture::ScreenCapture;

pub struct ScreenCaptureGeneric {
    buf: Vec<u8>,
}

impl ScreenCaptureGeneric {
    pub fn new() -> Self {
        Self { buf: Vec::<u8>::new() }
    }
}

impl ScreenCapture for ScreenCaptureGeneric {
    fn capture(&mut self) -> &[u8] {
        let img: DynamicImage = autopilot::bitmap::capture_screen().unwrap().image;
        self.buf.clear();
        img.write_to(&mut self.buf, ImageOutputFormat::PNG).unwrap();
        &self.buf
    }
}
