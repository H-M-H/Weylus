use image::ImageOutputFormat;

use crate::screen_capture::ScreenCapture;

pub struct ScreenCaptureGeneric {
    img: Option<autopilot::bitmap::Bitmap>,
    buf: Vec<u8>,
}

impl ScreenCaptureGeneric {
    pub fn new() -> Self {
        Self {
            img: None,
            buf: Vec::<u8>::new(),
        }
    }
}

impl ScreenCapture for ScreenCaptureGeneric {
    fn capture(&mut self) {
        self.img = Some(autopilot::bitmap::capture_screen().unwrap());
    }

    fn png(&mut self) -> &[u8] {
        self.buf.clear();
        if let Some(img) = &self.img {
            img.image
                .write_to(&mut self.buf, ImageOutputFormat::PNG)
                .unwrap();
        }
        &self.buf
    }

    fn fill_yuv(
        &self,
        y: &mut [u8],
        u: &mut [u8],
        v: &mut [u8],
        y_line_size: usize,
        u_line_size: usize,
        v_line_size: usize,
    ) {
    }

    fn size(&self) -> (usize, usize) {
        self.img.as_ref().map_or((0, 0), |img| {
            (
                img.size.width.round() as usize,
                img.size.height.round() as usize,
            )
        })
    }
}
