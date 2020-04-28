pub mod generic;

#[cfg(target_os = "linux")]
pub mod linux;

pub trait ScreenCapture {
    fn capture(&mut self) -> &[u8];
}
