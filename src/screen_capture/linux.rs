use libc::{c_int, c_void};
use std::slice::from_raw_parts;

use crate::cerror::CError;
use crate::screen_capture::ScreenCapture;

extern "C" {
    fn init_capture(err: *mut CError) -> *mut c_void;
    fn capture_sceen(handle: *mut c_void, img: *mut CImage, err: *mut CError);
    fn destroy_capture(handle: *mut c_void, err: *mut CError);
}

#[repr(C)]
struct CImage {
    data: *const u8,
    width: c_int,
    height: c_int,
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
    rgb_buf: Vec<u8>,
    png_buf: Vec<u8>,
}

impl ScreenCaptureX11 {
    pub fn new() -> Result<Self, CError> {
        let mut err = CError::new();
        let handle = unsafe { init_capture(&mut err) };
        if err.is_err() {
            return Err(err);
        } else {
            return Ok(Self {
                handle: handle,
                rgb_buf: Vec::<u8>::new(),
                png_buf: Vec::<u8>::new(),
            });
        }
    }

    fn convert_to_rgb(&mut self, img: &CImage) {
        let buf_size = (img.height * img.width * 3) as usize;
        if buf_size > self.rgb_buf.len() {
            self.rgb_buf = vec![0; buf_size];
        }
        let data = img.data();
        self.rgb_buf[0..buf_size]
            .iter_mut()
            .enumerate()
            .for_each(|(i, byte)| {
                let j = i / 3 * 4;
                match i % 3 {
                    0 => *byte = data[j + 2],
                    1 => *byte = data[j + 1],
                    2 => *byte = data[j],
                    _ => (),
                }
            });
    }

    fn convert_to_png(&mut self, img: &CImage) -> Result<&[u8], std::io::Error> {
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
        unsafe {
            destroy_capture(self.handle, &mut err);
        }
    }
}

impl ScreenCapture for ScreenCaptureX11 {
    fn capture(&mut self) -> &[u8] {
        let mut err = CError::new();
        let mut img = CImage::new();
        unsafe {
            capture_sceen(self.handle, &mut img, &mut err);
        }
        self.convert_to_png(&img).unwrap()
    }
}
