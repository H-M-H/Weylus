use std::os::raw::{c_int, c_uchar, c_void};
use std::time::Instant;

use tracing::warn;

use crate::cerror::CError;

extern "C" {
    fn init_video_encoder(
        rust_ctx: *mut c_void,
        width_in: c_int,
        height_in: c_int,
        width_out: c_int,
        height_out: c_int,
        try_vaapi: c_int,
        try_nvenc: c_int,
        try_videotoolbox: c_int,
        try_mediafoundation: c_int,
    ) -> *mut c_void;
    fn open_video(handle: *mut c_void, err: *mut CError);
    fn destroy_video_encoder(handle: *mut c_void);
    fn encode_video_frame(handle: *mut c_void, micros: c_int, err: *mut CError);

    fn fill_rgb(ctx: *mut c_void, data: *const u8, err: *mut CError);
    fn fill_rgb0(ctx: *mut c_void, data: *const u8, err: *mut CError);
    fn fill_bgr0(ctx: *mut c_void, data: *const u8, stride: c_int, err: *mut CError);
}

// this is used as callback in lib/encode_video.c via ffmpegs AVIOContext
#[no_mangle]
fn write_video_packet(video_encoder: *mut c_void, buf: *const c_uchar, buf_size: c_int) -> c_int {
    let video_encoder = unsafe { (video_encoder as *mut VideoEncoder).as_mut().unwrap() };
    (video_encoder.write_data)(unsafe {
        std::slice::from_raw_parts(buf as *const u8, buf_size as usize)
    });
    0
}

pub enum PixelProvider<'a> {
    // 8 bits per color
    RGB(usize, usize, &'a [u8]),
    RGB0(usize, usize, &'a [u8]),
    BGR0(usize, usize, &'a [u8]),
    // width, height, stride
    BGR0S(usize, usize, usize, &'a [u8]),
}

impl<'a> PixelProvider<'a> {
    pub fn size(&self) -> (usize, usize) {
        match self {
            PixelProvider::RGB(w, h, _) => (*w, *h),
            PixelProvider::RGB0(w, h, _) => (*w, *h),
            PixelProvider::BGR0(w, h, _) => (*w, *h),
            PixelProvider::BGR0S(w, h, _, _) => (*w, *h),
        }
    }
}

#[derive(Clone, Copy)]
pub struct EncoderOptions {
    pub try_vaapi: bool,
    pub try_nvenc: bool,
    pub try_videotoolbox: bool,
    pub try_mediafoundation: bool,
}

pub struct VideoEncoder {
    handle: *mut c_void,
    width_in: usize,
    height_in: usize,
    width_out: usize,
    height_out: usize,
    write_data: Box<dyn FnMut(&[u8])>,
    start_time: Instant,
}

impl VideoEncoder {
    pub fn new(
        width_in: usize,
        height_in: usize,
        width_out: usize,
        height_out: usize,
        mut write_data: impl FnMut(&[u8]) + 'static,
        options: EncoderOptions,
    ) -> Result<Box<Self>, CError> {
        let mut video_encoder = Box::new(Self {
            handle: std::ptr::null_mut(),
            width_in,
            height_in,
            width_out,
            height_out,
            write_data: Box::new(move |data| write_data(data)),
            start_time: Instant::now(),
        });
        let handle = unsafe {
            init_video_encoder(
                video_encoder.as_mut() as *mut _ as *mut c_void,
                width_in as c_int,
                height_in as c_int,
                width_out as c_int,
                height_out as c_int,
                options.try_vaapi.into(),
                options.try_nvenc.into(),
                options.try_videotoolbox.into(),
                options.try_mediafoundation.into(),
            )
        };
        video_encoder.handle = handle;

        let mut err = CError::new();
        unsafe { open_video(video_encoder.handle, &mut err) };
        if err.is_err() {
            return Err(err);
        }
        Ok(video_encoder)
    }

    pub fn encode(&mut self, pixel_provider: PixelProvider) {
        let mut err = CError::new();
        match pixel_provider {
            PixelProvider::BGR0(w, _, bgr0) => unsafe {
                fill_bgr0(self.handle, bgr0.as_ptr(), (w * 4) as c_int, &mut err);
            },
            PixelProvider::BGR0S(_, _, stride, bgr0) => unsafe {
                fill_bgr0(self.handle, bgr0.as_ptr(), stride as c_int, &mut err);
            },
            PixelProvider::RGB(_, _, rgb) => unsafe {
                fill_rgb(self.handle, rgb.as_ptr(), &mut err);
            },
            PixelProvider::RGB0(_, _, rgb) => unsafe {
                fill_rgb0(self.handle, rgb.as_ptr(), &mut err);
            },
        }
        if err.is_err() {
            warn!("Failed to fill video frame: {}", err);
            return;
        }
        unsafe {
            encode_video_frame(
                self.handle,
                (Instant::now() - self.start_time).as_millis() as c_int,
                &mut err,
            );
        }
        if err.is_err() {
            warn!("Failed to encode video frame: {}", err);
            return;
        }
    }

    pub fn check_size(
        &self,
        width_in: usize,
        height_in: usize,
        width_out: usize,
        height_out: usize,
    ) -> bool {
        (self.width_in == width_in)
            && (self.height_in == height_in)
            && (self.width_out == width_out)
            && (self.height_out == height_out)
    }
}

impl Drop for VideoEncoder {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { destroy_video_encoder(self.handle) }
        }
    }
}
