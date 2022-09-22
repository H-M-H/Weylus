use std::boxed::Box;
use std::error::Error;

use image_autopilot::GenericImageView;

use crate::capturable::{Capturable, Geometry, Recorder};

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
    fn geometry(&self) -> Result<Geometry, Box<dyn Error>> {
        Ok(Geometry::Relative(0.0, 0.0, 1.0, 1.0))
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
}

impl RecorderAutoPilot {
    pub fn new() -> Self {
        Self { img: Vec::new() }
    }
}

impl Recorder for RecorderAutoPilot {
    fn capture(&mut self) -> Result<crate::video::PixelProvider, Box<dyn Error>> {
        let img = autopilot::bitmap::capture_screen()?.image;
        let w = img.width() as usize;
        let h = img.height() as usize;
        self.img = img.into_rgb().into_raw();
        Ok(crate::video::PixelProvider::RGB(w, h, &self.img))
    }
}
