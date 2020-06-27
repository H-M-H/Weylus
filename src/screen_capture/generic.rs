use std::boxed::Box;
use std::error::Error;

use image_autopilot::GenericImageView;

use crate::screen_capture::ScreenCapture;

pub struct ScreenCaptureGeneric {
    img: Vec<u8>,
    width: usize,
    height: usize,
}

impl ScreenCaptureGeneric {
    pub fn new() -> Self {
        Self {
            img: Vec::new(),
            width: 0,
            height: 0,
        }
    }
}

impl ScreenCapture for ScreenCaptureGeneric {
    fn capture(&mut self) -> Result<(), Box<dyn Error>> {
        let img = autopilot::bitmap::capture_screen()?.image;
        self.width = img.width() as usize;
        self.height = img.height() as usize;
        self.img = img.into_rgb().into_raw();
        Ok(())
    }

    fn pixel_provider(&self) -> crate::video::PixelProvider {
        crate::video::PixelProvider::RGB(&self.img)
    }

    fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }
}
