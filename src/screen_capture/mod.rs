pub mod generic;

#[cfg(target_os = "linux")]
pub mod linux;

pub trait ScreenCapture {
    /// capture screen
    fn capture(&mut self);

    fn pixel_provider(&self) -> crate::video::PixelProvider;

    /// width and size of captured image
    fn size(&self) -> (usize, usize);
}
