use crate::capturable::{Capturable, Recorder};
use captrs::Capturer;
use std::boxed::Box;
use std::error::Error;
use tracing::{debug, info, warn};
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
    /// Right after the display wakes (or a sign-in or mode change) Windows can refuse the
    /// output duplication for a moment; a connection attempted a few seconds later succeeds.
    /// Retry for up to about 4 s before giving up.
    pub fn new(id: u8) -> Result<CaptrsRecorder, Box<dyn Error>> {
        const ATTEMPTS: u32 = 16;
        let mut attempt = 1;
        loop {
            match Capturer::new(id.into()) {
                Ok(capturer) => {
                    if attempt > 1 {
                        info!(display = id, attempt, "Screen capture ready after retrying.");
                    }
                    return Ok(CaptrsRecorder { capturer });
                }
                Err(err) if attempt < ATTEMPTS => {
                    debug!(display = id, attempt, "Screen capture not ready ({err}), retrying.");
                    attempt += 1;
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
                Err(err) => {
                    warn!(display = id, attempts = attempt, "Screen capture failed: {err}.");
                    return Err(err.into());
                }
            }
        }
    }
}

impl Recorder for CaptrsRecorder {
    fn capture(&mut self) -> Result<crate::video::PixelProvider, Box<dyn Error>> {
        self.capturer
            .capture_store_frame()
            .map_err(|e| CaptrsError(format!("Captrs failed to capture frame: {e:?}")))?;
        let (w, h) = self.capturer.geometry();
        Ok(crate::video::PixelProvider::BGR0(
            w as usize,
            h as usize,
            unsafe { std::mem::transmute(self.capturer.get_stored_frame().unwrap()) },
        ))
    }
}
