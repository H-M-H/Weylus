use std::os::raw::{c_uint, c_void};
use std::slice::from_raw_parts;

use crate::cerror::CError;
use crate::screen_capture::ScreenCapture;
use crate::x11helper::WindowInfo;

extern "C" {
    fn init_capture(window: *const WindowInfo, ctx: *mut c_void, err: *mut CError) -> *mut c_void;
    fn capture_sceen(handle: *mut c_void, img: *mut CImage, err: *mut CError);
    fn destroy_capture(handle: *mut c_void, err: *mut CError);
}

#[repr(C)]
struct CImage {
    data: *const u8,
    width: c_uint,
    height: c_uint,
}

impl CImage {
    pub fn new() -> Self {
        Self {
            data: std::ptr::null(),
            width: 0,
            height: 0,
        }
    }

    pub fn size(&self) -> usize {
        (self.width * self.height * 4) as usize
    }

    pub fn data(&self) -> &[u8] {
        unsafe { from_raw_parts(self.data, self.size()) }
    }
}

pub struct ScreenCaptureX11 {
    handle: *mut c_void,
    img: CImage,
    png_buf: Vec<u8>,
}

impl ScreenCaptureX11 {
    pub fn new(window: WindowInfo) -> Result<Self, CError> {
        let mut err = CError::new();
        fltk::app::lock().unwrap();
        let handle = unsafe { init_capture(&window, std::ptr::null_mut(), &mut err) };
        fltk::app::unlock();
        if err.is_err() {
            return Err(err);
        } else {
            return Ok(Self {
                handle,
                img: CImage::new(),
                png_buf: Vec::<u8>::new(),
            });
        }
    }

    fn convert_to_png(&mut self) -> Result<&[u8], std::io::Error> {
        let img = &self.img;
        let mut header = mtpng::Header::new();
        header.set_size(img.width as u32, img.height as u32)?;
        header.set_color(mtpng::ColorType::Truecolor, 8)?;
        let mut options = mtpng::encoder::Options::new();
        options.set_compression_level(mtpng::CompressionLevel::Fast)?;

        self.png_buf.clear();
        if self.png_buf.capacity() < img.size() {
            self.png_buf = Vec::<u8>::with_capacity(img.size());
        }
        let mut encoder = mtpng::encoder::Encoder::new(&mut self.png_buf, &options);
        encoder.write_header(&header)?;
        let mut row_buf = vec![0 as u8; 3 * img.width as usize];
        let data = img.data();
        let mut pos: usize = 0;
        for i in 0..img.height * img.width {
            row_buf[pos] = data[4 * i as usize + 2];
            row_buf[pos + 1] = data[4 * i as usize + 1];
            row_buf[pos + 2] = data[4 * i as usize];
            pos += 3;
            if (i + 1) % img.width == 0 {
                encoder.write_image_rows(&row_buf)?;
                pos = 0;
            }
        }
        encoder.finish()?;
        Ok(&self.png_buf)
    }
}

impl Drop for ScreenCaptureX11 {
    fn drop(&mut self) {
        let mut err = CError::new();
        fltk::app::lock().unwrap();
        unsafe {
            destroy_capture(self.handle, &mut err);
        }
        fltk::app::unlock();
    }
}

impl ScreenCapture for ScreenCaptureX11 {
    fn capture(&mut self) {
        let mut err = CError::new();
        fltk::app::lock().unwrap();
        unsafe {
            capture_sceen(self.handle, &mut self.img, &mut err);
        }
        fltk::app::unlock();
    }

    fn png(&mut self) -> &[u8] {
        self.convert_to_png().unwrap()
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
        let data = self.img.data();
        let width = self.img.width as usize;
        let height = self.img.height as usize;

        // Y
        for yy in 0..height-height%2 {
            for xx in 0..width-width%2 {
                let i = width * yy + xx;
                let b = data[4 * i] as i32;
                let g = data[4 * i + 1] as i32;
                let r = data[4 * i + 2] as i32;
                y[y_line_size * yy + xx] = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8;
            }
        }

        // Cb and Cr
        for yy in 0..(height / 2) {
            for xx in 0..(width / 2) {
                let mut b = data[8 * (yy * width + xx)] as i32 + data[8 * (yy * width + xx) + 4] as i32;
                let mut g =
                    data[8 * (yy * width + xx) + 1] as i32 + data[8 * (yy * width + xx) + 1 + 4] as i32;
                let mut r =
                    data[8 * (yy * width + xx) + 2] as i32 + data[8 * (yy * width + xx) + 2 + 4] as i32;
                b += data[8 * (yy * width + xx) + 4 * width] as i32
                    + data[8 * (yy * width + xx) + 4 + 4 * width] as i32;
                g += data[8 * (yy * width + xx) + 1 + 4 * width] as i32
                    + data[8 * (yy * width + xx) + 1 + 4 + 4 * width] as i32;
                r += data[8 * (yy * width + xx) + 2 + 4 * width] as i32
                    + data[8 * (yy * width + xx) + 2 + 4 + 4 * width] as i32;
                r /= 4;
                g /= 4;
                b /= 4;
                u[yy * u_line_size + xx] = (((128 + 112 * b - 38 * r - 74 * g) >> 8) + 128) as u8;
                v[yy * v_line_size + xx] = (((128 + 112 * r - 94 * g - 18 * b) >> 8) + 128) as u8;
            }
        }
    }

    fn size(&self) -> (usize, usize) {
        (self.img.width as usize, self.img.height as usize)
    }
}
