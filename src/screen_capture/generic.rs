use image_autopilot::GenericImageView;
use image_autopilot::Pixel;

use tracing::trace;

use crate::screen_capture::ScreenCapture;

pub struct ScreenCaptureGeneric {
    img: Option<autopilot::bitmap::Bitmap>,
}

impl ScreenCaptureGeneric {
    pub fn new() -> Self {
        Self { img: None }
    }
}

impl ScreenCapture for ScreenCaptureGeneric {
    fn capture(&mut self) {
        self.img = Some(autopilot::bitmap::capture_screen().unwrap());
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
        let img = self
            .img
            .as_ref()
            .expect("capture has to be called before invoking fill_yuv");
        let img = &img.image;

        let (width, height) = self.size();

        #[cfg(debug_assertions)]
        {
            trace!("Converting rgb to yuv.");
            trace!(
                "width: {} height: {} y.len(): {} u.len(): {} v.len(): {} \
                y_line_size: {} u_line_size: {} v_line_size: {} \
                img.width: {} img.height: {}",
                width,
                height,
                y.len(),
                u.len(),
                v.len(),
                y_line_size,
                u_line_size,
                v_line_size,
                img.width(),
                img.height()
            );
        }

        // Y
        for yy in 0..height - height % 2 {
            for xx in 0..width - width % 2 {
                let p = img.get_pixel(xx as u32, yy as u32).to_rgb();
                let p = p.channels();
                let r = p[0] as i32;
                let g = p[1] as i32;
                let b = p[2] as i32;
                y[y_line_size * yy + xx] = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8;
            }
        }

        // Cb and Cr
        for yy in 0..(height / 2) {
            for xx in 0..(width / 2) {
                let p11 = img.get_pixel(2 * xx as u32, 2 * yy as u32).to_rgb();
                let p11 = p11.channels();
                let p12 = img.get_pixel(2 * xx as u32 + 1, 2 * yy as u32).to_rgb();
                let p12 = p12.channels();
                let p21 = img.get_pixel(2 * xx as u32, 2 * yy as u32 + 1).to_rgb();
                let p21 = p21.channels();
                let p22 = img.get_pixel(2 * xx as u32 + 1, 2 * yy as u32 + 1).to_rgb();
                let p22 = p22.channels();
                let mut r = p11[0] as i32 + p12[0] as i32 + p21[0] as i32 + p22[0] as i32;
                let mut g = p11[1] as i32 + p12[1] as i32 + p21[1] as i32 + p22[1] as i32;
                let mut b = p11[2] as i32 + p12[2] as i32 + p21[2] as i32 + p22[2] as i32;
                r >>= 2;
                g >>= 2;
                b >>= 2;
                u[yy * u_line_size + xx] = (((128 + 112 * b - 38 * r - 74 * g) >> 8) + 128) as u8;
                v[yy * v_line_size + xx] = (((128 + 112 * r - 94 * g - 18 * b) >> 8) + 128) as u8;
            }
        }
    }

    fn size(&self) -> (usize, usize) {
        self.img.as_ref().map_or((0, 0), |img| {
            (
                img.image.width() as usize,
                img.image.height() as usize
            )
        })
    }
}
