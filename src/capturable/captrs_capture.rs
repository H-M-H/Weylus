use crate::capturable::{Capturable, Recorder};
use captrs::Capturer;
use std::boxed::Box;
use std::error::Error;
use winapi::shared::windef::RECT;

use super::Geometry;

#[derive(Clone)]
pub struct CaptrsCapturable {
    id: u8,
    name: String,
    screen: RECT,
    virtual_screen: RECT,
}

impl CaptrsCapturable {
    pub fn new(id: u8, name: String, screen: RECT, virtual_screen: RECT) -> CaptrsCapturable {
        CaptrsCapturable {
            id,
            name,
            screen,
            virtual_screen,
        }
    }
}

impl Capturable for CaptrsCapturable {
    fn name(&self) -> String {
        format!("Desktop {} (captrs)", self.name).into()
    }
    fn before_input(&mut self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    fn recorder(&self, _capture_cursor: bool) -> Result<Box<dyn Recorder>, Box<dyn Error>> {
        Ok(Box::new(CaptrsRecorder::new(self.id)?))
    }
    fn geometry(&self) -> Result<Geometry, Box<dyn Error>> {
        Ok(Geometry::VirtualScreen(
            self.screen.left - self.virtual_screen.left,
            self.screen.top - self.virtual_screen.top,
            (self.screen.right - self.screen.left) as u32,
            (self.screen.bottom - self.screen.top) as u32,
            self.screen.left,
            self.screen.top,
        ))
    }
}
#[derive(Debug)]
pub struct CaptrsError(String);

impl std::fmt::Display for CaptrsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self(s) = self;
        write!(f, "{}", s)
    }
}

impl Error for CaptrsError {}
pub struct CaptrsRecorder {
    capturer: Capturer,
}

impl CaptrsRecorder {
    pub fn new(id: u8) -> Result<CaptrsRecorder, Box<dyn Error>> {
        Ok(CaptrsRecorder {
            capturer: Capturer::new(id.into())?,
        })
    }
}

impl Recorder for CaptrsRecorder {
    fn capture(&mut self) -> Result<crate::video::PixelProvider, Box<dyn Error>> {
        self.capturer
            .capture_store_frame()
            .map_err(|_e| CaptrsError("Captrs failed to capture frame".into()))?;
        let (w, h) = self.capturer.geometry();
        Ok(crate::video::PixelProvider::BGR0(
            w as usize,
            h as usize,
            unsafe { std::mem::transmute(self.capturer.get_stored_frame().unwrap()) },
        ))
    }
}
