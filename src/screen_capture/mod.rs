pub mod generic;

#[cfg(target_os = "linux")]
pub mod linux;

pub trait ScreenCapture {
    /// capture screen
    fn capture(&mut self);

    /// encoded as PNG
    fn png(&mut self) -> &[u8];

    /// capture screen to YUV
    fn fill_yuv(
        &self,
        y: &mut [u8],
        u: &mut [u8],
        v: &mut [u8],
        y_line_size: usize,
        u_line_size: usize,
        v_line_size: usize,
    );

    /// width and size of captured image
    fn size(&self) -> (usize, usize);
}
