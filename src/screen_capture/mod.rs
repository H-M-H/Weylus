pub mod generic;
use std::boxed::Box;
use std::error::Error;

#[cfg(target_os = "linux")]
pub mod linux;

pub trait ScreenCapture {
    /// capture screen
    fn capture(&mut self) -> Result<(), Box<dyn Error>>;

    fn pixel_provider(&self) -> crate::video::PixelProvider;

    /// width and size of captured image
    fn size(&self) -> (usize, usize);
}

pub trait Capturable {
    fn name(&self) -> String;
    fn geometry_relative(&self) -> Result<(f64, f64, f64, f64), Box<dyn Error>>;
    fn before_input(&mut self) -> Result<(), Box<dyn Error>>;
    fn screen_capture(&self, capture_cursor: bool) -> Result<Box<dyn ScreenCapture>, Box<dyn Error>>;
}
