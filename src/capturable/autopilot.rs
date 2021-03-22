use std::boxed::Box;
use std::error::Error;

use image_autopilot::GenericImageView;

use crate::capturable::{Recorder, Capturable};

#[derive(Clone)]
pub struct AutoPilotCapturable {}

impl AutoPilotCapturable {
    pub fn new() -> Self {
        Self {}
    }
}

impl Capturable for AutoPilotCapturable {
    fn name(&self) -> String {
        "Desktop (autopilot)".into()
    }
    fn geometry_relative(&self) -> Result<(f64, f64, f64, f64), Box<dyn Error>> {
        Ok((0.0, 0.0, 1.0, 1.0))
    }
    fn before_input(&mut self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    fn recorder(&self, _capture_cursor: bool) -> Result<Box<dyn Recorder>, Box<dyn Error>> {
        Ok(Box::new(RecorderAutoPilot::new()))
    }
}

pub struct RecorderAutoPilot {
    img: Vec<u8>,
    width: usize,
    height: usize,
}

impl RecorderAutoPilot {
    pub fn new() -> Self {
        Self {
            img: Vec::new(),
            width: 0,
            height: 0,
        }
    }
}

impl Recorder for RecorderAutoPilot {
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
