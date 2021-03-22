pub mod generic;
use std::boxed::Box;
use std::error::Error;
use tracing::warn;

#[cfg(target_os = "linux")]
pub mod linux;

pub trait ScreenCapture {
    /// capture screen
    fn capture(&mut self) -> Result<(), Box<dyn Error>>;

    fn pixel_provider(&self) -> crate::video::PixelProvider;

    /// width and size of captured image
    fn size(&self) -> (usize, usize);
}

pub trait BoxCloneCapturable {
    fn box_clone(&self) -> Box<dyn Capturable>;
}

impl<T> BoxCloneCapturable for T
where
    T: Clone + Capturable + 'static,
{
    fn box_clone(&self) -> Box<dyn Capturable> {
        Box::new(self.clone())
    }
}

pub trait Capturable: Send + BoxCloneCapturable {
    fn name(&self) -> String;
    fn geometry_relative(&self) -> Result<(f64, f64, f64, f64), Box<dyn Error>>;
    fn before_input(&mut self) -> Result<(), Box<dyn Error>>;
    fn screen_capture(
        &self,
        capture_cursor: bool,
    ) -> Result<Box<dyn ScreenCapture>, Box<dyn Error>>;
}

impl Clone for Box<dyn Capturable> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

pub fn get_capturables() -> Vec<Box<dyn Capturable>> {
    let mut capturables: Vec<Box<dyn Capturable>> = vec![];
    #[cfg(target_os = "linux")]
    {
        use crate::x11helper::X11Context;
        let x11ctx = X11Context::new();
        if let Some(mut x11ctx) = x11ctx {
            match x11ctx.capturables() {
                Ok(captrs) => {
                    for c in captrs {
                        capturables.push(Box::new(c));
                    }
                }
                Err(err) => warn!("Failed to get list of capturables via X11: {}", err),
            }
        };
    }
    capturables
}
