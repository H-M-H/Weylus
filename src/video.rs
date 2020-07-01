use std::os::raw::{c_int, c_uchar, c_void};
use std::time::Instant;

use tracing::warn;

use crate::cerror::CError;

extern "C" {
    fn init_video_encoder(
        rust_ctx: *mut c_void,
        width: c_int,
        height: c_int,
        try_vaapi: c_int,
        try_nvenc: c_int,
    ) -> *mut c_void;
    fn open_video(handle: *mut c_void, err: *mut CError);
    fn destroy_video_encoder(handle: *mut c_void);
    fn encode_video_frame(handle: *mut c_void, micros: c_int, err: *mut CError);

    fn fill_rgb(ctx: *mut c_void, data: *const u8, err: *mut CError);
    fn fill_bgra(ctx: *mut c_void, data: *const u8, err: *mut CError);
}

#[no_mangle]
fn write_video_packet(video_encoder: *mut c_void, buf: *const c_uchar, buf_size: c_int) -> c_int {
    let video_encoder = unsafe { (video_encoder as *mut VideoEncoder).as_mut().unwrap() };
    (video_encoder.write_data)(unsafe {
        std::slice::from_raw_parts(buf as *const u8, buf_size as usize)
    });
    0
}

pub enum PixelProvider<'a> {
    None,
    // 8 bits per color
    RGB(&'a [u8]),
    BGR0(&'a [u8]),
}

pub struct VideoEncoder {
    handle: *mut c_void,
    width: usize,
    height: usize,
    write_data: Box<dyn Fn(&[u8])>,
    start_time: Instant,
}

impl VideoEncoder {
    pub fn new(
        width: usize,
        height: usize,
        write_data: impl Fn(&[u8]) + 'static,
        #[cfg(target_os = "linux")] try_vaapi: bool,
        #[cfg(target_os = "linux")] try_nvenc: bool,
    ) -> Result<Box<Self>, CError> {
        let width = width;
        let height = height;
        let mut video_encoder = Box::new(Self {
            handle: std::ptr::null_mut(),
            width,
            height,
            write_data: Box::new(move |data| write_data(data)),
            start_time: Instant::now(),
        });
        let handle = unsafe {
            init_video_encoder(
                video_encoder.as_mut() as *mut _ as *mut c_void,
                width as c_int,
                height as c_int,
                #[cfg(target_os = "linux")]
                try_vaapi.into(),
                #[cfg(target_os = "linux")]
                try_nvenc.into(),
                #[cfg(not(target_os = "linux"))]
                0,
                #[cfg(not(target_os = "linux"))]
                0,
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
            PixelProvider::None => {
                warn!("Nothing to encode!");
                return;
            }
            PixelProvider::BGR0(bgra) => unsafe {
                fill_bgra(self.handle, bgra.as_ptr(), &mut err);
            },
            PixelProvider::RGB(rgb) => unsafe {
                fill_rgb(self.handle, rgb.as_ptr(), &mut err);
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

    pub fn check_size(&self, width: usize, height: usize) -> bool {
        (self.width == width) && (self.height == height)
    }
}

impl Drop for VideoEncoder {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { destroy_video_encoder(self.handle) }
        }
    }
}
